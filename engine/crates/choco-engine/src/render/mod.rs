//! Render one clip: decode the source range, compose each output frame on the CPU (crop or
//! fit, captions), encode with VideoToolbox, and mux with trimmed, normalised AAC audio into a
//! fast-start MP4. Output is constant frame rate whatever the source does.

use crate::ProgressFn;
use crate::captions::{self, Rasterizer};
use crate::model::{CaptionTrack, CropPlan, Easing, Layout, MediaAsset, Progress, Rect};
use ff::{Dictionary, Packet, Rational, codec};
use ffmpeg_next as ff;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub mod audio;
pub mod encoders;
pub mod yuv;

const STAGE: &str = "render";

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("render: ffmpeg: {0}")]
    Ffmpeg(#[from] ff::Error),
    #[error("render: unsupported: {0}")]
    Unsupported(String),
    #[error("render: source has no video stream")]
    NoVideo,
    /// The encoder opened but failed while encoding; `render_clip` retries with the next one.
    #[error("render: encoder {encoder} failed mid-clip: {error}")]
    Encoder {
        encoder: &'static str,
        /// Position in `encoders::platform_candidates()`.
        index: usize,
        error: ff::Error,
    },
    #[error("render: cancelled")]
    Cancelled,
    #[error("render: io: {0}")]
    Io(#[from] std::io::Error),
    #[error("render: png: {0}")]
    Png(#[from] png::EncodingError),
}

#[derive(Debug, Clone)]
pub struct RenderSpec<'a> {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Output size, both even.
    pub size: (u32, u32),
    pub crop: &'a CropPlan,
    pub captions: Option<&'a CaptionTrack>,
    pub normalize_audio: bool,
    /// Never exceed this frame rate; sources above it are resampled.
    pub max_fps: u32,
    pub video_bitrate: usize,
    /// Write a PNG of the composed frame at this source time.
    pub poster: Option<(u64, &'a Path)>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RenderReport {
    pub frames: u64,
    pub fps: (u32, u32),
    pub duration_ms: u64,
    pub video_encoder: String,
    pub has_audio: bool,
    pub wall_ms: u64,
}

/// A static centred crop with the target aspect ratio, in normalised display coordinates.
pub fn center_crop_plan(display: (u32, u32), target: (u32, u32), t_ms: u64) -> CropPlan {
    let (sw, sh) = (display.0 as f32, display.1 as f32);
    let (ta, sa) = (target.0 as f32 / target.1 as f32, sw / sh);
    let rect = if sa > ta {
        let w = ta / sa;
        Rect {
            x: (1.0 - w) / 2.0,
            y: 0.0,
            w,
            h: 1.0,
        }
    } else {
        let h = sa / ta;
        Rect {
            x: 0.0,
            y: (1.0 - h) / 2.0,
            w: 1.0,
            h,
        }
    };
    CropPlan {
        layout: Layout::Static,
        keyframes: vec![crate::model::CropKeyframe {
            t_ms,
            rects: vec![rect],
            easing: Easing::Hold,
        }],
        source_size: display,
        target_size: target,
    }
}

fn ease(e: Easing, t: f32) -> f32 {
    match e {
        Easing::Hold | Easing::Cut => 0.0,
        Easing::Linear => t,
        Easing::EaseInOut => t * t * (3.0 - 2.0 * t),
    }
}

/// Crop rectangles at `t_ms`, interpolated between keyframes. A keyframe's easing describes how
/// the camera moves from it to the next one.
pub fn rects_at(plan: &CropPlan, t_ms: u64) -> Vec<Rect> {
    let k = &plan.keyframes;
    if k.is_empty() {
        return vec![Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        }];
    }
    let i = k.iter().rposition(|kf| kf.t_ms <= t_ms).unwrap_or(0);
    if i + 1 >= k.len() || t_ms < k[0].t_ms {
        return k[i].rects.clone();
    }
    let (a, b) = (&k[i], &k[i + 1]);
    let f = ease(
        a.easing,
        (t_ms - a.t_ms) as f32 / (b.t_ms - a.t_ms).max(1) as f32,
    );
    a.rects
        .iter()
        .zip(&b.rects)
        .map(|(p, q)| Rect {
            x: p.x + (q.x - p.x) * f,
            y: p.y + (q.y - p.y) * f,
            w: p.w + (q.w - p.w) * f,
            h: p.h + (q.h - p.h) * f,
        })
        .collect()
}

fn to_px(r: &Rect, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let cw = ((r.w.clamp(0.01, 1.0) * w as f32) as u32).clamp(2, w);
    let ch = ((r.h.clamp(0.01, 1.0) * h as f32) as u32).clamp(2, h);
    let cx = ((r.x.max(0.0) * w as f32) as u32).min(w - cw);
    let cy = ((r.y.max(0.0) * h as f32) as u32).min(h - ch);
    (cx, cy, cw, ch)
}

struct Composer {
    scalers: yuv::Scalers,
    out_w: u32,
    out_h: u32,
}

impl Composer {
    fn compose(
        &mut self,
        src: &yuv::Frame,
        plan: &CropPlan,
        t_ms: u64,
    ) -> Result<yuv::Frame, ff::Error> {
        let (sw, sh) = (src.width(), src.height());
        let (ow, oh) = (self.out_w, self.out_h);
        let mut out = yuv::new_frame(ow, oh);
        match plan.layout {
            Layout::Fit => {
                // Blurred, zoomed background; whole picture scaled to fit on top.
                let (bw, bh) = ((ow / 12).max(16) & !1, (oh / 12).max(16) & !1);
                let mut small = yuv::new_frame(bw, bh);
                let bg_rect = to_px(
                    &center_crop_plan((sw, sh), (ow, oh), 0).keyframes[0].rects[0],
                    sw,
                    sh,
                );
                self.scalers
                    .crop_scale_into(src, bg_rect, &mut small, (0, 0, bw, bh))?;
                yuv::blur(&mut small, 3);
                self.scalers
                    .crop_scale_into(&small, (0, 0, bw, bh), &mut out, (0, 0, ow, oh))?;
                let scale = (ow as f32 / sw as f32).min(oh as f32 / sh as f32);
                let (fw, fh) = (
                    ((sw as f32 * scale) as u32) & !1,
                    ((sh as f32 * scale) as u32) & !1,
                );
                self.scalers.crop_scale_into(
                    src,
                    (0, 0, sw, sh),
                    &mut out,
                    ((ow - fw) / 2, (oh - fh) / 2, fw, fh),
                )?;
            }
            Layout::Split => {
                let rects = rects_at(plan, t_ms);
                let half = (oh / 2) & !1;
                for (slot, r) in rects.iter().take(2).enumerate() {
                    self.scalers.crop_scale_into(
                        src,
                        to_px(r, sw, sh),
                        &mut out,
                        (0, slot as u32 * half, ow, half),
                    )?;
                }
            }
            Layout::Single | Layout::Static => {
                let r = rects_at(plan, t_ms);
                self.scalers.crop_scale_into(
                    src,
                    to_px(&r[0], sw, sh),
                    &mut out,
                    (0, 0, ow, oh),
                )?;
            }
        }
        Ok(out)
    }
}

fn pick_fps(asset: &MediaAsset, max_fps: u32) -> (u32, u32) {
    let (n, d) = asset
        .video
        .as_ref()
        .map(|v| (v.fps_num, v.fps_den))
        .unwrap_or((30, 1));
    let fps = n as f64 / d.max(1) as f64;
    if !(1.0..=max_fps as f64 + 0.01).contains(&fps) || d == 0 {
        (max_fps.min(30), 1)
    } else {
        (n, d)
    }
}

pub fn render_clip(
    asset: &MediaAsset,
    spec: &RenderSpec,
    out_path: &Path,
    raster: &mut Rasterizer,
    progress: ProgressFn,
    cancel: &AtomicBool,
) -> Result<RenderReport, RenderError> {
    // A hardware encoder can open and still fail mid-clip (driver reset, GPU memory). Render the
    // clip once more, starting from the next candidate; if that fails too, say what each did.
    match render_from(asset, spec, out_path, raster, progress, cancel, 0) {
        Err(RenderError::Encoder {
            encoder,
            index,
            error,
        }) => {
            let first = format!("{encoder} failed mid-clip: {error}");
            progress(Progress::new(
                STAGE,
                0.0,
                format!("{first}; rendering again with the next encoder"),
            ));
            render_from(asset, spec, out_path, raster, progress, cancel, index + 1).map_err(|e| {
                match e {
                    RenderError::Encoder { encoder, error, .. } => RenderError::Unsupported(
                        format!("{first}; {encoder} failed mid-clip too: {error}"),
                    ),
                    RenderError::Unsupported(m) => {
                        RenderError::Unsupported(format!("{first}; then {m}"))
                    }
                    other => other,
                }
            })
        }
        other => other,
    }
}

/// One render attempt using encoder candidates from position `from` on.
fn render_from(
    asset: &MediaAsset,
    spec: &RenderSpec,
    out_path: &Path,
    raster: &mut Rasterizer,
    progress: ProgressFn,
    cancel: &AtomicBool,
    from: usize,
) -> Result<RenderReport, RenderError> {
    crate::init();
    let started = Instant::now();
    let vinfo = asset.video.as_ref().ok_or(RenderError::NoVideo)?;
    let (ow, oh) = (spec.size.0 & !1, spec.size.1 & !1);
    let (fps_n, fps_d) = pick_fps(asset, spec.max_fps);
    let frame_ms = 1000.0 * fps_d as f64 / fps_n as f64;
    // Exact integer frame count: float division put 2000 ms at 30 fps on 59.999... frames.
    let total = ((spec.end_ms - spec.start_ms) as u128 * fps_n as u128
        / (1000 * fps_d.max(1) as u128))
        .max(1) as u64;
    progress(Progress::new(STAGE, 0.0, "preparing audio"));

    let audio = audio::encode(asset, spec.start_ms, spec.end_ms, spec.normalize_audio)?;

    // Video encoder: the first of this machine's candidates that opens (encoders::platform_candidates).
    let mut tried: Vec<String> = Vec::new();
    let mut opened = None;
    for (index, cand) in encoders::platform_candidates()
        .iter()
        .enumerate()
        .skip(from)
    {
        let Some(vcodec) = ff::encoder::find_by_name(cand.name) else {
            tried.push(format!("{}: not in this FFmpeg build", cand.name));
            continue;
        };
        let mut venc = codec::context::Context::new_with_codec(vcodec)
            .encoder()
            .video()?;
        venc.set_width(ow);
        venc.set_height(oh);
        venc.set_format(cand.pixel);
        venc.set_time_base(Rational(fps_d as i32, fps_n as i32));
        venc.set_frame_rate(Some(Rational(fps_n as i32, fps_d as i32)));
        venc.set_bit_rate(spec.video_bitrate);
        venc.set_max_bit_rate(spec.video_bitrate * 3 / 2);
        venc.set_gop((2 * fps_n / fps_d.max(1)).max(1));
        venc.set_colorspace(ff::color::Space::BT709);
        venc.set_color_range(ff::color::Range::MPEG);
        venc.set_flags(codec::Flags::GLOBAL_HEADER);
        let mut opts = Dictionary::new();
        for (k, v) in cand.options {
            opts.set(k, v);
        }
        match venc.open_with(opts) {
            Ok(v) => {
                opened = Some((vcodec, v, cand.name, index));
                break;
            }
            Err(e) => tried.push(format!("{}: {e}", cand.name)),
        }
    }
    let (vcodec, mut venc, encoder_name, encoder_index) = opened.ok_or_else(|| {
        RenderError::Unsupported(format!(
            "no H.264 encoder opened ({}){}",
            if !tried.is_empty() {
                tried.join("; ")
            } else if let Ok(only) = std::env::var("CHOCO_ENCODERS") {
                format!("CHOCO_ENCODERS={only} matches no encoder this machine offers")
            } else {
                "this platform has no H.264 encoder configured".into()
            },
            encoders::missing_runtime_hint().unwrap_or_default()
        ))
    })?;

    // Muxer.
    let mut octx = ff::format::output_as(out_path, "mp4")?;
    let v_index = {
        let mut ost = octx.add_stream(vcodec)?;
        ost.set_parameters(&venc);
        ost.set_time_base(Rational(fps_d as i32, fps_n as i32));
        ost.index()
    };
    let a_index = match &audio {
        Some(a) => {
            let mut ost = octx.add_stream(a.encoder.codec().ok_or(ff::Error::EncoderNotFound)?)?;
            ost.set_parameters(&a.encoder);
            ost.set_time_base(a.time_base);
            Some(ost.index())
        }
        None => None,
    };
    let mut mux_opts = Dictionary::new();
    mux_opts.set("movflags", "+faststart");
    octx.write_header_with(mux_opts)?;
    let v_tb = octx.stream(v_index).expect("video stream").time_base();
    let a_tb = a_index.map(|i| octx.stream(i).expect("audio stream").time_base());
    let enc_tb = Rational(fps_d as i32, fps_n as i32);

    let mut audio_packets = audio
        .as_ref()
        .map(|a| a.packets.clone())
        .unwrap_or_default();
    audio_packets.reverse(); // pop from the end in order
    let write_audio_until = |octx: &mut ff::format::context::Output,
                             pkts: &mut Vec<Packet>,
                             until_secs: f64|
     -> Result<(), ff::Error> {
        while let Some(p) = pkts.last() {
            let t = p.pts().unwrap_or(0) as f64 / audio::SAMPLE_RATE as f64;
            if t > until_secs {
                break;
            }
            let mut p = pkts.pop().expect("checked");
            p.set_stream(a_index.expect("audio packets imply an audio stream"));
            p.rescale_ts(Rational(1, audio::SAMPLE_RATE), a_tb.expect("audio tb"));
            p.write_interleaved(octx)?;
        }
        Ok(())
    };

    // Decoder.
    let mut ictx = ff::format::input(&asset.path)?;
    let stream = ictx.stream(vinfo.index).ok_or(RenderError::NoVideo)?;
    let tb = stream.time_base();
    let base = audio::stream_start_secs(&stream);
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())?
        .decoder()
        .video()
        .map_err(|_| RenderError::Unsupported(format!("video codec {}", vinfo.codec)))?;
    let seek_to = ((spec.start_ms as f64 / 1000.0 + base - 3.0).max(0.0)
        * ff::ffi::AV_TIME_BASE as f64) as i64;
    let _ = ictx.seek(seek_to, ..seek_to);

    let mut composer = Composer {
        scalers: yuv::Scalers::default(),
        out_w: ow,
        out_h: oh,
    };
    let mut sprite_cache: HashMap<(usize, Option<usize>), yuv::YuvSprite> = HashMap::new();
    let mut current_raw: Option<yuv::Frame> = None;
    let mut current: Option<yuv::Frame> = None;
    let mut n: u64 = 0;
    let mut last_report = Instant::now();
    let poster_frame = spec
        .poster
        .map(|(t, _)| ((t.saturating_sub(spec.start_ms)) as f64 / frame_ms) as u64);

    // Test hook (tests/encoder_retry.rs): the first encoder fails at frame 5.
    let fail_at =
        (from == 0 && std::env::var_os("CHOCO_TEST_FAIL_FIRST_ENCODER").is_some()).then_some(5);
    let mut emit = |src: &yuv::Frame,
                    n: &mut u64,
                    venc: &mut ff::encoder::Video,
                    octx: &mut ff::format::context::Output,
                    audio_packets: &mut Vec<Packet>,
                    composer: &mut Composer|
     -> Result<(), RenderError> {
        let t_ms = spec.start_ms + (*n as f64 * frame_ms) as u64;
        let mut frame = composer.compose(src, spec.crop, t_ms)?;
        if let Some(track) = spec.captions
            && let Some((ci, chunk)) = track
                .chunks
                .iter()
                .enumerate()
                .find(|(_, c)| t_ms >= c.start_ms && t_ms < c.end_ms)
        {
            let active = captions::active_word(chunk, t_ms);
            let sprite = sprite_cache.entry((ci, active)).or_insert_with(|| {
                yuv::YuvSprite::from_sprite(&raster.render(chunk, active, &track.style, ow, oh))
            });
            sprite.blend(&mut frame);
        }
        if poster_frame == Some(*n)
            && let Some((_, path)) = spec.poster
        {
            let (w, h, rgb) = yuv::to_rgb(&mut composer.scalers, &frame)?;
            let mut e =
                png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w, h);
            e.set_color(png::ColorType::Rgb);
            e.write_header()?.write_image_data(&rgb)?;
        }
        frame.set_pts(Some(*n as i64));
        let enc_err = |error: ff::Error| RenderError::Encoder {
            encoder: encoder_name,
            index: encoder_index,
            error,
        };
        if fail_at == Some(*n) {
            return Err(enc_err(ff::Error::External));
        }
        let write_ready = |venc: &mut ff::encoder::Video,
                           octx: &mut ff::format::context::Output,
                           audio_packets: &mut Vec<Packet>|
         -> Result<(), ff::Error> {
            let mut pkt = Packet::empty();
            while venc.receive_packet(&mut pkt).is_ok() {
                let t_secs = pkt.pts().unwrap_or(0) as f64 * f64::from(enc_tb);
                write_audio_until(octx, audio_packets, t_secs)?;
                pkt.set_stream(v_index);
                pkt.rescale_ts(enc_tb, v_tb);
                pkt.write_interleaved(octx)?;
            }
            Ok(())
        };
        // Send, collecting packets whenever the encoder asks for room (EAGAIN).
        loop {
            match venc.send_frame(&frame) {
                Err(ff::Error::Other { errno }) if errno == ff::util::error::EAGAIN => {
                    write_ready(venc, octx, audio_packets).map_err(enc_err)?
                }
                other => break other.map_err(enc_err)?,
            }
        }
        write_ready(venc, octx, audio_packets).map_err(enc_err)?;
        *n += 1;
        Ok(())
    };

    let prep = |raw: &yuv::Frame, composer: &mut Composer| -> Result<yuv::Frame, ff::Error> {
        let norm = composer.scalers.normalise(raw)?;
        Ok(yuv::rotate(&norm, vinfo.rotation_deg))
    };

    let mut decoded = ff::frame::Video::empty();
    let end_t = spec.end_ms as f64;
    'read: for (st, packet) in ictx.packets() {
        if st.index() != vinfo.index {
            continue;
        }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            if cancel.load(Ordering::Relaxed) {
                return Err(RenderError::Cancelled);
            }
            let pts = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
            let t = (pts as f64 * f64::from(tb) - base) * 1000.0;
            // Emit every output frame whose time comes before this source frame.
            while n < total
                && (spec.start_ms as f64 + n as f64 * frame_ms) < t
                && let Some(raw) = current_raw.as_ref()
            {
                if current.is_none() {
                    current = Some(prep(raw, &mut composer)?);
                }
                emit(
                    current.as_ref().expect("prepared"),
                    &mut n,
                    &mut venc,
                    &mut octx,
                    &mut audio_packets,
                    &mut composer,
                )?;
                if last_report.elapsed().as_millis() >= 300 {
                    progress(Progress::new(
                        STAGE,
                        n as f32 / total as f32,
                        format!("frame {n}/{total}"),
                    ));
                    last_report = Instant::now();
                }
            }
            current_raw = Some(decoded.clone());
            current = None;
            if n >= total || t > end_t + frame_ms {
                break 'read;
            }
        }
    }
    // Source ended (or we stopped reading): hold the last frame to fill the clip.
    if n < total {
        if current_raw.is_none() {
            decoder.send_eof()?;
            if decoder.receive_frame(&mut decoded).is_ok() {
                current_raw = Some(decoded.clone());
            }
        }
        if let Some(raw) = current_raw.as_ref() {
            let f = prep(raw, &mut composer)?;
            while n < total {
                emit(
                    &f,
                    &mut n,
                    &mut venc,
                    &mut octx,
                    &mut audio_packets,
                    &mut composer,
                )?;
            }
        }
    }
    venc.send_eof().map_err(|error| RenderError::Encoder {
        encoder: encoder_name,
        index: encoder_index,
        error,
    })?;
    let mut pkt = Packet::empty();
    while venc.receive_packet(&mut pkt).is_ok() {
        let t_secs = pkt.pts().unwrap_or(0) as f64 * f64::from(enc_tb);
        write_audio_until(&mut octx, &mut audio_packets, t_secs)?;
        pkt.set_stream(v_index);
        pkt.rescale_ts(enc_tb, v_tb);
        pkt.write_interleaved(&mut octx)?;
    }
    write_audio_until(&mut octx, &mut audio_packets, f64::MAX)?;
    octx.write_trailer()?;
    progress(Progress::new(STAGE, 1.0, format!("{n} frames")));
    Ok(RenderReport {
        frames: n,
        fps: (fps_n, fps_d),
        duration_ms: (n as f64 * frame_ms) as u64,
        video_encoder: encoder_name.into(),
        has_audio: audio.is_some(),
        wall_ms: started.elapsed().as_millis() as u64,
    })
}

/// Compose the single output frame at source time `t_ms` (crop, layout, captions) without
/// encoding. Used for previews while scrubbing and for snapshot tests.
pub fn frame_at(
    asset: &MediaAsset,
    spec: &RenderSpec,
    t_ms: u64,
    raster: &mut Rasterizer,
) -> Result<yuv::Frame, RenderError> {
    crate::init();
    let vinfo = asset.video.as_ref().ok_or(RenderError::NoVideo)?;
    let mut ictx = ff::format::input(&asset.path)?;
    let stream = ictx.stream(vinfo.index).ok_or(RenderError::NoVideo)?;
    let (tb, base) = (stream.time_base(), audio::stream_start_secs(&stream));
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())?
        .decoder()
        .video()
        .map_err(|_| RenderError::Unsupported(format!("video codec {}", vinfo.codec)))?;
    let seek_to =
        ((t_ms as f64 / 1000.0 + base - 3.0).max(0.0) * ff::ffi::AV_TIME_BASE as f64) as i64;
    let _ = ictx.seek(seek_to, ..seek_to);
    let mut decoded = ff::frame::Video::empty();
    let mut best: Option<yuv::Frame> = None;
    'read: for (st, packet) in ictx.packets() {
        if st.index() != vinfo.index {
            continue;
        }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            let pts = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
            let t = (pts as f64 * f64::from(tb) - base) * 1000.0;
            if t > t_ms as f64 && best.is_some() {
                break 'read;
            }
            best = Some(decoded.clone());
        }
    }
    if best.is_none() {
        decoder.send_eof()?;
        if decoder.receive_frame(&mut decoded).is_ok() {
            best = Some(decoded.clone());
        }
    }
    let raw = best.ok_or(RenderError::NoVideo)?;
    let (ow, oh) = (spec.size.0 & !1, spec.size.1 & !1);
    let mut composer = Composer {
        scalers: yuv::Scalers::default(),
        out_w: ow,
        out_h: oh,
    };
    let src = yuv::rotate(&composer.scalers.normalise(&raw)?, vinfo.rotation_deg);
    let mut frame = composer.compose(&src, spec.crop, t_ms)?;
    if let Some(track) = spec.captions
        && let Some(chunk) = track
            .chunks
            .iter()
            .find(|c| t_ms >= c.start_ms && t_ms < c.end_ms)
    {
        let sprite = raster.render(
            chunk,
            captions::active_word(chunk, t_ms),
            &track.style,
            ow,
            oh,
        );
        yuv::YuvSprite::from_sprite(&sprite).blend(&mut frame);
    }
    Ok(frame)
}

/// Decode `[start_ms, end_ms)` once, sequentially, and call `f(t_ms, frame)` with the
/// normalised, rotated (display orientation) frame current at each sample time, `fps` per second.
pub fn sample_frames(
    asset: &MediaAsset,
    start_ms: u64,
    end_ms: u64,
    fps: f64,
    cancel: &AtomicBool,
    mut f: impl FnMut(u64, &yuv::Frame) -> Result<(), RenderError>,
) -> Result<usize, RenderError> {
    crate::init();
    let vinfo = asset.video.as_ref().ok_or(RenderError::NoVideo)?;
    let mut ictx = ff::format::input(&asset.path)?;
    let stream = ictx.stream(vinfo.index).ok_or(RenderError::NoVideo)?;
    let (tb, base) = (stream.time_base(), audio::stream_start_secs(&stream));
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())?
        .decoder()
        .video()
        .map_err(|_| RenderError::Unsupported(format!("video codec {}", vinfo.codec)))?;
    let seek_to =
        ((start_ms as f64 / 1000.0 + base - 3.0).max(0.0) * ff::ffi::AV_TIME_BASE as f64) as i64;
    let _ = ictx.seek(seek_to, ..seek_to);
    let step = 1000.0 / fps.max(0.1);
    let total = (((end_ms - start_ms) as f64) / step).floor() as usize;
    let mut scalers = yuv::Scalers::default();
    let mut decoded = ff::frame::Video::empty();
    let mut current: Option<ff::frame::Video> = None;
    let mut k = 0usize;
    'read: for (st, packet) in ictx.packets() {
        if st.index() != vinfo.index {
            continue;
        }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            if cancel.load(Ordering::Relaxed) {
                return Err(RenderError::Cancelled);
            }
            let pts = decoded.timestamp().or(decoded.pts()).unwrap_or(0);
            let t = (pts as f64 * f64::from(tb) - base) * 1000.0;
            while k < total && (start_ms as f64 + k as f64 * step) < t {
                if let Some(raw) = current.as_ref() {
                    let fr = yuv::rotate(&scalers.normalise(raw)?, vinfo.rotation_deg);
                    f(start_ms + (k as f64 * step) as u64, &fr)?;
                }
                k += 1;
            }
            current = Some(decoded.clone());
            if k >= total {
                break 'read;
            }
        }
    }
    if let Some(raw) = current.as_ref() {
        let fr = yuv::rotate(&scalers.normalise(raw)?, vinfo.rotation_deg);
        while k < total {
            f(start_ms + (k as f64 * step) as u64, &fr)?;
            k += 1;
        }
    }
    Ok(k)
}

/// Downsampled luma fingerprint (`cols` x `rows` block means) for snapshot comparisons.
pub fn luma_fingerprint(f: &yuv::Frame, cols: usize, rows: usize) -> Vec<u8> {
    let (w, h, s) = (f.width() as usize, f.height() as usize, f.stride(0));
    let y = f.data(0);
    let mut out = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            let (x0, x1, y0, y1) = (
                c * w / cols,
                (c + 1) * w / cols,
                r * h / rows,
                (r + 1) * h / rows,
            );
            let mut sum = 0u64;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    sum += y[yy * s + xx] as u64;
                }
            }
            out.push((sum / ((x1 - x0) * (y1 - y0)).max(1) as u64) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CropKeyframe;

    #[test]
    fn center_crop_for_vertical_from_landscape() {
        let p = center_crop_plan((1920, 1080), (1080, 1920), 0);
        let r = &p.keyframes[0].rects[0];
        assert!((r.h - 1.0).abs() < 1e-6);
        assert!((r.w - (9.0 / 16.0) / (16.0 / 9.0)).abs() < 1e-4, "{r:?}");
        assert!((r.x + r.w / 2.0 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn interpolates_between_keyframes() {
        let a = Rect {
            x: 0.0,
            y: 0.0,
            w: 0.3,
            h: 1.0,
        };
        let b = Rect {
            x: 0.6,
            y: 0.0,
            w: 0.3,
            h: 1.0,
        };
        let plan = CropPlan {
            layout: Layout::Single,
            keyframes: vec![
                CropKeyframe {
                    t_ms: 1000,
                    rects: vec![a],
                    easing: Easing::Linear,
                },
                CropKeyframe {
                    t_ms: 2000,
                    rects: vec![b],
                    easing: Easing::Hold,
                },
            ],
            source_size: (1920, 1080),
            target_size: (1080, 1920),
        };
        assert!((rects_at(&plan, 500)[0].x - 0.0).abs() < 1e-6);
        assert!((rects_at(&plan, 1500)[0].x - 0.3).abs() < 1e-4);
        assert!((rects_at(&plan, 2500)[0].x - 0.6).abs() < 1e-6);
    }

    #[test]
    fn pixel_rects_stay_inside_the_frame() {
        let r = Rect {
            x: 0.95,
            y: -0.1,
            w: 0.3,
            h: 1.2,
        };
        let (x, y, w, h) = to_px(&r, 1920, 1080);
        assert!(x + w <= 1920 && y + h <= 1080 && w >= 2 && h >= 2);
    }
}

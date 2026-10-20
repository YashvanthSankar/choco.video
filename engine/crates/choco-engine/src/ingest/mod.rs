//! Ingest: probe a source file, extract speech-ready audio, take thumbnails.

use crate::ProgressFn;
use crate::model::{AudioStream, MediaAsset, Progress, VideoStream};
use ffmpeg_next as ff;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const STAGE: &str = "ingest";
/// Sample rate every speech engine receives.
pub const SPEECH_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("ingest: cannot open {path}: {source}")]
    Open { path: PathBuf, source: ff::Error },
    #[error("ingest: {path} has no {kind} stream")]
    NoStream { path: PathBuf, kind: &'static str },
    #[error("ingest: ffmpeg: {0}")]
    Ffmpeg(#[from] ff::Error),
    #[error("ingest: this build cannot decode {codec} ({kind}); file a bug with the codec name")]
    Unsupported { codec: String, kind: &'static str },
    #[error("ingest: io: {0}")]
    Io(#[from] std::io::Error),
    #[error("ingest: png: {0}")]
    Png(#[from] png::EncodingError),
}

pub type Result<T> = std::result::Result<T, IngestError>;

/// Read container and stream metadata. Does not decode.
pub fn probe(path: &Path) -> Result<MediaAsset> {
    crate::init();
    let ictx = ff::format::input(path).map_err(|source| IngestError::Open {
        path: path.into(),
        source,
    })?;
    let duration_ms = if ictx.duration() > 0 {
        (ictx.duration() as i128 * 1000 / ff::ffi::AV_TIME_BASE as i128) as u64
    } else {
        0
    };

    let video = match ictx.streams().best(ff::media::Type::Video) {
        Some(s) => {
            // Read size from the container's codec parameters, not a decoder: probing must work
            // even when this build cannot decode the codec, so the error surfaces at decode time
            // with a clear message instead of "decoder not found" on open.
            // SAFETY: codecpar is valid for the lifetime of the stream.
            let (w, h) = unsafe {
                let par = (*s.as_ptr()).codecpar;
                ((*par).width.max(0) as u32, (*par).height.max(0) as u32)
            };
            let rate = s.avg_frame_rate();
            let (num, den) = if rate.numerator() > 0 && rate.denominator() > 0 {
                (rate.numerator() as u32, rate.denominator() as u32)
            } else {
                (30, 1)
            };
            let rotation = rotation_deg(&s);
            let (dw, dh) = if rotation % 180 == 90 { (h, w) } else { (w, h) };
            Some(VideoStream {
                index: s.index(),
                codec: codec_name(s.parameters().id()),
                width: w,
                height: h,
                display_width: dw,
                display_height: dh,
                fps_num: num,
                fps_den: den,
                rotation_deg: rotation,
            })
        }
        None => None,
    };

    let audio = match ictx.streams().best(ff::media::Type::Audio) {
        Some(s) => {
            // SAFETY: codecpar is valid for the lifetime of the stream.
            let (rate, channels) = unsafe {
                let par = (*s.as_ptr()).codecpar;
                (
                    (*par).sample_rate.max(0) as u32,
                    (*par).ch_layout.nb_channels.max(0) as u16,
                )
            };
            Some(AudioStream {
                index: s.index(),
                codec: codec_name(s.parameters().id()),
                sample_rate: rate,
                channels,
            })
        }
        None => None,
    };

    Ok(MediaAsset {
        path: path.to_path_buf(),
        content_hash: content_hash(path)?,
        duration_ms,
        container: ictx.format().name().to_string(),
        video,
        audio,
    })
}

fn codec_name(id: ff::codec::Id) -> String {
    format!("{id:?}").to_lowercase()
}

/// Clockwise rotation in degrees from the stream's display matrix, normalised to 0/90/180/270.
fn rotation_deg(stream: &ff::format::stream::Stream) -> u16 {
    // SAFETY: the stream and its codec parameters are valid for the lifetime of `stream`;
    // the side data pointer, if found, points at nine i32 values per the FFmpeg ABI.
    let theta = unsafe {
        let par = (*stream.as_ptr()).codecpar;
        let sd = ff::ffi::av_packet_side_data_get(
            (*par).coded_side_data,
            (*par).nb_coded_side_data,
            ff::ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
        );
        if sd.is_null() || (*sd).size < 36 {
            return 0;
        }
        ff::ffi::av_display_rotation_get((*sd).data as *const i32)
    };
    if theta.is_nan() {
        return 0;
    }
    // av_display_rotation_get is counter-clockwise; displays rotate clockwise by -theta.
    let cw = (-theta).rem_euclid(360.0);
    (((cw / 90.0).round() as i64 % 4) * 90) as u16
}

/// blake3 over the file size and its first and last 8 MiB. Fast on multi-gigabyte files.
pub fn content_hash(path: &Path) -> std::io::Result<String> {
    const CHUNK: u64 = 8 * 1024 * 1024;
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut h = blake3::Hasher::new();
    h.update(&len.to_le_bytes());
    let mut buf = vec![0u8; CHUNK as usize];
    let n = read_up_to(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    if len > CHUNK {
        f.seek(SeekFrom::Start(len.saturating_sub(CHUNK).max(CHUNK)))?;
        let n = read_up_to(&mut f, &mut buf)?;
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match f.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

/// Decode the best audio stream, downmix to mono, resample to 16 kHz, write 32-bit float WAV.
/// Returns the number of samples written.
pub fn extract_audio(asset: &MediaAsset, out: &Path, progress: ProgressFn) -> Result<u64> {
    crate::init();
    let mut ictx = ff::format::input(&asset.path).map_err(|source| IngestError::Open {
        path: asset.path.clone(),
        source,
    })?;
    let stream =
        ictx.streams()
            .best(ff::media::Type::Audio)
            .ok_or_else(|| IngestError::NoStream {
                path: asset.path.clone(),
                kind: "audio",
            })?;
    let index = stream.index();
    let time_base = stream.time_base();
    let ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = ctx
        .decoder()
        .audio()
        .map_err(|_| IngestError::Unsupported {
            codec: codec_name(stream.parameters().id()),
            kind: "audio",
        })?;
    if decoder.channel_layout().is_empty() {
        decoder.set_channel_layout(ff::ChannelLayout::default(decoder.channels() as i32));
    }

    let target_fmt = ff::format::Sample::F32(ff::format::sample::Type::Packed);
    let mut resampler: Option<ff::software::resampling::Context> = None;

    let mut writer = WavF32Writer::create(out, SPEECH_SAMPLE_RATE)?;
    let duration = asset.duration_ms.max(1);
    let mut last_report = std::time::Instant::now();
    progress(Progress::new(STAGE, 0.0, "extracting audio"));

    let mut frame = ff::frame::Audio::empty();
    let mut drain = |decoder: &mut ff::decoder::Audio,
                     resampler: &mut Option<ff::software::resampling::Context>,
                     writer: &mut WavF32Writer|
     -> Result<()> {
        while decoder.receive_frame(&mut frame).is_ok() {
            if frame.channel_layout().is_empty() {
                frame.set_channel_layout(ff::ChannelLayout::default(frame.channels() as i32));
            }
            let r = match resampler {
                Some(r) => r,
                None => resampler.insert(ff::software::resampling::Context::get(
                    frame.format(),
                    frame.channel_layout(),
                    frame.rate(),
                    target_fmt,
                    ff::ChannelLayout::MONO,
                    SPEECH_SAMPLE_RATE,
                )?),
            };
            let mut out_frame = ff::frame::Audio::empty();
            r.run(&frame, &mut out_frame)?;
            writer.write_frame(&out_frame)?;
            if let Some(pts) = frame.pts()
                && last_report.elapsed().as_millis() >= 500
            {
                let ms = (pts as f64 * f64::from(time_base) * 1000.0) as u64;
                progress(Progress::new(
                    STAGE,
                    (ms as f32 / duration as f32).min(1.0),
                    "extracting audio",
                ));
                last_report = std::time::Instant::now();
            }
        }
        Ok(())
    };

    for (s, packet) in ictx.packets() {
        if s.index() != index {
            continue;
        }
        decoder.send_packet(&packet)?;
        drain(&mut decoder, &mut resampler, &mut writer)?;
    }
    decoder.send_eof()?;
    drain(&mut decoder, &mut resampler, &mut writer)?;
    if let Some(r) = resampler.as_mut() {
        // Flush samples buffered inside the resampler.
        loop {
            let mut out_frame = ff::frame::Audio::empty();
            match r.flush(&mut out_frame) {
                Ok(_) if out_frame.samples() > 0 => writer.write_frame(&out_frame)?,
                _ => break,
            }
        }
    }
    let n = writer.finish()?;
    progress(Progress::new(STAGE, 1.0, "audio extracted"));
    Ok(n)
}

/// Minimal mono 32-bit float WAV writer. Avoids a dependency for a 44-byte header.
struct WavF32Writer {
    file: BufWriter<File>,
    samples: u64,
    rate: u32,
}

impl WavF32Writer {
    fn create(path: &Path, rate: u32) -> std::io::Result<Self> {
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(&[0u8; 44])?;
        Ok(Self {
            file,
            samples: 0,
            rate,
        })
    }

    fn write_frame(&mut self, f: &ff::frame::Audio) -> std::io::Result<()> {
        let n = f.samples();
        if n == 0 {
            return Ok(());
        }
        let bytes = &f.data(0)[..n * 4];
        self.file.write_all(bytes)?;
        self.samples += n as u64;
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<u64> {
        let data_len = (self.samples * 4) as u32;
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&(36 + data_len).to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        h.extend_from_slice(&1u16.to_le_bytes()); // mono
        h.extend_from_slice(&self.rate.to_le_bytes());
        h.extend_from_slice(&(self.rate * 4).to_le_bytes());
        h.extend_from_slice(&4u16.to_le_bytes());
        h.extend_from_slice(&32u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&data_len.to_le_bytes());
        self.file.flush()?;
        let mut file = self.file.into_inner().map_err(|e| e.into_error())?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&h)?;
        file.flush()?;
        Ok(self.samples)
    }
}

/// Read a mono f32 WAV written by [`extract_audio`].
pub fn read_wav_f32(path: &Path) -> std::io::Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not a WAV file",
        ));
    }
    // Walk chunks to find "data"; our writer puts it at 36 but be tolerant.
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            let data = &bytes[pos + 8..(pos + 8 + len).min(bytes.len())];
            return Ok(data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect());
        }
        pos += 8 + len + (len & 1);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "WAV has no data chunk",
    ))
}

/// Decode the video frame nearest to `t_ms`, apply display rotation, scale to `max_width`, write PNG.
pub fn thumbnail(asset: &MediaAsset, t_ms: u64, max_width: u32, out: &Path) -> Result<()> {
    crate::init();
    let vinfo = asset.video.as_ref().ok_or_else(|| IngestError::NoStream {
        path: asset.path.clone(),
        kind: "video",
    })?;
    let mut ictx = ff::format::input(&asset.path).map_err(|source| IngestError::Open {
        path: asset.path.clone(),
        source,
    })?;
    let stream = ictx
        .stream(vinfo.index)
        .ok_or_else(|| IngestError::NoStream {
            path: asset.path.clone(),
            kind: "video",
        })?;
    let time_base = stream.time_base();
    let ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = ctx
        .decoder()
        .video()
        .map_err(|_| IngestError::Unsupported {
            codec: codec_name(stream.parameters().id()),
            kind: "video",
        })?;

    let target_ts = (t_ms as i64) * ff::ffi::AV_TIME_BASE as i64 / 1000;
    let _ = ictx.seek(target_ts, ..target_ts);
    let target_pts = (t_ms as f64 / 1000.0 / f64::from(time_base)) as i64;

    let mut decoded = ff::frame::Video::empty();
    let mut got: Option<ff::frame::Video> = None;
    'outer: for (s, packet) in ictx.packets() {
        if s.index() != vinfo.index {
            continue;
        }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            let pts = decoded.pts().unwrap_or(i64::MAX);
            got = Some(decoded.clone());
            if pts >= target_pts {
                break 'outer;
            }
        }
    }
    if got.is_none() {
        decoder.send_eof()?;
        if decoder.receive_frame(&mut decoded).is_ok() {
            got = Some(decoded.clone());
        }
    }
    let frame = got.ok_or_else(|| IngestError::NoStream {
        path: asset.path.clone(),
        kind: "decodable video",
    })?;

    let (sw, sh) = (frame.width(), frame.height());
    let rotated = vinfo.rotation_deg % 180 == 90;
    let (disp_w, disp_h) = if rotated { (sh, sw) } else { (sw, sh) };
    let out_w = disp_w.min(max_width).max(2) & !1;
    let out_h = ((disp_h as u64 * out_w as u64 / disp_w as u64) as u32).max(2) & !1;
    // Scale in source orientation, then rotate pixels.
    let (scale_w, scale_h) = if rotated {
        (out_h, out_w)
    } else {
        (out_w, out_h)
    };
    let mut scaler = ff::software::scaling::Context::get(
        frame.format(),
        sw,
        sh,
        ff::format::Pixel::RGB24,
        scale_w,
        scale_h,
        ff::software::scaling::Flags::BILINEAR,
    )?;
    let mut rgb = ff::frame::Video::empty();
    scaler.run(&frame, &mut rgb)?;
    let stride = rgb.stride(0);
    let data = rgb.data(0);
    let mut src = Vec::with_capacity((scale_w * scale_h * 3) as usize);
    for y in 0..scale_h as usize {
        src.extend_from_slice(&data[y * stride..y * stride + scale_w as usize * 3]);
    }
    let pixels = rotate_rgb(&src, scale_w, scale_h, vinfo.rotation_deg);

    let file = BufWriter::new(File::create(out)?);
    let mut enc = png::Encoder::new(file, out_w, out_h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

/// Rotate a packed RGB24 image clockwise by 0/90/180/270 degrees.
fn rotate_rgb(src: &[u8], w: u32, h: u32, deg: u16) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let px = |x: usize, y: usize| &src[(y * w + x) * 3..(y * w + x) * 3 + 3];
    let mut out = Vec::with_capacity(src.len());
    match deg {
        90 => {
            for y in 0..w {
                for x in 0..h {
                    out.extend_from_slice(px(y, h - 1 - x));
                }
            }
        }
        180 => {
            for y in 0..h {
                for x in 0..w {
                    out.extend_from_slice(px(w - 1 - x, h - 1 - y));
                }
            }
        }
        270 => {
            for y in 0..w {
                for x in 0..h {
                    out.extend_from_slice(px(w - 1 - y, x));
                }
            }
        }
        _ => out.extend_from_slice(src),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_rgb_90_moves_corners() {
        // 2x1 image: red, green. Rotated 90° clockwise becomes 1x2: red on top, green below.
        let src = [255, 0, 0, 0, 255, 0];
        assert_eq!(rotate_rgb(&src, 2, 1, 90), vec![255, 0, 0, 0, 255, 0]);
        // 1x2 image: red top, green bottom. Rotated 90° clockwise becomes 2x1: green, red.
        assert_eq!(rotate_rgb(&src, 1, 2, 90), vec![0, 255, 0, 255, 0, 0]);
        assert_eq!(rotate_rgb(&src, 2, 1, 180), vec![0, 255, 0, 255, 0, 0]);
        assert_eq!(rotate_rgb(&src, 1, 2, 270), vec![255, 0, 0, 0, 255, 0]);
    }

    #[test]
    fn wav_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        let mut w = WavF32Writer::create(&p, 16_000).unwrap();
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0) - 0.5).collect();
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        w.file.write_all(&bytes).unwrap();
        w.samples = samples.len() as u64;
        assert_eq!(w.finish().unwrap(), 1000);
        assert_eq!(read_wav_f32(&p).unwrap(), samples);
    }

    #[test]
    fn content_hash_is_stable_and_size_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::write(&a, b"hello").unwrap();
        let h1 = content_hash(&a).unwrap();
        assert_eq!(h1, content_hash(&a).unwrap());
        std::fs::write(&a, b"hello!").unwrap();
        assert_ne!(h1, content_hash(&a).unwrap());
    }
}

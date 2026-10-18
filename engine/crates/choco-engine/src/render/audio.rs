//! Clip audio: sample-accurate trim, optional loudness normalisation to -14 LUFS, 48 kHz
//! stereo AAC. Encoded up front (a few MB at most) so the muxer can interleave it with video.

use super::RenderError;
use crate::model::MediaAsset;
use ffmpeg_next as ff;
use ff::{ChannelLayout, Dictionary, Packet, Rational, codec, filter, format};

pub const SAMPLE_RATE: i32 = 48_000;

pub struct EncodedAudio {
    pub encoder: ff::encoder::Audio,
    pub packets: Vec<Packet>,
    pub time_base: Rational,
}

/// Seconds offset of a stream's first timestamp, so clip times line up with transcript times
/// (which start at the first decoded audio sample).
pub(crate) fn stream_start_secs(s: &ff::format::stream::Stream) -> f64 {
    let st = s.start_time();
    if st == ff::ffi::AV_NOPTS_VALUE { 0.0 } else { st as f64 * f64::from(s.time_base()) }
}

pub fn encode(asset: &MediaAsset, start_ms: u64, end_ms: u64, normalize: bool) -> Result<Option<EncodedAudio>, RenderError> {
    crate::init();
    let mut ictx = ff::format::input(&asset.path)?;
    let Some(stream) = ictx.streams().best(ff::media::Type::Audio) else { return Ok(None) };
    let index = stream.index();
    let tb = stream.time_base();
    let base = stream_start_secs(&stream);
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())?
        .decoder()
        .audio()
        .map_err(|_| RenderError::Unsupported(format!("audio codec {:?}", stream.parameters().id())))?;
    if decoder.channel_layout().is_empty() {
        decoder.set_channel_layout(ChannelLayout::default(decoder.channels() as i32));
    }

    let codec = ff::encoder::find_by_name("aac").ok_or_else(|| RenderError::Unsupported("aac encoder missing".into()))?;
    let mut enc = codec::context::Context::new_with_codec(codec).encoder().audio()?;
    enc.set_rate(SAMPLE_RATE);
    enc.set_channel_layout(ChannelLayout::STEREO);
    enc.set_format(format::Sample::F32(format::sample::Type::Planar));
    enc.set_bit_rate(192_000);
    enc.set_time_base(Rational(1, SAMPLE_RATE));
    enc.set_flags(codec::Flags::GLOBAL_HEADER);
    let mut encoder = enc.open_with(Dictionary::new())?;

    let (s, e) = (base + start_ms as f64 / 1000.0, base + end_ms as f64 / 1000.0);
    let mut spec = format!("atrim=start={s:.6}:end={e:.6},asetpts=PTS-STARTPTS");
    if normalize {
        // Single-pass dynamic loudnorm; it upsamples internally, so resample after it.
        spec.push_str(",loudnorm=I=-14:TP=-1.5:LRA=11");
    }
    spec.push_str(",aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo");
    let mut graph = filter::Graph::new();
    let args = format!(
        "time_base={}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        tb,
        decoder.rate(),
        decoder.format().name(),
        decoder.channel_layout().bits()
    );
    graph.add(&filter::find("abuffer").ok_or(ff::Error::FilterNotFound)?, "in", &args)?;
    graph.add(&filter::find("abuffersink").ok_or(ff::Error::FilterNotFound)?, "out", "")?;
    graph.output("in", 0)?.input("out", 0)?.parse(&spec)?;
    graph.validate()?;
    graph.get("out").expect("sink").sink().set_frame_size(encoder.frame_size());

    let seek_to = ((start_ms as f64 / 1000.0 + base - 2.0).max(0.0) * ff::ffi::AV_TIME_BASE as f64) as i64;
    let _ = ictx.seek(seek_to, ..seek_to);

    let mut packets = Vec::new();
    let mut samples: i64 = 0;
    let mut decoded = ff::frame::Audio::empty();
    let mut filtered = ff::frame::Audio::empty();
    let drain_encoder = |encoder: &mut ff::encoder::Audio, packets: &mut Vec<Packet>| {
        let mut p = Packet::empty();
        while encoder.receive_packet(&mut p).is_ok() {
            packets.push(p.clone());
        }
    };
    // Send one frame, collecting packets whenever the encoder asks for room (EAGAIN).
    let send = |encoder: &mut ff::encoder::Audio, frame: &ff::frame::Audio, packets: &mut Vec<Packet>| -> Result<(), ff::Error> {
        loop {
            match encoder.send_frame(frame) {
                Err(ff::Error::Other { errno }) if errno == ff::util::error::EAGAIN => drain_encoder(encoder, packets),
                other => return other,
            }
        }
    };
    let pull = |graph: &mut filter::Graph, encoder: &mut ff::encoder::Audio, filtered: &mut ff::frame::Audio, samples: &mut i64, packets: &mut Vec<Packet>| -> Result<(), ff::Error> {
        while graph.get("out").expect("sink").sink().frame(filtered).is_ok() {
            filtered.set_pts(Some(*samples));
            *samples += filtered.samples() as i64;
            send(encoder, filtered, packets)?;
            drain_encoder(encoder, packets);
        }
        Ok(())
    };

    let stop_after = e + 1.0;
    // Once atrim has passed the clip end the graph closes and add() reports EOF: we are done.
    let mut graph_done = false;
    'read: for (st, packet) in ictx.packets() {
        if st.index() != index {
            continue;
        }
        if let Some(pts) = packet.pts()
            && pts as f64 * f64::from(tb) > stop_after
        {
            break;
        }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            if decoded.channel_layout().is_empty() {
                decoded.set_channel_layout(ChannelLayout::default(decoded.channels() as i32));
            }
            match graph.get("in").expect("source").source().add(&decoded) {
                Err(ff::Error::Eof) => {
                    graph_done = true;
                    break 'read;
                }
                other => other?,
            }
            pull(&mut graph, &mut encoder, &mut filtered, &mut samples, &mut packets)?;
        }
    }
    if !graph_done {
        decoder.send_eof()?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            if let Err(ff::Error::Eof) = graph.get("in").expect("source").source().add(&decoded) {
                break;
            }
        }
        match graph.get("in").expect("source").source().flush() {
            Ok(()) | Err(ff::Error::Eof) => {}
            Err(e) => return Err(e.into()),
        }
    }
    pull(&mut graph, &mut encoder, &mut filtered, &mut samples, &mut packets)?;
    encoder.send_eof()?;
    drain_encoder(&mut encoder, &mut packets);
    Ok(Some(EncodedAudio { encoder, packets, time_base: Rational(1, SAMPLE_RATE) }))
}

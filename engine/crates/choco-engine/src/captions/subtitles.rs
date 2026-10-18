//! SRT and WebVTT sidecars from a caption track. Times are relative to the clip start.

use crate::model::CaptionTrack;

fn ts(ms: u64, sep: char) -> String {
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
}

fn text(c: &crate::model::CaptionChunk) -> String {
    c.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
}

pub fn srt(track: &CaptionTrack, clip_start_ms: u64) -> String {
    let mut out = String::new();
    for (i, c) in track.chunks.iter().enumerate() {
        let (a, b) = (c.start_ms.saturating_sub(clip_start_ms), c.end_ms.saturating_sub(clip_start_ms));
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, ts(a, ','), ts(b, ','), text(c)));
    }
    out
}

pub fn vtt(track: &CaptionTrack, clip_start_ms: u64) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for c in &track.chunks {
        let (a, b) = (c.start_ms.saturating_sub(clip_start_ms), c.end_ms.saturating_sub(clip_start_ms));
        out.push_str(&format!("{} --> {}\n{}\n\n", ts(a, '.'), ts(b, '.'), text(c)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CaptionChunk, Word};

    #[test]
    fn formats_relative_times() {
        let track = CaptionTrack {
            style: crate::captions::preset("clean").unwrap(),
            chunks: vec![CaptionChunk { start_ms: 61_500, end_ms: 62_250, words: vec![Word { start_ms: 61_500, end_ms: 62_000, text: "Hello".into(), confidence: 1.0 }] }],
        };
        assert_eq!(srt(&track, 60_000), "1\n00:00:01,500 --> 00:00:02,250\nHello\n\n");
        assert_eq!(vtt(&track, 60_000), "WEBVTT\n\n00:00:01.500 --> 00:00:02.250\nHello\n\n");
    }
}

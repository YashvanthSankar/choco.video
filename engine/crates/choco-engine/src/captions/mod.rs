//! Captions: group words into short on-screen chunks, rasterize them identically on every OS,
//! and export SRT / VTT sidecars.

use crate::model::{CaptionChunk, CaptionStyle, CaptionTrack, Transcript, Word};

pub mod raster;
pub mod subtitles;

pub use raster::{Rasterizer, Sprite};

/// The four built-in styles. Sizes are percent of frame height, positions percent from the bottom.
pub fn presets() -> Vec<CaptionStyle> {
    let base = CaptionStyle {
        name: String::new(),
        font_family: "Inter".into(),
        font_weight: 800,
        highlight: "word".into(),
        size_pct: 4.6,
        position_pct: 22.0,
        fill: "#FFFFFF".into(),
        stroke: Some("#000000".into()),
        shadow: true,
        active_fill: None,
        active_scale: 1.0,
        active_pill: None,
        max_lines: 2,
        uppercase: false,
    };
    vec![
        CaptionStyle { name: "clean".into(), ..base.clone() },
        CaptionStyle {
            name: "bold".into(),
            font_family: "Montserrat".into(),
            font_weight: 900,
            size_pct: 5.4,
            position_pct: 24.0,
            active_fill: Some("#FFE14D".into()),
            active_scale: 1.08,
            uppercase: true,
            ..base.clone()
        },
        CaptionStyle {
            name: "karaoke".into(),
            font_family: "Montserrat".into(),
            font_weight: 800,
            highlight: "progressive".into(),
            size_pct: 5.0,
            position_pct: 24.0,
            active_fill: Some("#3DFF7A".into()),
            uppercase: true,
            ..base.clone()
        },
        CaptionStyle {
            name: "minimal".into(),
            font_weight: 600,
            size_pct: 3.6,
            position_pct: 12.0,
            stroke: None,
            active_pill: Some("#000000B3".into()),
            ..base
        },
    ]
}

pub fn preset(name: &str) -> Option<CaptionStyle> {
    presets().into_iter().find(|s| s.name == name)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkOptions {
    pub max_words: usize,
    /// A pause this long starts a new chunk.
    pub gap_ms: u64,
    /// Keep a chunk on screen this long after its last word when nothing follows soon.
    pub tail_ms: u64,
    /// Text block width as a share of frame width, percent.
    pub width_pct: u8,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self { max_words: 4, gap_ms: 350, tail_ms: 400, width_pct: 86 }
    }
}

fn ends_clause(w: &str) -> bool {
    matches!(w.trim_end_matches(['"', '\'', ')', '”', '’']).chars().last(), Some('.' | '!' | '?' | '…' | ',' | ';' | ':'))
}

/// Words of `t` inside `[start_ms, end_ms)`, grouped into chunks that fit the frame width.
/// `range` is `(start_ms, end_ms)` in source time; `frame` is `(width, height)` in pixels.
pub fn chunk(t: &Transcript, range: (u64, u64), style: &CaptionStyle, frame: (u32, u32), raster: &mut Rasterizer, opts: &ChunkOptions) -> CaptionTrack {
    let ((start_ms, end_ms), (frame_w, frame_h)) = (range, frame);
    let words: Vec<&Word> = t
        .segments
        .iter()
        .flat_map(|s| s.words.iter())
        .filter(|w| w.start_ms >= start_ms && w.start_ms < end_ms && !w.text.trim().is_empty())
        .collect();
    let max_w = frame_w as f32 * opts.width_pct as f32 / 100.0;
    let mut chunks: Vec<CaptionChunk> = Vec::new();
    let mut cur: Vec<Word> = Vec::new();
    for w in words {
        if let Some(last) = cur.last() {
            let gap = w.start_ms.saturating_sub(last.end_ms);
            let mut candidate: Vec<&str> = cur.iter().map(|x| x.text.as_str()).collect();
            candidate.push(&w.text);
            let too_wide = raster.measure(&candidate.join(" "), style, frame_h) > max_w;
            if cur.len() >= opts.max_words || ends_clause(&last.text) || gap >= opts.gap_ms || too_wide {
                chunks.push(CaptionChunk { start_ms: cur[0].start_ms, end_ms: 0, words: std::mem::take(&mut cur) });
            }
        }
        cur.push(w.clone());
    }
    if !cur.is_empty() {
        chunks.push(CaptionChunk { start_ms: cur[0].start_ms, end_ms: 0, words: cur });
    }
    // A chunk stays up until the next one starts, unless the pause is long.
    for i in 0..chunks.len() {
        let last_end = chunks[i].words.last().map(|w| w.end_ms).unwrap_or(chunks[i].start_ms);
        let hold = (last_end + opts.tail_ms).min(end_ms);
        chunks[i].end_ms = match chunks.get(i + 1) {
            Some(next) if next.start_ms <= hold => next.start_ms,
            _ => hold,
        }
        .max(chunks[i].start_ms + 1);
    }
    CaptionTrack { style: style.clone(), chunks }
}

/// Index of the word being spoken at `t_ms` within a chunk, if any.
pub fn active_word(chunk: &CaptionChunk, t_ms: u64) -> Option<usize> {
    if t_ms < chunk.start_ms || t_ms >= chunk.end_ms {
        return None;
    }
    let mut idx = None;
    for (i, w) in chunk.words.iter().enumerate() {
        if t_ms >= w.start_ms {
            idx = Some(i);
        }
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Segment;

    fn transcript(words: &[(&str, u64, u64)]) -> Transcript {
        let ws: Vec<Word> = words.iter().map(|(t, s, e)| Word { start_ms: *s, end_ms: *e, text: t.to_string(), confidence: 1.0 }).collect();
        Transcript {
            language: "en".into(),
            engine: "t".into(),
            segments: vec![Segment { start_ms: ws[0].start_ms, end_ms: ws.last().unwrap().end_ms, text: String::new(), words: ws }],
        }
    }

    #[test]
    fn chunks_respect_size_punctuation_and_gaps() {
        let t = transcript(&[
            ("Charge", 0, 300), ("more", 300, 500), ("than", 500, 700), ("you", 700, 800), ("think.", 800, 1100),
            ("Then", 1200, 1400), ("wait", 1400, 1600),
            ("here", 2600, 2800), // long gap before
        ]);
        let mut r = Rasterizer::new();
        // Small text so only punctuation, pauses and the 4-word cap decide the breaks.
        let small = CaptionStyle { size_pct: 2.5, ..preset("clean").unwrap() };
        let track = chunk(&t, (0, 10_000), &small, (1080, 1920), &mut r, &ChunkOptions::default());
        let texts: Vec<String> = track.chunks.iter().map(|c| c.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")).collect();
        assert_eq!(texts, vec!["Charge more than you", "think.", "Then wait", "here"]);
        // Contiguous chunks hand over exactly; after a long pause the chunk ends after a short tail.
        assert_eq!(track.chunks[0].end_ms, track.chunks[1].start_ms);
        assert_eq!(track.chunks[2].end_ms, 1600 + 400);
    }

    #[test]
    fn active_word_tracks_time() {
        let c = CaptionChunk { start_ms: 0, end_ms: 1000, words: vec![
            Word { start_ms: 0, end_ms: 300, text: "a".into(), confidence: 1.0 },
            Word { start_ms: 400, end_ms: 900, text: "b".into(), confidence: 1.0 },
        ] };
        assert_eq!(active_word(&c, 100), Some(0));
        assert_eq!(active_word(&c, 350), Some(0));
        assert_eq!(active_word(&c, 500), Some(1));
        assert_eq!(active_word(&c, 1000), None);
    }

    #[test]
    fn big_bold_style_splits_on_width() {
        let t = transcript(&[("Charge", 0, 300), ("more", 300, 500), ("than", 500, 700), ("you", 700, 800), ("think.", 800, 1100)]);
        let mut r = Rasterizer::new();
        let track = chunk(&t, (0, 10_000), &preset("bold").unwrap(), (1080, 1920), &mut r, &ChunkOptions::default());
        for c in &track.chunks {
            let text = c.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            assert!(r.measure(&text, &track.style, 1920) <= 1080.0 * 0.86, "{text}");
        }
    }

    /// Writes preview PNGs of every preset when CHOCO_PREVIEW_DIR is set (manual visual check).
    #[test]
    fn preview_pngs() {
        let Some(dir) = std::env::var_os("CHOCO_PREVIEW_DIR") else { return };
        let mut r = Rasterizer::new();
        let c = CaptionChunk { start_ms: 0, end_ms: 1000, words: ["Charge", "more,", "not", "less."].iter().enumerate().map(|(i, t)| Word { start_ms: i as u64 * 200, end_ms: i as u64 * 200 + 150, text: t.to_string(), confidence: 1.0 }).collect() };
        for st in presets() {
            let s = r.render(&c, Some(1), &st, 1080, 1920);
            // Composite on a mid-grey 1080x480 strip around the caption for viewing.
            let (fw, fh) = (1080u32, s.h + 40);
            let mut img = vec![90u8; (fw * fh * 3) as usize];
            for y in 0..s.h {
                for x in 0..s.w {
                    let (dx, dy) = (s.x + x as i32, 20 + y as i32);
                    if dx < 0 || dx >= fw as i32 { continue; }
                    let si = ((y * s.w + x) * 4) as usize;
                    let a = s.rgba[si + 3] as u32;
                    let di = ((dy as u32 * fw + dx as u32) * 3) as usize;
                    for k in 0..3 { img[di + k] = (s.rgba[si + k] as u32 + img[di + k] as u32 * (255 - a) / 255) as u8; }
                }
            }
            let f = std::fs::File::create(std::path::Path::new(&dir).join(format!("{}.png", st.name))).unwrap();
            let mut e = png::Encoder::new(f, fw, fh);
            e.set_color(png::ColorType::Rgb);
            e.write_header().unwrap().write_image_data(&img).unwrap();
        }
    }

    #[test]
    fn narrow_frames_force_shorter_chunks() {
        let t = transcript(&[("Extraordinarily", 0, 500), ("unbelievable", 500, 1000), ("circumstances", 1000, 1500), ("everywhere", 1500, 2000)]);
        let mut r = Rasterizer::new();
        let wide = chunk(&t, (0, 5000), &preset("bold").unwrap(), (1920, 1080), &mut r, &ChunkOptions::default());
        let narrow = chunk(&t, (0, 5000), &preset("bold").unwrap(), (540, 1920), &mut r, &ChunkOptions::default());
        assert!(narrow.chunks.len() > wide.chunks.len(), "{} vs {}", narrow.chunks.len(), wide.chunks.len());
    }
}

//! Candidate windows: every clip starts at a sentence start and ends at a sentence end
//! ("complete thought"), 15–90 s long, with cheap pre-filters before any model sees them.

use crate::model::{Segment, Transcript};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowOptions {
    pub min_ms: u64,
    pub max_ms: u64,
    /// Target lengths: from each start, emit the first valid end at or after each of these.
    pub targets_ms: Vec<u64>,
    /// A segment ending with no punctuation still closes a thought if this much silence follows.
    pub boundary_gap_ms: u64,
    pub min_words: usize,
    pub max_silence_ratio_pct: u8,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            min_ms: 15_000,
            max_ms: 90_000,
            targets_ms: vec![15_000, 30_000, 45_000, 60_000],
            boundary_gap_ms: 700,
            min_words: 20,
            max_silence_ratio_pct: 25,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Inclusive segment index range.
    pub first_seg: usize,
    pub last_seg: usize,
    pub text: String,
    pub word_count: usize,
    /// Fraction of the window not covered by any word.
    pub silence_ratio: f32,
}

impl Window {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms - self.start_ms
    }
}

pub(crate) fn ends_thought(text: &str) -> bool {
    let t = text.trim_end_matches(['"', '\'', ')', '”', '’', ' ']);
    matches!(t.chars().last(), Some('.' | '!' | '?' | '…' | '。' | '！' | '？'))
}

const FILLERS: &[&str] = &["um", "uh", "erm", "hmm", "mm", "like", "yeah", "okay", "ok", "you", "know", "i", "mean", "so"];

/// True when the first three seconds are nothing but filler words.
fn filler_opening(segs: &[Segment], start_ms: u64) -> bool {
    let words: Vec<String> = segs
        .iter()
        .flat_map(|s| s.words.iter())
        .take_while(|w| w.start_ms < start_ms + 3_000)
        .map(|w| w.text.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    !words.is_empty() && words.iter().all(|w| FILLERS.contains(&w.as_str()))
}

pub fn windows(t: &Transcript, opts: &WindowOptions) -> Vec<Window> {
    let segs = &t.segments;
    let n = segs.len();
    // A boundary after segment i: punctuation, or a real pause before the next segment.
    let closes = |i: usize| -> bool {
        ends_thought(&segs[i].text) || i + 1 == n || segs[i + 1].start_ms.saturating_sub(segs[i].end_ms) >= opts.boundary_gap_ms
    };
    let starts = |i: usize| i == 0 || closes(i - 1);

    let mut out = Vec::new();
    for i in (0..n).filter(|&i| starts(i)) {
        let start = segs[i].start_ms;
        let mut emitted_targets = 0usize;
        let mut last_emitted_end = None;
        for j in i..n {
            let end = segs[j].end_ms;
            let dur = end.saturating_sub(start);
            if dur > opts.max_ms {
                break;
            }
            if dur < opts.min_ms || !closes(j) {
                continue;
            }
            // Emit once per target length we have reached and not yet covered.
            let reached = opts.targets_ms.iter().filter(|&&t| dur >= t).count();
            if reached > emitted_targets && last_emitted_end != Some(j) {
                emitted_targets = reached;
                last_emitted_end = Some(j);
                if let Some(w) = build(segs, i, j, opts) {
                    out.push(w);
                }
                if emitted_targets == opts.targets_ms.len() {
                    break;
                }
            }
        }
    }
    for (k, w) in out.iter_mut().enumerate() {
        w.id = format!("w{k}");
    }
    out
}

fn build(segs: &[Segment], i: usize, j: usize, opts: &WindowOptions) -> Option<Window> {
    let span = &segs[i..=j];
    let start_ms = span[0].start_ms;
    let end_ms = span[span.len() - 1].end_ms;
    let dur = (end_ms - start_ms).max(1);
    let word_count: usize = span.iter().map(|s| s.words.len()).sum();
    if word_count < opts.min_words {
        return None;
    }
    // Union of word intervals, to measure dead air inside the window.
    let mut covered = 0u64;
    let mut cursor = start_ms;
    for w in span.iter().flat_map(|s| s.words.iter()) {
        let (a, b) = (w.start_ms.max(cursor), w.end_ms.min(end_ms));
        if b > a {
            covered += b - a;
            cursor = b;
        }
    }
    let silence_ratio = 1.0 - covered as f32 / dur as f32;
    if silence_ratio * 100.0 > opts.max_silence_ratio_pct as f32 {
        return None;
    }
    if filler_opening(span, start_ms) {
        return None;
    }
    let text = span.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join(" ");
    Some(Window { id: String::new(), start_ms, end_ms, first_seg: i, last_seg: j, text, word_count, silence_ratio })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::Word;

    /// A transcript of `n` sentences, each `len_ms` long with `words` words, `gap_ms` apart.
    pub(crate) fn synthetic(n: usize, len_ms: u64, words: usize, gap_ms: u64) -> Transcript {
        let mut segs = Vec::new();
        let mut t = 0;
        for s in 0..n {
            let step = len_ms / words as u64;
            let ws: Vec<Word> = (0..words)
                .map(|k| Word {
                    start_ms: t + k as u64 * step,
                    end_ms: t + (k as u64 + 1) * step,
                    text: if k + 1 == words { format!("end{s}.") } else { format!("w{s}x{k}") },
                    confidence: 1.0,
                })
                .collect();
            let text = ws.iter().map(|w| w.text.clone()).collect::<Vec<_>>().join(" ");
            segs.push(Segment { start_ms: t, end_ms: t + len_ms, text, words: ws });
            t += len_ms + gap_ms;
        }
        Transcript { language: "en".into(), engine: "test".into(), segments: segs }
    }

    #[test]
    fn every_window_is_a_complete_thought_within_bounds() {
        let t = synthetic(40, 6_000, 15, 200);
        let ws = windows(&t, &WindowOptions::default());
        assert!(!ws.is_empty());
        for w in &ws {
            assert!(w.duration_ms() >= 15_000 && w.duration_ms() <= 90_000, "{w:?}");
            assert!(ends_thought(&t.segments[w.last_seg].text));
            assert_eq!(w.start_ms, t.segments[w.first_seg].start_ms);
            assert!(w.first_seg == 0 || ends_thought(&t.segments[w.first_seg - 1].text));
        }
        // Several lengths per start, not one.
        let from_zero = ws.iter().filter(|w| w.first_seg == 0).count();
        assert_eq!(from_zero, 4);
    }

    #[test]
    fn never_starts_or_ends_mid_sentence() {
        // Segments without punctuation and tiny gaps cannot be boundaries.
        let mut t = synthetic(30, 5_000, 14, 100);
        for (k, s) in t.segments.iter_mut().enumerate() {
            if k % 3 != 2 {
                s.text = s.text.trim_end_matches('.').to_string();
            }
        }
        for w in windows(&t, &WindowOptions::default()) {
            assert_eq!(w.last_seg % 3, 2, "ends mid-sentence: {w:?}");
            assert!(w.first_seg % 3 == 0, "starts mid-sentence: {w:?}");
        }
    }

    #[test]
    fn drops_sparse_and_silent_windows() {
        // 3 words per 6 s sentence: too few words and mostly silence.
        let t = synthetic(30, 6_000, 3, 200);
        let mut sparse = t.clone();
        for s in &mut sparse.segments {
            for w in &mut s.words {
                w.end_ms = w.start_ms + 100;
            }
        }
        assert!(windows(&sparse, &WindowOptions::default()).is_empty());
    }
}

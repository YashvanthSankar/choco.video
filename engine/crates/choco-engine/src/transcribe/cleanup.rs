//! Shared cleanup applied to every engine's output so downstream sees one behaviour:
//! drop hallucinated repeats, clamp word times, re-split into sentence-sized segments.

use crate::model::{Segment, Transcript, Word};

/// Gap that ends a segment even without punctuation.
const SILENCE_SPLIT_MS: u64 = 600;
/// Segments longer than this are split at the next word boundary.
const MAX_SEGMENT_MS: u64 = 30_000;

pub fn clean(t: Transcript) -> Transcript {
    let mut words: Vec<Word> = t
        .segments
        .into_iter()
        .flat_map(|s| clamp_words(s.words, s.start_ms, s.end_ms))
        .filter(|w| !w.text.trim().is_empty())
        .collect();
    words.sort_by_key(|w| w.start_ms);
    // Whisper loops whole sentences on real audio (measured: one 8-word sentence 174 times in a
    // row on a 72-minute podcast). Catch periods up to 16 words.
    let words = drop_repeated_ngrams(words, 16, 3);
    Transcript { language: t.language, engine: t.engine, segments: resegment(words) }
}

fn clamp_words(words: Vec<Word>, seg_start: u64, seg_end: u64) -> Vec<Word> {
    let mut prev_end = seg_start;
    words
        .into_iter()
        .map(|mut w| {
            w.start_ms = w.start_ms.clamp(seg_start.min(prev_end), seg_end.max(seg_start));
            w.end_ms = w.end_ms.clamp(w.start_ms, seg_end.max(w.start_ms));
            if w.end_ms == w.start_ms {
                w.end_ms = w.start_ms + 1;
            }
            prev_end = w.end_ms;
            w.text = w.text.trim().to_string();
            w
        })
        .collect()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Whisper loops on silence: "thank you thank you thank you ...". When the same `n`-gram
/// appears `times` or more in a row, keep the first occurrence only.
pub(crate) fn drop_repeated_ngrams(words: Vec<Word>, n: usize, times: usize) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let mut dropped = false;
        // Shortest period first, so six repeats of "thank you" collapse to one, not two.
        for k in 1..=n {
            if i + k * times > words.len() {
                continue;
            }
            let gram: Vec<String> = words[i..i + k].iter().map(|w| norm(&w.text)).collect();
            if gram.iter().all(String::is_empty) {
                continue;
            }
            let mut reps = 1;
            while i + (reps + 1) * k <= words.len()
                && words[i + reps * k..i + (reps + 1) * k].iter().map(|w| norm(&w.text)).eq(gram.iter().cloned())
            {
                reps += 1;
            }
            if reps >= times {
                out.extend_from_slice(&words[i..i + k]);
                i += reps * k;
                dropped = true;
                break;
            }
        }
        if !dropped {
            out.push(words[i].clone());
            i += 1;
        }
    }
    out
}

fn ends_sentence(text: &str) -> bool {
    matches!(text.trim_end_matches(['"', '\'', ')', '”', '’']).chars().last(), Some('.' | '!' | '?' | '…' | '。' | '！' | '？'))
}

fn join_words(words: &[Word]) -> String {
    let mut s = String::new();
    for w in words {
        let attach = w.text.starts_with([',', '.', '!', '?', ';', ':', '\'', '’', '%', ')']);
        if !s.is_empty() && !attach {
            s.push(' ');
        }
        s.push_str(&w.text);
    }
    s
}

pub(crate) fn resegment(words: Vec<Word>) -> Vec<Segment> {
    let mut segs = Vec::new();
    let mut cur: Vec<Word> = Vec::new();
    for w in words {
        if let Some(last) = cur.last() {
            let gap = w.start_ms.saturating_sub(last.end_ms);
            let long = w.start_ms.saturating_sub(cur[0].start_ms) > MAX_SEGMENT_MS;
            if ends_sentence(&last.text) || gap >= SILENCE_SPLIT_MS || long {
                segs.push(make_segment(std::mem::take(&mut cur)));
            }
        }
        cur.push(w);
    }
    if !cur.is_empty() {
        segs.push(make_segment(cur));
    }
    segs
}

fn make_segment(words: Vec<Word>) -> Segment {
    Segment { start_ms: words[0].start_ms, end_ms: words.last().unwrap().end_ms, text: join_words(&words), words }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: u64, e: u64, t: &str) -> Word {
        Word { start_ms: s, end_ms: e, text: t.into(), confidence: 1.0 }
    }

    #[test]
    fn drops_looping_hallucination() {
        let mut v = vec![w(0, 100, "So")];
        for i in 0..6 {
            v.push(w(100 + i * 200, 200 + i * 200, "thank"));
            v.push(w(200 + i * 200, 300 + i * 200, "you."));
        }
        v.push(w(5000, 5100, "Bye"));
        let out = drop_repeated_ngrams(v, 4, 3);
        let text: Vec<_> = out.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(text, vec!["So", "thank", "you.", "Bye"]);
    }

    #[test]
    fn drops_looping_sentence() {
        let sentence = ["And", "so", "we", "have", "the", "ability", "to", "do", "stuff."];
        let mut v = vec![w(0, 100, "Before")];
        let mut t = 100;
        for _ in 0..174 {
            for word in sentence {
                v.push(w(t, t + 10, word));
                t += 10;
            }
        }
        v.push(w(t, t + 100, "After"));
        let out = drop_repeated_ngrams(v, 16, 3);
        let text: Vec<_> = out.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(text.len(), 11, "{text:?}");
        assert_eq!(text.first(), Some(&"Before"));
        assert_eq!(text.last(), Some(&"After"));
    }

    #[test]
    fn keeps_legit_double_words() {
        let v = vec![w(0, 100, "very"), w(100, 200, "very"), w(200, 300, "good")];
        assert_eq!(drop_repeated_ngrams(v.clone(), 4, 3), v);
    }

    #[test]
    fn resegments_on_punctuation_and_silence() {
        let v = vec![
            w(0, 200, "Hello"), w(200, 400, "there."),
            w(500, 700, "This"), w(700, 900, "is"),
            w(2000, 2200, "after"), w(2200, 2400, "a"), w(2400, 2600, "pause"),
        ];
        let segs = resegment(v);
        let texts: Vec<_> = segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["Hello there.", "This is", "after a pause"]);
        assert_eq!(segs[2].start_ms, 2000);
    }

    #[test]
    fn clamps_and_orders_word_times() {
        let t = Transcript {
            language: "en".into(),
            engine: "t".into(),
            segments: vec![Segment {
                start_ms: 1000,
                end_ms: 2000,
                text: String::new(),
                words: vec![w(900, 1200, "a"), w(1300, 2500, "b"), w(1500, 1500, "c")],
            }],
        };
        let out = clean(t);
        let ws = &out.segments.iter().flat_map(|s| s.words.clone()).collect::<Vec<_>>();
        assert!(ws.iter().all(|w| w.start_ms >= 1000 && w.end_ms <= 2000 && w.end_ms > w.start_ms));
    }

    #[test]
    fn joins_punctuation_without_spaces() {
        let segs = resegment(vec![w(0, 1, "Wait"), w(1, 2, ","), w(2, 3, "what?")]);
        assert_eq!(segs[0].text, "Wait, what?");
    }
}

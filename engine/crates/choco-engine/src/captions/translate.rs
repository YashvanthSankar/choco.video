//! Caption translation through the user's picker model (their provider or Apple on-device), so
//! nothing new is downloaded and transcript text goes only where it already goes for scoring.
//!
//! Segments are sent as numbered lines and must come back one line per number. Each translated
//! segment keeps its source start and end; its words are spread over the time the speaker was
//! actually talking, in proportion to their length, so word-timed styles (karaoke) still move.
//! The word-level highlight is approximate by nature: word order differs between languages.
//!
//! Scripts: Japanese and Chinese have no spaces, so [`split_words`] cuts them into short
//! character groups and caption chunks join those without spaces. Thai, Lao, Khmer and Burmese
//! also go unspaced and are not split: a whole phrase becomes one caption word. The SRT and VTT
//! sidecars are plain UTF-8 and correct in every script. Burned-in captions are not: the
//! rasterizer loads only the bundled Inter and Montserrat, Latin-only subsets with no system
//! fallback, so Cyrillic, Greek, CJK, Hangul, Arabic, Hebrew, Devanagari and Thai draw as empty
//! boxes ([`Rasterizer::can_draw`](super::Rasterizer::can_draw) tells which). Right-to-left
//! text is shaped and reordered by cosmic-text (it marks Arabic and Hebrew runs RTL), but with
//! no glyphs that has not been seen on screen, and the active-word highlight has never been
//! checked against RTL order. So the pipeline writes such a clip's captions to the sidecars
//! only and does not burn them in, until fonts for those scripts are an optional download.

use super::{is_cjk, join_words};
use crate::highlights::picker::{self, Llm, PickerError, Role};
use crate::model::{Segment, Transcript, Word};
use std::ops::Range;

#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("translate: no target language given")]
    NoLanguage,
    #[error("translate: {0}")]
    Picker(#[from] PickerError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Translation {
    /// The clip range only, in the target language; `language` is the target as given.
    pub transcript: Transcript,
    /// Indexes into `transcript.segments` that kept their original text and word timings
    /// because the model's line was missing, empty or implausible, or it declined the batch.
    pub untranslated: Vec<usize>,
    /// Model calls made.
    pub calls: usize,
}

/// Lines per call, however large the context: past this, models start merging or skipping lines.
const MAX_LINES: usize = 40;
/// Gaps between source words shorter than this are filled when spreading translated words, so
/// translated words only pause where the speaker did. Caption chunks break at 350 ms.
const PAUSE_MS: u64 = 300;
/// CJK letters per caption word: a Japanese phrase (bunsetsu) rarely runs longer; Chinese words
/// are mostly two characters.
const JA_GROUP: usize = 6;
const ZH_GROUP: usize = 2;

/// One clip segment to translate.
struct Source {
    start_ms: u64,
    end_ms: u64,
    text: String,
    /// The segment's source words inside the range, kept when the translation falls back.
    words: Vec<Word>,
    /// When the speaker was talking, merged across short gaps.
    spans: Vec<(u64, u64)>,
}

/// Translate the segments of `t` inside `range_ms` into `to` (a BCP-47 tag or a language name)
/// with the picker's summary model, in as few calls as the model's context allows.
pub fn translate_range(
    t: &Transcript,
    range_ms: (u64, u64),
    to: &str,
    llm: &dyn Llm,
) -> Result<Translation, TranslateError> {
    let to = to.trim();
    if to.is_empty() {
        return Err(TranslateError::NoLanguage);
    }
    let source = clip_segments(t, range_ms);
    let system = picker::TRANSLATE_PROMPT.replace("{language}", to);
    let mut lines: Vec<Option<String>> = vec![None; source.len()];
    let mut calls = 0;
    for batch in batches(&source, budget(llm, &system)) {
        translate_batch(llm, &system, &source, batch, &mut lines, &mut calls)?;
    }
    let mut untranslated = Vec::new();
    let segments = source
        .into_iter()
        .zip(lines)
        .enumerate()
        .map(|(i, (s, line))| match line {
            Some(text) => Segment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                words: spread(&split_words(&text), &s.spans),
                text,
            },
            None => {
                untranslated.push(i);
                let words = if s.words.is_empty() {
                    spread(&split_words(&s.text), &s.spans)
                } else {
                    s.words
                };
                Segment {
                    start_ms: s.start_ms,
                    end_ms: s.end_ms,
                    text: s.text,
                    words,
                }
            }
        })
        .collect();
    Ok(Translation {
        transcript: Transcript {
            language: to.to_string(),
            engine: format!("{}+translate:{}", t.engine, llm.model(Role::Summarize)),
            segments,
        },
        untranslated,
        calls,
    })
}

/// Segments overlapping the range. A segment cut by the range keeps only its words inside it,
/// so the caption says what is heard in the clip.
fn clip_segments(t: &Transcript, (start_ms, end_ms): (u64, u64)) -> Vec<Source> {
    let mut out = Vec::new();
    for s in t
        .segments
        .iter()
        .filter(|s| s.end_ms > start_ms && s.start_ms < end_ms)
    {
        let inside: Vec<Word> = s
            .words
            .iter()
            .filter(|w| w.start_ms >= start_ms && w.start_ms < end_ms && !w.text.trim().is_empty())
            .cloned()
            .collect();
        let whole = inside.len() == s.words.len() && !s.text.trim().is_empty();
        let (a, b, text) = match (inside.first(), inside.last()) {
            _ if whole => (
                s.start_ms.max(start_ms),
                s.end_ms.min(end_ms),
                s.text.as_str(),
            ),
            (Some(f), Some(l)) => (f.start_ms, l.end_ms.min(end_ms), ""),
            _ => continue,
        };
        let text = if text.is_empty() {
            join_words(inside.iter().map(|w| w.text.trim()))
        } else {
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        };
        if text.is_empty() {
            continue;
        }
        let mut spans: Vec<(u64, u64)> = Vec::new();
        for w in &inside {
            let (wa, wb) = (w.start_ms.clamp(a, b), w.end_ms.clamp(a, b));
            match spans.last_mut() {
                Some(last) if wa <= last.1 + PAUSE_MS => last.1 = last.1.max(wb),
                _ => spans.push((wa, wb.max(wa))),
            }
        }
        spans.retain(|(x, y)| y > x);
        if spans.is_empty() {
            spans.push((a, b.max(a)));
        }
        out.push(Source {
            start_ms: a,
            end_ms: b.max(a),
            text,
            words: inside,
            spans,
        });
    }
    out
}

/// Input tokens per call. The system prompt and a reserve come off the context and the rest is
/// shared with the reply, allowed twice the input because CJK and Devanagari cost more tokens
/// per sentence than English.
fn budget(llm: &dyn Llm, system: &str) -> usize {
    llm.context_tokens()
        .saturating_sub(picker::estimate_tokens(system) + 256)
        / 3
}

/// Consecutive runs of segments that fit `budget`; a segment larger than the budget goes alone.
fn batches(source: &[Source], budget: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut tokens) = (0, 0);
    for (i, s) in source.iter().enumerate() {
        let need = picker::estimate_tokens(&s.text) + 2;
        if i > start && (tokens + need > budget || i - start >= MAX_LINES) {
            out.push(start..i);
            (start, tokens) = (i, 0);
        }
        tokens += need;
    }
    if start < source.len() {
        out.push(start..source.len());
    }
    out
}

/// Translate one batch into `lines`. A batch too long for the model is split in two; a declined
/// batch keeps its original text. Any other model error fails the translation.
fn translate_batch(
    llm: &dyn Llm,
    system: &str,
    source: &[Source],
    batch: Range<usize>,
    lines: &mut [Option<String>],
    calls: &mut usize,
) -> Result<(), PickerError> {
    let user: String = source[batch.clone()]
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}\n", i + 1, s.text))
        .collect();
    *calls += 1;
    match llm.text(Role::Summarize, system, &user) {
        Ok(raw) => {
            for (i, line) in parse_lines(&raw, batch.len()).into_iter().enumerate() {
                let src = &source[batch.start + i].text;
                lines[batch.start + i] = line.filter(|l| plausible(src, l));
            }
            Ok(())
        }
        Err(PickerError::ContextTooLong(_)) if batch.len() > 1 => {
            let mid = batch.start + batch.len() / 2;
            translate_batch(llm, system, source, batch.start..mid, lines, calls)?;
            translate_batch(llm, system, source, mid..batch.end, lines, calls)
        }
        Err(PickerError::Declined(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// The model's reply as one entry per requested line. Numbered lines (`1.`, `1)`, `1:`, `[1]`,
/// `**1.**`) go to their number; fences, reasoning blocks and unnumbered chatter are ignored.
/// A reply with no numbers at all is taken line by line only when the count matches exactly.
fn parse_lines(raw: &str, n: usize) -> Vec<Option<String>> {
    let body = raw
        .rfind("</think>")
        .map_or(raw, |i| &raw[i + "</think>".len()..]);
    let mut out: Vec<Option<String>> = vec![None; n];
    let mut plain = Vec::new();
    let mut numbered = false;
    for line in body.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        match split_number(line) {
            Some((k, text)) => {
                numbered = true;
                if let Some(slot) = out.get_mut(k.wrapping_sub(1))
                    && slot.is_none()
                {
                    *slot = clean(text);
                }
            }
            None => plain.push(line),
        }
    }
    if !numbered && plain.len() == n {
        out = plain.into_iter().map(clean).collect();
    }
    out
}

/// `(number, rest)` for a line that starts with a list number and a delimiter.
fn split_number(line: &str) -> Option<(usize, &str)> {
    let s = line.trim_start_matches(['-', '*', '#', '>', '[', '(', ' ']);
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    let rest = s.get(digits..)?;
    if digits == 0 || !rest.starts_with(['.', ')', ':', ']', '-', '–', '．', '、']) {
        return None;
    }
    let k = s[..digits].parse().ok()?;
    Some((
        k,
        rest.trim_start_matches(['.', ')', ':', ']', '-', '–', '．', '、', '*', ' ']),
    ))
}

/// Trim, collapse whitespace and drop quotes the model wrapped the whole line in.
fn clean(text: &str) -> Option<String> {
    let t = text.trim().trim_end_matches("**").trim();
    let t = [('"', '"'), ('“', '”')]
        .iter()
        .find_map(|&(a, b)| {
            t.strip_prefix(a)
                .and_then(|r| r.strip_suffix(b))
                .filter(|r| !r.contains([a, b]))
        })
        .unwrap_or(t);
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty()).then_some(t)
}

/// A translation many times longer than its source is commentary, not a caption.
fn plausible(src: &str, tr: &str) -> bool {
    tr.chars().count() <= 4 * src.chars().count() + 40
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Han,
    Hiragana,
    Katakana,
    /// Latin letters, digits, Hangul: anything spaced scripts write.
    Other,
    /// Long-vowel and iteration marks: part of the word before.
    Mark,
    Open,
    /// CJK punctuation that ends a phrase: 。、！？」
    Close,
    /// Other punctuation: sticks to the word before.
    Punct,
}

fn class(c: char) -> Class {
    match c {
        '「' | '『' | '（' | '【' | '〈' | '《' | '〔' | '“' | '‘' | '(' | '[' => {
            Class::Open
        }
        'ー' | '々' | '〆' | '〇' | 'ゝ' | 'ゞ' | 'ヽ' | 'ヾ' => Class::Mark,
        '\u{3040}'..='\u{309F}' => Class::Hiragana,
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => {
            Class::Katakana
        }
        c if c.is_alphanumeric() && is_cjk(c) => Class::Han,
        c if c.is_alphanumeric() => Class::Other,
        c if is_cjk(c) => Class::Close,
        _ => Class::Punct,
    }
}

/// Caption words for `text`: whitespace-separated words, with Japanese and Chinese runs cut
/// into short character groups. Japanese breaks where kanji or katakana follow kana, which
/// approximates phrase boundaries (東京に / 行きました); Chinese, which has no kana to go by,
/// breaks every two characters. Punctuation stays on the group it follows.
pub fn split_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        if !token.chars().any(is_cjk) {
            out.push(token.to_string());
            continue;
        }
        let kana = token
            .chars()
            .any(|c| matches!(class(c), Class::Hiragana | Class::Katakana));
        let max = if kana { JA_GROUP } else { ZH_GROUP };
        let mut cur = String::new();
        // Class of the last letter in `cur`, CJK letters in `cur`, and whether `cur` ends a phrase.
        let (mut prev, mut letters, mut closed) = (None::<Class>, 0, false);
        for c in token.chars() {
            let k = class(c);
            let cut = match k {
                Class::Open => prev.is_some() || closed,
                Class::Close | Class::Punct | Class::Mark => false,
                _ if closed => true,
                _ => match (prev, k) {
                    (None, _) => false,
                    (Some(Class::Other), Class::Other | Class::Hiragana) => false,
                    (Some(Class::Other), _) | (_, Class::Other) => true,
                    (Some(Class::Hiragana), Class::Han | Class::Katakana) => true,
                    _ => letters >= max,
                },
            };
            if cut && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                (prev, letters, closed) = (None, 0, false);
            }
            cur.push(c);
            match k {
                Class::Close => closed = true,
                Class::Open | Class::Punct => {}
                Class::Mark => letters += 1,
                Class::Other => prev = Some(k),
                _ => (prev, letters) = (Some(k), letters + 1),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

/// The span index and time `offset` ms into the speech `spans`. A start on a span boundary goes
/// to the next span, an end stays on the one before.
fn locate(spans: &[(u64, u64)], offset: u64, is_end: bool) -> (usize, u64) {
    let mut left = offset;
    for (i, &(a, b)) in spans.iter().enumerate() {
        let d = b - a;
        if left < d || (is_end && left == d) {
            return (i, a + left);
        }
        left -= d;
    }
    (
        spans.len().saturating_sub(1),
        spans.last().map_or(0, |s| s.1),
    )
}

/// Words laid over the speech spans in order, each taking time in proportion to its length. A
/// word never runs across a pause: it stays inside the span holding its middle, so the pause
/// stays silent and caption chunks still break there.
fn spread(words: &[String], spans: &[(u64, u64)]) -> Vec<Word> {
    let weights: Vec<u64> = words
        .iter()
        .map(|w| w.chars().count().max(1) as u64)
        .collect();
    let total_w: u64 = weights.iter().sum::<u64>().max(1);
    let total_t: u64 = spans.iter().map(|(a, b)| b - a).sum();
    let mut done = 0;
    words
        .iter()
        .zip(weights)
        .map(|(w, n)| {
            let (o0, o1) = (done * total_t / total_w, (done + n) * total_t / total_w);
            done += n;
            let ((s0, t0), (s1, t1)) = (locate(spans, o0, false), locate(spans, o1, true));
            let (mid, _) = locate(spans, (o0 + o1) / 2, false);
            let (a, b) = spans.get(mid).copied().unwrap_or((t0, t1));
            let start_ms = if s0 < mid { a } else { t0 };
            let end_ms = if s1 > mid { b } else { t1 }.max(start_ms);
            Word {
                start_ms,
                end_ms,
                text: w.clone(),
                confidence: 1.0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captions::{self, ChunkOptions, Rasterizer};
    use std::sync::Mutex;

    /// Answers each numbered line with a fixed rewrite, logging every request.
    struct FakeLlm {
        context: usize,
        calls: Mutex<Vec<String>>,
        reply: fn(&[(usize, String)]) -> String,
    }

    impl FakeLlm {
        fn new(context: usize, reply: fn(&[(usize, String)]) -> String) -> Self {
            Self {
                context,
                calls: Mutex::new(Vec::new()),
                reply,
            }
        }
    }

    impl Llm for FakeLlm {
        fn tier(&self) -> &'static str {
            "fake"
        }
        fn model(&self, _: Role) -> String {
            "fake".into()
        }
        fn context_tokens(&self) -> usize {
            self.context
        }
        fn text(&self, role: Role, system: &str, user: &str) -> picker::Result<String> {
            assert_eq!(role, Role::Summarize);
            assert!(system.contains("into Spanish"));
            self.calls.lock().unwrap().push(user.to_string());
            let lines: Vec<(usize, String)> = user
                .lines()
                .filter_map(|l| split_number(l).map(|(k, t)| (k, t.to_string())))
                .collect();
            Ok((self.reply)(&lines))
        }
        fn scores_json(&self, _: &str, _: &str) -> picker::Result<String> {
            unreachable!()
        }
    }

    fn upper(lines: &[(usize, String)]) -> String {
        lines
            .iter()
            .map(|(k, t)| format!("{k}. {}\n", t.to_uppercase()))
            .collect()
    }

    fn word(text: &str, start_ms: u64, end_ms: u64) -> Word {
        Word {
            start_ms,
            end_ms,
            text: text.into(),
            confidence: 0.9,
        }
    }

    /// Three segments of two words each, the second with a long pause between its words.
    fn transcript() -> Transcript {
        let seg = |words: Vec<Word>| Segment {
            start_ms: words[0].start_ms,
            end_ms: words.last().unwrap().end_ms,
            text: words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            words,
        };
        Transcript {
            language: "en-US".into(),
            engine: "apple-speech".into(),
            segments: vec![
                seg(vec![word("hello", 0, 400), word("there.", 450, 900)]),
                seg(vec![word("charge", 1000, 1400), word("more.", 2400, 2800)]),
                seg(vec![word("then", 3000, 3300), word("wait.", 3300, 3600)]),
            ],
        }
    }

    #[test]
    fn keeps_segment_count_and_timing() {
        let llm = FakeLlm::new(8_000, upper);
        let tr = translate_range(&transcript(), (0, 10_000), " Spanish ", &llm).unwrap();
        let t = &tr.transcript;
        assert_eq!(t.language, "Spanish");
        assert_eq!(t.engine, "apple-speech+translate:fake");
        assert_eq!(tr.calls, 1);
        assert!(tr.untranslated.is_empty());
        let texts: Vec<&str> = t.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["HELLO THERE.", "CHARGE MORE.", "THEN WAIT."]);
        for (s, src) in t.segments.iter().zip(&transcript().segments) {
            assert_eq!((s.start_ms, s.end_ms), (src.start_ms, src.end_ms));
            assert_eq!(s.words.first().unwrap().start_ms, s.start_ms);
            assert_eq!(s.words.last().unwrap().end_ms, s.end_ms);
            for w in s.words.windows(2) {
                assert!(w[0].start_ms <= w[0].end_ms && w[0].end_ms <= w[1].start_ms);
            }
        }
        // The pause in the second segment stays a pause: no word is spoken during it.
        let mid = &t.segments[1].words;
        assert!(mid.iter().all(|w| w.end_ms <= 1400 || w.start_ms >= 2400));
    }

    #[test]
    fn range_cuts_segments_to_heard_words() {
        let llm = FakeLlm::new(8_000, upper);
        let tr = translate_range(&transcript(), (1_000, 3_300), "Spanish", &llm).unwrap();
        let t = &tr.transcript;
        let texts: Vec<&str> = t.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["CHARGE MORE.", "THEN"]);
        assert_eq!((t.segments[1].start_ms, t.segments[1].end_ms), (3000, 3300));
    }

    #[test]
    fn garbled_and_missing_lines_fall_back() {
        // Fenced, out of order, line 2 missing, line 3 is commentary, plus chatter.
        fn reply(_: &[(usize, String)]) -> String {
            format!(
                "Sure! Here you go:\n```text\n3. {}\n1) \"hola ahí.\"\n```\nNote: kept the tone.",
                "x".repeat(200)
            )
        }
        let llm = FakeLlm::new(8_000, reply);
        let src = transcript();
        let tr = translate_range(&src, (0, 10_000), "Spanish", &llm).unwrap();
        assert_eq!(tr.untranslated, [1, 2]);
        let t = &tr.transcript;
        assert_eq!(t.segments.len(), 3);
        assert_eq!(t.segments[0].text, "hola ahí.");
        // Fallbacks keep the original text and the original word timings.
        assert_eq!(t.segments[1], src.segments[1]);
        assert_eq!(t.segments[2], src.segments[2]);
    }

    #[test]
    fn declined_batch_keeps_originals_and_errors_propagate() {
        struct Refuses(bool);
        impl Llm for Refuses {
            fn tier(&self) -> &'static str {
                "fake"
            }
            fn model(&self, _: Role) -> String {
                "fake".into()
            }
            fn context_tokens(&self) -> usize {
                4_096
            }
            fn text(&self, _: Role, _: &str, _: &str) -> picker::Result<String> {
                Err(if self.0 {
                    PickerError::Declined("sensitive".into())
                } else {
                    PickerError::Transport("offline".into())
                })
            }
            fn scores_json(&self, _: &str, _: &str) -> picker::Result<String> {
                unreachable!()
            }
        }
        let tr = translate_range(&transcript(), (0, 10_000), "es", &Refuses(true)).unwrap();
        assert_eq!(tr.untranslated, [0, 1, 2]);
        assert_eq!(tr.transcript.segments, transcript().segments);
        assert!(translate_range(&transcript(), (0, 10_000), "es", &Refuses(false)).is_err());
        assert!(matches!(
            translate_range(&transcript(), (0, 10_000), " ", &Refuses(true)),
            Err(TranslateError::NoLanguage)
        ));
    }

    #[test]
    fn small_context_splits_into_batches() {
        let mut t = transcript();
        let seg = t.segments[0].clone();
        t.segments = (0..30)
            .map(|i| Segment {
                start_ms: i * 1000,
                end_ms: i * 1000 + 900,
                text: format!("sentence number {i} with a few more words in it"),
                words: seg
                    .words
                    .iter()
                    .map(|w| word(&w.text, w.start_ms + i * 1000, w.end_ms + i * 1000))
                    .collect(),
            })
            .collect();
        let system_tokens = picker::estimate_tokens(picker::TRANSLATE_PROMPT);
        let llm = FakeLlm::new(system_tokens + 256 + 3 * 60, upper);
        let tr = translate_range(&t, (0, 60_000), "Spanish", &llm).unwrap();
        assert!(tr.calls > 1, "{} calls", tr.calls);
        assert!(tr.untranslated.is_empty());
        assert_eq!(tr.transcript.segments.len(), 30);
        // Every call is numbered from 1, so the model never sees a gap.
        for call in llm.calls.lock().unwrap().iter() {
            assert!(call.starts_with("1. "));
        }
        assert_eq!(
            tr.transcript.segments[29].text,
            "SENTENCE NUMBER 29 WITH A FEW MORE WORDS IN IT"
        );
        // A roomy context takes the whole clip in one call.
        let big = FakeLlm::new(32_000, upper);
        assert_eq!(
            translate_range(&t, (0, 60_000), "Spanish", &big)
                .unwrap()
                .calls,
            1
        );
    }

    #[test]
    fn context_too_long_splits_the_batch() {
        struct Half(Mutex<usize>);
        impl Llm for Half {
            fn tier(&self) -> &'static str {
                "fake"
            }
            fn model(&self, _: Role) -> String {
                "fake".into()
            }
            fn context_tokens(&self) -> usize {
                8_000
            }
            fn text(&self, _: Role, _: &str, user: &str) -> picker::Result<String> {
                *self.0.lock().unwrap() += 1;
                if user.lines().count() > 1 {
                    return Err(PickerError::ContextTooLong("too long".into()));
                }
                Ok("1. uno".into())
            }
            fn scores_json(&self, _: &str, _: &str) -> picker::Result<String> {
                unreachable!()
            }
        }
        let llm = Half(Mutex::new(0));
        let tr = translate_range(&transcript(), (0, 10_000), "es", &llm).unwrap();
        assert!(tr.untranslated.is_empty());
        assert!(tr.transcript.segments.iter().all(|s| s.text == "uno"));
        assert_eq!(*llm.0.lock().unwrap(), 5);
    }

    #[test]
    fn parses_numbering_styles() {
        let raw =
            "<think>1. no</think>\n**1.** uno\n2) dos\n[3] tres\n4: cuatro\n- 5. cinco\n6 seis";
        let got = parse_lines(raw, 6);
        assert_eq!(
            got,
            [
                Some("uno".to_string()),
                Some("dos".into()),
                Some("tres".into()),
                Some("cuatro".into()),
                Some("cinco".into()),
                None,
            ]
        );
        // No numbers at all: positional only when the count matches.
        assert_eq!(
            parse_lines("uno\ndos", 2),
            [Some("uno".into()), Some("dos".into())]
        );
        assert_eq!(parse_lines("uno\ndos\ntres", 2), [None, None]);
    }

    #[test]
    fn splits_cjk_into_groups() {
        assert_eq!(
            split_words("東京に行きました。"),
            ["東京に", "行きました。"]
        );
        assert_eq!(
            split_words("「コーヒーを」飲みたい"),
            ["「コーヒーを」", "飲みたい"]
        );
        assert_eq!(split_words("iPhone15を買った"), ["iPhone15を", "買った"]);
        assert_eq!(
            split_words("我们今天讨论一下这个问题。"),
            ["我们", "今天", "讨论", "一下", "这个", "问题。"]
        );
        assert_eq!(split_words("用 GPU 渲染"), ["用", "GPU", "渲染"]);
        assert_eq!(split_words("Hola, ¿qué tal?"), ["Hola,", "¿qué", "tal?"]);
        assert_eq!(split_words("안녕하세요 여러분"), ["안녕하세요", "여러분"]);
    }

    #[test]
    fn japanese_translation_chunks_without_spaces() {
        fn ja(lines: &[(usize, String)]) -> String {
            let out = [
                "東京に行きました。",
                "もっと高く売りましょう。",
                "そして待つ。",
            ];
            lines
                .iter()
                .map(|(k, _)| format!("{k}. {}\n", out[k - 1]))
                .collect()
        }
        // The fake checks for the Spanish prompt; what matters here is the Japanese it returns.
        let llm = FakeLlm::new(8_000, ja);
        let t = translate_range(&transcript(), (0, 10_000), "Spanish", &llm)
            .unwrap()
            .transcript;
        assert_eq!(t.segments[0].words.len(), 2, "{:?}", t.segments[0].words);
        let mut r = Rasterizer::new();
        let track = captions::chunk(
            &t,
            (0, 10_000),
            &captions::preset("clean").unwrap(),
            (1080, 1920),
            &mut r,
            &ChunkOptions::default(),
        );
        // Several groups in one chunk, joined without spaces.
        let srt = captions::subtitles::srt(&track, 0);
        assert!(srt.contains("\n東京に行きました。\n"), "{srt}");
    }
}

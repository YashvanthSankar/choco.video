//! Hand corrections to caption words. Only the text of a word changes: timings, word count and
//! order stay as transcribed, so burned-in captions, SRT and VTT keep their sync. A word cannot be
//! emptied (that would be a delete, which shifts what the viewer reads against the audio); type a
//! replacement instead.

use crate::model::{Transcript, Word};
use crate::transcribe::cleanup::join_words;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordEdit {
    /// Position among the words that start inside the clip, in transcript order (see `clip_words`).
    pub index: usize,
    pub text: String,
}

/// Word numbers in messages count from 1, as the editor shows them.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("captions: word {} is outside this clip, which has {count} words", .index + 1)]
    OutOfRange { index: usize, count: usize },
    #[error("captions: word {} is edited twice", .0 + 1)]
    Duplicate(usize),
    #[error("captions: word {} is empty; type a replacement, words cannot be removed", .0 + 1)]
    Empty(usize),
    #[error("captions: word {} contains a line break or control character", .0 + 1)]
    Control(usize),
    #[error("captions: \"{0}\" has an emoji; the caption fonts cannot draw emoji yet")]
    Emoji(String),
}

/// The bundled caption fonts are a Latin subset without emoji, and an emoji font would cost
/// megabytes, so emoji are refused instead of being burned in as blank boxes. Covers the emoji
/// blocks and the emoji presentation selector, not every pictograph.
fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF | 0x2300..=0x23FF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0xFE0F
    )
}

/// `(segment, word)` positions of the words starting inside `range`, in transcript order. The
/// index into this list is a word's edit index. `captions::chunk` uses the same range test.
fn positions(t: &Transcript, (start_ms, end_ms): (u64, u64)) -> Vec<(usize, usize)> {
    t.segments
        .iter()
        .enumerate()
        .flat_map(|(si, s)| s.words.iter().enumerate().map(move |(wi, w)| (si, wi, w)))
        .filter(|(_, _, w)| w.start_ms >= start_ms && w.start_ms < end_ms)
        .map(|(si, wi, _)| (si, wi))
        .collect()
}

/// The words of a clip with their edit indices, one group per transcript segment (the way they
/// read). Blank words are left out but keep their index.
pub fn clip_words(t: &Transcript, range: (u64, u64)) -> Vec<Vec<(usize, &Word)>> {
    let mut groups: Vec<Vec<(usize, &Word)>> = Vec::new();
    let mut last_seg = None;
    for (i, (si, wi)) in positions(t, range).into_iter().enumerate() {
        let w = &t.segments[si].words[wi];
        if w.text.trim().is_empty() {
            continue;
        }
        if last_seg != Some(si) {
            groups.push(Vec::new());
            last_seg = Some(si);
        }
        groups.last_mut().expect("pushed above").push((i, w));
    }
    groups
}

/// Replace the text of the clip's words named in `edits`, then rebuild the text of every segment
/// that changed. All edits are checked before any is applied. Returns how many words changed.
pub fn edit_words(
    t: &mut Transcript,
    range: (u64, u64),
    edits: &[WordEdit],
) -> Result<usize, EditError> {
    let pos = positions(t, range);
    let mut seen = vec![false; pos.len()];
    for e in edits {
        let text = e.text.trim();
        if e.index >= pos.len() {
            return Err(EditError::OutOfRange {
                index: e.index,
                count: pos.len(),
            });
        }
        if std::mem::replace(&mut seen[e.index], true) {
            return Err(EditError::Duplicate(e.index));
        }
        if text.is_empty() {
            return Err(EditError::Empty(e.index));
        }
        if text.chars().any(char::is_control) {
            return Err(EditError::Control(e.index));
        }
        if text.chars().any(is_emoji) {
            return Err(EditError::Emoji(text.into()));
        }
    }
    let mut changed = 0;
    let mut touched = vec![false; t.segments.len()];
    for e in edits {
        let (si, wi) = pos[e.index];
        let w = &mut t.segments[si].words[wi];
        let text = e.text.trim();
        if w.text != text {
            w.text = text.into();
            touched[si] = true;
            changed += 1;
        }
    }
    for (s, _) in t.segments.iter_mut().zip(&touched).filter(|(_, t)| **t) {
        s.text = join_words(&s.words);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Segment;

    fn seg(words: &[(&str, u64, u64)]) -> Segment {
        let words: Vec<Word> = words
            .iter()
            .map(|(t, s, e)| Word {
                start_ms: *s,
                end_ms: *e,
                text: t.to_string(),
                confidence: 0.5,
            })
            .collect();
        Segment {
            start_ms: words[0].start_ms,
            end_ms: words.last().unwrap().end_ms,
            text: join_words(&words),
            words,
        }
    }

    fn transcript() -> Transcript {
        Transcript {
            language: "en".into(),
            engine: "t".into(),
            segments: vec![
                seg(&[("Before", 0, 400), ("the", 400, 600), ("clip.", 600, 900)]),
                seg(&[
                    ("Charge", 1000, 1300),
                    ("moor", 1300, 1500),
                    ("then", 1500, 1700),
                ]),
                seg(&[
                    ("you", 1800, 1900),
                    ("think", 1900, 2100),
                    (".", 2100, 2150),
                ]),
            ],
        }
    }

    fn edit(index: usize, text: &str) -> WordEdit {
        WordEdit {
            index,
            text: text.into(),
        }
    }

    const CLIP: (u64, u64) = (1000, 2200);

    #[test]
    fn lists_clip_words_by_segment_with_indices() {
        let t = transcript();
        let groups: Vec<Vec<(usize, &str)>> = clip_words(&t, CLIP)
            .into_iter()
            .map(|g| g.into_iter().map(|(i, w)| (i, w.text.as_str())).collect())
            .collect();
        assert_eq!(
            groups,
            vec![
                vec![(0, "Charge"), (1, "moor"), (2, "then")],
                vec![(3, "you"), (4, "think"), (5, ".")],
            ]
        );
    }

    #[test]
    fn replaces_text_keeps_timings_and_rebuilds_segment_text() {
        let mut t = transcript();
        let before = t.clone();
        let n = edit_words(
            &mut t,
            CLIP,
            &[edit(1, " more "), edit(2, "than"), edit(4, "think")],
        )
        .unwrap();
        assert_eq!(n, 2, "an unchanged word does not count");
        assert_eq!(t.segments[1].text, "Charge more than");
        assert_eq!(t.segments[1].words[1].text, "more");
        // Untouched segments keep their text, even outside the clip.
        assert_eq!(t.segments[0], before.segments[0]);
        assert_eq!(t.segments[2], before.segments[2]);
        for (a, b) in t.segments.iter().zip(&before.segments) {
            assert_eq!((a.start_ms, a.end_ms), (b.start_ms, b.end_ms));
            assert_eq!(a.words.len(), b.words.len());
            for (x, y) in a.words.iter().zip(&b.words) {
                assert_eq!(
                    (x.start_ms, x.end_ms, x.confidence),
                    (y.start_ms, y.end_ms, y.confidence)
                );
            }
        }
    }

    #[test]
    fn rejects_bad_edits_without_applying_any() {
        let cases = [
            (edit(6, "x"), EditError::OutOfRange { index: 6, count: 6 }),
            (edit(1, "   "), EditError::Empty(1)),
            (edit(1, "mo\nre"), EditError::Control(1)),
            (edit(1, "more🔥"), EditError::Emoji("more🔥".into())),
            (
                edit(1, "fire\u{2764}\u{FE0F}"),
                EditError::Emoji("fire\u{2764}\u{FE0F}".into()),
            ),
        ];
        for (bad, want) in cases {
            let mut t = transcript();
            let err = edit_words(&mut t, CLIP, &[edit(0, "Charged"), bad]).unwrap_err();
            assert_eq!(err, want);
            assert_eq!(t, transcript(), "nothing applied on error");
        }
        let mut t = transcript();
        assert_eq!(
            edit_words(&mut t, CLIP, &[edit(2, "a"), edit(2, "b")]),
            Err(EditError::Duplicate(2))
        );
        assert_eq!(
            EditError::Empty(1).to_string(),
            "captions: word 2 is empty; type a replacement, words cannot be removed"
        );
    }

    #[test]
    fn accented_and_typographic_text_is_allowed() {
        let mut t = transcript();
        edit_words(&mut t, CLIP, &[edit(1, "café"), edit(2, "—“then”")]).unwrap();
        assert_eq!(t.segments[1].text, "Charge café —“then”");
    }
}

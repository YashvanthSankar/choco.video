//! Shortlist: cut a few hundred windows to the ~60 worth a model's attention.
//! Default mode ranks by a cheap quality prior; prompt mode ranks by BM25 against the prompt.
//! Pure Rust, no model download.

use super::audio::WindowAudio;
use super::windows::Window;
use std::collections::{HashMap, HashSet};

pub fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|t| !t.is_empty())
        .map(|t| t.trim_matches('\'').to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

const STOP: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "so", "to", "of", "in", "on", "for", "with", "is", "it", "that", "this", "was",
    "are", "be", "i", "you", "we", "they", "he", "she", "at", "as", "by", "do", "not", "have", "has", "just", "like",
    "um", "uh", "yeah", "know", "my", "me", "your", "our", "if", "what", "there", "their", "about",
];

fn content_tokens(s: &str) -> HashSet<String> {
    tokens(s).into_iter().filter(|t| !STOP.contains(&t.as_str()) && t.len() > 2).collect()
}

/// Words that tend to open a clip people keep watching.
const HOOK_WORDS: &[&str] = &[
    "secret", "mistake", "never", "always", "nobody", "everyone", "truth", "wrong", "why", "how", "biggest", "worst",
    "best", "crazy", "insane", "actually", "honestly", "imagine", "problem", "lesson", "stop", "realized", "changed",
];
const WEAK_OPENERS: &[&str] = &["and", "but", "so", "because", "which", "or", "then", "also"];

/// 0..1, higher is more promising. Cheap, transparent, and only used to order the shortlist.
pub fn prior(w: &Window, audio: Option<&WindowAudio>) -> f32 {
    let toks = tokens(&w.text);
    if toks.is_empty() {
        return 0.0;
    }
    let minutes = w.duration_ms() as f32 / 60_000.0;
    let wpm = w.word_count as f32 / minutes.max(0.01);
    // Engaged speech sits around 150-190 wpm.
    let pace = (1.0 - ((wpm - 170.0) / 90.0).abs()).clamp(0.0, 1.0);

    let opening: Vec<&String> = toks.iter().take(12).collect();
    let hook = opening.iter().filter(|t| HOOK_WORDS.contains(&t.as_str())).count().min(2) as f32 / 2.0;
    let first_sentence_end = w.text.find(['.', '?', '!']).unwrap_or(w.text.len());
    let opens_with_question = w.text[..first_sentence_end.min(w.text.len())].len() < w.text.len()
        && w.text.as_bytes().get(first_sentence_end) == Some(&b'?');
    let weak_open = WEAK_OPENERS.contains(&toks[0].as_str());
    let has_number = toks.iter().any(|t| t.chars().any(|c| c.is_ascii_digit()))
        || toks.iter().any(|t| ["one", "two", "three", "four", "five", "ten", "hundred", "thousand", "million", "percent"].contains(&t.as_str()));
    let questions = w.text.matches('?').count().min(3) as f32 / 3.0;
    // Mid-length clips perform best; very short and very long are penalised gently.
    let secs = w.duration_ms() as f32 / 1000.0;
    let length = (1.0 - ((secs - 40.0) / 50.0).abs()).clamp(0.2, 1.0);

    let mut score = 0.22 * pace
        + 0.18 * hook
        + 0.12 * opens_with_question as u8 as f32
        + 0.08 * has_number as u8 as f32
        + 0.08 * questions
        + 0.12 * length
        - 0.10 * weak_open as u8 as f32
        - 0.20 * w.silence_ratio;
    if let Some(a) = audio {
        score += 0.08 * (a.loudness_z / 2.0).clamp(-1.0, 1.0);
        score += 0.06 * (a.spikes_per_min / 10.0).clamp(0.0, 1.0);
        score -= 0.10 * (a.longest_pause_ms as f32 / 3_000.0).clamp(0.0, 1.0);
    }
    score.clamp(0.0, 1.0)
}

/// Okapi BM25 over window texts.
pub fn bm25(windows: &[Window], query: &str) -> Vec<f32> {
    let (k1, b) = (1.5f32, 0.75f32);
    let docs: Vec<Vec<String>> = windows.iter().map(|w| tokens(&w.text)).collect();
    let avg = docs.iter().map(Vec::len).sum::<usize>() as f32 / docs.len().max(1) as f32;
    let n = docs.len() as f32;
    let q: Vec<String> = expand(&tokens(query));
    let mut df: HashMap<&str, f32> = HashMap::new();
    for d in &docs {
        let uniq: HashSet<&str> = d.iter().map(String::as_str).collect();
        for t in &q {
            if uniq.contains(t.as_str()) {
                *df.entry(t.as_str()).or_default() += 1.0;
            }
        }
    }
    docs.iter()
        .map(|d| {
            let len = d.len() as f32;
            q.iter()
                .map(|t| {
                    let f = d.iter().filter(|x| *x == t).count() as f32;
                    if f == 0.0 {
                        return 0.0;
                    }
                    let dfi = df.get(t.as_str()).copied().unwrap_or(0.0);
                    let idf = ((n - dfi + 0.5) / (dfi + 0.5) + 1.0).ln();
                    idf * f * (k1 + 1.0) / (f + k1 * (1.0 - b + b * len / avg.max(1.0)))
                })
                .sum()
        })
        .collect()
}

/// Tiny built-in synonym and inflection expansion so "pricing" also finds "price" and "cost".
fn expand(q: &[String]) -> Vec<String> {
    const GROUPS: &[&[&str]] = &[
        &["price", "prices", "pricing", "priced", "cost", "costs", "charge", "charging", "expensive", "cheap"],
        &["money", "revenue", "income", "profit", "paid", "pay", "earn", "earning"],
        &["funny", "laugh", "laughing", "joke", "hilarious"],
        &["advice", "tip", "tips", "lesson", "lessons", "learned"],
        &["mistake", "mistakes", "failure", "failed", "wrong", "regret"],
        &["ai", "artificial", "intelligence", "model", "models", "machine"],
        &["start", "started", "beginning", "began", "founded"],
    ];
    let mut out: Vec<String> = q.iter().filter(|t| !STOP.contains(&t.as_str())).cloned().collect();
    for t in q {
        for g in GROUPS {
            if g.contains(&t.as_str()) {
                out.extend(g.iter().map(|s| s.to_string()));
            }
        }
        // Light stemming: plural and -ing.
        if let Some(s) = t.strip_suffix('s') {
            out.push(s.to_string());
        }
        if let Some(s) = t.strip_suffix("ing") {
            out.push(s.to_string());
            out.push(format!("{s}e"));
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn jaccard(a: &str, b: &str) -> f32 {
    let (x, y) = (content_tokens(a), content_tokens(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    x.intersection(&y).count() as f32 / x.union(&y).count() as f32
}

fn overlap_frac(a: &Window, b: &Window) -> f32 {
    let inter = a.end_ms.min(b.end_ms).saturating_sub(a.start_ms.max(b.start_ms));
    inter as f32 / a.duration_ms().min(b.duration_ms()).max(1) as f32
}

/// Keep the best `k` windows by `score`, skipping near-duplicates of ones already kept.
/// Overlapping windows are allowed here (different lengths from the same start are useful
/// to the model); only near-identical ones are dropped.
pub fn shortlist(windows: &[Window], score: &[f32], k: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..windows.len()).collect();
    order.sort_by(|&a, &b| score[b].total_cmp(&score[a]));
    let mut kept: Vec<usize> = Vec::new();
    for i in order {
        if score[i] <= 0.0 && !kept.is_empty() && kept.len() >= k / 2 {
            break;
        }
        let dup = kept.iter().any(|&j| {
            let o = overlap_frac(&windows[i], &windows[j]);
            o > 0.85 || (o > 0.5 && jaccard(&windows[i].text, &windows[j].text) > 0.8)
        });
        if !dup {
            kept.push(i);
            if kept.len() == k {
                break;
            }
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: &str, s: u64, e: u64, text: &str) -> Window {
        Window {
            id: id.into(),
            start_ms: s,
            end_ms: e,
            first_seg: 0,
            last_seg: 0,
            text: text.into(),
            word_count: text.split_whitespace().count(),
            silence_ratio: 0.0,
        }
    }

    #[test]
    fn bm25_finds_the_topic_with_synonyms() {
        let ws = vec![
            win("a", 0, 30_000, "We talked about our dog and the weather in the park today."),
            win("b", 30_000, 60_000, "Charge more than you think. The price you set tells customers what it is worth."),
            win("c", 60_000, 90_000, "Distribution means posting every single day on every channel."),
        ];
        let s = bm25(&ws, "pricing");
        assert!(s[1] > s[0] && s[1] > s[2], "{s:?}");
        assert_eq!(s[0], 0.0);
    }

    #[test]
    fn prior_prefers_hooks_over_weak_openers() {
        let words = |n| "word ".repeat(n);
        let strong = win("s", 0, 40_000, &format!("Why does nobody talk about the biggest mistake? {}", words(100)));
        let weak = win("w", 0, 40_000, &format!("And then we went there and did a thing. {}", words(100)));
        assert!(prior(&strong, None) > prior(&weak, None));
    }

    #[test]
    fn shortlist_drops_near_duplicates() {
        let t = "pricing matters more than features for every creator who sells anything online";
        let ws = vec![win("a", 0, 30_000, t), win("b", 500, 30_500, t), win("c", 60_000, 90_000, "something else entirely different here")];
        let kept = shortlist(&ws, &[0.9, 0.8, 0.5], 3);
        assert_eq!(kept, vec![0, 2]);
    }
}

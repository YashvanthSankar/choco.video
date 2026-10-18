//! Highlights: transcript (+ audio features) in, ranked clip candidates out.
//!
//! windows → shortlist (prior or BM25) → episode summary → model scores in batches → rank.

use crate::model::{Candidate, Progress, Transcript};
use crate::ProgressFn;
use picker::{Llm, PickerError, Role, ScoreOut};

pub mod audio;
pub mod picker;
pub mod rank;
pub mod search;
pub mod windows;

const STAGE: &str = "highlights";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HighlightOptions {
    pub windows: windows::WindowOptions,
    pub rank: rank::RankOptions,
    /// Prompt mode: "find every moment about pricing". None = best clips overall.
    pub prompt: Option<String>,
    /// Windows sent to the model.
    pub shortlist: usize,
    /// Words in the episode summary.
    pub summary_words: usize,
}

impl Default for HighlightOptions {
    fn default() -> Self {
        Self {
            windows: Default::default(),
            rank: Default::default(),
            prompt: None,
            shortlist: 60,
            summary_words: 250,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct HighlightReport {
    pub windows: usize,
    pub shortlisted: usize,
    pub scored: usize,
    pub declined_batches: usize,
    pub failed_batches: usize,
    pub summary: String,
    pub model_calls: usize,
    /// First few failure messages, for diagnosis.
    pub errors: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HighlightError {
    #[error("highlights: transcript has no usable windows (too short or too sparse)")]
    NoWindows,
    #[error("highlights: prompt matched nothing in the transcript")]
    NoMatches,
    #[error(transparent)]
    Picker(#[from] PickerError),
}

fn fmt_ts(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60)
}

/// Summarise the whole transcript, map-reducing when it exceeds the model's context.
fn summarize(llm: &dyn Llm, t: &Transcript, words: usize, calls: &mut usize) -> Result<String, PickerError> {
    let system = picker::SUMMARIZE_PROMPT.replace("{max_words}", &words.to_string());
    let full: Vec<String> = t.segments.iter().map(|s| format!("[{}] {}", fmt_ts(s.start_ms), s.text)).collect();
    let budget = llm.context_tokens().saturating_sub(picker::estimate_tokens(&system) + words * 2 + 256);
    let joined = full.join("\n");
    if picker::estimate_tokens(&joined) <= budget {
        *calls += 1;
        return llm.text(Role::Summarize, &system, &joined);
    }
    // Map: summarise chunks that fit, then reduce the partial summaries.
    let chunk_words = words.min(120);
    let chunk_system = picker::SUMMARIZE_PROMPT.replace("{max_words}", &chunk_words.to_string());
    let mut partials = Vec::new();
    let mut cur = String::new();
    for line in full {
        if picker::estimate_tokens(&cur) + picker::estimate_tokens(&line) > budget && !cur.is_empty() {
            *calls += 1;
            partials.push(llm.text(Role::Summarize, &chunk_system, &cur)?);
            cur.clear();
        }
        cur.push_str(&line);
        cur.push('\n');
    }
    if !cur.is_empty() {
        *calls += 1;
        partials.push(llm.text(Role::Summarize, &chunk_system, &cur)?);
    }
    *calls += 1;
    llm.text(Role::Summarize, &system, &partials.join("\n\n"))
}

fn batch_prompt(summary: &str, batch: &[&windows::Window], prompt: Option<&str>) -> String {
    let mut s = format!("Episode summary:\n{summary}\n\n");
    if let Some(p) = prompt {
        s.push_str(&format!("The creator asked for clips about: {p}\nScore clips that are not about this low.\n\n"));
    }
    s.push_str("Candidates:\n");
    for w in batch {
        s.push_str(&format!("[{}] ({}–{}, {}s)\n{}\n\n", w.id, fmt_ts(w.start_ms), fmt_ts(w.end_ms), w.duration_ms() / 1000, w.text));
    }
    s
}

/// Score one batch: strict parse, one retry on bad output, one re-ask for ids the model skipped.
fn score_batch(llm: &dyn Llm, summary: &str, batch: &[&windows::Window], prompt: Option<&str>, calls: &mut usize) -> Result<Vec<ScoreOut>, PickerError> {
    let ids: Vec<String> = batch.iter().map(|w| w.id.clone()).collect();
    let user = batch_prompt(summary, batch, prompt);
    *calls += 1;
    let mut scores = match picker::parse_scores(&llm.scores_json(picker::SCORE_PROMPT, &user)?, &ids) {
        Ok(s) => s,
        Err(PickerError::Parse(e)) => {
            *calls += 1;
            let retry = format!("{user}\nYour previous reply could not be parsed ({e}). Reply with JSON only.");
            picker::parse_scores(&llm.scores_json(picker::SCORE_PROMPT, &retry)?, &ids)?
        }
        Err(e) => return Err(e),
    };
    let missing: Vec<&windows::Window> = batch.iter().copied().filter(|w| !scores.iter().any(|s| s.id == w.id)).collect();
    if !missing.is_empty() {
        let mids: Vec<String> = missing.iter().map(|w| w.id.clone()).collect();
        *calls += 1;
        if let Ok(more) = llm
            .scores_json(picker::SCORE_PROMPT, &batch_prompt(summary, &missing, prompt))
            .and_then(|raw| picker::parse_scores(&raw, &mids))
        {
            scores.extend(more);
        }
    }
    Ok(scores)
}

pub fn highlights(
    t: &Transcript,
    audio: Option<&audio::AudioFeatures>,
    llm: &dyn Llm,
    opts: &HighlightOptions,
    progress: ProgressFn,
) -> Result<(Vec<Candidate>, HighlightReport), HighlightError> {
    let mut report = HighlightReport::default();
    progress(Progress::new(STAGE, 0.0, "finding candidate clips"));
    let ws = windows::windows(t, &opts.windows);
    report.windows = ws.len();
    if ws.is_empty() {
        return Err(HighlightError::NoWindows);
    }

    let scores: Vec<f32> = match &opts.prompt {
        Some(p) => {
            let b = search::bm25(&ws, p);
            if b.iter().all(|&s| s <= 0.0) {
                return Err(HighlightError::NoMatches);
            }
            // Relevance first, quality as a tie-breaker.
            b.iter().zip(&ws).map(|(&s, w)| s + 0.1 * search::prior(w, audio.map(|a| a.window(w.start_ms, w.end_ms)).as_ref())).collect()
        }
        None => ws.iter().map(|w| search::prior(w, audio.map(|a| a.window(w.start_ms, w.end_ms)).as_ref())).collect(),
    };
    let short = search::shortlist(&ws, &scores, opts.shortlist);
    report.shortlisted = short.len();

    progress(Progress::new(STAGE, 0.05, format!("summarizing episode with {}", llm.model(Role::Summarize))));
    let mut calls = 0;
    let summary = summarize(llm, t, opts.summary_words, &mut calls)?;
    report.summary = summary.clone();

    let picked: Vec<&windows::Window> = short.iter().map(|&i| &ws[i]).collect();
    let mut by_id: std::collections::HashMap<String, ScoreOut> = Default::default();
    let batches: Vec<&[&windows::Window]> = picked.chunks(llm.batch_size().max(1)).collect();
    for (bi, batch) in batches.iter().enumerate() {
        progress(Progress::new(
            STAGE,
            0.1 + 0.85 * bi as f32 / batches.len() as f32,
            format!("scoring batch {}/{} with {}", bi + 1, batches.len(), llm.model(Role::Score)),
        ));
        match score_batch(llm, &summary, batch, opts.prompt.as_deref(), &mut calls) {
            Ok(s) => s.into_iter().for_each(|x| {
                by_id.insert(x.id.clone(), x);
            }),
            Err(PickerError::Declined(m)) => {
                report.declined_batches += 1;
                if report.errors.len() < 8 {
                    report.errors.push(format!("batch {}: declined: {m}", bi + 1));
                }
            }
            Err(e @ (PickerError::NotConfigured(_) | PickerError::Unavailable { .. } | PickerError::Http { status: 401 | 403, .. })) => {
                return Err(e.into());
            }
            Err(e) => {
                report.failed_batches += 1;
                if report.errors.len() < 8 {
                    report.errors.push(format!("batch {}: {e}", bi + 1));
                }
            }
        }
    }
    report.model_calls = calls;
    report.scored = by_id.len();
    if by_id.is_empty() {
        return Err(PickerError::Parse(format!(
            "no batch produced scores ({} declined, {} failed)",
            report.declined_batches, report.failed_batches
        ))
        .into());
    }

    let prov = picker::provenance(llm);
    let scored: Vec<(Candidate, String)> = picked
        .iter()
        .filter_map(|w| {
            by_id.get(&w.id).map(|s| {
                (
                    Candidate {
                        id: w.id.clone(),
                        start_ms: w.start_ms,
                        end_ms: w.end_ms,
                        score: s.score.round() as u8,
                        reasons: s.reasons.clone(),
                        title: s.title.clone(),
                        description: s.description.clone(),
                        hashtags: s.hashtags.clone(),
                        provenance: prov.clone(),
                    },
                    w.text.clone(),
                )
            })
        })
        .collect();
    let ranked = rank::rank(scored, &opts.rank);
    progress(Progress::new(STAGE, 1.0, format!("{} clips", ranked.len())));
    Ok((ranked, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Scores a window by how many times it says "gold"; skips ids on the first ask to test re-asking.
    struct FakeLlm {
        calls: Mutex<Vec<String>>,
        texts: std::collections::HashMap<String, String>,
    }

    impl Llm for FakeLlm {
        fn tier(&self) -> &'static str {
            "fake"
        }
        fn model(&self, _: Role) -> String {
            "fake".into()
        }
        fn context_tokens(&self) -> usize {
            1_000
        }
        fn batch_size(&self) -> usize {
            3
        }
        fn text(&self, _: Role, _: &str, user: &str) -> Result<String, PickerError> {
            self.calls.lock().unwrap().push(format!("text:{}", user.len()));
            Ok("a summary".into())
        }
        fn scores_json(&self, _: &str, user: &str) -> Result<String, PickerError> {
            let first_ask = !self.calls.lock().unwrap().iter().any(|c| c == user);
            self.calls.lock().unwrap().push(user.to_string());
            let ids: Vec<&str> = user.lines().filter_map(|l| l.strip_prefix('[').and_then(|r| r.split(']').next())).filter(|id| id.starts_with('w')).collect();
            let skip_last = first_ask && ids.len() > 1;
            let scores: Vec<serde_json::Value> = ids
                .iter()
                .take(if skip_last { ids.len() - 1 } else { ids.len() })
                .map(|id| {
                    let golds = self.texts.get(*id).map(|t| t.matches("gold").count()).unwrap_or(0);
                    serde_json::json!({"id": id, "score": (40 + 20 * golds).min(100), "reasons": [], "title": "t", "description": "d", "hashtags": ["#x"]})
                })
                .collect();
            Ok(serde_json::json!({ "scores": scores }).to_string())
        }
    }

    #[test]
    fn end_to_end_with_fake_model() {
        let mut t = windows::tests::synthetic(60, 6_000, 16, 200);
        // Make a few sentences special.
        for k in [10usize, 30, 50] {
            t.segments[k].text = format!("Why is gold gold gold the answer? {}", t.segments[k].text);
        }
        let ws = windows::windows(&t, &windows::WindowOptions::default());
        let texts = ws.iter().map(|w| (w.id.clone(), w.text.clone())).collect();
        let llm = FakeLlm { calls: Mutex::new(vec![]), texts };
        let (cands, report) = highlights(&t, None, &llm, &HighlightOptions { shortlist: 40, ..Default::default() }, &|_| {}).unwrap();
        assert!(report.windows > 0 && report.shortlisted <= 40);
        assert!(!cands.is_empty());
        // Top clips contain the special sentences and never overlap each other much.
        assert!(cands[0].score >= 80, "{:?}", cands[0]);
        for (i, a) in cands.iter().enumerate() {
            for b in &cands[i + 1..] {
                let inter = a.end_ms.min(b.end_ms).saturating_sub(a.start_ms.max(b.start_ms));
                assert!(inter * 100 <= (a.end_ms - a.start_ms).min(b.end_ms - b.start_ms) * 40);
            }
        }
        // Long transcript vs 1k-token context forced a map-reduce summary.
        let text_calls = llm.calls.lock().unwrap().iter().filter(|c| c.starts_with("text:")).count();
        assert!(text_calls > 1, "expected map-reduce, got {text_calls} summary calls");
        // Skipped ids were re-asked: every shortlisted window ended up scored.
        assert_eq!(report.scored, report.shortlisted);
    }

    #[test]
    fn prompt_mode_requires_a_match() {
        let t = windows::tests::synthetic(30, 6_000, 16, 200);
        let llm = FakeLlm { calls: Mutex::new(vec![]), texts: Default::default() };
        let opts = HighlightOptions { prompt: Some("zebras".into()), ..Default::default() };
        assert!(matches!(highlights(&t, None, &llm, &opts, &|_| {}), Err(HighlightError::NoMatches)));
    }
}

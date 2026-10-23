//! Highlights: transcript (+ audio features) in, ranked clip candidates out.
//!
//! windows → shortlist (prior or BM25) → episode summary → model scores in batches → rank.

use crate::ProgressFn;
use crate::model::{Candidate, Progress, Transcript};
use picker::{Llm, PickerError, Role, ScoreOut, Usage};
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// Shortlisted windows may overlap each other by at most this share of the shorter one,
    /// in percent. Lower spreads the shortlist across the whole episode.
    pub shortlist_max_overlap_pct: u8,
}

impl Default for HighlightOptions {
    fn default() -> Self {
        Self {
            windows: Default::default(),
            rank: Default::default(),
            prompt: None,
            // Measured on the golden set: 60 windows at 85% overlap left only 47% of golden clips
            // reachable by the model; 80 at 20% reaches 93% for 1.33x the scoring tokens.
            shortlist: 80,
            summary_words: 250,
            shortlist_max_overlap_pct: 20,
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
    pub usage: Usage,
}

#[derive(Debug, thiserror::Error)]
pub enum HighlightError {
    #[error("highlights: transcript has no usable windows (too short or too sparse)")]
    NoWindows,
    #[error("highlights: prompt matched nothing in the transcript")]
    NoMatches,
    #[error(
        "highlights: the picker failed on every batch ({declined} declined, {failed} failed); last error: {last}"
    )]
    AllBatchesFailed {
        declined: usize,
        failed: usize,
        last: String,
    },
    #[error("highlights: cancelled")]
    Cancelled,
    #[error(transparent)]
    Picker(#[from] PickerError),
}

/// Errors that mean the picker is misconfigured or unreachable: stop the job instead of
/// retrying the same failure on every batch.
fn is_fatal(e: &PickerError) -> bool {
    matches!(
        e,
        PickerError::NotConfigured(_)
            | PickerError::Unavailable { .. }
            | PickerError::Http {
                status: 400..=404 | 422,
                ..
            }
    )
}

fn fmt_ts(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60)
}

/// Summarise the whole transcript, map-reducing when it exceeds the model's context.
fn summarize(
    llm: &dyn Llm,
    t: &Transcript,
    words: usize,
    calls: &mut usize,
) -> Result<String, PickerError> {
    let system = picker::SUMMARIZE_PROMPT.replace("{max_words}", &words.to_string());
    let full: Vec<String> = t
        .segments
        .iter()
        .map(|s| format!("[{}] {}", fmt_ts(s.start_ms), s.text))
        .collect();
    let budget = llm
        .context_tokens()
        .saturating_sub(picker::estimate_tokens(&system) + words * 2 + 256);
    let joined = full.join("\n");
    if picker::estimate_tokens(&joined) <= budget {
        *calls += 1;
        match llm.text(Role::Summarize, &system, &joined) {
            // The estimate was low for this tokenizer: summarise in chunks instead.
            Err(PickerError::ContextTooLong(_)) => {}
            r => return r,
        }
    }
    // Map: summarise chunks that fit, then reduce the partial summaries. Chunks keep a quarter
    // of the budget spare: timestamps and names tokenize denser than the 4-chars estimate
    // (Qwen3.5 measured 3.4 on a podcast transcript).
    let budget = budget * 3 / 4;
    let chunk_words = words.min(120);
    let chunk_system = picker::SUMMARIZE_PROMPT.replace("{max_words}", &chunk_words.to_string());
    let mut partials = Vec::new();
    let mut cur = String::new();
    for line in full {
        if picker::estimate_tokens(&cur) + picker::estimate_tokens(&line) > budget
            && !cur.is_empty()
        {
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
        s.push_str(&format!(
            "The creator asked for clips about: {p}\nScore clips that are not about this low.\n\n"
        ));
    }
    s.push_str("Candidates:\n");
    for w in batch {
        s.push_str(&format!(
            "[{}] ({}–{}, {}s)\n{}\n\n",
            w.id,
            fmt_ts(w.start_ms),
            fmt_ts(w.end_ms),
            w.duration_ms() / 1000,
            w.text
        ));
    }
    s
}

/// Score one batch: strict parse, one retry on bad output, one re-ask for ids the model skipped.
/// Score a batch; when the model's context is too small for it, split and score the halves.
fn score_batch(
    llm: &dyn Llm,
    summary: &str,
    batch: &[&windows::Window],
    prompt: Option<&str>,
    calls: &mut usize,
    skipped: &mut Vec<String>,
) -> Result<Vec<ScoreOut>, PickerError> {
    match score_batch_once(llm, summary, batch, prompt, calls, skipped) {
        Err(PickerError::ContextTooLong(m)) if batch.len() > 1 => {
            let (a, b) = batch.split_at(batch.len() / 2);
            let mut out = score_batch(llm, summary, a, prompt, calls, skipped)?;
            out.extend(score_batch(llm, summary, b, prompt, calls, skipped)?);
            skipped.push(format!("batch split in two: {m}"));
            Ok(out)
        }
        Err(PickerError::ContextTooLong(m)) => {
            // A single window does not fit: drop the episode summary for it.
            let short = "(summary omitted: context limit)";
            skipped.push(format!(
                "window {} scored without the summary: {m}",
                batch[0].id
            ));
            score_batch_once(llm, short, batch, prompt, calls, skipped)
        }
        other => other,
    }
}

fn score_batch_once(
    llm: &dyn Llm,
    summary: &str,
    batch: &[&windows::Window],
    prompt: Option<&str>,
    calls: &mut usize,
    skipped: &mut Vec<String>,
) -> Result<Vec<ScoreOut>, PickerError> {
    let ids: Vec<String> = batch.iter().map(|w| w.id.clone()).collect();
    let user = batch_prompt(summary, batch, prompt);
    *calls += 1;
    let mut scores =
        match picker::parse_scores(&llm.scores_json(picker::SCORE_PROMPT, &user)?, &ids) {
            Ok(s) => s,
            Err(PickerError::Parse(e)) => {
                *calls += 1;
                let retry = format!(
                    "{user}\nYour previous reply could not be parsed ({e}). Reply with JSON only."
                );
                picker::parse_scores(&llm.scores_json(picker::SCORE_PROMPT, &retry)?, &ids)?
            }
            Err(e) => return Err(e),
        };
    let missing: Vec<&windows::Window> = batch
        .iter()
        .copied()
        .filter(|w| !scores.iter().any(|s| s.id == w.id))
        .collect();
    if !missing.is_empty() {
        let mids: Vec<String> = missing.iter().map(|w| w.id.clone()).collect();
        *calls += 1;
        match llm
            .scores_json(
                picker::SCORE_PROMPT,
                &batch_prompt(summary, &missing, prompt),
            )
            .and_then(|raw| picker::parse_scores(&raw, &mids))
        {
            Ok(more) => scores.extend(more),
            Err(e) if is_fatal(&e) => return Err(e),
            Err(e) => skipped.push(format!("re-ask for {} skipped ids failed: {e}", mids.len())),
        }
    }
    Ok(scores)
}

/// The windows the model would see, without calling it. For measuring the shortlist ceiling.
pub fn shortlist_only(
    t: &Transcript,
    audio: Option<&audio::AudioFeatures>,
    opts: &HighlightOptions,
) -> Vec<windows::Window> {
    let ws = windows::windows(t, &opts.windows);
    let scores: Vec<f32> = match &opts.prompt {
        Some(p) => search::bm25(&ws, p),
        None => ws
            .iter()
            .map(|w| search::prior(w, audio.map(|a| a.window(w.start_ms, w.end_ms)).as_ref()))
            .collect(),
    };
    search::shortlist_with(
        &ws,
        &scores,
        opts.shortlist,
        opts.shortlist_max_overlap_pct as f32 / 100.0,
    )
    .into_iter()
    .map(|i| ws[i].clone())
    .collect()
}

pub fn highlights(
    t: &Transcript,
    audio: Option<&audio::AudioFeatures>,
    llm: &dyn Llm,
    opts: &HighlightOptions,
    progress: ProgressFn,
    cancel: Option<&AtomicBool>,
) -> Result<(Vec<Candidate>, HighlightReport), HighlightError> {
    let cancelled = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    let mut report = HighlightReport::default();
    progress(Progress::new(STAGE, 0.0, "finding candidate clips"));
    let ws = windows::windows(t, &opts.windows);
    report.windows = ws.len();
    if ws.is_empty() {
        return Err(HighlightError::NoWindows);
    }

    // Prompt mode: only windows that actually match are candidates; the quality prior breaks
    // ties. Otherwise the model would be paid to score 80 unrelated windows for a 3-clip answer.
    let (ws, scores): (Vec<windows::Window>, Vec<f32>) = match &opts.prompt {
        Some(p) => {
            let b = search::bm25(&ws, p);
            let kept: Vec<(windows::Window, f32)> = ws
                .into_iter()
                .zip(b)
                .filter(|(_, s)| *s > 0.0)
                .map(|(w, s)| {
                    let prior =
                        search::prior(&w, audio.map(|a| a.window(w.start_ms, w.end_ms)).as_ref());
                    (w, s + 0.1 * prior)
                })
                .collect();
            if kept.is_empty() {
                return Err(HighlightError::NoMatches);
            }
            kept.into_iter().unzip()
        }
        None => {
            let s = ws
                .iter()
                .map(|w| search::prior(w, audio.map(|a| a.window(w.start_ms, w.end_ms)).as_ref()))
                .collect();
            (ws, s)
        }
    };
    let short = search::shortlist_with(
        &ws,
        &scores,
        opts.shortlist,
        opts.shortlist_max_overlap_pct as f32 / 100.0,
    );
    report.shortlisted = short.len();

    progress(Progress::new(
        STAGE,
        0.05,
        format!("summarizing episode with {}", llm.model(Role::Summarize)),
    ));
    let mut calls = 0;
    // The summary is context, not a requirement: if it fails (slow reasoning model, outage),
    // score without it rather than failing the whole job. Auth and config errors still stop.
    if cancelled() {
        return Err(HighlightError::Cancelled);
    }
    let summary = match summarize(llm, t, opts.summary_words, &mut calls) {
        Ok(s) => {
            // The model does not always respect the word limit; the summary is pasted into
            // every batch, so cap it.
            let words: Vec<&str> = s.split_whitespace().collect();
            if words.len() > opts.summary_words * 3 / 2 {
                words[..opts.summary_words * 3 / 2].join(" ")
            } else {
                s
            }
        }
        Err(e) if is_fatal(&e) => return Err(e.into()),
        Err(e) => {
            report.errors.push(format!("summary skipped: {e}"));
            String::from("(no summary available; judge each clip on its own)")
        }
    };
    report.summary = summary.clone();

    let picked: Vec<&windows::Window> = short.iter().map(|&i| &ws[i]).collect();
    let mut by_id: std::collections::HashMap<String, ScoreOut> = Default::default();
    let batches: Vec<&[&windows::Window]> = picked.chunks(llm.batch_size().max(1)).collect();
    let mut consecutive_failures = 0usize;
    let mut last_error = String::new();
    let mut skipped: Vec<String> = Vec::new();
    // Batches go out in waves of `concurrency`: a 72-min episode is ten batches, and waiting on
    // each in turn was most of the picker's wall time. Results are handled in batch order.
    let wave = llm.concurrency().max(1);
    'waves: for (wi, group) in batches.chunks(wave).enumerate() {
        if cancelled() {
            return Err(HighlightError::Cancelled);
        }
        let first = wi * wave;
        progress(Progress::new(
            STAGE,
            0.1 + 0.85 * first as f32 / batches.len() as f32,
            format!(
                "scoring batch {}/{} with {}",
                first + group.len(),
                batches.len(),
                llm.model(Role::Score)
            ),
        ));
        let summary = summary.as_str();
        let prompt = opts.prompt.as_deref();
        let results: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = group
                .iter()
                .map(|batch| {
                    s.spawn(move || {
                        let (mut c, mut sk) = (0, Vec::new());
                        let r = score_batch(llm, summary, batch, prompt, &mut c, &mut sk);
                        (r, c, sk)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("scoring thread panicked"))
                .collect()
        });
        for (j, (result, c, sk)) in results.into_iter().enumerate() {
            let bi = first + j;
            calls += c;
            skipped.extend(sk);
            match result {
                Ok(s) => {
                    consecutive_failures = 0;
                    s.into_iter().for_each(|x| {
                        by_id.insert(x.id.clone(), x);
                    })
                }
                Err(PickerError::Declined(m)) => {
                    consecutive_failures = 0;
                    report.declined_batches += 1;
                    if report.errors.len() < 8 {
                        report
                            .errors
                            .push(format!("batch {}: declined: {m}", bi + 1));
                    }
                }
                Err(e) if is_fatal(&e) => return Err(e.into()),
                Err(e) => {
                    report.failed_batches += 1;
                    consecutive_failures += 1;
                    last_error = e.to_string();
                    if report.errors.len() < 8 {
                        report.errors.push(format!("batch {}: {e}", bi + 1));
                    }
                    // Two failures in a row with nothing scored yet means the provider is not
                    // working for this job; stop before paying for every remaining batch.
                    if consecutive_failures >= 2 && by_id.is_empty() {
                        break 'waves;
                    }
                }
            }
        }
    }
    report.errors.extend(skipped.into_iter().take(4));
    report.model_calls = calls;
    report.usage = llm.usage();
    report.scored = by_id.len();
    if by_id.is_empty() {
        return Err(HighlightError::AllBatchesFailed {
            declined: report.declined_batches,
            failed: report.failed_batches,
            last: last_error,
        });
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
            self.calls
                .lock()
                .unwrap()
                .push(format!("text:{}", user.len()));
            Ok("a summary".into())
        }
        fn scores_json(&self, _: &str, user: &str) -> Result<String, PickerError> {
            let first_ask = !self.calls.lock().unwrap().iter().any(|c| c == user);
            self.calls.lock().unwrap().push(user.to_string());
            let ids: Vec<&str> = user
                .lines()
                .filter_map(|l| l.strip_prefix('[').and_then(|r| r.split(']').next()))
                .filter(|id| id.starts_with('w'))
                .collect();
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
            t.segments[k].text =
                format!("Why is gold gold gold the answer? {}", t.segments[k].text);
        }
        let ws = windows::windows(&t, &windows::WindowOptions::default());
        let texts = ws.iter().map(|w| (w.id.clone(), w.text.clone())).collect();
        let llm = FakeLlm {
            calls: Mutex::new(vec![]),
            texts,
        };
        let (cands, report) = highlights(
            &t,
            None,
            &llm,
            &HighlightOptions {
                shortlist: 40,
                ..Default::default()
            },
            &|_| {},
            None,
        )
        .unwrap();
        assert!(report.windows > 0 && report.shortlisted <= 40);
        assert!(!cands.is_empty());
        // Top clips contain the special sentences and never overlap each other much.
        assert!(cands[0].score >= 80, "{:?}", cands[0]);
        for (i, a) in cands.iter().enumerate() {
            for b in &cands[i + 1..] {
                let inter = a
                    .end_ms
                    .min(b.end_ms)
                    .saturating_sub(a.start_ms.max(b.start_ms));
                assert!(inter * 100 <= (a.end_ms - a.start_ms).min(b.end_ms - b.start_ms) * 40);
            }
        }
        // Long transcript vs 1k-token context forced a map-reduce summary.
        let text_calls = llm
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("text:"))
            .count();
        assert!(
            text_calls > 1,
            "expected map-reduce, got {text_calls} summary calls"
        );
        // Skipped ids were re-asked: every shortlisted window ended up scored.
        assert_eq!(report.scored, report.shortlisted);
    }

    /// Scores like FakeLlm, four batches at a time, recording how many calls overlap.
    struct Parallel {
        inner: FakeLlm,
        in_flight: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
    }
    impl Llm for Parallel {
        fn tier(&self) -> &'static str {
            "fake"
        }
        fn model(&self, r: Role) -> String {
            self.inner.model(r)
        }
        fn context_tokens(&self) -> usize {
            100_000
        }
        fn batch_size(&self) -> usize {
            3
        }
        fn concurrency(&self) -> usize {
            4
        }
        fn text(&self, r: Role, s: &str, u: &str) -> Result<String, PickerError> {
            self.inner.text(r, s, u)
        }
        fn scores_json(&self, s: &str, u: &str) -> Result<String, PickerError> {
            use std::sync::atomic::Ordering::SeqCst;
            let now = self.in_flight.fetch_add(1, SeqCst) + 1;
            self.peak.fetch_max(now, SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(30));
            let out = self.inner.scores_json(s, u);
            self.in_flight.fetch_sub(1, SeqCst);
            out
        }
    }

    #[test]
    fn hosted_batches_are_scored_side_by_side() {
        let t = windows::tests::synthetic(60, 6_000, 16, 200);
        let ws = windows::windows(&t, &windows::WindowOptions::default());
        let llm = Parallel {
            inner: FakeLlm {
                calls: Mutex::new(vec![]),
                texts: ws.iter().map(|w| (w.id.clone(), w.text.clone())).collect(),
            },
            in_flight: Default::default(),
            peak: Default::default(),
        };
        let opts = HighlightOptions {
            shortlist: 24,
            ..Default::default()
        };
        let (_, report) = highlights(&t, None, &llm, &opts, &|_| {}, None).unwrap();
        let peak = llm.peak.load(std::sync::atomic::Ordering::SeqCst);
        assert!((2..=4).contains(&peak), "peak {peak}");
        assert_eq!(report.scored, report.shortlisted);
    }

    /// Rejects any batch of more than one candidate as too long.
    struct Tiny(FakeLlm);
    impl Llm for Tiny {
        fn tier(&self) -> &'static str {
            "fake"
        }
        fn model(&self, r: Role) -> String {
            self.0.model(r)
        }
        fn context_tokens(&self) -> usize {
            100_000
        }
        fn text(&self, r: Role, s: &str, u: &str) -> Result<String, PickerError> {
            self.0.text(r, s, u)
        }
        fn scores_json(&self, s: &str, u: &str) -> Result<String, PickerError> {
            let n = u.lines().filter(|l| l.starts_with("[w")).count();
            if n > 1 {
                Err(PickerError::ContextTooLong("too long".into()))
            } else {
                self.0.scores_json(s, u)
            }
        }
    }

    #[test]
    fn oversized_batches_are_split_until_they_fit() {
        let t = windows::tests::synthetic(40, 6_000, 16, 200);
        let llm = Tiny(FakeLlm {
            calls: Mutex::new(vec![]),
            texts: Default::default(),
        });
        let (cands, report) = highlights(
            &t,
            None,
            &llm,
            &HighlightOptions {
                rank: rank::RankOptions {
                    min_score: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            &|_| {},
            None,
        )
        .unwrap();
        assert!(!cands.is_empty());
        assert_eq!(report.failed_batches, 0);
        assert_eq!(report.scored, report.shortlisted);
    }

    /// Summaries always fail; scoring works.
    struct NoSummary(FakeLlm);
    impl Llm for NoSummary {
        fn tier(&self) -> &'static str {
            "fake"
        }
        fn model(&self, r: Role) -> String {
            self.0.model(r)
        }
        fn context_tokens(&self) -> usize {
            100_000
        }
        fn text(&self, _: Role, _: &str, _: &str) -> Result<String, PickerError> {
            Err(PickerError::Transport("timeout: global".into()))
        }
        fn scores_json(&self, s: &str, u: &str) -> Result<String, PickerError> {
            self.0.scores_json(s, u)
        }
    }

    #[test]
    fn a_failed_summary_does_not_fail_the_job() {
        let t = windows::tests::synthetic(40, 6_000, 16, 200);
        let llm = NoSummary(FakeLlm {
            calls: Mutex::new(vec![]),
            texts: Default::default(),
        });
        let (cands, report) = highlights(
            &t,
            None,
            &llm,
            &HighlightOptions {
                rank: rank::RankOptions {
                    min_score: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            &|_| {},
            None,
        )
        .unwrap();
        assert!(!cands.is_empty());
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.starts_with("summary skipped"))
        );
    }

    #[test]
    fn prompt_mode_requires_a_match() {
        let t = windows::tests::synthetic(30, 6_000, 16, 200);
        let llm = FakeLlm {
            calls: Mutex::new(vec![]),
            texts: Default::default(),
        };
        let opts = HighlightOptions {
            prompt: Some("zebras".into()),
            ..Default::default()
        };
        assert!(matches!(
            highlights(&t, None, &llm, &opts, &|_| {}, None),
            Err(HighlightError::NoMatches)
        ));
    }
}

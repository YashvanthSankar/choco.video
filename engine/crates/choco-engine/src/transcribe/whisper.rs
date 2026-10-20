//! whisper.cpp transcriber. Default on Windows and on Macs older than macOS 26.
//! whisper.cpp windows long audio internally (30 s with context carry-over), so we feed
//! the whole file and rebuild words from sub-word tokens.

use super::{Result, STAGE, TranscribeError, TranscribeOptions};
use crate::ProgressFn;
use crate::backend::Device;
use crate::model::{Progress, Segment, Transcript, Word};
use crate::models_registry;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters,
};

fn werr(e: impl std::fmt::Display) -> TranscribeError {
    TranscribeError::Whisper(e.to_string())
}

pub fn transcribe(
    wav: &Path,
    device: Device,
    opts: &TranscribeOptions,
    progress: ProgressFn,
) -> Result<Transcript> {
    let manifest = models_registry::find(&opts.whisper_model)
        .ok_or_else(|| TranscribeError::ModelMissing(opts.whisper_model.clone()))?;
    if !models_registry::is_installed(&opts.models_dir, &manifest) {
        return Err(TranscribeError::ModelMissing(opts.whisper_model.clone()));
    }
    let model_path = models_registry::path_for(&opts.models_dir, &manifest);
    let audio = crate::ingest::read_wav_f32(wav)?;

    whisper_rs::install_logging_hooks();
    progress(Progress::new(
        STAGE,
        0.0,
        format!("loading {}", opts.whisper_model),
    ));
    let mut cparams = WhisperContextParameters::default();
    cparams.use_gpu(device != Device::Cpu);
    // Dynamic-time-warping token timestamps from the cross-attention heads: far tighter word
    // onsets than the default energy heuristic (measured in fixtures/timing.py). Needs the
    // preset that matches the model; unknown models keep the heuristic times.
    if let Some(preset) = dtw_preset(&opts.whisper_model) {
        cparams.dtw_parameters(DtwParameters {
            mode: DtwMode::ModelPreset {
                model_preset: preset,
            },
            ..Default::default()
        });
    }
    let ctx = WhisperContext::new_with_params(&model_path, cparams).map_err(werr)?;
    let mut state = ctx.create_state().map_err(werr)?;

    // whisper.cpp wants a two-letter code or "auto".
    let lang = opts
        .language
        .as_deref()
        .map(|l| l.split(['-', '_']).next().unwrap_or(l).to_lowercase());
    let lang_ref = lang.as_deref().unwrap_or("auto");

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8) as i32;
    // whisper-rs leaks the boxed progress closure (no Drop on FullParams), so anything it
    // captures lives forever. A channel sender inside it would never close; a shared atomic
    // polled until the worker finishes cannot deadlock and leaks only one small Arc.
    let pct = Arc::new(AtomicI32::new(0));
    let pct_cb = Arc::clone(&pct);
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(threads);
    params.set_language(Some(lang_ref));
    params.set_token_timestamps(true);
    params.set_no_context(true); // fewer runaway repeats across windows
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    params.set_progress_callback_safe(move |p: i32| pct_cb.store(p, Ordering::Relaxed));

    let result = std::thread::scope(|s| {
        let worker = s.spawn(|| state.full(params, &audio).map(|_| ()));
        let mut last = -1;
        while !worker.is_finished() {
            let p = pct.load(Ordering::Relaxed);
            if p != last {
                progress(Progress::new(STAGE, p as f32 / 100.0, "transcribing"));
                last = p;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        worker.join()
    });
    match result {
        Ok(r) => r.map_err(werr)?,
        Err(_) => return Err(werr("whisper worker panicked")),
    }

    let language = whisper_rs::get_lang_str(state.full_lang_id_from_state())
        .unwrap_or(lang_ref)
        .to_string();
    let mut segments = Vec::new();
    for seg in state.as_iter() {
        let mut words: Vec<Word> = Vec::new();
        // Whisper tokens are byte pieces: one Devanagari or CJK character can span several
        // tokens, so decode each word's bytes once, not each token (per-token lossy decoding
        // turned every non-Latin script into replacement characters).
        let mut cur: Vec<u8> = Vec::new();
        let (mut cur_start, mut cur_end, mut cur_conf) = (0u64, 0u64, 1.0f32);
        let flush = |cur: &mut Vec<u8>, start: u64, end: u64, conf: f32, words: &mut Vec<Word>| {
            let text = String::from_utf8_lossy(cur).trim().to_string();
            if !text.is_empty() {
                words.push(Word {
                    start_ms: start,
                    end_ms: end.max(start + 1),
                    text,
                    confidence: conf,
                });
            }
            cur.clear();
        };
        for t in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(t) else {
                continue;
            };
            let Ok(bytes) = tok.to_bytes() else { continue };
            if bytes.starts_with(b"[_") || bytes.starts_with(b"<|") {
                continue; // special token
            }
            let data = tok.token_data();
            let (mut t0, t1) = ((data.t0.max(0) as u64) * 10, (data.t1.max(0) as u64) * 10);
            if data.t_dtw >= 0 {
                t0 = data.t_dtw as u64 * 10;
            }
            let starts_word = bytes.starts_with(b" ") || cur.is_empty();
            if starts_word && !cur.is_empty() {
                flush(&mut cur, cur_start, cur_end, cur_conf, &mut words);
            }
            if cur.is_empty() {
                cur_start = t0;
                cur_end = t1;
                cur_conf = data.p;
            } else {
                cur_end = t1.max(cur_end);
                cur_conf = cur_conf.min(data.p);
            }
            cur.extend_from_slice(bytes);
        }
        flush(&mut cur, cur_start, cur_end, cur_conf, &mut words);
        let text = seg
            .to_str_lossy()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if words.is_empty() && text.is_empty() {
            continue;
        }
        segments.push(Segment {
            start_ms: (seg.start_timestamp().max(0) as u64) * 10,
            end_ms: (seg.end_timestamp().max(0) as u64) * 10,
            text,
            words,
        });
    }
    progress(Progress::new(STAGE, 1.0, "transcribed"));
    Ok(Transcript {
        language,
        engine: format!("whisper:{}", manifest.id.trim_start_matches("whisper-")),
        segments,
    })
}

/// The whisper.cpp DTW alignment-head preset for a registry model id, if it is a standard model.
fn dtw_preset(model_id: &str) -> Option<DtwModelPreset> {
    let id = model_id.to_lowercase();
    Some(if id.contains("large-v3-turbo") {
        DtwModelPreset::LargeV3Turbo
    } else if id.contains("large-v3") {
        DtwModelPreset::LargeV3
    } else if id.contains("large-v2") {
        DtwModelPreset::LargeV2
    } else if id.contains("medium.en") {
        DtwModelPreset::MediumEn
    } else if id.contains("medium") {
        DtwModelPreset::Medium
    } else if id.contains("small.en") {
        DtwModelPreset::SmallEn
    } else if id.contains("small") {
        DtwModelPreset::Small
    } else if id.contains("base.en") {
        DtwModelPreset::BaseEn
    } else if id.contains("base") {
        DtwModelPreset::Base
    } else if id.contains("tiny.en") {
        DtwModelPreset::TinyEn
    } else if id.contains("tiny") {
        DtwModelPreset::Tiny
    } else {
        return None;
    })
}

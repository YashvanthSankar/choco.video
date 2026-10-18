//! whisper.cpp transcriber. Default on Windows and on Macs older than macOS 26.
//! whisper.cpp windows long audio internally (30 s with context carry-over), so we feed
//! the whole file and rebuild words from sub-word tokens.

use super::{Result, TranscribeError, TranscribeOptions, STAGE};
use crate::backend::Device;
use crate::model::{Progress, Segment, Transcript, Word};
use crate::models_registry;
use crate::ProgressFn;
use std::path::Path;
use std::sync::mpsc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn werr(e: impl std::fmt::Display) -> TranscribeError {
    TranscribeError::Whisper(e.to_string())
}

pub fn transcribe(wav: &Path, device: Device, opts: &TranscribeOptions, progress: ProgressFn) -> Result<Transcript> {
    let manifest = models_registry::find(&opts.whisper_model).ok_or_else(|| TranscribeError::ModelMissing(opts.whisper_model.clone()))?;
    if !models_registry::is_installed(&opts.models_dir, &manifest) {
        return Err(TranscribeError::ModelMissing(opts.whisper_model.clone()));
    }
    let model_path = models_registry::path_for(&opts.models_dir, &manifest);
    let audio = crate::ingest::read_wav_f32(wav)?;

    whisper_rs::install_logging_hooks();
    progress(Progress::new(STAGE, 0.0, format!("loading {}", opts.whisper_model)));
    let mut cparams = WhisperContextParameters::default();
    cparams.use_gpu(device != Device::Cpu);
    let ctx = WhisperContext::new_with_params(&model_path, cparams).map_err(werr)?;
    let mut state = ctx.create_state().map_err(werr)?;

    // whisper.cpp wants a two-letter code or "auto".
    let lang = opts.language.as_deref().map(|l| l.split(['-', '_']).next().unwrap_or(l).to_lowercase());
    let lang_ref = lang.as_deref().unwrap_or("auto");

    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8) as i32;
    let (tx, rx) = mpsc::channel::<i32>();
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
    params.set_progress_callback_safe(move |p: i32| {
        let _ = tx.send(p);
    });

    let result = std::thread::scope(|s| {
        let worker = s.spawn(|| state.full(params, &audio).map(|_| ()));
        for p in rx {
            progress(Progress::new(STAGE, p as f32 / 100.0, "transcribing"));
        }
        worker.join()
    });
    match result {
        Ok(r) => r.map_err(werr)?,
        Err(_) => return Err(werr("whisper worker panicked")),
    }

    let language = whisper_rs::get_lang_str(state.full_lang_id_from_state()).unwrap_or(lang_ref).to_string();
    let mut segments = Vec::new();
    for seg in state.as_iter() {
        let mut words: Vec<Word> = Vec::new();
        for t in 0..seg.n_tokens() {
            let Some(tok) = seg.get_token(t) else { continue };
            let Ok(text) = tok.to_str_lossy() else { continue };
            if text.starts_with("[_") || text.starts_with("<|") {
                continue; // special token
            }
            let data = tok.token_data();
            let (t0, t1) = ((data.t0.max(0) as u64) * 10, (data.t1.max(0) as u64) * 10);
            let starts_word = text.starts_with(' ') || words.is_empty();
            let piece = text.trim();
            if piece.is_empty() {
                continue;
            }
            if starts_word {
                words.push(Word { start_ms: t0, end_ms: t1.max(t0 + 1), text: piece.to_string(), confidence: data.p });
            } else if let Some(w) = words.last_mut() {
                w.text.push_str(piece);
                w.end_ms = t1.max(w.end_ms);
                w.confidence = w.confidence.min(data.p);
            }
        }
        let text = seg.to_str_lossy().map(|s| s.trim().to_string()).unwrap_or_default();
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
    Ok(Transcript { language, engine: format!("whisper:{}", manifest.id.trim_start_matches("whisper-")), segments })
}

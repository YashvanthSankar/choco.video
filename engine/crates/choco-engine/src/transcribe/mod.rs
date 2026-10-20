//! Transcribe: speech to a word-timed [`Transcript`]. Two engines, one output type.

use crate::backend::{Device, SpeechEngine};
use crate::model::Transcript;
use crate::models_registry;
use crate::{Progress, ProgressFn};
use std::path::{Path, PathBuf};

pub mod apple;
pub mod cleanup;
#[cfg(feature = "whisper")]
pub mod whisper;

pub(crate) const STAGE: &str = "transcribe";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TranscribeOptions {
    /// BCP-47 tag such as "en-US". Apple speech requires one; whisper auto-detects when None.
    pub language: Option<String>,
    /// Whisper model id from the registry. Ignored by Apple speech.
    pub whisper_model: String,
    /// Directory holding downloaded models.
    pub models_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    #[error("transcribe: engine {0:?} is not available in this build or on this OS")]
    Unavailable(SpeechEngine),
    #[error("transcribe: model {0} is not downloaded; run `choco models pull {0}`")]
    ModelMissing(String),
    #[error("transcribe: apple speech: {0}")]
    Apple(String),
    #[error("transcribe: whisper: {0}")]
    Whisper(String),
    #[error("transcribe: io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TranscribeError>;

/// Transcribe a 16 kHz mono f32 WAV produced by [`crate::ingest::extract_audio`].
pub fn transcribe(audio_wav: &Path, engine: SpeechEngine, opts: &TranscribeOptions, progress: ProgressFn) -> Result<Transcript> {
    let raw = match engine {
        SpeechEngine::AppleSpeech => match apple::transcribe(audio_wav, opts, progress) {
            // Apple speech covers ~45 locales. For any other language fall back to whisper when
            // its model is on disk, otherwise say exactly what to do.
            #[cfg(feature = "whisper")]
            Err(TranscribeError::Apple(m)) if m.contains("is not supported by Apple speech") => {
                let manifest = models_registry::find(&opts.whisper_model);
                if manifest.is_some_and(|mf| models_registry::is_installed(&opts.models_dir, &mf)) {
                    progress(Progress::new(STAGE, 0.0, format!("{m}; using whisper")));
                    let raw = whisper::transcribe(audio_wav, Device::Metal, opts, progress)?;
                    let mut t = cleanup::clean(raw);
                    calibrate(&mut t, SpeechEngine::Whisper { device: Device::Metal });
                    refine_onsets(&mut t, audio_wav);
                    return Ok(t);
                }
                return Err(TranscribeError::Apple(format!("{m}; download the whisper model to transcribe this language (`choco models pull {}`)", opts.whisper_model)));
            }
            r => r?,
        },
        #[cfg(feature = "whisper")]
        SpeechEngine::Whisper { device } => whisper::transcribe(audio_wav, device, opts, progress)?,
        #[cfg(not(feature = "whisper"))]
        e @ SpeechEngine::Whisper { .. } => return Err(TranscribeError::Unavailable(e)),
    };
    let mut t = cleanup::clean(raw);
    calibrate(&mut t, engine);
    refine_onsets(&mut t, audio_wav);
    Ok(t)
}

/// Snap each word start to the nearest audio onset within a short window. Both engines place a
/// word within ~100 ms of where the voice actually starts; the energy rise says exactly where.
/// Only words preceded by a dip in energy are moved (a word mid-phrase has no onset to find).
fn refine_onsets(t: &mut Transcript, audio_wav: &Path) {
    let Ok(audio) = crate::ingest::read_wav_f32(audio_wav) else { return };
    const HOP: usize = 80; // 5 ms frames at 16 kHz
    let frames: Vec<f32> = audio.chunks(HOP).map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len().max(1) as f32).sqrt()).collect();
    if frames.len() < 40 {
        return;
    }
    let noise = {
        let mut v = frames.clone();
        v.sort_by(f32::total_cmp);
        v[v.len() / 10].max(1e-4)
    };
    const WIN_MS: i64 = 90;
    for s in &mut t.segments {
        for w in &mut s.words {
            let center = w.start_ms as i64 / 5;
            let lo = (center - WIN_MS / 5).max(2) as usize;
            let hi = ((center + WIN_MS / 5) as usize).min(frames.len() - 2);
            if lo >= hi {
                continue;
            }
            // Candidate onset: the frame with the largest rise over the previous 20 ms whose
            // level before the rise is near the noise floor or well below the level after it.
            let mut best: Option<(usize, f32)> = None;
            for i in lo..hi {
                let before = (frames[i.saturating_sub(4)..i].iter().sum::<f32>() / 4.0).max(1e-5);
                let after = frames[i..(i + 4).min(frames.len())].iter().sum::<f32>() / 4.0;
                let rise = after / before;
                if after > noise * 3.0 && rise > 2.5 && best.is_none_or(|b| rise > b.1) {
                    best = Some((i, rise));
                }
            }
            if let Some((i, _)) = best {
                let onset = (i * 5) as u64;
                let dur = w.end_ms.saturating_sub(w.start_ms);
                w.start_ms = onset;
                w.end_ms = w.end_ms.max(onset + dur.clamp(1, 40));
            }
        }
        if let Some(f) = s.words.first() {
            s.start_ms = s.start_ms.min(f.start_ms);
        }
    }
}

/// Measured systematic bias of each engine's word start times against synthesized ground truth
/// (`fixtures/say_aligned.swift`, three voices; `fixtures/timing.py`). Apple stamps words about
/// 100 ms early. Whisper's DTW token times mark the end of each token's attention peak, so they
/// run about 200 ms late with a tight spread; shift them back by that median bias.
pub fn timing_offset_ms(engine: SpeechEngine) -> i64 {
    match engine {
        SpeechEngine::AppleSpeech => 100,
        SpeechEngine::Whisper { .. } => -200,
    }
}

fn calibrate(t: &mut Transcript, engine: SpeechEngine) {
    let off = timing_offset_ms(engine);
    let shift = |ms: u64| (ms as i64 + off).max(0) as u64;
    for s in &mut t.segments {
        s.start_ms = shift(s.start_ms);
        s.end_ms = shift(s.end_ms);
        for w in &mut s.words {
            w.start_ms = shift(w.start_ms);
            w.end_ms = shift(w.end_ms);
        }
    }
}

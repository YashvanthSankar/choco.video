//! Transcribe: speech to a word-timed [`Transcript`]. Two engines, one output type.

use crate::backend::SpeechEngine;
use crate::model::Transcript;
use crate::ProgressFn;
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
        SpeechEngine::AppleSpeech => apple::transcribe(audio_wav, opts, progress)?,
        #[cfg(feature = "whisper")]
        SpeechEngine::Whisper { device } => whisper::transcribe(audio_wav, device, opts, progress)?,
        #[cfg(not(feature = "whisper"))]
        e @ SpeechEngine::Whisper { .. } => return Err(TranscribeError::Unavailable(e)),
    };
    let mut t = cleanup::clean(raw);
    calibrate(&mut t, engine);
    Ok(t)
}

/// Measured systematic bias of each engine's word start times against synthesized ground truth
/// (`fixtures/say_aligned.swift`, three voices). Both engines stamp words early; captions are
/// driven by these times, so shift them by the median bias.
pub fn timing_offset_ms(engine: SpeechEngine) -> u64 {
    match engine {
        SpeechEngine::AppleSpeech => 100,
        SpeechEngine::Whisper { .. } => 260,
    }
}

fn calibrate(t: &mut Transcript, engine: SpeechEngine) {
    let off = timing_offset_ms(engine);
    for s in &mut t.segments {
        s.start_ms += off;
        s.end_ms += off;
        for w in &mut s.words {
            w.start_ms += off;
            w.end_ms += off;
        }
    }
}

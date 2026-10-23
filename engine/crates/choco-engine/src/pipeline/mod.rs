//! The whole job, used by both the CLI and the app: probe, transcribe (or reuse), audio
//! features, highlights, export. Stage progress is reported as one stream.

use crate::backend::{self, SpeechEngine};
use crate::captions::{Rasterizer, translate};
use crate::export::{self, ExportOptions, ExportedClip, Job};
use crate::highlights::{self, HighlightOptions, HighlightReport, audio, picker::Llm};
use crate::model::{Candidate, MediaAsset, Progress, Transcript};
use crate::transcribe::{self, TranscribeOptions};
use crate::{ProgressFn, ingest, models_registry};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Ingest(#[from] ingest::IngestError),
    #[error(transparent)]
    Transcribe(#[from] transcribe::TranscribeError),
    #[error(transparent)]
    Highlights(#[from] highlights::HighlightError),
    #[error(transparent)]
    Export(#[from] export::ExportError),
    #[error(transparent)]
    Translate(#[from] translate::TranslateError),
    #[error("pipeline: no clip scored at least {0}; lower the minimum score or try another picker")]
    NoClips(u8),
    #[error("pipeline: cancelled")]
    Cancelled,
    #[error("pipeline: io: {0}")]
    Io(#[from] std::io::Error),
    #[error("pipeline: json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub highlights: HighlightOptions,
    pub export: ExportOptions,
    /// Reuse this transcript instead of transcribing.
    pub transcript: Option<Transcript>,
    /// BCP-47 language; None lets the engine decide.
    pub language: Option<String>,
    /// Force a speech engine; None picks the best available.
    pub speech: Option<SpeechEngine>,
    /// Translate each clip's captions into this language (BCP-47 or a name) with the picker
    /// model before export. The transcript JSON stays in the original language.
    pub translate: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub source: MediaAsset,
    pub transcript_path: PathBuf,
    pub picker: HighlightReport,
    pub clips: Vec<ExportedClip>,
    /// Final Cut Pro XML cutting every clip from the source; None for a source without video.
    pub fcpxml_path: Option<PathBuf>,
    /// Final Cut Pro 7 XML (xmeml) of the same cuts, for Premiere Pro; None without video.
    pub premiere_xml_path: Option<PathBuf>,
    pub summary_path: PathBuf,
}

fn check(cancel: &AtomicBool) -> Result<(), PipelineError> {
    if cancel.load(Ordering::Relaxed) {
        Err(PipelineError::Cancelled)
    } else {
        Ok(())
    }
}

pub fn run(
    file: &Path,
    opts: &RunOptions,
    llm: &dyn Llm,
    progress: ProgressFn,
    cancel: &AtomicBool,
) -> Result<RunResult, PipelineError> {
    let asset = ingest::probe(file)?;
    std::fs::create_dir_all(&opts.export.out_dir)?;
    let stem = file
        .file_stem()
        .map(|s| export::slug(&s.to_string_lossy()))
        .unwrap_or_else(|| "video".into());
    let tmp = TempDir::new()?;
    let wav = tmp.path.join("audio.wav");
    ingest::extract_audio(&asset, &wav, progress)?;
    check(cancel)?;
    let feats = audio::analyze(&wav)?;

    let transcript_path = opts.export.out_dir.join(format!("{stem}.transcript.json"));
    let t = match &opts.transcript {
        Some(t) => t.clone(),
        None => {
            let b = backend::detect();
            let engine = opts.speech.unwrap_or(b.speech_engines()[0]);
            let topts = TranscribeOptions {
                language: opts.language.clone(),
                whisper_model: b
                    .recommended_models()
                    .whisper_model
                    .unwrap_or_else(|| models_registry::WHISPER_DEFAULT.into()),
                models_dir: models_registry::default_dir(),
            };
            transcribe::transcribe(&wav, engine, &topts, progress)?
        }
    };
    std::fs::write(&transcript_path, serde_json::to_string(&t)?)?;
    drop(tmp);
    check(cancel)?;

    let (cands, report) = highlights::highlights(
        &t,
        Some(&feats),
        llm,
        &opts.highlights,
        progress,
        Some(cancel),
    )?;
    if cands.is_empty() {
        return Err(PipelineError::NoClips(opts.highlights.rank.min_score));
    }
    let mut raster = Rasterizer::new();
    // Translate every clip before any rendering, so a local picker model and the renderer
    // never overlap. A translation the bundled fonts cannot draw would burn in as boxes, so
    // that clip keeps its translated SRT and VTT and renders without burned captions.
    let bare = ExportOptions {
        style: None,
        ..opts.export.clone()
    };
    let mut translated = Vec::new();
    if let Some(lang) = &opts.translate {
        let n = cands.len();
        for (i, c) in cands.iter().enumerate() {
            check(cancel)?;
            let p = |m: String| progress(Progress::new("translate", i as f32 / n as f32, m));
            p(format!("clip {} of {n} into {lang}", i + 1));
            let tr = translate::translate_range(&t, (c.start_ms, c.end_ms), lang, llm)?;
            if !tr.untranslated.is_empty() {
                p(format!(
                    "clip {}: kept {} of {} lines untranslated (model output unusable)",
                    i + 1,
                    tr.untranslated.len(),
                    tr.transcript.segments.len()
                ));
            }
            let st = opts.export.style.as_ref();
            let drawable = st.is_none_or(|st| raster.can_draw(&tr.transcript.text(), st));
            if !drawable {
                p(format!(
                    "clip {}: the bundled caption fonts cannot draw {lang}; captions go to the SRT and VTT only, not burned in",
                    i + 1
                ));
            }
            translated.push((tr.transcript, drawable));
        }
    }
    // The model is done: a local picker's memory goes back before the renderer needs it.
    llm.unload();
    let mut job = Job {
        raster: &mut raster,
        progress,
        cancel,
    };
    let mut clips = Vec::new();
    for (i, c) in cands.iter().enumerate() {
        check(cancel)?;
        progress(Progress::new(
            "export",
            i as f32 / cands.len() as f32,
            format!("clip {} of {}: {}", i + 1, cands.len(), c.title),
        ));
        let (captions, eopts) = match translated.get(i) {
            Some((tr, true)) => (tr, &opts.export),
            Some((tr, false)) => (tr, &bare),
            None => (&t, &opts.export),
        };
        clips.push(export::export_clip(
            &asset,
            captions,
            c,
            i + 1,
            eopts,
            &mut job,
        )?);
    }
    let ranked: Vec<(usize, &Candidate)> = clips.iter().map(|c| (c.rank, &c.candidate)).collect();
    let fcpxml_path = match export::fcpxml::fcpxml(&asset, &ranked) {
        Some(doc) => {
            let p = opts.export.out_dir.join(format!("{stem}.fcpxml"));
            std::fs::write(&p, doc)?;
            Some(p)
        }
        None => None,
    };
    let premiere_xml_path = match export::xmeml::xmeml(&asset, &ranked) {
        Some(doc) => {
            let p = opts.export.out_dir.join(format!("{stem}.xml"));
            std::fs::write(&p, doc)?;
            Some(p)
        }
        None => None,
    };
    let summary_path = opts.export.out_dir.join(format!("{stem}.run.json"));
    let result = RunResult {
        source: asset,
        transcript_path,
        picker: report,
        clips,
        fcpxml_path,
        premiere_xml_path,
        summary_path: summary_path.clone(),
    };
    std::fs::write(&summary_path, serde_json::to_string_pretty(&result)?)?;
    progress(Progress::new(
        "export",
        1.0,
        format!("{} clips", result.clips.len()),
    ));
    Ok(result)
}

/// A private temporary directory, removed when dropped (also on error and cancel paths).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "choco-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

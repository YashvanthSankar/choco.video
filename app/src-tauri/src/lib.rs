//! choco.video desktop app: a thin Tauri shell over `choco-engine`. Every pipeline step is the
//! same code the CLI and the engine tests run.

mod settings;

use choco_engine::backend::{self, SpeechEngine};
use choco_engine::captions;
use choco_engine::export::{self, ExportOptions, ExportedClip, Job};
use choco_engine::highlights::HighlightOptions;
use choco_engine::highlights::picker::{Llm, apple::AppleLlm, openai_compat};
use choco_engine::pipeline::{self, RunOptions};
use choco_engine::{Layout, Progress, Transcript, models_registry};
use serde::Serialize;
use settings::Settings;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct Jobs {
    next: AtomicU64,
    cancel: Mutex<HashMap<u64, Arc<AtomicBool>>>,
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    job: u64,
    stage: String,
    fraction: f32,
    message: String,
}

#[derive(Serialize)]
struct Status {
    settings: Settings,
    has_key: bool,
    speech_engines: Vec<SpeechEngine>,
    apple: Option<choco_engine::transcribe::apple::AppleStatus>,
    whisper_needed: Option<String>,
    whisper_installed: bool,
    hardware: backend::HardwareInfo,
    engine_version: &'static str,
    app_version: String,
    recommended: &'static [openai_compat::Recommended],
    styles: Vec<String>,
    presets: &'static [&'static str],
}

#[tauri::command]
fn status(app: AppHandle) -> Result<Status, String> {
    let dir = config_dir(&app)?;
    let b = backend::detect();
    let rec = b.recommended_models();
    let whisper_installed = rec
        .whisper_model
        .as_deref()
        .and_then(models_registry::find)
        .map(|m| models_registry::is_installed(&models_registry::default_dir(), &m))
        .unwrap_or(true);
    Ok(Status {
        settings: Settings::load(&dir),
        has_key: settings::key_get().is_some(),
        speech_engines: b.speech_engines(),
        apple: choco_engine::transcribe::apple::status(),
        whisper_needed: rec.whisper_model,
        whisper_installed,
        hardware: b.info(),
        engine_version: choco_engine::VERSION,
        app_version: app.package_info().version.to_string(),
        recommended: openai_compat::OPENCODE_RECOMMENDED,
        styles: captions::presets().into_iter().map(|s| s.name).collect(),
        presets: export::PRESETS,
    })
}

/// What a bug report needs about this machine and setup, as JSON for "Copy diagnostics":
/// hardware, encoders, speech engines and the chosen picker. Never the key, file paths, media or
/// transcripts; nothing is sent anywhere.
#[tauri::command]
fn diagnostics(app: AppHandle) -> Result<serde_json::Value, String> {
    let s = Settings::load(&config_dir(&app)?);
    let b = backend::detect();
    let rec = b.recommended_models();
    let host = s
        .provider
        .base_url
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    Ok(serde_json::json!({
        "app_version": app.package_info().version.to_string(),
        "engine_version": choco_engine::VERSION,
        "hardware": b.info(),
        "gpus": b.gpus(),
        "encoders": b.hw_encoders(),
        "speech_engines": b.speech_engines(),
        "speech_setting": s.speech,
        "whisper_model": rec.whisper_model,
        "whisper_installed": rec.whisper_model.as_deref().and_then(models_registry::find)
            .is_some_and(|m| models_registry::is_installed(&models_registry::default_dir(), &m)),
        "apple": choco_engine::transcribe::apple::status(),
        "picker": { "kind": s.provider.kind, "host": host, "score_model": s.provider.score_model, "summary_model": s.provider.summary_model },
        "has_key": settings::key_get().is_some(),
    }))
}

#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

/// Ask GitHub whether a newer signed release exists. Only called when the user clicks "Check for
/// updates" or opted in to checking at launch.
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body,
    }))
}

/// Download the update, verify its signature against the public key in tauri.conf.json, install
/// it over this app and restart.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let Some(update) = update else { return Ok(()) };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    settings.save(&config_dir(&app)?)
}

#[tauri::command]
fn set_api_key(key: String) -> Result<(), String> {
    if key.trim().is_empty() {
        settings::key_delete()
    } else {
        settings::key_set(&key)
    }
}

/// Model ids at `base_url`, using `key` if given, else the stored one.
#[tauri::command]
async fn provider_models(base_url: String, key: Option<String>) -> Result<Vec<String>, String> {
    let key = key
        .filter(|k| !k.trim().is_empty())
        .or_else(settings::key_get)
        .ok_or("No API key yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        openai_compat::list_models(&base_url, &key).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One real scoring request with `model`. Returns seconds taken.
#[tauri::command]
async fn provider_test(
    base_url: String,
    model: String,
    key: Option<String>,
) -> Result<f64, String> {
    let key = key
        .filter(|k| !k.trim().is_empty())
        .or_else(settings::key_get)
        .ok_or("No API key yet")?;
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = openai_compat::OpenAiCompatConfig {
            tier: "provider",
            base_url,
            api_key: key,
            summarize_model: model.clone(),
            score_model: model,
            context_tokens: 64_000,
            timeout: std::time::Duration::from_secs(60),
        };
        openai_compat::test_connection(cfg)
            .map(|d| d.as_secs_f64())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn make_llm(s: &Settings) -> Result<Box<dyn Llm>, String> {
    match s.provider.kind.as_str() {
        "apple" => {
            if !choco_engine::highlights::picker::apple::available() {
                return Err("Apple Intelligence is not available on this Mac. Choose OpenCode or another provider in Settings.".into());
            }
            Ok(Box::new(AppleLlm))
        }
        _ => {
            let key = settings::key_get().ok_or("Add your provider API key in Settings")?;
            if s.provider.score_model.is_empty() {
                return Err("Choose a scoring model in Settings".into());
            }
            Ok(Box::new(openai_compat::OpenAiCompat::new(
                openai_compat::OpenAiCompatConfig {
                    tier: "provider",
                    base_url: s.provider.base_url.clone(),
                    api_key: key,
                    score_model: s.provider.score_model.clone(),
                    summarize_model: if s.provider.summary_model.is_empty() {
                        s.provider.score_model.clone()
                    } else {
                        s.provider.summary_model.clone()
                    },
                    context_tokens: 64_000,
                    timeout: std::time::Duration::from_secs(420),
                },
            )))
        }
    }
}

#[derive(serde::Deserialize, Debug)]
struct JobRequest {
    file: String,
    prompt: Option<String>,
    style: String,
    preset: String,
    layout: String,
    max_clips: usize,
    min_score: u8,
    output_dir: String,
}

fn layout_of(name: &str) -> (Layout, bool) {
    match name {
        "static" => (Layout::Static, false),
        "fit" => (Layout::Fit, false),
        _ => (Layout::Single, true),
    }
}

fn export_options(
    style: &str,
    preset: &str,
    layout: &str,
    out: PathBuf,
) -> Result<ExportOptions, String> {
    if export::preset_size(preset).is_none() {
        return Err(format!("Unknown size preset {preset}"));
    }
    let (layout, auto_reframe) = layout_of(layout);
    let style = if style == "none" {
        None
    } else {
        Some(captions::preset(style).ok_or(format!("Unknown caption style {style}"))?)
    };
    Ok(ExportOptions {
        preset: preset.into(),
        style,
        layout,
        auto_reframe,
        out_dir: out,
        ..Default::default()
    })
}

/// Start the whole pipeline on a worker thread. Progress arrives as `job-progress` events,
/// then one `job-done` (RunResult) or `job-error` (message).
#[tauri::command]
fn start_job(app: AppHandle, jobs: State<'_, Arc<Jobs>>, req: JobRequest) -> Result<u64, String> {
    let s = Settings::load(&config_dir(&app)?);
    let llm = make_llm(&s)?;
    let file = PathBuf::from(&req.file);
    if !file.is_file() {
        return Err(format!("File not found: {}", req.file));
    }
    let stem = file
        .file_stem()
        .map(|x| export::slug(&x.to_string_lossy()))
        .unwrap_or_else(|| "video".into());
    let out = PathBuf::from(&req.output_dir).join(&stem);
    allow_media(&app, &file, &out);
    let mut hopts = HighlightOptions {
        prompt: req.prompt.filter(|p| !p.trim().is_empty()),
        ..Default::default()
    };
    hopts.rank.max_clips = req.max_clips.clamp(1, 30);
    hopts.rank.min_score = req.min_score;
    let speech = match s.speech.as_str() {
        "apple" => Some(SpeechEngine::AppleSpeech),
        "whisper" => backend::detect()
            .speech_engines()
            .into_iter()
            .find(|e| matches!(e, SpeechEngine::Whisper { .. })),
        _ => None,
    };
    let opts = RunOptions {
        highlights: hopts,
        export: export_options(&req.style, &req.preset, &req.layout, out)?,
        transcript: None,
        language: (!s.language.is_empty()).then(|| s.language.clone()),
        speech,
    };
    let id = jobs.next.fetch_add(1, Ordering::Relaxed) + 1;
    let cancel = Arc::new(AtomicBool::new(false));
    jobs.cancel
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id, cancel.clone());
    let jobs = jobs.inner().clone();
    std::thread::spawn(move || {
        let app2 = app.clone();
        let progress = move |p: Progress| {
            let _ = app2.emit(
                "job-progress",
                ProgressEvent {
                    job: id,
                    stage: p.stage,
                    fraction: p.fraction,
                    message: p.message,
                },
            );
        };
        let result = pipeline::run(&file, &opts, llm.as_ref(), &progress, &cancel);
        if let Ok(mut m) = jobs.cancel.lock() {
            m.remove(&id);
        }
        match result {
            Ok(r) => {
                let _ = app.emit("job-done", serde_json::json!({ "job": id, "result": r }));
            }
            Err(e) => {
                let _ = app.emit(
                    "job-error",
                    serde_json::json!({ "job": id, "error": e.to_string() }),
                );
            }
        }
    });
    Ok(id)
}

#[tauri::command]
fn cancel_job(jobs: State<'_, Arc<Jobs>>, job: u64) -> Result<(), String> {
    if let Some(c) = jobs.cancel.lock().map_err(|e| e.to_string())?.get(&job) {
        c.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

/// One clip of a finished run, with what re-rendering it needs.
struct RunClip {
    asset: choco_engine::MediaAsset,
    transcript_path: PathBuf,
    transcript: Transcript,
    clip: serde_json::Value,
    cand: choco_engine::Candidate,
}

fn run_clip(summary_path: &str, rank: usize) -> Result<RunClip, String> {
    let run: serde_json::Value = read_json(Path::new(summary_path))?;
    let transcript_path = PathBuf::from(run["transcript_path"].as_str().unwrap_or_default());
    let clip = run["clips"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["rank"].as_u64() == Some(rank as u64)))
        .ok_or("Clip not found in this run")?
        .clone();
    Ok(RunClip {
        asset: serde_json::from_value(run["source"].clone()).map_err(|e| e.to_string())?,
        transcript: read_json(&transcript_path)?,
        transcript_path,
        cand: serde_json::from_value(clip["candidate"].clone()).map_err(|e| e.to_string())?,
        clip,
    })
}

fn rerender(
    summary_path: &str,
    rc: &RunClip,
    rank: usize,
    style: &str,
    preset: &str,
    layout: &str,
) -> Result<ExportedClip, String> {
    let out = Path::new(summary_path)
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let opts = export_options(style, preset, layout, out)?;
    let mut raster = captions::Rasterizer::new();
    let cancel = AtomicBool::new(false);
    let mut job = Job {
        raster: &mut raster,
        progress: &|_| {},
        cancel: &cancel,
    };
    export::export_clip(&rc.asset, &rc.transcript, &rc.cand, rank, &opts, &mut job)
        .map_err(|e| e.to_string())
}

/// Re-render one clip of a finished run with a different style, size or layout.
#[tauri::command]
async fn reexport_clip(
    summary_path: String,
    rank: usize,
    style: String,
    preset: String,
    layout: String,
) -> Result<ExportedClip, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let rc = run_clip(&summary_path, rank)?;
        rerender(&summary_path, &rc, rank, &style, &preset, &layout)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct CaptionWord {
    index: usize,
    text: String,
}

/// The clip's transcript words for the caption editor, one group per transcript segment.
#[tauri::command]
fn clip_words(summary_path: String, rank: usize) -> Result<Vec<Vec<CaptionWord>>, String> {
    let rc = run_clip(&summary_path, rank)?;
    let range = (rc.cand.start_ms, rc.cand.end_ms);
    Ok(captions::edit::clip_words(&rc.transcript, range)
        .into_iter()
        .map(|g| {
            g.into_iter()
                .map(|(index, w)| CaptionWord {
                    index,
                    text: w.text.trim().into(),
                })
                .collect()
        })
        .collect())
}

/// Serializes read-edit-write of transcripts: the clips of a run share one transcript file.
#[derive(Default)]
struct TranscriptWrites(Mutex<()>);

/// Replace `path` in one step, so a crash never leaves half a transcript.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let res = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|_| f.sync_all()))
        .and_then(|_| std::fs::rename(&tmp, path));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Save caption corrections to the run's transcript, then re-render the clip (MP4, poster, SRT,
/// VTT) in its current style and size. Overlapping clips of the same run pick the corrections up
/// the next time they are rendered. `layout` is "auto" for a speaker-following clip, the card's
/// fixed layout after a restyle, or None to use the default fixed layout from Settings.
#[tauri::command]
async fn update_caption_words(
    app: AppHandle,
    writes: State<'_, Arc<TranscriptWrites>>,
    summary_path: String,
    rank: usize,
    edits: Vec<captions::edit::WordEdit>,
    layout: Option<String>,
) -> Result<ExportedClip, String> {
    let fixed = Settings::load(&config_dir(&app)?).defaults.layout;
    let layout = layout.unwrap_or_else(|| if fixed == "fit" { "fit" } else { "static" }.into());
    let writes = writes.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        save_caption_words(&writes, &summary_path, rank, &edits, &layout)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn save_caption_words(
    writes: &TranscriptWrites,
    summary_path: &str,
    rank: usize,
    edits: &[captions::edit::WordEdit],
    layout: &str,
) -> Result<ExportedClip, String> {
    let rc = {
        let _lock = writes.0.lock().map_err(|e| e.to_string())?;
        let mut rc = run_clip(summary_path, rank)?;
        // Only ever rewrite the pipeline's own transcript, next to the summary.
        let name = rc.transcript_path.to_string_lossy();
        if rc.transcript_path.parent() != Path::new(summary_path).parent()
            || !name.ends_with(".transcript.json")
        {
            return Err("This run's transcript is not in its folder".into());
        }
        let range = (rc.cand.start_ms, rc.cand.end_ms);
        let changed = captions::edit::edit_words(&mut rc.transcript, range, edits)
            .map_err(|e| e.to_string())?;
        if changed > 0 {
            let json = serde_json::to_vec(&rc.transcript).map_err(|e| e.to_string())?;
            write_atomic(&rc.transcript_path, &json).map_err(|e| e.to_string())?;
        }
        rc
    };
    // Style and size come from the clip's sidecar, which every render rewrites.
    let side: serde_json::Value = read_json(Path::new(rc.clip["json"].as_str().unwrap_or("")))?;
    let style = side["caption_style"].as_str().unwrap_or("none");
    let preset = side["preset"].as_str().unwrap_or("shorts");
    rerender(summary_path, &rc, rank, style, preset, layout)
}

/// The webview plays the source and the clips through the asset protocol. Its static scope is the
/// home folder (plus `/Volumes` on macOS); a source on another drive (`D:\`, a USB stick) or an
/// output folder outside home is allowed here, file by file and folder by folder, as the user
/// picks them, instead of a static `**`.
fn allow_media(app: &AppHandle, source: &std::path::Path, out_dir: &std::path::Path) {
    let scope = app.asset_protocol_scope();
    let _ = scope.allow_file(source);
    let _ = scope.allow_directory(out_dir, true);
}

/// Load a previous run summary (for reopening results).
#[tauri::command]
fn load_run(app: AppHandle, summary_path: String) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(&summary_path);
    let run: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if let Some(source) = run["source"]["path"].as_str() {
        allow_media(
            &app,
            std::path::Path::new(source),
            path.parent().unwrap_or(&path),
        );
    }
    Ok(run)
}

/// Download the speech model this Mac needs, with `model-progress` events.
#[tauri::command]
async fn download_speech_model(app: AppHandle) -> Result<(), String> {
    let Some(id) = backend::detect().recommended_models().whisper_model else {
        return Ok(());
    };
    tauri::async_runtime::spawn_blocking(move || {
        let progress = move |p: Progress| {
            let _ = app.emit(
                "model-progress",
                ProgressEvent {
                    job: 0,
                    stage: p.stage,
                    fraction: p.fraction,
                    message: p.message,
                },
            );
        };
        models_registry::pull(&models_registry::default_dir(), &id, &progress)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Arc::new(Jobs::default()))
        .manage(Arc::new(TranscriptWrites::default()))
        .invoke_handler(tauri::generate_handler![
            status,
            diagnostics,
            check_update,
            install_update,
            save_settings,
            set_api_key,
            provider_models,
            provider_test,
            start_job,
            cancel_job,
            reexport_clip,
            clip_words,
            update_caption_words,
            load_run,
            download_speech_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running choco.video");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("choco-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("talk.transcript.json");
        std::fs::write(&path, "old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert!(!dir.join("talk.transcript.json.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}

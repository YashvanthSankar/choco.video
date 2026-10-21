//! `choco`: command-line interface to choco-engine.

use anyhow::{Context, Result, bail};
use choco_engine::backend::{self, SpeechEngine};
use choco_engine::highlights::{self, HighlightOptions, audio, picker};
use choco_engine::transcribe::{TranscribeOptions, apple};
use choco_engine::{Progress, Transcript, ingest, models_registry, transcribe};
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "choco",
    version,
    about = "Turn long video into vertical shorts, on your machine."
)]
struct Cli {
    /// Print machine-readable JSON on stdout. Progress always goes to stderr.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show container and stream metadata.
    Probe { file: PathBuf },
    /// Transcribe speech to a word-timed transcript.
    Transcribe {
        file: PathBuf,
        /// Speech engine. `auto` picks the best available on this machine.
        #[arg(long, value_enum, default_value_t = EngineArg::Auto)]
        engine: EngineArg,
        /// Whisper model id (see `choco models list`).
        #[arg(long)]
        model: Option<String>,
        /// BCP-47 language such as en-US. Apple speech defaults to en-US; whisper auto-detects.
        #[arg(long)]
        language: Option<String>,
        /// Write the transcript JSON here.
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    /// Pick the best clips. Reuses a transcript JSON when given, otherwise transcribes first.
    Highlights {
        file: PathBuf,
        /// Transcript from `choco transcribe -o`; skips transcription.
        #[arg(long)]
        transcript: Option<PathBuf>,
        /// Maximum clips; content decides the count.
        #[arg(long, default_value_t = 10)]
        n: usize,
        /// Only clips about this, e.g. "pricing".
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long, value_enum, default_value_t = PickerArg::Auto)]
        picker: PickerArg,
        /// Minimum score to keep a clip.
        #[arg(long, default_value_t = 55)]
        min_score: u8,
        /// Write candidates JSON here.
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    /// Score the picker against the golden set (precision@5, recall@10).
    Eval {
        /// Golden set JSON.
        #[arg(long, default_value = "eval/golden.json")]
        golden: PathBuf,
        /// Score saved `choco highlights -o` files named `<fixture>.json` in this directory
        /// instead of running the picker.
        #[arg(long)]
        results: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = PickerArg::Auto)]
        picker: PickerArg,
        /// Label for the results file, e.g. the model name.
        #[arg(long)]
        label: Option<String>,
        /// Where to write per-run results.
        #[arg(long, default_value = "eval/results")]
        out_dir: PathBuf,
        /// Only measure how many golden clips survive the shortlist (no model calls).
        #[arg(long)]
        shortlist_only: bool,
        /// Shortlist size to test with --shortlist-only.
        #[arg(long, default_value_t = 80)]
        shortlist: usize,
        /// Max overlap between shortlisted windows, percent (--shortlist-only).
        #[arg(long, default_value_t = 20)]
        overlap: u8,
    },
    /// Render one vertical clip with captions.
    Render {
        file: PathBuf,
        #[arg(long)]
        start_ms: u64,
        #[arg(long)]
        end_ms: u64,
        /// Transcript JSON for captions (from `choco transcribe -o`). Omit for no captions.
        #[arg(long)]
        transcript: Option<PathBuf>,
        /// Caption style: clean, bold, karaoke, minimal.
        #[arg(long, default_value = "bold")]
        style: String,
        /// Output size preset: shorts (1080x1920), square (1080x1080), portrait (1080x1350), landscape (1920x1080).
        #[arg(long, default_value = "shorts")]
        preset: String,
        /// auto (face tracking), static (centre crop) or fit (whole frame over a blurred background).
        #[arg(long, default_value = "auto")]
        layout: String,
        #[arg(long, short)]
        out: PathBuf,
        /// Also write a poster PNG of the middle frame here.
        #[arg(long)]
        poster: Option<PathBuf>,
        /// Translate the captions into this language ("es", "ja", "Brazilian Portuguese") with
        /// the picker's summary model. Needs --transcript and a Latin-script language (the
        /// bundled fonts cover no other).
        #[arg(long, value_name = "LANG")]
        translate: Option<String>,
        /// Model used by --translate.
        #[arg(long, value_enum, default_value_t = PickerArg::Auto)]
        picker: PickerArg,
    },
    /// Whole pipeline: transcribe, pick the best clips, render and export them.
    Run {
        file: PathBuf,
        /// Maximum clips; content decides the count.
        #[arg(long, default_value_t = 10)]
        n: usize,
        /// Only clips about this, e.g. "pricing".
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long, value_enum, default_value_t = PickerArg::Auto)]
        picker: PickerArg,
        /// Caption style (clean, bold, karaoke, minimal) or "none".
        #[arg(long, default_value = "bold")]
        style: String,
        /// shorts, square, portrait, landscape.
        #[arg(long, default_value = "shorts")]
        preset: String,
        /// auto (follow the speaker, split for two people), static (centre crop) or fit.
        #[arg(long, default_value = "auto")]
        layout: String,
        /// Reuse a transcript instead of transcribing.
        #[arg(long)]
        transcript: Option<PathBuf>,
        #[arg(long, default_value_t = 55)]
        min_score: u8,
        /// Output folder.
        #[arg(long, short, default_value = "clips")]
        out: PathBuf,
        /// Translate each clip's captions into this language ("es", "ja", "Brazilian
        /// Portuguese") with the picker's summary model. SRT/VTT always; burned in only for
        /// Latin scripts, which the bundled fonts cover. The transcript JSON stays as it was.
        #[arg(long, value_name = "LANG")]
        translate: Option<String>,
    },
    /// Write a PNG frame from the video.
    Thumbnail {
        file: PathBuf,
        #[arg(long, default_value_t = 0)]
        at_ms: u64,
        #[arg(long, default_value_t = 640)]
        width: u32,
        #[arg(long, short)]
        out: PathBuf,
    },
    /// Check hardware, engines, libraries and models, with fix hints.
    Doctor,
    /// Check the configured picker provider (CHOCO_LLM_BASE_URL, CHOCO_LLM_API_KEY, CHOCO_LLM_MODEL).
    Provider {
        #[command(subcommand)]
        cmd: ProviderCmd,
    },
    /// Manage runtime-downloaded models.
    Models {
        #[command(subcommand)]
        cmd: ModelsCmd,
    },
}

#[derive(Subcommand)]
enum ProviderCmd {
    /// List the models the endpoint offers.
    Models,
    /// Send one tiny scoring request with CHOCO_LLM_MODEL.
    Test,
}

#[derive(Subcommand)]
enum ModelsCmd {
    List,
    Pull { id: String },
    Rm { id: String },
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum PickerArg {
    /// The configured provider if CHOCO_LLM_API_KEY is set, else Apple on-device.
    Auto,
    /// Any OpenAI-compatible endpoint: CHOCO_LLM_BASE_URL (default OpenCode Zen), CHOCO_LLM_API_KEY, CHOCO_LLM_MODEL, CHOCO_LLM_SUMMARY_MODEL.
    Provider,
    /// Apple Intelligence on-device model, macOS 26+.
    Apple,
}

fn make_picker(p: PickerArg) -> Result<Box<dyn picker::Llm>> {
    use picker::openai_compat::{OpenAiCompat, OpenAiCompatConfig};
    Ok(match p {
        PickerArg::Provider => {
            Box::new(OpenAiCompat::new(OpenAiCompatConfig::from_env("provider")?))
        }
        PickerArg::Apple => {
            if !picker::apple::available() {
                bail!(
                    "Apple Intelligence model is not available on this Mac (needs macOS 26+ with Apple Intelligence enabled)"
                );
            }
            Box::new(picker::apple::AppleLlm)
        }
        PickerArg::Auto => {
            if std::env::var("CHOCO_LLM_API_KEY").is_ok_and(|k| !k.is_empty()) {
                make_picker(PickerArg::Provider)?
            } else {
                make_picker(PickerArg::Apple)?
            }
        }
    })
}

#[derive(Clone, Copy, ValueEnum)]
enum EngineArg {
    Auto,
    Apple,
    Whisper,
}

fn progress_printer(quiet: bool) -> impl Fn(Progress) + Sync {
    move |p: Progress| {
        if quiet {
            return;
        }
        let mut e = std::io::stderr().lock();
        if p.fraction >= 0.0 {
            let _ = write!(
                e,
                "\r\x1b[2K[{}] {:>3.0}% {}",
                p.stage,
                p.fraction * 100.0,
                p.message
            );
        } else {
            let _ = write!(e, "\r\x1b[2K[{}] {}", p.stage, p.message);
        }
        if p.fraction >= 1.0 {
            let _ = writeln!(e);
        }
        let _ = e.flush();
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Probe { file } => {
            let a = ingest::probe(&file).with_context(|| format!("probing {}", file.display()))?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&a)?);
            } else {
                println!(
                    "{}  {}  {:.1}s  hash {}",
                    a.path.display(),
                    a.container,
                    a.duration_ms as f64 / 1000.0,
                    &a.content_hash[..12]
                );
                if let Some(v) = &a.video {
                    println!(
                        "  video  {} {}x{} → displays {}x{}  {:.2} fps  rotation {}°",
                        v.codec,
                        v.width,
                        v.height,
                        v.display_width,
                        v.display_height,
                        v.fps_num as f64 / v.fps_den as f64,
                        v.rotation_deg
                    );
                }
                if let Some(au) = &a.audio {
                    println!(
                        "  audio  {} {} Hz {} ch",
                        au.codec, au.sample_rate, au.channels
                    );
                }
            }
        }
        Cmd::Transcribe {
            file,
            engine,
            model,
            language,
            out,
        } => {
            let started = Instant::now();
            let asset =
                ingest::probe(&file).with_context(|| format!("probing {}", file.display()))?;
            let b = backend::detect();
            let available = b.speech_engines();
            let chosen = match engine {
                EngineArg::Auto => available[0],
                EngineArg::Apple => *available
                    .iter()
                    .find(|e| **e == SpeechEngine::AppleSpeech)
                    .context("Apple speech is not available on this Mac (needs macOS 26+)")?,
                EngineArg::Whisper => *available
                    .iter()
                    .find(|e| matches!(e, SpeechEngine::Whisper { .. }))
                    .context("whisper not compiled in")?,
            };
            let opts = TranscribeOptions {
                language,
                whisper_model: model.unwrap_or_else(|| {
                    b.recommended_models()
                        .whisper_model
                        .unwrap_or_else(|| models_registry::WHISPER_DEFAULT.into())
                }),
                models_dir: models_registry::default_dir(),
            };
            let pr = progress_printer(false);
            let tmp = tempfile::tempdir()?;
            let wav = tmp.path().join("audio.wav");
            let t_audio = Instant::now();
            let samples = ingest::extract_audio(&asset, &wav, &pr)?;
            let audio_s = t_audio.elapsed().as_secs_f64();
            let t_speech = Instant::now();
            let t = transcribe::transcribe(&wav, chosen, &opts, &pr)?;
            let speech_s = t_speech.elapsed().as_secs_f64();
            let media_s = samples as f64 / ingest::SPEECH_SAMPLE_RATE as f64;
            eprintln!(
                "{} · {} segments, {} words · audio {:.1}s, speech {:.1}s ({:.0}x realtime), total {:.1}s",
                t.engine,
                t.segments.len(),
                t.word_count(),
                audio_s,
                speech_s,
                media_s / speech_s.max(0.001),
                started.elapsed().as_secs_f64()
            );
            let json = serde_json::to_string_pretty(&t)?;
            if let Some(out) = out {
                std::fs::write(&out, &json)?;
                eprintln!("wrote {}", out.display());
            }
            if cli.json {
                println!("{json}");
            } else {
                for s in t.segments.iter().take(12) {
                    println!("{:>8.2}  {}", s.start_ms as f64 / 1000.0, s.text);
                }
                if t.segments.len() > 12 {
                    println!("   ... {} more segments", t.segments.len() - 12);
                }
            }
        }
        Cmd::Highlights {
            file,
            transcript,
            n,
            prompt,
            picker: pk,
            min_score,
            out,
        } => {
            let started = Instant::now();
            let llm = make_picker(pk)?;
            let pr = progress_printer(false);
            let asset =
                ingest::probe(&file).with_context(|| format!("probing {}", file.display()))?;
            let tmp = tempfile::tempdir()?;
            let wav = tmp.path().join("audio.wav");
            ingest::extract_audio(&asset, &wav, &pr)?;
            let feats = audio::analyze(&wav)?;
            let t: Transcript = match transcript {
                Some(p) => serde_json::from_str(&std::fs::read_to_string(&p)?)
                    .with_context(|| format!("reading {}", p.display()))?,
                None => {
                    let b = backend::detect();
                    let opts = TranscribeOptions {
                        language: None,
                        whisper_model: b
                            .recommended_models()
                            .whisper_model
                            .unwrap_or_else(|| models_registry::WHISPER_DEFAULT.into()),
                        models_dir: models_registry::default_dir(),
                    };
                    transcribe::transcribe(&wav, b.speech_engines()[0], &opts, &pr)?
                }
            };
            let mut opts = HighlightOptions {
                prompt,
                ..Default::default()
            };
            opts.rank.max_clips = n;
            opts.rank.min_score = min_score;
            let (cands, report) =
                highlights::highlights(&t, Some(&feats), llm.as_ref(), &opts, &pr, None)?;
            eprintln!(
                "{} via {} · {} windows → {} shortlisted → {} scored ({} declined, {} failed batches) · {} model calls · {:.1}s",
                llm.tier(),
                llm.model(picker::Role::Score),
                report.windows,
                report.shortlisted,
                report.scored,
                report.declined_batches,
                report.failed_batches,
                report.model_calls,
                started.elapsed().as_secs_f64()
            );
            let u = report.usage;
            if u.score_input + u.summarize_input > 0 {
                eprintln!(
                    "tokens · score {} in / {} out · summarize {} in / {} out",
                    u.score_input, u.score_output, u.summarize_input, u.summarize_output
                );
            }
            let json = serde_json::to_string_pretty(
                &serde_json::json!({"candidates": cands, "report": report}),
            )?;
            if let Some(out) = out {
                std::fs::write(&out, &json)?;
                eprintln!("wrote {}", out.display());
            }
            if cli.json {
                println!("{json}");
            } else {
                for (i, c) in cands.iter().enumerate() {
                    let ts = |ms: u64| format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60);
                    println!(
                        "{:>2}. [{:>3}] {}–{} ({:>2}s)  {}",
                        i + 1,
                        c.score,
                        ts(c.start_ms),
                        ts(c.end_ms),
                        (c.end_ms - c.start_ms) / 1000,
                        c.title
                    );
                    if !c.reasons.is_empty() {
                        println!("               {}", c.reasons.join(" · "));
                    }
                }
            }
        }
        Cmd::Eval {
            golden,
            results,
            picker: pk,
            label,
            out_dir,
            shortlist_only,
            shortlist,
            overlap,
        } => {
            use choco_engine::eval;
            let set: eval::GoldenSet = serde_json::from_str(
                &std::fs::read_to_string(&golden)
                    .with_context(|| format!("reading {}", golden.display()))?,
            )?;
            let base = golden
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(std::path::Path::new("."));
            if shortlist_only {
                let pr = progress_printer(true);
                let mut total = 0.0;
                println!("{:<12} {:>8} {:>8}", "fixture", "golden", "in list");
                for f in &set.fixtures {
                    let t: Transcript =
                        serde_json::from_str(&std::fs::read_to_string(base.join(&f.transcript))?)?;
                    let asset = ingest::probe(&base.join(&f.media))?;
                    let tmp = tempfile::tempdir()?;
                    let wav = tmp.path().join("audio.wav");
                    ingest::extract_audio(&asset, &wav, &pr)?;
                    let feats = audio::analyze(&wav)?;
                    let opts = HighlightOptions {
                        shortlist,
                        shortlist_max_overlap_pct: overlap,
                        ..Default::default()
                    };
                    let ws = highlights::shortlist_only(&t, Some(&feats), &opts);
                    let found = f
                        .clips
                        .iter()
                        .filter(|g| {
                            ws.iter()
                                .any(|w| eval::hits((w.start_ms, w.end_ms), (g.start_ms, g.end_ms)))
                        })
                        .count();
                    total += found as f32 / f.clips.len().max(1) as f32;
                    println!("{:<12} {:>8} {:>8}", f.id, f.clips.len(), found);
                }
                println!(
                    "shortlist recall (golden clips reachable by the model): {:.2}",
                    total / set.fixtures.len().max(1) as f32
                );
                return Ok(());
            }
            let llm = if results.is_none() {
                Some(make_picker(pk)?)
            } else {
                None
            };
            let pr = progress_printer(false);
            let mut scores = Vec::new();
            let mut cost_tokens = picker::Usage::default();
            let mut source_ms = 0u64;
            for f in &set.fixtures {
                let cands: Vec<choco_engine::Candidate> = if let Some(dir) = &results {
                    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
                        dir.join(format!("{}.json", f.id)),
                    )?)?;
                    serde_json::from_value(v["candidates"].clone())?
                } else {
                    let llm = llm
                        .as_ref()
                        .expect("picker built when not scoring saved results");
                    let t: Transcript =
                        serde_json::from_str(&std::fs::read_to_string(base.join(&f.transcript))?)?;
                    let asset = ingest::probe(&base.join(&f.media))?;
                    let tmp = tempfile::tempdir()?;
                    let wav = tmp.path().join("audio.wav");
                    ingest::extract_audio(&asset, &wav, &pr)?;
                    let feats = audio::analyze(&wav)?;
                    let before = llm.usage();
                    let (c, _) = highlights::highlights(
                        &t,
                        Some(&feats),
                        llm.as_ref(),
                        &HighlightOptions::default(),
                        &pr,
                        None,
                    )?;
                    let after = llm.usage();
                    cost_tokens.score_input += after.score_input - before.score_input;
                    cost_tokens.score_output += after.score_output - before.score_output;
                    cost_tokens.summarize_input += after.summarize_input - before.summarize_input;
                    cost_tokens.summarize_output +=
                        after.summarize_output - before.summarize_output;
                    source_ms += asset.duration_ms;
                    c
                };
                scores.push(eval::score_fixture(&f.id, &cands, &f.clips));
            }
            let total = eval::aggregate(scores, &set);
            let label = label.unwrap_or_else(|| {
                llm.as_ref()
                    .map(|l| format!("{}-{}", l.tier(), l.model(picker::Role::Score)))
                    .unwrap_or_else(|| "saved".into())
            });
            println!(
                "{:<12} {:>6} {:>6} {:>6} {:>5}",
                "fixture", "P@5", "R@10", "IoU", "picks"
            );
            for f in &total.fixtures {
                println!(
                    "{:<12} {:>6.2} {:>6.2} {:>6.2} {:>5}",
                    f.id, f.precision_at_5, f.recall_at_10, f.mean_iou, f.picks
                );
            }
            println!(
                "{:<12} {:>6.2} {:>6.2} {:>6.2}",
                "MEAN", total.precision_at_5, total.recall_at_10, total.mean_iou
            );
            if !total.human_reviewed {
                println!(
                    "note: golden clips are not yet human-reviewed; treat these numbers as provisional"
                );
            }
            std::fs::create_dir_all(&out_dir)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let path = out_dir.join(format!("{stamp}-{}.json", label.replace(['/', ' '], "_")));
            std::fs::write(
                &path,
                serde_json::to_string_pretty(&serde_json::json!({
                    "label": label, "score": total, "usage": cost_tokens, "source_ms": source_ms,
                }))?,
            )?;
            eprintln!("wrote {}", path.display());
        }
        Cmd::Render {
            file,
            start_ms,
            end_ms,
            transcript,
            style,
            preset,
            layout,
            out,
            poster,
            translate,
            picker: pk,
        } => {
            use choco_engine::{captions, render};
            if end_ms <= start_ms {
                bail!("--end-ms must be after --start-ms");
            }
            if translate.is_some() && transcript.is_none() {
                bail!("--translate needs --transcript");
            }
            let size = match preset.as_str() {
                "shorts" | "tiktok" | "reels" => (1080, 1920),
                "square" => (1080, 1080),
                "portrait" => (1080, 1350),
                "landscape" => (1920, 1080),
                other => bail!("unknown preset {other}"),
            };
            let asset = ingest::probe(&file)?;
            let v = asset.video.as_ref().context("source has no video")?;
            let mut plan =
                render::center_crop_plan((v.display_width, v.display_height), size, start_ms);
            let transcript_v: Option<Transcript> = match &transcript {
                Some(p) => Some(serde_json::from_str(&std::fs::read_to_string(p)?)?),
                None => None,
            };
            match layout.as_str() {
                "static" => {}
                "fit" => plan.layout = choco_engine::Layout::Fit,
                "auto" => {
                    let empty = Transcript {
                        language: String::new(),
                        engine: String::new(),
                        segments: vec![],
                    };
                    let po = choco_engine::reframe::PlanOptions {
                        target: size,
                        ..Default::default()
                    };
                    let cancel = std::sync::atomic::AtomicBool::new(false);
                    let (p, r) = choco_engine::reframe::auto_plan(
                        &asset,
                        transcript_v.as_ref().unwrap_or(&empty),
                        start_ms,
                        end_ms,
                        &po,
                        &cancel,
                    )?;
                    eprintln!(
                        "reframe: {} · {} tracks, {} subjects, {} switches, {} moves",
                        r.layout, r.tracks, r.subjects, r.switches, r.moves
                    );
                    plan = p;
                }
                other => bail!("unknown layout {other}"),
            }
            let mut raster = captions::Rasterizer::new();
            // Reframe above follows the original speech; only the captions are translated.
            let transcript_v = match (transcript_v, &translate) {
                (Some(t), Some(lang)) => {
                    let llm = make_picker(pk)?;
                    let tr = captions::translate::translate_range(
                        &t,
                        (start_ms, end_ms),
                        lang,
                        llm.as_ref(),
                    )?;
                    eprintln!(
                        "translate: {} lines into {lang} in {} calls, {} kept untranslated",
                        tr.transcript.segments.len(),
                        tr.calls,
                        tr.untranslated.len()
                    );
                    Some(tr.transcript)
                }
                (t, _) => t,
            };
            let track = match &transcript_v {
                Some(t) => {
                    let mut st = captions::preset(&style)
                        .with_context(|| format!("unknown style {style}"))?;
                    if plan.layout == choco_engine::Layout::Split {
                        st.position_pct = 46.0;
                    }
                    if let Some(lang) = &translate
                        && !raster.can_draw(&t.text(), &st)
                    {
                        bail!(
                            "the bundled caption fonts cannot draw {lang}, so burned captions would be boxes; `choco run --translate {lang}` writes them as SRT and VTT instead"
                        );
                    }
                    Some(captions::chunk(
                        t,
                        (start_ms, end_ms),
                        &st,
                        size,
                        &mut raster,
                        &captions::ChunkOptions::default(),
                    ))
                }
                None => None,
            };
            let mid = (start_ms + end_ms) / 2;
            let spec = render::RenderSpec {
                start_ms,
                end_ms,
                size,
                crop: &plan,
                captions: track.as_ref(),
                normalize_audio: true,
                max_fps: 60,
                video_bitrate: 10_000_000,
                poster: poster.as_deref().map(|p| (mid, p)),
            };
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let pr = progress_printer(false);
            let r = render::render_clip(&asset, &spec, &out, &mut raster, &pr, &cancel)?;
            eprintln!(
                "{} frames at {}/{} fps, {:.1}s of video in {:.1}s ({:.1}x realtime), audio {}",
                r.frames,
                r.fps.0,
                r.fps.1,
                r.duration_ms as f64 / 1000.0,
                r.wall_ms as f64 / 1000.0,
                r.duration_ms as f64 / r.wall_ms.max(1) as f64,
                if r.has_audio { "yes" } else { "no" }
            );
            eprintln!("wrote {}", out.display());
        }
        Cmd::Run {
            file,
            n,
            prompt,
            picker: pk,
            style,
            preset,
            layout,
            transcript,
            min_score,
            out,
            translate,
        } => {
            use choco_engine::{captions, export, pipeline};
            let started = Instant::now();
            if export::preset_size(&preset).is_none() {
                bail!("unknown preset {preset} (use one of {:?})", export::PRESETS);
            }
            let style = if style == "none" {
                None
            } else {
                Some(captions::preset(&style).with_context(|| format!("unknown style {style}"))?)
            };
            let (layout, auto_reframe) = match layout.as_str() {
                "auto" => (choco_engine::Layout::Single, true),
                "static" => (choco_engine::Layout::Static, false),
                "fit" => (choco_engine::Layout::Fit, false),
                other => bail!("unknown layout {other}"),
            };
            let llm = make_picker(pk)?;
            let mut hopts = HighlightOptions {
                prompt,
                ..Default::default()
            };
            hopts.rank.max_clips = n;
            hopts.rank.min_score = min_score;
            let transcript = match transcript {
                Some(p) => Some(serde_json::from_str::<Transcript>(
                    &std::fs::read_to_string(&p)?,
                )?),
                None => None,
            };
            let opts = pipeline::RunOptions {
                highlights: hopts,
                export: export::ExportOptions {
                    preset,
                    style,
                    layout,
                    auto_reframe,
                    out_dir: out.clone(),
                    ..Default::default()
                },
                transcript,
                language: None,
                speech: None,
                translate,
            };
            let pr = progress_printer(false);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let r = pipeline::run(&file, &opts, llm.as_ref(), &pr, &cancel)?;
            eprintln!(
                "{} clips in {} · {:.0}s total",
                r.clips.len(),
                out.display(),
                started.elapsed().as_secs_f64()
            );
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                for e in &r.clips {
                    println!(
                        "{:>2}. [{:>3}] {}",
                        e.rank,
                        e.candidate.score,
                        e.mp4.display()
                    );
                }
            }
        }
        Cmd::Thumbnail {
            file,
            at_ms,
            width,
            out,
        } => {
            let a = ingest::probe(&file)?;
            ingest::thumbnail(&a, at_ms, width, &out)?;
            eprintln!("wrote {}", out.display());
        }
        Cmd::Doctor => doctor(cli.json)?,
        Cmd::Provider { cmd } => {
            use picker::openai_compat::{self, OpenAiCompatConfig};
            match cmd {
                ProviderCmd::Models => {
                    let base = std::env::var("CHOCO_LLM_BASE_URL")
                        .unwrap_or_else(|_| openai_compat::OPENCODE_ZEN_BASE_URL.into());
                    let key =
                        std::env::var("CHOCO_LLM_API_KEY").context("set CHOCO_LLM_API_KEY")?;
                    for m in openai_compat::list_models(&base, &key)? {
                        println!("{m}");
                    }
                }
                ProviderCmd::Test => {
                    let cfg = OpenAiCompatConfig::from_env("provider")?;
                    let (base, model) = (cfg.base_url.clone(), cfg.score_model.clone());
                    let took = openai_compat::test_connection(cfg)?;
                    println!(
                        "ok: {model} at {base} answered with valid scores in {:.1}s",
                        took.as_secs_f64()
                    );
                }
            }
        }
        Cmd::Models { cmd } => {
            let dir = models_registry::default_dir();
            match cmd {
                ModelsCmd::List => {
                    let rows: Vec<_> = models_registry::manifests()
                        .into_iter()
                        .map(|m| (models_registry::is_installed(&dir, &m), m))
                        .collect();
                    if cli.json {
                        let v: Vec<_> = rows
                            .iter()
                            .map(|(i, m)| serde_json::json!({"installed": i, "model": m}))
                            .collect();
                        println!("{}", serde_json::to_string_pretty(&v)?);
                    } else {
                        println!("models dir: {}", dir.display());
                        for (installed, m) in rows {
                            println!(
                                "  {} {:<30} {:>7.1} MB  {}",
                                if installed { "✓" } else { " " },
                                m.id,
                                m.size as f64 / 1e6,
                                m.license
                            );
                        }
                    }
                }
                ModelsCmd::Pull { id } => {
                    let pr = progress_printer(false);
                    let p = models_registry::pull(&dir, &id, &pr)?;
                    eprintln!("{}", p.display());
                }
                ModelsCmd::Rm { id } => {
                    if models_registry::remove(&dir, &id)? {
                        eprintln!("removed {id}");
                    } else {
                        eprintln!("{id} was not installed");
                    }
                }
            }
        }
    }
    Ok(())
}

fn doctor(json: bool) -> Result<()> {
    let b = backend::detect();
    let info = b.info();
    let engines = b.speech_engines();
    let rec = b.recommended_models();
    let media = choco_engine::media_library_info();
    let apple_status = apple::status();
    let dir = models_registry::default_dir();
    let mut hints: Vec<String> = Vec::new();
    let mut problems = 0;

    if !media.is_lgpl {
        problems += 1;
        hints.push(format!("FFmpeg reports license \"{}\". Only the LGPL build from scripts/ffmpeg/ may be linked.", media.license));
    }
    if let Some(id) = &rec.whisper_model {
        let m = models_registry::find(id).expect("recommended model is in the registry");
        if !models_registry::is_installed(&dir, &m) {
            hints.push(format!(
                "Speech needs {id} ({:.0} MB). Run: choco models pull {id}",
                m.size as f64 / 1e6
            ));
        }
    }
    if info.os == "macos" && !engines.contains(&SpeechEngine::AppleSpeech) {
        if backend_major(&info.os_version) >= 26 {
            hints.push("Apple speech helper not found. Rebuild with the apple-speech feature, or set CHOCO_APPLE_HELPER.".into());
        } else {
            hints.push(format!("macOS {} has no SpeechAnalyzer; whisper will be used. macOS 26+ transcribes with zero download.", info.os_version));
        }
    }
    if let Some(s) = &apple_status
        && s.available
        && s.installed_locales.is_empty()
    {
        hints.push(
            "Apple speech has no language assets yet; the first transcription will download them."
                .into(),
        );
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "engine_version": choco_engine::VERSION,
                "hardware": info,
                "speech_engines": engines,
                "apple_speech": apple_status,
                "hw_decoders": b.hw_decoders(),
                "hw_encoders": b.hw_encoders(),
                "media_library": media,
                "recommended_models": rec,
                "models_dir": dir,
                "hints": hints,
            }))?
        );
    } else {
        let mem = info
            .memory_bytes
            .map(|m| format!("{:.0} GB", m as f64 / 1073741824.0))
            .unwrap_or_else(|| "?".into());
        println!("choco-engine {}", choco_engine::VERSION);
        println!(
            "  machine      {} {} {} · {} cores · {}",
            info.os, info.os_version, info.arch, info.cpu_cores, mem
        );
        if let Some(g) = &info.gpu {
            println!("  chip         {g}");
        }
        println!(
            "  speech       {}",
            engines
                .iter()
                .map(|e| format!("{e:?}"))
                .collect::<Vec<_>>()
                .join(" → ")
        );
        if let Some(s) = &apple_status {
            println!(
                "  apple speech available={} · {} locales · installed: {}",
                s.available,
                s.locales.len(),
                s.installed_locales.join(", ")
            );
        }
        println!(
            "  video        decode {:?} · encode {:?}",
            b.hw_decoders(),
            b.hw_encoders()
        );
        println!(
            "  ffmpeg       {} · {}",
            media.avcodec_version, media.license
        );
        println!("  models dir   {}", dir.display());
        if hints.is_empty() {
            println!("\n✓ ready");
        } else {
            println!();
            for h in &hints {
                println!("• {h}");
            }
        }
    }
    if problems > 0 {
        bail!("{problems} blocking problem(s)");
    }
    Ok(())
}

fn backend_major(v: &str) -> u32 {
    v.split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0)
}

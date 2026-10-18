//! `choco`: command-line interface to choco-engine.

use anyhow::{Context, Result, bail};
use choco_engine::backend::{self, SpeechEngine};
use choco_engine::transcribe::{TranscribeOptions, apple};
use choco_engine::highlights::{self, HighlightOptions, audio, picker};
use choco_engine::{Progress, Transcript, ingest, models_registry, transcribe};
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "choco", version, about = "Turn long video into vertical shorts, on your machine.")]
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
    /// Manage runtime-downloaded models.
    Models {
        #[command(subcommand)]
        cmd: ModelsCmd,
    },
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
        PickerArg::Provider => Box::new(OpenAiCompat::new(OpenAiCompatConfig::from_env("provider")?)),
        PickerArg::Apple => {
            if !picker::apple::available() {
                bail!("Apple Intelligence model is not available on this Mac (needs macOS 26+ with Apple Intelligence enabled)");
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
            let _ = write!(e, "\r\x1b[2K[{}] {:>3.0}% {}", p.stage, p.fraction * 100.0, p.message);
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
                println!("{}  {}  {:.1}s  hash {}", a.path.display(), a.container, a.duration_ms as f64 / 1000.0, &a.content_hash[..12]);
                if let Some(v) = &a.video {
                    println!(
                        "  video  {} {}x{} → displays {}x{}  {:.2} fps  rotation {}°",
                        v.codec, v.width, v.height, v.display_width, v.display_height,
                        v.fps_num as f64 / v.fps_den as f64, v.rotation_deg
                    );
                }
                if let Some(au) = &a.audio {
                    println!("  audio  {} {} Hz {} ch", au.codec, au.sample_rate, au.channels);
                }
            }
        }
        Cmd::Transcribe { file, engine, model, language, out } => {
            let started = Instant::now();
            let asset = ingest::probe(&file).with_context(|| format!("probing {}", file.display()))?;
            let b = backend::detect();
            let available = b.speech_engines();
            let chosen = match engine {
                EngineArg::Auto => available[0],
                EngineArg::Apple => *available.iter().find(|e| **e == SpeechEngine::AppleSpeech).context("Apple speech is not available on this Mac (needs macOS 26+)")?,
                EngineArg::Whisper => *available.iter().find(|e| matches!(e, SpeechEngine::Whisper { .. })).context("whisper not compiled in")?,
            };
            let opts = TranscribeOptions {
                language,
                whisper_model: model.unwrap_or_else(|| b.recommended_models().whisper_model.unwrap_or_else(|| models_registry::WHISPER_DEFAULT.into())),
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
                t.engine, t.segments.len(), t.word_count(), audio_s, speech_s, media_s / speech_s.max(0.001), started.elapsed().as_secs_f64()
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
        Cmd::Highlights { file, transcript, n, prompt, picker: pk, min_score, out } => {
            let started = Instant::now();
            let llm = make_picker(pk)?;
            let pr = progress_printer(false);
            let asset = ingest::probe(&file).with_context(|| format!("probing {}", file.display()))?;
            let tmp = tempfile::tempdir()?;
            let wav = tmp.path().join("audio.wav");
            ingest::extract_audio(&asset, &wav, &pr)?;
            let feats = audio::analyze(&wav)?;
            let t: Transcript = match transcript {
                Some(p) => serde_json::from_str(&std::fs::read_to_string(&p)?).with_context(|| format!("reading {}", p.display()))?,
                None => {
                    let b = backend::detect();
                    let opts = TranscribeOptions {
                        language: None,
                        whisper_model: b.recommended_models().whisper_model.unwrap_or_else(|| models_registry::WHISPER_DEFAULT.into()),
                        models_dir: models_registry::default_dir(),
                    };
                    transcribe::transcribe(&wav, b.speech_engines()[0], &opts, &pr)?
                }
            };
            let mut opts = HighlightOptions { prompt, ..Default::default() };
            opts.rank.max_clips = n;
            opts.rank.min_score = min_score;
            let (cands, report) = highlights::highlights(&t, Some(&feats), llm.as_ref(), &opts, &pr)?;
            eprintln!(
                "{} via {} · {} windows → {} shortlisted → {} scored ({} declined, {} failed batches) · {} model calls · {:.1}s",
                llm.tier(), llm.model(picker::Role::Score), report.windows, report.shortlisted, report.scored,
                report.declined_batches, report.failed_batches, report.model_calls, started.elapsed().as_secs_f64()
            );
            let u = report.usage;
            if u.score_input + u.summarize_input > 0 {
                eprintln!(
                    "tokens · score {} in / {} out · summarize {} in / {} out",
                    u.score_input, u.score_output, u.summarize_input, u.summarize_output
                );
            }
            let json = serde_json::to_string_pretty(&serde_json::json!({"candidates": cands, "report": report}))?;
            if let Some(out) = out {
                std::fs::write(&out, &json)?;
                eprintln!("wrote {}", out.display());
            }
            if cli.json {
                println!("{json}");
            } else {
                for (i, c) in cands.iter().enumerate() {
                    let ts = |ms: u64| format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60);
                    println!("{:>2}. [{:>3}] {}–{} ({:>2}s)  {}", i + 1, c.score, ts(c.start_ms), ts(c.end_ms), (c.end_ms - c.start_ms) / 1000, c.title);
                    if !c.reasons.is_empty() {
                        println!("               {}", c.reasons.join(" · "));
                    }
                }
            }
        }
        Cmd::Thumbnail { file, at_ms, width, out } => {
            let a = ingest::probe(&file)?;
            ingest::thumbnail(&a, at_ms, width, &out)?;
            eprintln!("wrote {}", out.display());
        }
        Cmd::Doctor => doctor(cli.json)?,
        Cmd::Models { cmd } => {
            let dir = models_registry::default_dir();
            match cmd {
                ModelsCmd::List => {
                    let rows: Vec<_> = models_registry::manifests()
                        .into_iter()
                        .map(|m| (models_registry::is_installed(&dir, &m), m))
                        .collect();
                    if cli.json {
                        let v: Vec<_> = rows.iter().map(|(i, m)| serde_json::json!({"installed": i, "model": m})).collect();
                        println!("{}", serde_json::to_string_pretty(&v)?);
                    } else {
                        println!("models dir: {}", dir.display());
                        for (installed, m) in rows {
                            println!("  {} {:<30} {:>7.1} MB  {}", if installed { "✓" } else { " " }, m.id, m.size as f64 / 1e6, m.license);
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
            hints.push(format!("Speech needs {id} ({:.0} MB). Run: choco models pull {id}", m.size as f64 / 1e6));
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
        hints.push("Apple speech has no language assets yet; the first transcription will download them.".into());
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
        let mem = info.memory_bytes.map(|m| format!("{:.0} GB", m as f64 / 1073741824.0)).unwrap_or_else(|| "?".into());
        println!("choco-engine {}", choco_engine::VERSION);
        println!("  machine      {} {} {} · {} cores · {}", info.os, info.os_version, info.arch, info.cpu_cores, mem);
        if let Some(g) = &info.gpu {
            println!("  chip         {g}");
        }
        println!("  speech       {}", engines.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>().join(" → "));
        if let Some(s) = &apple_status {
            println!("  apple speech available={} · {} locales · installed: {}", s.available, s.locales.len(), s.installed_locales.join(", "));
        }
        println!("  video        decode {:?} · encode {:?}", b.hw_decoders(), b.hw_encoders());
        println!("  ffmpeg       {} · {}", media.avcodec_version, media.license);
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
    v.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0)
}

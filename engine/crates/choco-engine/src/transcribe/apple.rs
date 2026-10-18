//! Apple SpeechAnalyzer (macOS 26+) through the `choco-apple` helper process.
//! Protocol: progress and errors as JSON lines on stderr, the transcript as JSON on stdout.

use super::{Result, TranscribeError, TranscribeOptions, STAGE};
use crate::model::{Progress, Transcript};
use crate::ProgressFn;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HELPER: &str = "choco-apple";

/// Locate the helper: explicit override, next to the executable, inside an app bundle, or the build output.
pub fn helper_path() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    if let Some(p) = std::env::var_os("CHOCO_APPLE_HELPER").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for cand in [dir.join(HELPER), dir.join("../Resources").join(HELPER), dir.join("../Helpers").join(HELPER)] {
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    let built = option_env!("CHOCO_APPLE_HELPER_BUILT").unwrap_or("");
    (!built.is_empty() && Path::new(built).is_file()).then(|| PathBuf::from(built))
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct AppleStatus {
    pub available: bool,
    pub os: String,
    pub locales: Vec<String>,
    pub installed_locales: Vec<String>,
    #[serde(default)]
    pub llm_available: bool,
    #[serde(default)]
    pub llm_status: String,
}

/// Ask the helper whether SpeechAnalyzer is usable and which locales it supports.
pub fn status() -> Option<AppleStatus> {
    let out = Command::new(helper_path()?).arg("status").output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

#[derive(serde::Deserialize)]
struct HelperLine {
    fraction: Option<f32>,
    message: Option<String>,
    error: Option<String>,
}

pub fn transcribe(wav: &Path, opts: &TranscribeOptions, progress: ProgressFn) -> Result<Transcript> {
    let helper = helper_path().ok_or(TranscribeError::Unavailable(crate::backend::SpeechEngine::AppleSpeech))?;
    let locale = opts.language.clone().unwrap_or_else(|| "en-US".into());
    let mut child = Command::new(helper)
        .arg("transcribe")
        .arg(wav)
        .arg(&locale)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stderr = child.stderr.take().expect("piped stderr");
    let mut last_error: Option<String> = None;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        stdout.read_to_end(&mut s).map(|_| s)
    });
    for line in BufReader::new(stderr).lines() {
        let line = line?;
        match serde_json::from_str::<HelperLine>(&line) {
            Ok(HelperLine { error: Some(e), .. }) => last_error = Some(e),
            Ok(HelperLine { fraction, message, .. }) => {
                progress(Progress::new(STAGE, fraction.unwrap_or(-1.0), message.unwrap_or_default()));
            }
            Err(_) if !line.trim().is_empty() => last_error = Some(line),
            Err(_) => {}
        }
    }
    let status = child.wait()?;
    let out = reader.join().map_err(|_| TranscribeError::Apple("stdout reader panicked".into()))??;
    if !status.success() {
        return Err(TranscribeError::Apple(last_error.unwrap_or_else(|| format!("helper exited with {status}"))));
    }
    serde_json::from_slice(&out).map_err(|e| TranscribeError::Apple(format!("bad helper output: {e}")))
}

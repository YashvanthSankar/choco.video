//! Apple Foundation Models (Apple Intelligence, macOS 26+) through the `choco-apple` helper.
//! Zero download, offline, small context: batches stay small.

use super::{Llm, PickerError, Result, Role};
use std::io::{Read, Write};
use std::process::{Command, Stdio};

pub struct AppleLlm;

/// Usable when the helper exists and reports the on-device model as available.
pub fn available() -> bool {
    crate::transcribe::apple::status().is_some_and(|s| s.llm_available)
}

impl AppleLlm {
    fn run(&self, mode: &str, system: &str, user: &str) -> Result<String> {
        let helper = crate::transcribe::apple::helper_path()
            .ok_or(PickerError::Unavailable { tier: "apple", msg: "choco-apple helper not found".into() })?;
        let req = serde_json::json!({"mode": mode, "instructions": system, "prompt": user}).to_string();
        let mut child = Command::new(helper)
            .arg("generate")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| PickerError::Transport(e.to_string()))?;
        child.stdin.take().expect("piped").write_all(req.as_bytes()).map_err(|e| PickerError::Transport(e.to_string()))?;
        let mut out = String::new();
        let mut err = String::new();
        child.stdout.take().expect("piped").read_to_string(&mut out).map_err(|e| PickerError::Transport(e.to_string()))?;
        child.stderr.take().expect("piped").read_to_string(&mut err).map_err(|e| PickerError::Transport(e.to_string()))?;
        let status = child.wait().map_err(|e| PickerError::Transport(e.to_string()))?;
        if status.success() {
            return Ok(out);
        }
        let msg = err
            .lines()
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| v["error"].as_str().map(str::to_string))
            .next_back()
            .unwrap_or_else(|| err.trim().to_string());
        Err(if let Some(m) = msg.strip_prefix("declined: ").or_else(|| msg.contains("sensitive content").then_some(msg.as_str())) {
            PickerError::Declined(m.into())
        } else if let Some(m) = msg.strip_prefix("unavailable: ") {
            PickerError::Unavailable { tier: "apple", msg: m.into() }
        } else {
            PickerError::Transport(msg)
        })
    }
}

impl Llm for AppleLlm {
    fn tier(&self) -> &'static str {
        "apple"
    }
    fn model(&self, _role: Role) -> String {
        "apple-foundation-models".into()
    }
    fn context_tokens(&self) -> usize {
        4_096
    }
    fn batch_size(&self) -> usize {
        4
    }
    fn text(&self, _role: Role, system: &str, user: &str) -> Result<String> {
        let out = self.run("text", system, user)?;
        let v: serde_json::Value = serde_json::from_str(&out).map_err(|e| PickerError::Parse(e.to_string()))?;
        Ok(v["text"].as_str().unwrap_or_default().to_string())
    }
    /// Guided generation first (always schema-valid). If the default guardrails decline the
    /// batch, retry as plain text under the permissive content-transformation guardrails and
    /// let the shared parser validate the JSON.
    fn scores_json(&self, system: &str, user: &str) -> Result<String> {
        match self.run("scores", system, user) {
            Err(PickerError::Declined(_)) => {
                let out = self.run("text", system, &format!("{user}\nReply with the JSON object only."))?;
                let v: serde_json::Value = serde_json::from_str(&out).map_err(|e| PickerError::Parse(e.to_string()))?;
                Ok(v["text"].as_str().unwrap_or_default().to_string())
            }
            other => other,
        }
    }
}

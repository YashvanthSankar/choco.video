//! Local tier: llama.cpp's `llama-server` as a child process on 127.0.0.1, spoken to through the
//! same OpenAI-compatible client as the provider tier, so batching, JSON schemas and parsing are
//! shared. The server and the GGUF model are runtime downloads the user opts into; nothing here
//! ships in the installer.
//!
//! A separate process rather than linking llama.cpp: whisper.cpp already links its own ggml, and
//! two ggml versions in one binary clash. It also keeps AGENTS rule 6 simple: dropping
//! [`LocalLlm`] stops the server and frees the model's memory before rendering starts.

use super::openai_compat::{OpenAiCompat, OpenAiCompatConfig};
use super::{Llm, PickerError, Result, Role, Usage};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const TIER: &str = "local";
/// Loading a 2.7 GB model from a cold disk takes seconds on an SSD, longer on a laptop HDD.
const START_TIMEOUT: Duration = Duration::from_secs(180);
/// Lines of server output kept for the error when it fails to start.
const LOG_LINES: usize = 20;

#[derive(Debug, Clone)]
pub struct LocalConfig {
    /// The `llama-server` executable.
    pub server: PathBuf,
    /// The GGUF model.
    pub model: PathBuf,
    /// Context the server allocates, in tokens; the summary map-reduces to fit it.
    pub context_tokens: usize,
}

impl LocalConfig {
    /// `CHOCO_LLAMA_SERVER` and `CHOCO_LOCAL_MODEL` override the downloaded server and model.
    pub fn from_env_or(server: PathBuf, model: PathBuf) -> Self {
        let get = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            server: get("CHOCO_LLAMA_SERVER").unwrap_or(server),
            model: get("CHOCO_LOCAL_MODEL").unwrap_or(model),
            context_tokens: 16_384,
        }
    }
}

pub struct LocalLlm {
    /// None once [`Llm::unload`] stopped the server.
    child: Mutex<Option<Child>>,
    inner: OpenAiCompat,
}

impl LocalLlm {
    /// Start the server and wait until the model is loaded.
    pub fn start(cfg: &LocalConfig) -> Result<Self> {
        for (what, p) in [("llama-server", &cfg.server), ("model", &cfg.model)] {
            if !p.is_file() {
                return Err(PickerError::NotConfigured(format!(
                    "local {what} not found at {}",
                    p.display()
                )));
            }
        }
        let port = free_port()?;
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let mut child = Command::new(&cfg.server)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .arg("--model")
            .arg(&cfg.model)
            .args(["--ctx-size", &cfg.context_tokens.to_string()])
            // One request at a time gets the whole context; the picker sends them in turn.
            .args(["--parallel", "1", "--threads", &threads.to_string()])
            // Qwen3.5 thinks by default; the picker wants short JSON, not minutes of reasoning.
            .args(["--jinja", "--reasoning-budget", "0"])
            // All layers on the GPU where the build has one (Metal); ignored by CPU builds.
            .args(["--n-gpu-layers", "999", "--no-webui"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| unavailable(format!("could not start {}: {e}", cfg.server.display())))?;
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        if let Some(err) = child.stderr.take() {
            let log = Arc::clone(&log);
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(std::io::Result::ok) {
                    let mut l = log.lock().unwrap();
                    if l.len() == LOG_LINES {
                        l.remove(0);
                    }
                    l.push(line);
                }
            });
        }
        let base_url = format!("http://127.0.0.1:{port}/v1");
        let name = cfg
            .model
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "local".into());
        let llm = Self {
            child: Mutex::new(Some(child)),
            inner: OpenAiCompat::new(OpenAiCompatConfig {
                tier: TIER,
                base_url,
                api_key: "local".into(),
                score_model: name.clone(),
                summarize_model: name,
                context_tokens: cfg.context_tokens,
                // A CPU-only laptop reads a full batch prompt at a few hundred tokens a second.
                timeout: Duration::from_secs(600),
            }),
        };
        llm.wait_ready(port, &log)?;
        Ok(llm)
    }

    fn wait_ready(&self, port: u16, log: &Mutex<Vec<String>>) -> Result<()> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .into();
        let url = format!("http://127.0.0.1:{port}/health");
        let started = Instant::now();
        loop {
            if let Some(Ok(Some(status))) = self.child().as_mut().map(Child::try_wait) {
                return Err(unavailable(format!(
                    "llama-server exited ({status}): {}",
                    log.lock().unwrap().join(" | ")
                )));
            }
            // 503 while the model loads, 200 once it can answer.
            if agent
                .get(&url)
                .call()
                .is_ok_and(|r| r.status().as_u16() == 200)
            {
                return Ok(());
            }
            if started.elapsed() > START_TIMEOUT {
                return Err(unavailable(format!(
                    "llama-server not ready after {} s: {}",
                    START_TIMEOUT.as_secs(),
                    log.lock().unwrap().join(" | ")
                )));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn child(&self) -> MutexGuard<'_, Option<Child>> {
        self.child.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn stop(&self) {
        if let Some(mut c) = self.child().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn running(&self) -> Result<()> {
        match self.child().is_some() {
            true => Ok(()),
            false => Err(unavailable("the local model was unloaded".into())),
        }
    }
}

impl Drop for LocalLlm {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Llm for LocalLlm {
    fn tier(&self) -> &'static str {
        TIER
    }
    fn model(&self, role: Role) -> String {
        self.inner.model(role)
    }
    fn context_tokens(&self) -> usize {
        self.inner.context_tokens()
    }
    fn usage(&self) -> Usage {
        self.inner.usage()
    }
    fn text(&self, role: Role, system: &str, user: &str) -> Result<String> {
        self.running()?;
        self.inner.text(role, system, user)
    }
    fn scores_json(&self, system: &str, user: &str) -> Result<String> {
        self.running()?;
        self.inner.scores_json(system, user)
    }
    fn unload(&self) {
        self.stop();
    }
}

fn unavailable(msg: String) -> PickerError {
    PickerError::Unavailable { tier: TIER, msg }
}

/// A port nothing is listening on right now. The server binds it a moment later; a collision
/// in between shows up as a start error, not a wrong answer.
fn free_port() -> Result<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| unavailable(format!("no free local port: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_are_a_setup_error_not_a_crash() {
        let cfg = LocalConfig {
            server: "/nonexistent/llama-server".into(),
            model: "/nonexistent/model.gguf".into(),
            context_tokens: 4_096,
        };
        match LocalLlm::start(&cfg) {
            Err(PickerError::NotConfigured(m)) => assert!(m.contains("llama-server"), "{m}"),
            Err(e) => panic!("unexpected error: {e}"),
            Ok(_) => panic!("started without a server"),
        }
    }

    #[test]
    fn a_server_that_exits_reports_its_output() {
        // `false` stands in for a server that dies on startup.
        let Some(f) = ["/usr/bin/false", "/bin/false"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
        else {
            return;
        };
        let model = std::env::temp_dir().join("choco-local-test.gguf");
        std::fs::write(&model, b"not a model").unwrap();
        let cfg = LocalConfig {
            server: f,
            model: model.clone(),
            context_tokens: 4_096,
        };
        let err = LocalLlm::start(&cfg).err().expect("must fail");
        std::fs::remove_file(model).ok();
        assert!(
            matches!(err, PickerError::Unavailable { tier: "local", .. }),
            "{err}"
        );
    }
}

//! One client for every OpenAI-compatible `/chat/completions` endpoint: OpenCode Zen (the
//! recommended default) or any provider the user configures. No vendor SDKs, no server of ours.

use super::{Llm, PickerError, Result, Role, Usage, scores_schema};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

pub const OPENCODE_ZEN_BASE_URL: &str = "https://opencode.ai/zen/v1";

#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    pub tier: &'static str,
    pub base_url: String,
    pub api_key: String,
    pub score_model: String,
    pub summarize_model: String,
    pub context_tokens: usize,
    pub timeout: Duration,
}

impl OpenAiCompatConfig {
    /// Provider config from `CHOCO_LLM_BASE_URL`, `CHOCO_LLM_API_KEY`, `CHOCO_LLM_MODEL`, optional
    /// `CHOCO_LLM_SUMMARY_MODEL` and `CHOCO_LLM_CONTEXT_TOKENS`.
    pub fn from_env(tier: &'static str) -> Result<Self> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let api_key = get("CHOCO_LLM_API_KEY").ok_or_else(|| PickerError::NotConfigured("set CHOCO_LLM_API_KEY".into()))?;
        let score_model = get("CHOCO_LLM_MODEL").ok_or_else(|| PickerError::NotConfigured("set CHOCO_LLM_MODEL".into()))?;
        Ok(Self {
            tier,
            base_url: get("CHOCO_LLM_BASE_URL").unwrap_or_else(|| OPENCODE_ZEN_BASE_URL.into()),
            api_key,
            summarize_model: get("CHOCO_LLM_SUMMARY_MODEL").unwrap_or_else(|| score_model.clone()),
            score_model,
            context_tokens: get("CHOCO_LLM_CONTEXT_TOKENS").and_then(|v| v.parse().ok()).unwrap_or(64_000),
            timeout: Duration::from_secs(180),
        })
    }
}

pub struct OpenAiCompat {
    cfg: OpenAiCompatConfig,
    agent: ureq::Agent,
    /// Flipped off the first time the provider rejects `json_schema`.
    json_schema_ok: AtomicBool,
    /// score in, score out, summarize in, summarize out
    tokens: [AtomicU64; 4],
}

impl OpenAiCompat {
    pub fn new(cfg: OpenAiCompatConfig) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(cfg.timeout))
            .build()
            .into();
        Self { cfg, agent, json_schema_ok: AtomicBool::new(true), tokens: Default::default() }
    }

    fn post(&self, body: &Value) -> Result<(u16, String)> {
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let mut resp = self
            .agent
            .post(&url)
            .header("Authorization", &format!("Bearer {}", self.cfg.api_key))
            // OpenCode Zen sits behind Cloudflare, which rejects some default client signatures
            // (error 1010). Always identify ourselves.
            .header("User-Agent", concat!("choco-video/", env!("CARGO_PKG_VERSION")))
            .header("Content-Type", "application/json")
            .send(body.to_string().as_bytes())
            .map_err(|e| PickerError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().map_err(|e| PickerError::Transport(e.to_string()))?;
        Ok((status, text))
    }

    fn complete(&self, role: Role, system: &str, user: &str, format: Option<Value>) -> Result<String> {
        let model = self.model(role);
        let mut body = json!({
            "model": model,
            "temperature": 0.2,
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        });
        if let Some(f) = format {
            body["response_format"] = f;
        }
        let mut attempt = 0;
        loop {
            let (status, text) = self.post(&body)?;
            if status == 200 {
                let v: Value = serde_json::from_str(&text).map_err(|e| PickerError::Transport(format!("bad response JSON: {e}")))?;
                let base = if role == Role::Score { 0 } else { 2 };
                self.tokens[base].fetch_add(v["usage"]["prompt_tokens"].as_u64().unwrap_or(0), Ordering::Relaxed);
                self.tokens[base + 1].fetch_add(v["usage"]["completion_tokens"].as_u64().unwrap_or(0), Ordering::Relaxed);
                let msg = &v["choices"][0]["message"];
                if let Some(r) = msg["refusal"].as_str().filter(|r| !r.is_empty()) {
                    return Err(PickerError::Declined(r.to_string()));
                }
                return msg["content"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| PickerError::Transport(format!("no message content in: {}", text.chars().take(300).collect::<String>())));
            }
            if matches!(status, 429 | 500..=599) && attempt < 3 {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(1000 * 2u64.pow(attempt)));
                continue;
            }
            return Err(PickerError::Http { status, body: text.chars().take(500).collect() });
        }
    }
}

impl Llm for OpenAiCompat {
    fn tier(&self) -> &'static str {
        self.cfg.tier
    }
    fn model(&self, role: Role) -> String {
        match role {
            Role::Score => self.cfg.score_model.clone(),
            Role::Summarize => self.cfg.summarize_model.clone(),
        }
    }
    fn context_tokens(&self) -> usize {
        self.cfg.context_tokens
    }
    fn usage(&self) -> Usage {
        let t = |i: usize| self.tokens[i].load(Ordering::Relaxed);
        Usage { score_input: t(0), score_output: t(1), summarize_input: t(2), summarize_output: t(3) }
    }
    fn text(&self, role: Role, system: &str, user: &str) -> Result<String> {
        self.complete(role, system, user, None)
    }
    fn scores_json(&self, system: &str, user: &str) -> Result<String> {
        if self.json_schema_ok.load(Ordering::Relaxed) {
            let fmt = json!({"type": "json_schema", "json_schema": {"name": "scores", "strict": true, "schema": scores_schema()}});
            match self.complete(Role::Score, system, user, Some(fmt)) {
                Err(PickerError::Http { status: 400 | 422, body }) if body.contains("response_format") || body.contains("json_schema") => {
                    self.json_schema_ok.store(false, Ordering::Relaxed);
                }
                other => return other,
            }
        }
        self.complete(Role::Score, system, user, Some(json!({"type": "json_object"})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// Minimal HTTP server: answers each request with the next canned (status, body) and records requests.
    fn mock(responses: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf).unwrap();
                seen2.lock().unwrap().push(String::from_utf8(buf).unwrap());
                let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                stream.write_all(reply.as_bytes()).unwrap();
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    fn client(base: String) -> OpenAiCompat {
        OpenAiCompat::new(OpenAiCompatConfig {
            tier: "provider",
            base_url: base,
            api_key: "k".into(),
            score_model: "cheap".into(),
            summarize_model: "strong".into(),
            context_tokens: 32_000,
            timeout: Duration::from_secs(5),
        })
    }

    fn ok(content: &str) -> (u16, String) {
        (200, json!({"choices": [{"message": {"role": "assistant", "content": content}}], "usage": {"prompt_tokens": 10, "completion_tokens": 3}}).to_string())
    }

    #[test]
    fn sends_json_schema_and_uses_role_models() {
        let (base, seen) = mock(vec![ok("{\"scores\":[]}"), ok("a summary")]);
        let c = client(base);
        assert_eq!(c.scores_json("sys", "user").unwrap(), "{\"scores\":[]}");
        assert_eq!(c.text(Role::Summarize, "sys", "user").unwrap(), "a summary");
        assert_eq!(c.usage(), Usage { score_input: 10, score_output: 3, summarize_input: 10, summarize_output: 3 });
        let reqs = seen.lock().unwrap();
        let first: Value = serde_json::from_str(&reqs[0]).unwrap();
        assert_eq!(first["model"], "cheap");
        assert_eq!(first["response_format"]["type"], "json_schema");
        let second: Value = serde_json::from_str(&reqs[1]).unwrap();
        assert_eq!(second["model"], "strong");
        assert!(second.get("response_format").is_none());
    }

    #[test]
    fn falls_back_to_json_object_once_and_remembers() {
        let (base, seen) = mock(vec![
            (400, "{\"error\":{\"message\":\"response_format json_schema is not supported\"}}".into()),
            ok("{\"scores\":[]}"),
            ok("{\"scores\":[]}"),
        ]);
        let c = client(base);
        c.scores_json("s", "u").unwrap();
        c.scores_json("s", "u").unwrap();
        let reqs = seen.lock().unwrap();
        let kinds: Vec<String> = reqs.iter().map(|r| serde_json::from_str::<Value>(r).unwrap()["response_format"]["type"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, vec!["json_schema", "json_object", "json_object"]);
    }

    #[test]
    fn surfaces_auth_errors_without_retrying() {
        let (base, seen) = mock(vec![(401, "{\"error\":\"bad key\"}".into())]);
        let err = client(base).text(Role::Score, "s", "u").unwrap_err();
        assert!(matches!(err, PickerError::Http { status: 401, .. }), "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}

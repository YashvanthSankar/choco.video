//! The picker: the one generative step. A backend only has to answer two kinds of request,
//! plain text (episode summary) and scores JSON; batching, prompts, parsing and retries are
//! shared so every tier behaves the same.

use crate::model::PickerProvenance;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod apple;
pub mod openai_compat;

pub const SUMMARIZE_PROMPT: &str = include_str!("prompts/summarize_v1.md");
pub const SCORE_PROMPT: &str = include_str!("prompts/score_v1.md");

#[derive(Debug, thiserror::Error)]
pub enum PickerError {
    #[error("picker: not configured: {0}")]
    NotConfigured(String),
    #[error("picker: {tier} unavailable: {msg}")]
    Unavailable { tier: &'static str, msg: String },
    #[error("picker: http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("picker: transport: {0}")]
    Transport(String),
    #[error("picker: model output was not valid scores JSON: {0}")]
    Parse(String),
    #[error("picker: the model declined this batch: {0}")]
    Declined(String),
}

pub type Result<T> = std::result::Result<T, PickerError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Cheap model, many calls.
    Score,
    /// Stronger model, few calls.
    Summarize,
}

pub trait Llm: Send + Sync {
    /// "provider", "apple" or "local".
    fn tier(&self) -> &'static str;
    fn model(&self, role: Role) -> String;
    /// Total context the backend can take, in tokens.
    fn context_tokens(&self) -> usize;
    /// Candidates per scoring call.
    fn batch_size(&self) -> usize {
        8
    }
    /// Tokens consumed so far, per role, when the backend reports them.
    fn usage(&self) -> Usage {
        Usage::default()
    }
    fn text(&self, role: Role, system: &str, user: &str) -> Result<String>;
    /// Must return a JSON string shaped like [`ScoresOut`]; the caller parses and validates it.
    fn scores_json(&self, system: &str, user: &str) -> Result<String>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub score_input: u64,
    pub score_output: u64,
    pub summarize_input: u64,
    pub summarize_output: u64,
}

impl Usage {
    /// Dollar cost given per-million-token prices for the scoring and summarizing models.
    pub fn cost(&self, score: (f64, f64), summarize: (f64, f64)) -> f64 {
        (self.score_input as f64 * score.0 + self.score_output as f64 * score.1 + self.summarize_input as f64 * summarize.0
            + self.summarize_output as f64 * summarize.1)
            / 1e6
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreOut {
    pub id: String,
    pub score: f32,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub hashtags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoresOut {
    pub scores: Vec<ScoreOut>,
}

/// JSON Schema for [`ScoresOut`], used for `response_format: json_schema`.
pub fn scores_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["scores"],
        "properties": {
            "scores": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "score", "reasons", "title", "description", "hashtags"],
                    "properties": {
                        "id": {"type": "string"},
                        "score": {"type": "integer", "minimum": 0, "maximum": 100},
                        "reasons": {"type": "array", "items": {"type": "string"}},
                        "title": {"type": "string"},
                        "description": {"type": "string"},
                        "hashtags": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
        }
    })
}

/// Strip reasoning blocks and code fences, then take the outermost JSON object.
pub(crate) fn extract_json(raw: &str) -> &str {
    let mut s = raw;
    if let Some(end) = s.rfind("</think>") {
        s = &s[end + "</think>".len()..];
    }
    match (s.find('{'), s.rfind('}')) {
        (Some(a), Some(b)) if b > a => &s[a..=b],
        _ => s.trim(),
    }
}

/// Quote our own field names when a model leaves them bare or half-quoted
/// (`{score:72` or `,reasons":`). Only touches the schema's keys, never values.
pub(crate) fn repair_keys(s: &str) -> String {
    const KEYS: &[&str] = &["scores", "id", "score", "reasons", "title", "description", "hashtags"];
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i] as char;
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '{' || c == ',' {
            out.push(c);
            let mut j = i + 1;
            while j < b.len() && (b[j] as char).is_whitespace() {
                j += 1;
            }
            let ident_start = j;
            while j < b.len() && ((b[j] as char).is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let ident = &s[ident_start..j];
            let mut k = j;
            if k < b.len() && b[k] == b'"' {
                k += 1; // half-quoted: reasons":
            }
            if KEYS.contains(&ident) && k < b.len() && b[k] == b':' && ident_start < b.len() && b[ident_start] != b'"' {
                out.push('"');
                out.push_str(ident);
                out.push('"');
                i = k;
                continue;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Parse scores; accept a bare array too, clamp scores, drop unknown ids.
pub fn parse_scores(raw: &str, expected_ids: &[String]) -> Result<Vec<ScoreOut>> {
    let repaired = repair_keys(extract_json(raw));
    let body = repaired.as_str();
    let mut scores: Vec<ScoreOut> = match serde_json::from_str::<ScoresOut>(body) {
        Ok(s) => s.scores,
        Err(e) => match serde_json::from_str::<Vec<ScoreOut>>(raw.trim()) {
            Ok(v) => v,
            Err(_) => return Err(PickerError::Parse(format!("{e}; output began: {}", raw.chars().take(200).collect::<String>()))),
        },
    };
    scores.retain(|s| expected_ids.contains(&s.id));
    for s in &mut scores {
        s.score = s.score.clamp(0.0, 100.0);
        s.title = s.title.chars().take(80).collect();
        s.hashtags.retain(|h| h.starts_with('#') && h.len() > 1);
        s.hashtags.truncate(5);
    }
    if scores.is_empty() {
        return Err(PickerError::Parse("no scores for the requested ids".into()));
    }
    Ok(scores)
}

pub fn estimate_tokens(s: &str) -> usize {
    s.len() / 4 + 1
}

pub fn prompt_hash() -> String {
    let mut h = blake3::Hasher::new();
    h.update(SUMMARIZE_PROMPT.as_bytes());
    h.update(SCORE_PROMPT.as_bytes());
    h.finalize().to_hex()[..16].to_string()
}

pub fn provenance(llm: &dyn Llm) -> PickerProvenance {
    PickerProvenance { tier: llm.tier().into(), model: llm.model(Role::Score), prompt_hash: prompt_hash() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_and_thinking_output() {
        let ids = vec!["c1".to_string(), "c2".to_string()];
        let raw = "<think>hmm</think>\n```json\n{\"scores\":[{\"id\":\"c1\",\"score\":140,\"reasons\":[],\"title\":\"t\",\"description\":\"d\",\"hashtags\":[\"#a\",\"b\"]},{\"id\":\"zz\",\"score\":5}]}\n```";
        let s = parse_scores(raw, &ids).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].score, 100.0);
        assert_eq!(s[0].hashtags, vec!["#a"]);
    }

    #[test]
    fn repairs_bare_and_half_quoted_keys() {
        let raw = r##"{"scores":[{"id":"w923",score:72,reasons":["a, b: c"],"title":"x:y","description":"d","hashtags":["#h"]}]}"##;
        let s = parse_scores(raw, &["w923".into()]).unwrap();
        assert_eq!(s[0].score, 72.0);
        assert_eq!(s[0].reasons, vec!["a, b: c"]);
        assert_eq!(s[0].title, "x:y");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_scores("I cannot help with that", &["c1".into()]).is_err());
    }

    #[test]
    fn schema_is_strict() {
        let s = scores_schema();
        assert_eq!(s["properties"]["scores"]["items"]["additionalProperties"], false);
    }
}

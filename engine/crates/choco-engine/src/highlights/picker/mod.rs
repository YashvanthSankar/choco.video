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
    /// "hosted", "apple", "byok" or "local".
    fn tier(&self) -> &'static str;
    fn model(&self, role: Role) -> String;
    /// Total context the backend can take, in tokens.
    fn context_tokens(&self) -> usize;
    /// Candidates per scoring call.
    fn batch_size(&self) -> usize {
        8
    }
    fn text(&self, role: Role, system: &str, user: &str) -> Result<String>;
    /// Must return a JSON string shaped like [`ScoresOut`]; the caller parses and validates it.
    fn scores_json(&self, system: &str, user: &str) -> Result<String>;
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

/// Parse scores; accept a bare array too, clamp scores, drop unknown ids.
pub fn parse_scores(raw: &str, expected_ids: &[String]) -> Result<Vec<ScoreOut>> {
    let body = extract_json(raw);
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
    fn rejects_garbage() {
        assert!(parse_scores("I cannot help with that", &["c1".into()]).is_err());
    }

    #[test]
    fn schema_is_strict() {
        let s = scores_schema();
        assert_eq!(s["properties"]["scores"]["items"]["additionalProperties"], false);
    }
}

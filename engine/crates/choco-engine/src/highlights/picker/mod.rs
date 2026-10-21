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
pub const TRANSLATE_PROMPT: &str = include_str!("prompts/translate_v1.md");

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
    #[error("picker: the request did not fit the model's context: {0}")]
    ContextTooLong(String),
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
    /// Scoring calls in flight at once. A hosted endpoint answers several side by side; the
    /// on-device model runs one at a time.
    fn concurrency(&self) -> usize {
        1
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
        (self.score_input as f64 * score.0
            + self.score_output as f64 * score.1
            + self.summarize_input as f64 * summarize.0
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

/// Strip reasoning blocks and code fences, then take the outermost JSON object, or the outermost
/// array when the reply is a bare array of scores.
pub(crate) fn extract_json(raw: &str) -> &str {
    let mut s = raw;
    if let Some(end) = s.rfind("</think>") {
        s = &s[end + "</think>".len()..];
    }
    let obj = s.find('{').map(|a| (a, s.rfind('}')));
    let arr = s.find('[').map(|a| (a, s.rfind(']')));
    match (obj, arr) {
        (Some((a, Some(b))), Some((c, _))) if b > a && a <= c => &s[a..=b],
        (_, Some((c, Some(d)))) if d > c => &s[c..=d],
        (Some((a, Some(b))), _) if b > a => &s[a..=b],
        _ => s.trim(),
    }
}

/// Quote our own field names when a model leaves them bare or half-quoted
/// (`{score:72` or `,reasons":`). Only touches the schema's keys, never values.
pub(crate) fn repair_keys(s: &str) -> String {
    const KEYS: &[&str] = &[
        "scores",
        "id",
        "score",
        "reasons",
        "title",
        "description",
        "hashtags",
    ];
    // Work on bytes but copy them through untouched, so multi-byte UTF-8 (dashes, accents,
    // Devanagari, emoji) survives. Only ASCII structure characters are ever inspected.
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len() + 16);
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'{' || c == b',' {
            out.push(c);
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let ident_start = j;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let ident = &s[ident_start..j];
            let mut k = j;
            if k < b.len() && b[k] == b'"' {
                k += 1; // half-quoted: reasons":
            }
            if KEYS.contains(&ident)
                && k < b.len()
                && b[k] == b':'
                && ident_start < b.len()
                && b[ident_start] != b'"'
            {
                out.push(b'"');
                out.extend_from_slice(ident.as_bytes());
                out.push(b'"');
                i = k;
                continue;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
        }
        out.push(c);
        i += 1;
    }
    // Only ASCII bytes were inserted and only at ASCII boundaries, so this cannot fail.
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Parse scores; accept a bare array too, clamp scores, drop unknown ids.
pub fn parse_scores(raw: &str, expected_ids: &[String]) -> Result<Vec<ScoreOut>> {
    let repaired = repair_keys(extract_json(raw));
    let body = repaired.as_str();
    // Parse loosely first, then convert entries one by one so a single malformed entry
    // (score as a string, missing field) drops that entry instead of the whole batch.
    let value: Value = serde_json::from_str(body).map_err(|e| {
        PickerError::Parse(format!(
            "{e}; output began: {}",
            raw.chars().take(200).collect::<String>()
        ))
    })?;
    let entries: Vec<Value> = match value {
        Value::Object(mut m) => m
            .remove("scores")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Value::Array(a) => a,
        _ => Vec::new(),
    };
    let mut scores: Vec<ScoreOut> = entries
        .into_iter()
        .filter_map(|mut e| {
            // Tolerate numeric strings and float ids.
            if let Some(s) = e
                .get("score")
                .and_then(|v| v.as_str())
                .and_then(|s| s.trim().parse::<f32>().ok())
            {
                e["score"] = Value::from(s);
            }
            if let Some(n) = e.get("id").and_then(|v| v.as_u64()) {
                e["id"] = Value::from(n.to_string());
            }
            serde_json::from_value::<ScoreOut>(e).ok()
        })
        .collect();
    scores.retain(|s| expected_ids.contains(&s.id));
    for s in &mut scores {
        s.score = s.score.clamp(0.0, 100.0);
        s.title = trim_title(&s.title, 60);
        s.hashtags.retain(|h| h.starts_with('#') && h.len() > 1);
        s.hashtags.truncate(5);
    }
    if scores.is_empty() {
        return Err(PickerError::Parse("no scores for the requested ids".into()));
    }
    Ok(scores)
}

/// Cut at a word boundary to at most `max` characters, no ellipsis.
pub(crate) fn trim_title(t: &str, max: usize) -> String {
    let t = t.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let cut: String = t.chars().take(max).collect();
    match cut.rfind(' ') {
        Some(i) if i > max / 2 => cut[..i].trim_end_matches([',', ':', ';', '-']).to_string(),
        _ => cut,
    }
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
    PickerProvenance {
        tier: llm.tier().into(),
        model: llm.model(Role::Score),
        prompt_hash: prompt_hash(),
    }
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
    fn repair_keeps_unicode_intact() {
        let raw = "{\"scores\":[{\"id\":\"w1\",score:80,\"reasons\":[\"naïve – ok\"],\"title\":\"Leadership — hypocrisy 🎯 नेतृत्व\",\"description\":\"d\",\"hashtags\":[\"#x\"]}]}";
        assert_eq!(repair_keys("{\"t\":\"— é 🎯\"}"), "{\"t\":\"— é 🎯\"}");
        let s = parse_scores(raw, &["w1".into()]).unwrap();
        assert_eq!(s[0].title, "Leadership — hypocrisy 🎯 नेतृत्व");
        assert_eq!(s[0].reasons, vec!["naïve – ok"]);
    }

    #[test]
    fn bare_arrays_fences_and_bad_entries() {
        let ids = vec!["c1".to_string(), "c2".to_string()];
        let raw = "```json\n[{\"id\":\"c1\",\"score\":\"77\",\"reasons\":[],\"title\":\"t\",\"description\":\"d\",\"hashtags\":[]},{\"id\":\"c2\",\"score\":\"high\"}]\n```";
        let s = parse_scores(raw, &ids).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].score, 77.0);
        assert_eq!(
            trim_title(
                "A very long title that goes on and on and on past sixty characters for sure",
                60
            ),
            "A very long title that goes on and on and on past sixty"
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_scores("I cannot help with that", &["c1".into()]).is_err());
    }

    #[test]
    fn schema_is_strict() {
        let s = scores_schema();
        assert_eq!(
            s["properties"]["scores"]["items"]["additionalProperties"],
            false
        );
    }
}

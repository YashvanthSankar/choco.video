//! Data contracts between stages. Every stage takes these in and returns them out.
//! Times are `u64` milliseconds everywhere. Never floats for time.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A probed source file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaAsset {
    pub path: PathBuf,
    /// blake3 over size + first and last 8 MiB. Stable cache key that does not read whole files.
    pub content_hash: String,
    pub duration_ms: u64,
    pub container: String,
    pub video: Option<VideoStream>,
    pub audio: Option<AudioStream>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoStream {
    pub index: usize,
    pub codec: String,
    /// Coded size, before rotation.
    pub width: u32,
    pub height: u32,
    /// Size as displayed, after applying `rotation_deg`.
    pub display_width: u32,
    pub display_height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    /// Clockwise display rotation from the container's display matrix. 0, 90, 180 or 270.
    pub rotation_deg: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStream {
    pub index: usize,
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    /// BCP-47 language tag, e.g. "en-US".
    pub language: String,
    /// Which engine and model produced this, e.g. "apple-speech" or "whisper:large-v3-turbo-q5_0".
    pub engine: String,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// 0.0..=1.0. Engines that do not report confidence use 1.0.
    pub confidence: f32,
}

impl Transcript {
    pub fn word_count(&self) -> usize {
        self.segments.iter().map(|s| s.words.len()).sum()
    }
    pub fn text(&self) -> String {
        self.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// 0..=100
    pub score: u8,
    pub reasons: Vec<String>,
    /// Hook title, at most 60 characters.
    pub title: String,
    pub description: String,
    pub hashtags: Vec<String>,
    pub provenance: PickerProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PickerProvenance {
    /// "provider", "apple" or "local".
    pub tier: String,
    pub model: String,
    /// blake3 of the prompt file contents used.
    pub prompt_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    Single,
    Split,
    Static,
    Fit,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Hold,
    Linear,
    EaseInOut,
    Cut,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CropKeyframe {
    pub t_ms: u64,
    /// One rect per face slot: 1 for Single/Static/Fit, 2 for Split. Normalized 0..1 in source space.
    pub rects: Vec<Rect>,
    pub easing: Easing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CropPlan {
    pub layout: Layout,
    pub keyframes: Vec<CropKeyframe>,
    pub source_size: (u32, u32),
    pub target_size: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionStyle {
    pub name: String,
    pub font_family: String,
    /// Percent of frame height.
    pub size_pct: f32,
    /// Percent of frame height from the bottom edge.
    pub position_pct: f32,
    pub fill: String,
    pub stroke: Option<String>,
    pub shadow: bool,
    pub active_fill: Option<String>,
    pub active_scale: f32,
    pub active_pill: Option<String>,
    pub max_lines: u8,
    pub uppercase: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionChunk {
    pub start_ms: u64,
    pub end_ms: u64,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionTrack {
    pub style: CaptionStyle,
    pub chunks: Vec<CaptionChunk>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderStatus {
    Pending,
    Rendering,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub candidate_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub crop: Option<CropPlan>,
    pub style: Option<CaptionStyle>,
    pub rejected: bool,
    pub status: RenderStatus,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub asset: MediaAsset,
    pub transcript: Option<Transcript>,
    pub candidates: Vec<Candidate>,
    pub clips: Vec<Clip>,
}

impl Project {
    pub const VERSION: u32 = 1;
}

/// Progress events. The CLI prints them, the app forwards them to the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub stage: String,
    /// 0.0..=1.0, or negative when unknown.
    pub fraction: f32,
    pub message: String,
}

impl Progress {
    pub fn new(stage: &str, fraction: f32, message: impl Into<String>) -> Self {
        Self { stage: stage.to_string(), fraction, message: message.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: &T) {
        let json = serde_json::to_string(v).unwrap();
        let back: T = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, v);
    }

    fn word(s: u64, e: u64, t: &str) -> Word {
        Word { start_ms: s, end_ms: e, text: t.into(), confidence: 0.9 }
    }

    #[test]
    fn transcript_roundtrip_and_helpers() {
        let t = Transcript {
            language: "en-US".into(),
            engine: "test".into(),
            segments: vec![Segment {
                start_ms: 0,
                end_ms: 900,
                text: "hello world".into(),
                words: vec![word(0, 400, "hello"), word(450, 900, "world")],
            }],
        };
        roundtrip(&t);
        assert_eq!(t.word_count(), 2);
        assert_eq!(t.text(), "hello world");
    }

    #[test]
    fn project_roundtrip() {
        let p = Project {
            version: Project::VERSION,
            asset: MediaAsset {
                path: "/tmp/a.mp4".into(),
                content_hash: "abc".into(),
                duration_ms: 1000,
                container: "mov,mp4".into(),
                video: Some(VideoStream {
                    index: 0, codec: "h264".into(), width: 1920, height: 1080,
                    display_width: 1920, display_height: 1080, fps_num: 30, fps_den: 1, rotation_deg: 0,
                }),
                audio: Some(AudioStream { index: 1, codec: "aac".into(), sample_rate: 48000, channels: 2 }),
            },
            transcript: None,
            candidates: vec![Candidate {
                id: "c1".into(), start_ms: 0, end_ms: 1000, score: 80, reasons: vec!["hook".into()],
                title: "t".into(), description: "d".into(), hashtags: vec!["#x".into()],
                provenance: PickerProvenance { tier: "hosted".into(), model: "m".into(), prompt_hash: "h".into() },
            }],
            clips: vec![Clip {
                candidate_id: "c1".into(), start_ms: 0, end_ms: 1000,
                crop: Some(CropPlan {
                    layout: Layout::Split,
                    keyframes: vec![CropKeyframe {
                        t_ms: 0,
                        rects: vec![Rect { x: 0.1, y: 0.0, w: 0.3, h: 0.5 }; 2],
                        easing: Easing::EaseInOut,
                    }],
                    source_size: (1920, 1080), target_size: (1080, 1920),
                }),
                style: None, rejected: false, status: RenderStatus::Pending, output: None,
            }],
        };
        roundtrip(&p);
        let json = serde_json::to_string(&p.clips[0].crop.as_ref().unwrap().layout).unwrap();
        assert_eq!(json, "\"split\"");
    }
}

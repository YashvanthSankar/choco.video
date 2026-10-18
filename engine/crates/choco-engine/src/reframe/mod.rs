//! Reframe: decide where the vertical crop goes for each moment of a clip.
//!
//! Samples the clip at 5 fps, detects faces (YuNet via tract), tracks them, measures mouth
//! motion and scene cuts, then plans single-speaker follow, split screen, or a centre crop.

use crate::model::{CropPlan, MediaAsset, Transcript};
use crate::render::{self, RenderError, yuv};
use std::sync::atomic::AtomicBool;

pub mod faces;
pub mod plan;
pub mod track;

pub use plan::{PlanOptions, PlanReport};

#[derive(Debug, thiserror::Error)]
pub enum ReframeError {
    #[error("reframe: face model: {0}")]
    Model(String),
    #[error(transparent)]
    Render(#[from] RenderError),
}

pub const SAMPLE_FPS: f64 = 5.0;

/// Plan the crop for `[start_ms, end_ms)` of `asset`.
pub fn auto_plan(asset: &MediaAsset, t: &Transcript, start_ms: u64, end_ms: u64, opts: &PlanOptions, cancel: &AtomicBool) -> Result<(CropPlan, PlanReport), ReframeError> {
    let v = asset.video.as_ref().ok_or(RenderError::NoVideo)?;
    let display = (v.display_width, v.display_height);
    let mut det = faces::FaceDetector::new().map_err(|e| ReframeError::Model(e.to_string()))?;
    let mut samples: Vec<track::Sample> = Vec::new();
    let mut prev: Option<(yuv::Frame, [f32; 32])> = None;
    let mut model_err: Option<String> = None;
    render::sample_frames(asset, start_ms, end_ms, SAMPLE_FPS, cancel, |t_ms, frame| {
        let found = match det.detect(frame) {
            Ok(f) => f,
            Err(e) => {
                model_err.get_or_insert(e.to_string());
                Vec::new()
            }
        };
        let hist = track::histogram(frame);
        let cut = prev.as_ref().is_some_and(|(_, h)| track::is_cut(h, &hist));
        let motion = found.iter().map(|f| prev.as_ref().map(|(p, _)| track::mouth_motion(f, frame, p)).unwrap_or(0.0)).collect();
        samples.push(track::Sample { t_ms, faces: found, motion, cut });
        prev = Some((frame.clone(), hist));
        Ok(())
    })?;
    if let Some(e) = model_err {
        return Err(ReframeError::Model(e));
    }
    let tracks = track::track(&samples, 600, 1000);
    let times: Vec<u64> = samples.iter().map(|s| s.t_ms).collect();
    let cuts: Vec<u64> = samples.iter().filter(|s| s.cut).map(|s| s.t_ms).collect();
    let words: Vec<(u64, u64)> = t
        .segments
        .iter()
        .flat_map(|s| s.words.iter())
        .filter(|w| w.end_ms > start_ms && w.start_ms < end_ms)
        .map(|w| (w.start_ms, w.end_ms))
        .collect();
    Ok(plan::plan(&tracks, &times, &cuts, &words, display, (start_ms, end_ms), opts))
}

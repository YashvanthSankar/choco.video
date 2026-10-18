//! Export a picked clip: MP4, poster PNG, SRT and VTT captions, and a metadata sidecar with
//! title, description, hashtags, score and source timecodes. Names are
//! `<source>-<rank>-<title-slug>.*` so a folder of exports sorts in rank order.

use crate::captions::{self, ChunkOptions, Rasterizer, subtitles};
use crate::model::{Candidate, CaptionStyle, Layout, MediaAsset, Transcript};
use crate::render::{self, RenderError, RenderReport, RenderSpec};
use crate::ProgressFn;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Output size presets. TikTok, Reels and Shorts share the same spec today.
pub fn preset_size(name: &str) -> Option<(u32, u32)> {
    match name {
        "shorts" | "tiktok" | "reels" | "vertical" => Some((1080, 1920)),
        "square" => Some((1080, 1080)),
        "portrait" => Some((1080, 1350)),
        "landscape" => Some((1920, 1080)),
        _ => None,
    }
}

pub const PRESETS: &[&str] = &["shorts", "square", "portrait", "landscape"];

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub preset: String,
    /// None = no burned-in captions (sidecars are still written).
    pub style: Option<CaptionStyle>,
    pub layout: Layout,
    /// Track faces and follow the speaker (or split for two people); `layout` is then ignored.
    pub auto_reframe: bool,
    pub normalize_audio: bool,
    pub video_bitrate: usize,
    pub out_dir: PathBuf,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            preset: "shorts".into(),
            style: captions::preset("bold"),
            layout: Layout::Static,
            auto_reframe: true,
            normalize_audio: true,
            video_bitrate: 10_000_000,
            out_dir: PathBuf::from("."),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportedClip {
    pub rank: usize,
    pub candidate: Candidate,
    pub mp4: PathBuf,
    pub poster: PathBuf,
    pub srt: PathBuf,
    pub vtt: PathBuf,
    pub json: PathBuf,
    pub render: RenderReport,
    /// How the crop was decided, when auto-reframe ran.
    pub reframe: Option<crate::reframe::PlanReport>,
}

#[derive(Serialize)]
struct Sidecar<'a> {
    title: &'a str,
    description: &'a str,
    hashtags: &'a [String],
    score: u8,
    reasons: &'a [String],
    source: &'a Path,
    source_start_ms: u64,
    source_end_ms: u64,
    duration_ms: u64,
    preset: &'a str,
    size: (u32, u32),
    caption_style: Option<&'a str>,
    picker: &'a crate::model::PickerProvenance,
    transcript_engine: &'a str,
    files: Files<'a>,
}

#[derive(Serialize)]
struct Files<'a> {
    video: &'a str,
    poster: &'a str,
    srt: &'a str,
    vtt: &'a str,
}

/// Lowercase ASCII slug, at most 40 characters, never empty.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "clip".into() } else { out }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("export: unknown preset {0} (use one of {PRESETS:?})")]
    Preset(String),
    #[error("export: source has no video stream")]
    NoVideo,
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    Reframe(#[from] crate::reframe::ReframeError),
    #[error("export: io: {0}")]
    Io(#[from] std::io::Error),
    #[error("export: json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Shared per-job state: the caption rasterizer, a progress sink and a cancel flag.
pub struct Job<'a> {
    pub raster: &'a mut Rasterizer,
    pub progress: ProgressFn<'a>,
    pub cancel: &'a AtomicBool,
}

pub fn export_clip(asset: &MediaAsset, transcript: &Transcript, cand: &Candidate, rank: usize, opts: &ExportOptions, job: &mut Job) -> Result<ExportedClip, ExportError> {
    let raster = &mut *job.raster;
    let size = preset_size(&opts.preset).ok_or_else(|| ExportError::Preset(opts.preset.clone()))?;
    let v = asset.video.as_ref().ok_or(ExportError::NoVideo)?;
    std::fs::create_dir_all(&opts.out_dir)?;
    let stem_src = asset.path.file_stem().map(|s| slug(&s.to_string_lossy())).unwrap_or_else(|| "video".into());
    let stem = format!("{stem_src}-{rank:02}-{}", slug(&cand.title));
    let path = |ext: &str| opts.out_dir.join(format!("{stem}.{ext}"));
    let (mp4, poster, srt, vtt, json) = (path("mp4"), path("png"), path("srt"), path("vtt"), path("json"));

    let (plan, reframe) = if opts.auto_reframe {
        let po = crate::reframe::PlanOptions { target: size, ..Default::default() };
        let (p, r) = crate::reframe::auto_plan(asset, transcript, cand.start_ms, cand.end_ms, &po, job.cancel)?;
        (p, Some(r))
    } else {
        let mut p = render::center_crop_plan((v.display_width, v.display_height), size, cand.start_ms);
        p.layout = opts.layout;
        (p, None)
    };

    // Sidecar captions always exist; burned-in captions only when a style is chosen. In a split
    // the captions sit on the seam between the two people, not over the lower face.
    let mut sidecar_style = opts.style.clone().unwrap_or_else(|| captions::preset("clean").expect("built-in"));
    if plan.layout == Layout::Split {
        sidecar_style.position_pct = 46.0;
    }
    let track = captions::chunk(transcript, (cand.start_ms, cand.end_ms), &sidecar_style, size, raster, &ChunkOptions::default());
    std::fs::write(&srt, subtitles::srt(&track, cand.start_ms))?;
    std::fs::write(&vtt, subtitles::vtt(&track, cand.start_ms))?;

    let spec = RenderSpec {
        start_ms: cand.start_ms,
        end_ms: cand.end_ms,
        size,
        crop: &plan,
        captions: opts.style.as_ref().map(|_| &track),
        normalize_audio: opts.normalize_audio,
        max_fps: 60,
        video_bitrate: opts.video_bitrate,
        poster: Some(((cand.start_ms + cand.end_ms) / 2, &poster)),
    };
    let report = render::render_clip(asset, &spec, &mp4, raster, job.progress, job.cancel)?;

    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (vn, pn, sn, tn) = (name(&mp4), name(&poster), name(&srt), name(&vtt));
    let side = Sidecar {
        title: &cand.title,
        description: &cand.description,
        hashtags: &cand.hashtags,
        score: cand.score,
        reasons: &cand.reasons,
        source: &asset.path,
        source_start_ms: cand.start_ms,
        source_end_ms: cand.end_ms,
        duration_ms: report.duration_ms,
        preset: &opts.preset,
        size,
        caption_style: opts.style.as_ref().map(|s| s.name.as_str()),
        picker: &cand.provenance,
        transcript_engine: &transcript.engine,
        files: Files { video: &vn, poster: &pn, srt: &sn, vtt: &tn },
    };
    std::fs::write(&json, serde_json::to_string_pretty(&side)?)?;
    Ok(ExportedClip { rank, candidate: cand.clone(), mp4, poster, srt, vtt, json, render: report, reframe })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_safe_and_bounded() {
        assert_eq!(slug("That's not leadership — that's hypocrisy!"), "that-s-not-leadership-that-s-hypocrisy");
        assert_eq!(slug("🎯🎯"), "clip");
        assert!(slug(&"a".repeat(100)).len() <= 40);
        assert_eq!(slug("  --Hello  World-- "), "hello-world");
    }

    #[test]
    fn presets_resolve() {
        assert_eq!(preset_size("reels"), Some((1080, 1920)));
        assert_eq!(preset_size("square"), Some((1080, 1080)));
        assert_eq!(preset_size("nope"), None);
    }
}

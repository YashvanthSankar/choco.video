//! Render real clips from every fixture format and check what came out. Snapshot tests compare
//! composed frames (before encoding, so deterministic) against stored luma fingerprints.
//! Regenerate fingerprints with CHOCO_UPDATE_SNAPSHOTS=1 and say why in the commit.

use choco_engine::captions::{self, Rasterizer};
use choco_engine::render::{self, RenderSpec};
use choco_engine::{Layout, Segment, Transcript, Word, ingest};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

fn media(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/media").join(name);
    assert!(p.is_file(), "missing fixture {} — run engine/fixtures/generate.sh", p.display());
    p
}

fn words_transcript() -> Transcript {
    let text = ["Every", "creator", "wants", "clips", "that", "land."];
    let words: Vec<Word> = text
        .iter()
        .enumerate()
        .map(|(i, t)| Word { start_ms: 500 + i as u64 * 300, end_ms: 780 + i as u64 * 300, text: t.to_string(), confidence: 1.0 })
        .collect();
    Transcript { language: "en".into(), engine: "test".into(), segments: vec![Segment { start_ms: 500, end_ms: 2300, text: text.join(" "), words }] }
}

fn probe_out(p: &std::path::Path) -> choco_engine::MediaAsset {
    ingest::probe(p).unwrap_or_else(|e| panic!("output does not probe: {e}"))
}

fn render(name: &str, start: u64, end: u64, layout: Layout, with_captions: bool) -> (choco_engine::MediaAsset, render::RenderReport) {
    let asset = ingest::probe(&media(name)).unwrap();
    let v = asset.video.as_ref().unwrap();
    let mut plan = render::center_crop_plan((v.display_width, v.display_height), (1080, 1920), start);
    plan.layout = layout;
    let mut raster = Rasterizer::new();
    let t = words_transcript();
    let track = with_captions.then(|| captions::chunk(&t, (start, end), &captions::preset("bold").unwrap(), (1080, 1920), &mut raster, &Default::default()));
    let spec = RenderSpec {
        start_ms: start,
        end_ms: end,
        size: (1080, 1920),
        crop: &plan,
        captions: track.as_ref(),
        normalize_audio: true,
        max_fps: 60,
        video_bitrate: 6_000_000,
        poster: None,
    };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.mp4");
    let report = render::render_clip(&asset, &spec, &out, &mut raster, &|_| {}, &AtomicBool::new(false))
        .unwrap_or_else(|e| panic!("{name}: render failed: {e}"));
    (probe_out(&out), report)
}

fn assert_vertical(out: &choco_engine::MediaAsset, expect_ms: u64, audio: bool) {
    let v = out.video.as_ref().expect("video stream");
    assert_eq!((v.width, v.height), (1080, 1920));
    assert!(v.codec.contains("h264"), "{}", v.codec);
    assert!(out.duration_ms.abs_diff(expect_ms) <= 100, "duration {} vs {expect_ms}", out.duration_ms);
    assert_eq!(out.audio.is_some(), audio, "audio stream presence");
    if let Some(a) = &out.audio {
        assert_eq!((a.sample_rate, a.channels), (48_000, 2));
    }
}

#[test]
fn h264_with_captions() {
    let (out, r) = render("speech.mp4", 1_000, 4_000, Layout::Static, true);
    assert_vertical(&out, 3_000, true);
    assert_eq!(r.frames, 90);
}

#[test]
fn rotated_phone_clip_renders_upright() {
    let (out, r) = render("rotated.mp4", 1_000, 3_000, Layout::Static, false);
    assert_vertical(&out, 2_000, true);
    assert_eq!(r.frames, 60, "2 s at 30 fps is exactly 60 frames");
}

#[test]
fn vp9_opus_source() {
    let (out, _) = render("vp9-opus.mkv", 500, 3_500, Layout::Static, false);
    assert_vertical(&out, 3_000, true);
}

#[test]
fn av1_opus_source() {
    let (out, _) = render("av1-opus.webm", 500, 3_500, Layout::Static, true);
    assert_vertical(&out, 3_000, true);
}

#[test]
fn source_without_audio() {
    let (out, r) = render("silent.mp4", 0, 2_000, Layout::Static, false);
    assert_vertical(&out, 2_000, false);
    assert!(!r.has_audio);
}

#[test]
fn fit_layout() {
    let (out, _) = render("speech.mp4", 0, 2_000, Layout::Fit, false);
    assert_vertical(&out, 2_000, true);
}

#[test]
fn clip_past_the_end_holds_the_last_frame() {
    let (out, r) = render("silent.mp4", 3_000, 6_000, Layout::Static, false);
    // Source is 5 s long: render still yields the requested length.
    assert_eq!(r.frames, 75);
    assert!(out.duration_ms >= 2_900);
}

fn snapshot(name: &str, file: &str, t_ms: u64, layout: Layout, with_captions: bool) {
    let asset = ingest::probe(&media(file)).unwrap();
    let v = asset.video.as_ref().unwrap();
    let mut plan = render::center_crop_plan((v.display_width, v.display_height), (1080, 1920), 0);
    plan.layout = layout;
    let mut raster = Rasterizer::new();
    let t = words_transcript();
    let track = with_captions.then(|| captions::chunk(&t, (0, 10_000), &captions::preset("bold").unwrap(), (1080, 1920), &mut raster, &Default::default()));
    let spec = RenderSpec { start_ms: 0, end_ms: 10_000, size: (1080, 1920), crop: &plan, captions: track.as_ref(), normalize_audio: false, max_fps: 60, video_bitrate: 1, poster: None };
    let f = render::frame_at(&asset, &spec, t_ms, &mut raster).unwrap();
    let fp = render::luma_fingerprint(&f, 18, 32);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots").join(format!("{name}.json"));
    if std::env::var_os("CHOCO_UPDATE_SNAPSHOTS").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(&fp).unwrap()).unwrap();
        if std::env::var_os("CHOCO_UPDATE_SNAPSHOTS").is_none() {
            panic!("created missing snapshot {}; review it and rerun", path.display());
        }
        return;
    }
    let want: Vec<u8> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mad = fp.iter().zip(&want).map(|(a, b)| a.abs_diff(*b) as u32).sum::<u32>() as f32 / fp.len() as f32;
    assert!(mad <= 2.0, "{name}: mean luma difference {mad:.2} vs snapshot");
}

#[test]
fn snapshot_static_with_captions() {
    snapshot("static_captions_1200", "speech.mp4", 1_200, Layout::Static, true);
}

#[test]
fn snapshot_rotated_static() {
    snapshot("rotated_static_1000", "rotated.mp4", 1_000, Layout::Static, false);
}

#[test]
fn snapshot_fit() {
    snapshot("fit_1000", "speech.mp4", 1_000, Layout::Fit, false);
}

//! Every container and codec we claim to support: probe, decode audio to the speech WAV, and
//! decode a video frame. Run `fixtures/generate.sh` first; missing fixtures fail loudly so a
//! skipped check never looks like a pass.

use choco_engine::ingest;
use std::path::PathBuf;

fn media(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/media").join(name);
    assert!(p.is_file(), "missing fixture {} — run engine/fixtures/generate.sh", p.display());
    p
}

fn check(name: &str, expect_video: bool, expect_audio: bool, rotation: u16) {
    let a = ingest::probe(&media(name)).unwrap_or_else(|e| panic!("{name}: probe failed: {e}"));
    assert!(a.duration_ms > 1_000, "{name}: duration {}", a.duration_ms);
    assert_eq!(a.video.is_some(), expect_video, "{name}: video stream");
    assert_eq!(a.audio.is_some(), expect_audio, "{name}: audio stream");
    let dir = tempfile::tempdir().unwrap();
    if expect_audio {
        let wav = dir.path().join("a.wav");
        let n = ingest::extract_audio(&a, &wav, &|_| {}).unwrap_or_else(|e| panic!("{name}: audio decode failed: {e}"));
        let expected = a.duration_ms * ingest::SPEECH_SAMPLE_RATE as u64 / 1000;
        let diff = n.abs_diff(expected);
        assert!(diff < expected / 20 + 1_600, "{name}: {n} samples, expected about {expected}");
        let samples = ingest::read_wav_f32(&wav).unwrap();
        assert!(samples.iter().any(|s| s.abs() > 0.01), "{name}: decoded audio is silent");
    }
    if let Some(v) = &a.video {
        assert_eq!(v.rotation_deg, rotation, "{name}: rotation");
        let png = dir.path().join("t.png");
        ingest::thumbnail(&a, 1_000, 320, &png).unwrap_or_else(|e| panic!("{name}: video decode failed: {e}"));
        assert!(std::fs::metadata(&png).unwrap().len() > 1_000, "{name}: thumbnail too small");
    }
}

#[test]
fn h264_aac_mp4() {
    check("speech.mp4", true, true, 0);
}

#[test]
fn rotated_phone_clip() {
    check("rotated.mp4", true, true, 270);
}

#[test]
fn video_without_audio() {
    check("silent.mp4", true, false, 0);
}

#[test]
fn audio_only_m4a() {
    check("audio-only.m4a", false, true, 0);
}

#[test]
fn vp9_opus_mkv() {
    check("vp9-opus.mkv", true, true, 0);
}

#[test]
fn av1_opus_webm() {
    check("av1-opus.webm", true, true, 0);
}

#[test]
fn float_wav() {
    check("aligned.wav", false, true, 0);
}

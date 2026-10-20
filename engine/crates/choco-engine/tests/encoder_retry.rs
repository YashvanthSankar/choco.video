//! An encoder that opens and then fails mid-clip: the clip is rendered once more from the next
//! candidate, and when none is left the error names what each encoder did. Its own test binary,
//! because the failure is switched on through the process environment.

use choco_engine::captions::Rasterizer;
use choco_engine::ingest;
use choco_engine::render::{self, RenderError, RenderSpec};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

#[test]
fn mid_clip_encoder_failure_retries_with_the_next_encoder() {
    // SAFETY: the only test in this binary; nothing else reads the environment concurrently.
    unsafe { std::env::set_var("CHOCO_TEST_FAIL_FIRST_ENCODER", "1") };
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/media/speech.mp4");
    assert!(src.is_file(), "run engine/fixtures/generate.sh");
    let asset = ingest::probe(&src).unwrap();
    let v = asset.video.as_ref().unwrap();
    let plan = render::center_crop_plan((v.display_width, v.display_height), (1080, 1920), 1_000);
    let spec = RenderSpec {
        start_ms: 1_000,
        end_ms: 2_000,
        size: (1080, 1920),
        crop: &plan,
        captions: None,
        normalize_audio: true,
        max_fps: 60,
        video_bitrate: 6_000_000,
        poster: None,
    };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.mp4");
    let messages = Mutex::new(Vec::new());
    let progress = |p: choco_engine::Progress| messages.lock().unwrap().push(p.message);
    let result = render::render_clip(
        &asset,
        &spec,
        &out,
        &mut Rasterizer::new(),
        &progress,
        &AtomicBool::new(false),
    );
    assert!(
        messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("rendering again with the next encoder")),
        "the retry is reported"
    );
    // Which way it ends depends on whether another encoder after the failed one opens here
    // (a GPU machine: hardware failed, software finishes) or none does (macOS, or the GPU-less
    // CI runner). Both must be handled cleanly.
    match result {
        Ok(report) => {
            assert_eq!(report.frames, 30);
            assert_ne!(report.video_encoder, "", "names the encoder that finished");
            assert!(ingest::probe(&out).unwrap().video.is_some());
        }
        Err(err) => {
            assert!(matches!(err, RenderError::Unsupported(_)), "{err}");
            let msg = err.to_string();
            assert!(msg.contains("failed mid-clip"), "{msg}");
            assert!(
                msg.contains("no H.264 encoder opened") || msg.contains("failed mid-clip too"),
                "{msg}"
            );
        }
    }
}

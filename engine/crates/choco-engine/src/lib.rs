//! choco-engine: turn long video into ranked, captioned, speaker-centered vertical clips.
//!
//! Every stage is a pure function over the serde types in [`model`]. Platform differences
//! live only in [`backend`]. See `docs/TECH.md` in the main repository.

pub mod backend;
pub mod captions;
pub mod eval;
pub mod export;
pub mod highlights;
pub mod ingest;
pub mod model;
pub mod models_registry;
pub mod pipeline;
pub mod render;
pub mod transcribe;

pub use model::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Progress sink passed to every long-running stage.
pub type ProgressFn<'a> = &'a (dyn Fn(Progress) + Sync);

/// Initialise FFmpeg once and silence its own logging. Safe to call repeatedly.
pub fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = ffmpeg_next::init();
        ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);
    });
}

/// Version and license of the FFmpeg libraries actually loaded at runtime.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaLibraryInfo {
    pub avcodec_version: String,
    pub license: String,
    pub is_lgpl: bool,
}

pub fn media_library_info() -> MediaLibraryInfo {
    init();
    let v = ffmpeg_next::util::version();
    // SAFETY: avutil_license returns a static NUL-terminated string.
    let license = unsafe { std::ffi::CStr::from_ptr(ffmpeg_next::ffi::avutil_license()) }.to_string_lossy().into_owned();
    let c = ffmpeg_next::codec::version();
    MediaLibraryInfo {
        avcodec_version: format!("{}.{}.{} (avutil {}.{}.{})", c >> 16, (c >> 8) & 0xff, c & 0xff, v >> 16, (v >> 8) & 0xff, v & 0xff),
        is_lgpl: license.starts_with("LGPL"),
        license,
    }
}

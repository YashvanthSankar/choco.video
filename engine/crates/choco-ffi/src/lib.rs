//! C ABI over choco-engine. Strings in and out are UTF-8 JSON. Free returned strings with
//! [`choco_string_free`]. The full surface (project open, run stage, progress, cancel) lands with
//! the pipeline module; M1 exposes version, hardware detection and probe.

use std::ffi::{CStr, CString, c_char};

fn to_c(s: String) -> *mut c_char {
    CString::new(s)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

fn err_json(msg: impl std::fmt::Display) -> *mut c_char {
    to_c(serde_json::json!({ "error": msg.to_string() }).to_string())
}

/// Engine version as a static NUL-terminated string. Do not free.
#[unsafe(no_mangle)]
pub extern "C" fn choco_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Hardware and backend report as JSON.
#[unsafe(no_mangle)]
pub extern "C" fn choco_doctor_json() -> *mut c_char {
    let b = choco_engine::backend::detect();
    to_c(
        serde_json::json!({
            "info": b.info(),
            "speech_engines": b.speech_engines(),
            "hw_encoders": b.hw_encoders(),
            "recommended_models": b.recommended_models(),
        })
        .to_string(),
    )
}

/// Probe a media file. Returns `MediaAsset` JSON or `{"error": ...}`.
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn choco_probe_json(path: *const c_char) -> *mut c_char {
    if path.is_null() {
        return err_json("null path");
    }
    // SAFETY: caller guarantees a valid C string.
    let p = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(e) => return err_json(e),
    };
    match choco_engine::ingest::probe(std::path::Path::new(p)) {
        Ok(a) => to_c(serde_json::to_string(&a).unwrap_or_default()),
        Err(e) => err_json(e),
    }
}

/// Free a string returned by this library.
///
/// # Safety
/// `s` must come from this library and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn choco_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: produced by CString::into_raw in this crate.
        drop(unsafe { CString::from_raw(s) });
    }
}

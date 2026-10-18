use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CHOCO_FFMPEG_LIB_DIR");
    // Tests and examples of this crate need to find the FFmpeg dylibs at runtime.
    if let Ok(dir) = env::var("CHOCO_FFMPEG_LIB_DIR") {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" && env::var_os("CARGO_FEATURE_APPLE_SPEECH").is_some() {
        build_apple_helper();
    }
}

/// Compile the Swift helper that wraps SpeechAnalyzer. Runs as a child process so the
/// engine stays pure Rust and a crash in the OS speech stack cannot take the app down.
fn build_apple_helper() {
    let src = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../apple/AppleHelper.swift");
    println!("cargo::rerun-if-changed={}", src.display());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("choco-apple");
    let status = Command::new("xcrun")
        .args(["swiftc", "-O", "-parse-as-library", "-target", "arm64-apple-macos14.0", "-o"])
        .arg(&out)
        .arg(&src)
        .status();
    match status {
        Ok(s) if s.success() => {
            println!("cargo::rustc-env=CHOCO_APPLE_HELPER_BUILT={}", out.display());
        }
        other => {
            println!("cargo::warning=Apple helper not built ({other:?}); whisper will be used");
            println!("cargo::rustc-env=CHOCO_APPLE_HELPER_BUILT=");
        }
    }
}

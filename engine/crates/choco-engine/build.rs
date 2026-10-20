use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CHOCO_FFMPEG_LIB_DIR");
    // Tests and examples of this crate need to find the FFmpeg dylibs at runtime. `-rpath` is a
    // Unix linker flag; on Windows the DLLs sit beside the executable instead.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        && let Ok(dir) = env::var("CHOCO_FFMPEG_LIB_DIR")
    {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" && env::var_os("CARGO_FEATURE_APPLE_SPEECH").is_some() {
        build_apple_helper();
    }
    if target_os == "macos" {
        link_clang_runtime();
    }
}

/// whisper.cpp's Metal backend uses `@available` checks. When built for an older minimum macOS
/// (the app targets 14.0) these compile to `__isPlatformVersionAtLeast`, which lives in clang's
/// compiler runtime; rustc's link line does not include it. Linking it as a static library from
/// here propagates to every binary that uses the engine.
fn link_clang_runtime() {
    let Ok(out) = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
    else {
        println!(
            "cargo::warning=could not locate clang's resource dir; builds for older macOS targets may fail to link"
        );
        return;
    };
    let dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()).join("lib/darwin");
    if dir.join("libclang_rt.osx.a").is_file() {
        println!("cargo::rustc-link-search=native={}", dir.display());
        println!("cargo::rustc-link-lib=static=clang_rt.osx");
    } else {
        println!(
            "cargo::warning=libclang_rt.osx.a not found in {}",
            dir.display()
        );
    }
}

/// Compile the Swift helper that wraps SpeechAnalyzer. Runs as a child process so the
/// engine stays pure Rust and a crash in the OS speech stack cannot take the app down.
fn build_apple_helper() {
    let src =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../apple/AppleHelper.swift");
    println!("cargo::rerun-if-changed={}", src.display());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("choco-apple");
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-O",
            "-parse-as-library",
            "-target",
            "arm64-apple-macos14.0",
            "-o",
        ])
        .arg(&out)
        .arg(&src)
        .status();
    match status {
        Ok(s) if s.success() => {
            println!(
                "cargo::rustc-env=CHOCO_APPLE_HELPER_BUILT={}",
                out.display()
            );
        }
        other => {
            println!("cargo::warning=Apple helper not built ({other:?}); whisper will be used");
            println!("cargo::rustc-env=CHOCO_APPLE_HELPER_BUILT=");
        }
    }
}

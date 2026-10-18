fn main() {
    println!("cargo::rerun-if-env-changed=CHOCO_FFMPEG_LIB_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // Bundled layout: Contents/MacOS/choco next to Contents/Frameworks/*.dylib.
        println!("cargo::rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        println!("cargo::rustc-link-arg=-Wl,-rpath,@executable_path");
    }
    // Development: find the vendored FFmpeg build.
    if let Ok(dir) = std::env::var("CHOCO_FFMPEG_LIB_DIR") {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
    }
}

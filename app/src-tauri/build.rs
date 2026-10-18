fn main() {
    println!("cargo::rerun-if-env-changed=CHOCO_FFMPEG_LIB_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // Bundled: Contents/MacOS/choco-app next to Contents/Frameworks/*.dylib.
        println!("cargo::rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    }
    // Development only (`tauri dev`): the vendored FFmpeg build. Release binaries must not carry
    // a path from the build machine; they load FFmpeg from Contents/Frameworks.
    if std::env::var("PROFILE").as_deref() == Ok("debug")
        && let Ok(dir) = std::env::var("CHOCO_FFMPEG_LIB_DIR")
    {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{dir}");
    }
    tauri_build::build()
}

# FFmpeg build for choco.video

The engine links FFmpeg dynamically as an **LGPL-2.1** shared library. Never add `--enable-gpl`, `--enable-nonfree`, libx264, libx265 or openh264.

## macOS arm64

```
./scripts/ffmpeg/build-mac.sh
export PKG_CONFIG_PATH="$PWD/vendor/ffmpeg/8.1.3-macos-arm64/lib/pkgconfig"
```

Output lands in `vendor/ffmpeg/<version>-macos-arm64/` (gitignored). The script verifies the tarball against `VERSIONS`, refuses to finish if `config.h` reports GPL or nonfree, and installs dylibs with `@rpath` install names so the app bundle can carry them in `Contents/Frameworks`.

`--disable-everything` plus an allowlist. To add a codec, filter or format, add it to the arrays in `build-mac.sh`, rebuild, and record the new size below. `--disable-autodetect` means no zlib, so the PNG encoder is off; thumbnails and posters are encoded by the pure-Rust `png` crate instead.

## Measured

| Date | Version | Target | Total dylib size | License |
|---|---|---|---|---|
| 2026-10-18 | 8.1.3 | macos-arm64 | 7.5 MB (avcodec 4.5, swscale 1.1, avformat 0.8, avutil 0.7, avfilter 0.3, swresample 0.1) | LGPL 2.1 or later |

## Windows

Lands at M6: same allowlist, `--enable-d3d11va --enable-mediafoundation --enable-nvenc --enable-amf --enable-libvpl` (all LGPL-compatible headers), built in CI with MSYS2 and cached.

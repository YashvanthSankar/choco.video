# FFmpeg build for choco.video

The engine links FFmpeg dynamically as an **LGPL-2.1** shared library. Never add `--enable-gpl`, `--enable-nonfree`, libx264, libx265 or openh264.

## macOS arm64

```
./scripts/ffmpeg/build-mac.sh
export PKG_CONFIG_PATH="$PWD/vendor/ffmpeg/8.1.3-macos-arm64/lib/pkgconfig"
```

Output lands in `vendor/ffmpeg/<version>-macos-arm64/` (gitignored). The script verifies the tarball against `VERSIONS`, refuses to finish if `config.h` reports GPL or nonfree, and installs dylibs with `@rpath` install names so the app bundle can carry them in `Contents/Frameworks`.

`--disable-everything` plus an allowlist. AV1 decodes in software with dav1d (built static from a VideoLAN-checksummed tarball; needs `meson` and `ninja`, override with `MESON=`) and in hardware through VideoToolbox on chips that have it. To add a codec, filter or format, add it to the arrays in `build-mac.sh`, rebuild, and record the new size below. `--disable-autodetect` means no zlib, so the PNG encoder is off; thumbnails and posters are encoded by the pure-Rust `png` crate instead.

## Measured

| Date | Version | Target | Total dylib size | License |
|---|---|---|---|---|
| 2026-10-18 | 8.1.3 | macos-arm64 | 7.5 MB (avcodec 4.5, swscale 1.1, avformat 0.8, avutil 0.7, avfilter 0.3, swresample 0.1) | LGPL 2.1 or later |
| 2026-10-18 | 8.1.3 + dav1d 1.5.4 | macos-arm64 | 8.3 MB (avcodec 5.6 with dav1d static) | LGPL 2.1 or later; dav1d BSD-2 |

## Windows (M6, not before)

Until the minimal build exists, CI can link this pinned general LGPL build (166 MB of DLLs, avcodec alone 91 MB, which is why we build our own):

- URL: `https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-17-20-51/ffmpeg-n8.1.3-18-g46d899d7a0-win64-lgpl-shared-8.1.zip`
- sha256: `a5b362ae7ddacb2d97974cb8dc1fa84339a77d996c274cf3fad32605f40288be`
- Set `FFMPEG_DIR` to the extracted folder and add its `bin/` to `PATH`; set `LIBCLANG_PATH` for bindgen.

The minimal build lands at M6: same allowlist, `--enable-d3d11va --enable-mediafoundation --enable-nvenc --enable-amf --enable-libvpl` (all LGPL-compatible headers), built in CI with MSYS2 and cached.

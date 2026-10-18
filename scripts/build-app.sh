#!/usr/bin/env bash
# Build the macOS app bundle and DMG.
#   scripts/build-app.sh
# Output: app/src-tauri/target/release/bundle/{macos/choco.video.app, dmg/*.dmg}
# Signing: set APPLE_SIGNING_IDENTITY (and notarization variables) for a distributable build;
# without them the bundle is ad-hoc signed and runs on this Mac only.
set -euo pipefail
# Homebrew installs rustup keg-only; make cargo reachable either way.
command -v cargo >/dev/null || export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null || { echo "cargo not found; install Rust (brew install rustup)" >&2; exit 1; }
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FF="$ROOT/vendor/ffmpeg/8.1.3-macos-arm64/lib"
TAURI="$ROOT/app/src-tauri"
[[ -d "$FF" ]] || "$ROOT/scripts/ffmpeg/build-mac.sh"

echo "== Apple helper (sidecar)"
mkdir -p "$TAURI/binaries"
xcrun swiftc -O -parse-as-library -target arm64-apple-macos14.0 \
  -o "$TAURI/binaries/choco-apple-aarch64-apple-darwin" "$ROOT/engine/crates/apple/AppleHelper.swift"

echo "== FFmpeg frameworks"
mkdir -p "$TAURI/frameworks"
rm -f "$TAURI/frameworks/"*.dylib
for lib in avcodec.62 avformat.62 avfilter.11 avutil.60 swresample.6 swscale.9; do
  cp -L "$FF/lib$lib.dylib" "$TAURI/frameworks/lib$lib.dylib"
done

cat > "$TAURI/tauri.release.conf.json" <<JSON
{
  "bundle": {
    "externalBin": ["binaries/choco-apple"],
    "macOS": {
      "minimumSystemVersion": "14.0",
      "frameworks": [
        "frameworks/libavcodec.62.dylib", "frameworks/libavformat.62.dylib", "frameworks/libavfilter.11.dylib",
        "frameworks/libavutil.60.dylib", "frameworks/libswresample.6.dylib", "frameworks/libswscale.9.dylib"
      ]
    }
  }
}
JSON

echo "== tauri build"
cd "$ROOT/app"
pnpm install --frozen-lockfile >/dev/null
pnpm tauri build --config src-tauri/tauri.release.conf.json

APP="$TAURI/target/release/bundle/macos/choco.video.app"
DMG="$(ls "$TAURI"/target/release/bundle/dmg/*.dmg | head -1)"
echo "== verify"
otool -L "$APP/Contents/MacOS/choco-app" | grep -E "libav|libsw" | sed 's/^/  /'
if otool -l "$APP/Contents/MacOS/choco-app" | grep -A2 LC_RPATH | grep -q "$ROOT"; then
  echo "release binary carries a build-machine rpath" >&2; exit 1
fi
ls "$APP/Contents/Frameworks" "$APP/Contents/MacOS"
echo "app: $(du -sh "$APP" | cut -f1)   dmg: $(du -sh "$DMG" | cut -f1)"

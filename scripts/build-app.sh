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

# Hardened runtime enforces library validation: every loaded dylib must carry the app's Team ID.
# An ad-hoc signature has no Team ID, so the bundled FFmpeg dylibs would be refused at launch
# ("different Team IDs"). Enable it only for Developer ID builds, where notarization requires it.
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then HARDENED=true; else HARDENED=false; fi
cat > "$TAURI/tauri.release.conf.json" <<JSON
{
  "bundle": {
    "externalBin": ["binaries/choco-apple"],
    "macOS": {
      "minimumSystemVersion": "14.0",
      "signingIdentity": "${APPLE_SIGNING_IDENTITY:--}",
      "hardenedRuntime": ${HARDENED},
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
# The whole bundle must be sealed (ad-hoc when no Developer ID): an unsealed bundle is reported
# as "damaged" by Gatekeeper once downloaded, instead of offering right-click > Open.
codesign --verify --deep --strict "$APP" || { echo "bundle signature does not verify" >&2; exit 1; }
# Launch smoke test: the app must load its frameworks and stay up. A signature can verify and the
# app still die in dyld, which is exactly how 0.1.0 shipped broken.
"$APP/Contents/MacOS/choco-app" >/tmp/choco-smoke.log 2>&1 & PID=$!
sleep 6
if ! kill -0 $PID 2>/dev/null; then echo "app exited at launch:" >&2; head -5 /tmp/choco-smoke.log >&2; exit 1; fi
kill $PID 2>/dev/null; wait $PID 2>/dev/null || true
echo "launch smoke test passed"
if otool -l "$APP/Contents/MacOS/choco-app" | grep -A2 LC_RPATH | grep -q "$ROOT"; then
  echo "release binary carries a build-machine rpath" >&2; exit 1
fi
ls "$APP/Contents/Frameworks" "$APP/Contents/MacOS"
echo "app: $(du -sh "$APP" | cut -f1)   dmg: $(du -sh "$DMG" | cut -f1)"

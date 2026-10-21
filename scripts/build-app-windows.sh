#!/usr/bin/env bash
# Build the Windows installer (NSIS, per-user) and check it.  Runs in Git Bash on Windows.
#   scripts/build-app-windows.sh
# Output: app/src-tauri/target/release/bundle/nsis/choco.video_<ver>_x64-setup.exe
# Not signed yet (docs/TODO.md M6, signing): SmartScreen warns on first run.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI="$ROOT/app/src-tauri"
FFBIN="$ROOT/vendor/ffmpeg/8.1.3-windows-x64/bin"
export FFMPEG_DIR="${FFMPEG_DIR:-$(cygpath -w "$ROOT/vendor/ffmpeg/8.1.3-windows-x64")}"
"$ROOT/scripts/ffmpeg/fetch-windows.sh"

echo "== FFmpeg DLLs (installed beside the exe)"
mkdir -p "$TAURI/windows/ffmpeg"
rm -f "$TAURI/windows/ffmpeg/"*.dll
cp "$FFBIN"/*.dll "$TAURI/windows/ffmpeg/"
ls "$TAURI/windows/ffmpeg"

echo "== tauri build (tauri.windows.conf.json is merged in by Tauri)"
cd "$ROOT/app"
pnpm install --frozen-lockfile >/dev/null
pnpm tauri build

EXE="$TAURI/target/release/choco-app.exe"
SETUP="$(ls "$TAURI"/target/release/bundle/nsis/*-setup.exe | head -1)"

echo "== imports"
"$ROOT/scripts/check-imports.sh" "$TAURI/windows/ffmpeg" "$EXE"

echo "== size"
size() { stat -c%s "$1"; }
installed=$(( $(size "$EXE") + $(cat "$TAURI"/windows/ffmpeg/*.dll | wc -c) ))
installer=$(size "$SETUP")
mb() { awk -v b="$1" 'BEGIN { printf "%.1f MB", b / 1000000 }'; }
echo "installer $(mb "$installer") (budget 25 MB) · installed $(mb "$installed") without models (budget 50 MB)"
(( installer <= 25000000 )) || { echo "installer over budget" >&2; exit 1; }
(( installed <= 50000000 )) || { echo "installed size over budget" >&2; exit 1; }

echo "== install, launch, uninstall"
# Git Bash rewrites arguments that start with "/" into Windows paths, so "/S" would reach NSIS as
# a path and the installer would open its window and wait for a click (CI hung for 6 h).
export MSYS_NO_PATHCONV=1
DEST="$LOCALAPPDATA\\choco.video"
timeout 300 "$SETUP" /S || { echo "silent install failed or hung" >&2; exit 1; }
APP="$(cygpath -u "$DEST")/choco-app.exe"
[[ -f "$APP" ]] || { echo "installer did not put choco-app.exe in $DEST" >&2; exit 1; }
ls "$(dirname "$APP")"
"$APP" > "$TAURI/target/smoke.log" 2>&1 & PID=$!
sleep 8
if ! kill -0 $PID 2>/dev/null; then echo "app exited at launch:" >&2; head -20 "$TAURI/target/smoke.log" >&2; exit 1; fi
taskkill /F /IM choco-app.exe >/dev/null 2>&1 || kill $PID || true
echo "launch smoke test passed"
timeout 300 "$(cygpath -u "$DEST")/uninstall.exe" /S || { echo "silent uninstall failed or hung" >&2; exit 1; }
sleep 3
[[ ! -f "$APP" ]] || { echo "uninstall left choco-app.exe behind" >&2; exit 1; }
echo "uninstall removed the app"
echo "setup: $SETUP"

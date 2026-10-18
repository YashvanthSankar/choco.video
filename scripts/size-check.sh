#!/usr/bin/env bash
# Size budget check (docs/STACK.md). Until the app exists (M5), this measures what the app will
# ship from the engine: the CLI binary, the Apple speech helper and the FFmpeg dylibs, plus a
# compressed estimate standing in for the installer. Prints a markdown table; exits 1 over budget.
# Usage: scripts/size-check.sh [engine/target/release]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REL="${1:-$ROOT/engine/target/release}"
FF="$ROOT/vendor/ffmpeg/8.1.3-macos-arm64/lib"
INSTALLED_BUDGET=$((50 * 1000 * 1000))
INSTALLER_BUDGET=$((25 * 1000 * 1000))

T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
mkdir -p "$T/stage"
rows=()
add() { # label path
  [[ -f "$2" ]] || return 0
  cp -L "$2" "$T/stage/"
  rows+=("| $1 | $(stat -f%z "$2" 2>/dev/null || stat -c%s "$2") |")
}
add "choco CLI" "$REL/choco"
add "choco CLI" "$REL/choco.exe"
helper="$(ls "$REL"/build/choco-engine-*/out/choco-apple-speech 2>/dev/null | head -1 || true)"
[[ -n "$helper" ]] && add "Apple speech helper" "$helper"
if [[ -d "$FF" ]]; then
  for f in "$FF"/lib*.[0-9]*.[0-9]*.[0-9]*.dylib; do add "$(basename "$f")" "$f"; done
fi
installed=$(du -sk "$T/stage" | awk '{print $1 * 1024}')
( cd "$T" && zip -qr -9 est.zip stage )
installer=$(stat -f%z "$T/est.zip" 2>/dev/null || stat -c%s "$T/est.zip")

mb() { awk -v b="$1" 'BEGIN { printf "%.1f MB", b / 1000000 }'; }
echo "### Size budget"
echo
echo "| Component | Bytes |"
echo "|---|---|"
printf '%s\n' "${rows[@]}"
echo
echo "| Measure | Size | Budget |"
echo "|---|---|---|"
echo "| Installed (engine part, no models) | $(mb "$installed") | $(mb $INSTALLED_BUDGET) |"
echo "| Installer estimate (zip -9) | $(mb "$installer") | $(mb $INSTALLER_BUDGET) |"
status=0
(( installed > INSTALLED_BUDGET )) && { echo; echo "**Over installed budget.**"; status=1; }
(( installer > INSTALLER_BUDGET )) && { echo; echo "**Over installer budget.**"; status=1; }
exit $status

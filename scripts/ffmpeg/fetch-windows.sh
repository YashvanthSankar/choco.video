#!/usr/bin/env bash
# Fetch the pinned Windows FFmpeg into vendor/ffmpeg/8.1.3-windows-x64 and verify its sha256.
#   scripts/ffmpeg/fetch-windows.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="$ROOT/vendor/ffmpeg/8.1.3-windows-x64"
read -r URL SHA < <(grep -v '^#' "$ROOT/scripts/ffmpeg/VERSIONS-windows" | head -1)
# The stamp names the archive DEST was unpacked from, so a changed pin replaces the old build.
[[ -f "$DEST/lib/avcodec.lib" && "$(cat "$DEST/.sha256" 2>/dev/null)" == "$SHA" ]] && { echo "already present: $DEST"; exit 0; }
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
curl -fsSL -o "$TMP/ff.zip" "$URL"
GOT="$( (sha256sum "$TMP/ff.zip" 2>/dev/null || shasum -a 256 "$TMP/ff.zip") | cut -d' ' -f1)"
[[ "$GOT" == "$SHA" ]] || { echo "sha256 mismatch: got $GOT, want $SHA" >&2; exit 1; }
unzip -q "$TMP/ff.zip" -d "$TMP/x"
rm -rf "$DEST"; mkdir -p "$DEST"
cp -R "$TMP"/x/*/{bin,include,lib,LICENSE.txt} "$DEST"/
# FFmpeg's MSVC install puts the import libraries (avcodec.lib, ...) in bin/ beside the DLLs;
# ffmpeg-sys-next links from FFMPEG_DIR/lib.
for l in "$DEST"/bin/*.lib; do [[ -e "$l" ]] && cp -n "$l" "$DEST/lib/"; done
echo "$SHA" > "$DEST/.sha256"
echo "FFmpeg ready in $DEST"

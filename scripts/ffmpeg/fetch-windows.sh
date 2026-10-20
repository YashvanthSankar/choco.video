#!/usr/bin/env bash
# Fetch the pinned Windows FFmpeg into vendor/ffmpeg/8.1.3-windows-x64 and verify its sha256.
#   scripts/ffmpeg/fetch-windows.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEST="$ROOT/vendor/ffmpeg/8.1.3-windows-x64"
read -r URL SHA < <(grep -v '^#' "$ROOT/scripts/ffmpeg/VERSIONS-windows" | head -1)
[[ -f "$DEST/include/libavcodec/avcodec.h" && -f "$DEST/lib/avcodec.lib" ]] && { echo "already present: $DEST"; exit 0; }
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
curl -fsSL -o "$TMP/ff.zip" "$URL"
GOT="$( (sha256sum "$TMP/ff.zip" 2>/dev/null || shasum -a 256 "$TMP/ff.zip") | cut -d' ' -f1)"
[[ "$GOT" == "$SHA" ]] || { echo "sha256 mismatch: got $GOT, want $SHA" >&2; exit 1; }
unzip -q "$TMP/ff.zip" -d "$TMP/x"
mkdir -p "$DEST"
cp -R "$TMP"/x/*/{bin,include,lib,LICENSE.txt} "$DEST"/
# FFmpeg's MSVC install puts the import libraries (avcodec.lib, ...) in bin/ beside the DLLs;
# ffmpeg-sys-next links from FFMPEG_DIR/lib.
for l in "$DEST"/bin/*.lib; do [[ -e "$l" ]] && cp -n "$l" "$DEST/lib/"; done
echo "FFmpeg ready in $DEST"

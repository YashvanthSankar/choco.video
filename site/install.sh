#!/bin/sh
# Install the latest choco.video on an Apple Silicon Mac.
#   curl -fsSL https://chocovideo.vercel.app/install.sh | sh
#
# Why a script: choco.video is open source and not notarized by Apple, so a DMG downloaded in a
# browser is quarantined and macOS blocks its first launch. Files fetched with curl carry no
# quarantine flag. This script downloads the latest release from GitHub, verifies its SHA-256
# against the checksum published with it, and copies the app into Applications. Read it first:
# https://github.com/YashvanthSankar/choco.video/blob/main/scripts/install.sh
set -eu

REPO="YashvanthSankar/choco.video"
ASSET="choco.video-macos-arm64.dmg"
BASE="https://github.com/$REPO/releases/latest/download"

say() { printf '%s\n' "$*"; }
fail() { printf 'choco.video install: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "this installer is for macOS"
[ "$(uname -m)" = "arm64" ] || fail "choco.video needs an Apple Silicon Mac (M1 or later)"
major="$(sw_vers -productVersion | cut -d. -f1)"
[ "$major" -ge 14 ] || fail "choco.video needs macOS 14 or later (this Mac runs $(sw_vers -productVersion))"

tmp="$(mktemp -d)"
mnt=""
cleanup() { [ -n "$mnt" ] && hdiutil detach "$mnt" -quiet >/dev/null 2>&1 || true; rm -rf "$tmp"; }
trap cleanup EXIT INT TERM

say "Downloading the latest choco.video…"
curl -fL --progress-bar -o "$tmp/$ASSET" "$BASE/$ASSET" || fail "download failed"
curl -fsSL -o "$tmp/$ASSET.sha256" "$BASE/$ASSET.sha256" || fail "checksum download failed"
want="$(cut -d' ' -f1 "$tmp/$ASSET.sha256")"
got="$(shasum -a 256 "$tmp/$ASSET" | cut -d' ' -f1)"
[ "$want" = "$got" ] || fail "checksum mismatch (expected $want, got $got); nothing was installed"
say "Checksum verified."

mnt="$(hdiutil attach -nobrowse -readonly -noautoopen "$tmp/$ASSET" | awk -F'\t' '/\/Volumes\// {print $NF; exit}')"
[ -n "$mnt" ] && [ -d "$mnt/choco.video.app" ] || fail "could not open the disk image"

dest="/Applications"
[ -w "$dest" ] || dest="$HOME/Applications"
mkdir -p "$dest"
if pgrep -f "choco.video.app/Contents/MacOS/choco-app" >/dev/null 2>&1; then
  osascript -e 'quit app "choco.video"' >/dev/null 2>&1 || true
  sleep 1
fi
rm -rf "$dest/choco.video.app"
ditto "$mnt/choco.video.app" "$dest/choco.video.app"
xattr -dr com.apple.quarantine "$dest/choco.video.app" 2>/dev/null || true
version="$(defaults read "$dest/choco.video.app/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || echo "?")"
say "Installed choco.video $version in $dest."
open "$dest/choco.video.app"

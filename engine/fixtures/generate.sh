#!/usr/bin/env bash
# Synthetic fixtures with known content. Uses the macOS `say` voice and Homebrew ffmpeg.
# Dev-only: Homebrew ffmpeg is a GPL build and is never shipped or linked; it only writes test files.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p media
FF="${FFMPEG:-ffmpeg}"
command -v "$FF" >/dev/null || { echo "need ffmpeg on PATH (brew install ffmpeg)" >&2; exit 1; }
command -v say >/dev/null || { echo "need macOS say" >&2; exit 1; }

SCRIPT="The quick brown fox jumps over the lazy dog. Every creator wants clips that land. \
Today we will talk about three things: pricing, distribution, and the one mistake that cost us a year. \
First, pricing. Charge more than you think. Second, distribution. Post every single day. \
And the mistake? We built for agencies instead of solo creators. That was the whole problem."
printf '%s\n' "$SCRIPT" > media/speech.txt

T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
say -v Samantha -o "$T/a.aiff" "$SCRIPT"
D=$( ("$FF" -hide_banner -i "$T/a.aiff" 2>&1 || true) | sed -n 's/.*Duration: \([0-9:.]*\).*/\1/p')

# 1. Landscape talk with speech: the standard case.
# Cut every stream to the speech length explicitly: newer FFmpeg's -shortest leaves the video
# track ~1.7 s longer than the audio, so the container no longer matches the speech.
"$FF" -y -hide_banner -loglevel error -f lavfi -i "testsrc2=size=1920x1080:rate=30" -i "$T/a.aiff" \
  -t "$D" -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a aac -b:a 128k media/speech.mp4

# 2. Phone footage: coded landscape, displayed portrait via a 90-degree display matrix.
"$FF" -y -hide_banner -loglevel error -display_rotation 90 -i media/speech.mp4 -c copy media/rotated.mp4

# 3. Video with no audio track.
"$FF" -y -hide_banner -loglevel error -f lavfi -i "testsrc2=size=1280x720:rate=25" -t 5 \
  -c:v libx264 -pix_fmt yuv420p -preset veryfast media/silent.mp4

# 4. Two voices alternating, for diarization and split-screen work later.
say -v Samantha -o "$T/s1.aiff" "So tell me how you got started."
say -v Daniel   -o "$T/s2.aiff" "Honestly, by accident. I was editing clips for a friend's podcast."
say -v Samantha -o "$T/s3.aiff" "And when did it become a business?"
say -v Daniel   -o "$T/s4.aiff" "The day someone offered to pay me for it."
"$FF" -y -hide_banner -loglevel error -i "$T/s1.aiff" -i "$T/s2.aiff" -i "$T/s3.aiff" -i "$T/s4.aiff" \
  -filter_complex "[0][1][2][3]concat=n=4:v=0:a=1" "$T/two.wav"
D2=$( ("$FF" -hide_banner -i "$T/two.wav" 2>&1 || true) | sed -n 's/.*Duration: \([0-9:.]*\).*/\1/p')
"$FF" -y -hide_banner -loglevel error -f lavfi -i "smptebars=size=1920x1080:rate=30" -i "$T/two.wav" \
  -t "$D2" -c:v libx264 -pix_fmt yuv420p -preset veryfast -c:a aac media/two-voices.mp4

# 5. Audio only, stereo 44.1 kHz, to exercise downmix and resample.
"$FF" -y -hide_banner -loglevel error -i "$T/a.aiff" -ac 2 -ar 44100 -c:a aac media/audio-only.m4a

# 6. MKV with VP9 + Opus, the codecs most downloaded video uses.
"$FF" -y -hide_banner -loglevel error -i media/speech.mp4 -t 8 -c:v libvpx-vp9 -b:v 500k -deadline realtime -c:a libopus media/vp9-opus.mkv 2>/dev/null \
  || echo "skipped vp9-opus.mkv (ffmpeg lacks libvpx or libopus)"

# 6b. AV1 + Opus WebM, the codec most modern web video and many YouTube downloads use.
"$FF" -y -hide_banner -loglevel error -i media/speech.mp4 -t 8 -vf scale=1280:720 -c:v libsvtav1 -preset 12 -c:a libopus media/av1-opus.webm 2>/dev/null \
  || echo "skipped av1-opus.webm (ffmpeg lacks libsvtav1 or libopus)"

# 7. Same script with exact word onsets from AVSpeechSynthesizer, for timing accuracy (timing.py).
xcrun swiftc -O say_aligned.swift -o "$T/say_aligned"
"$T/say_aligned" media/speech.txt "$T/aligned.caf" media/aligned.words.json
"$FF" -y -hide_banner -loglevel error -i "$T/aligned.caf" -c:a pcm_f32le media/aligned.wav

echo "speech duration $D"; ls -la media

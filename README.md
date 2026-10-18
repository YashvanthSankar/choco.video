# choco.video

Turn long videos into vertical clips on your Mac. Drop in a podcast, talk or stream; get back ranked, captioned, speaker-centered 9:16 clips ready for TikTok, Reels and Shorts.

- **Your video never leaves your machine.** Transcription, face tracking, captions and rendering run locally. Only the transcript text goes to the clip picker you choose, and only if you choose a hosted one.
- **Small.** About 20 MB installed, 10 MB download. No Python, no Ollama, no system FFmpeg.
- **No subscription to us.** Open source (MIT). The picker runs on Apple's on-device model for free, or on your own OpenAI-compatible provider key for a few cents per hour of video.

## Status

Pre-release, macOS (Apple Silicon) only. Windows is planned after the Mac app ships. See `docs/TODO.md` for exactly what works and what is measured.

## Layout

| Path | What | License |
|---|---|---|
| `engine/` | Rust engine and the `choco` CLI: ingest, transcribe, pick, reframe, caption, render, export | MIT |
| `app/` | macOS app (Tauri 2 + Svelte) over the engine | MIT |
| `scripts/` | FFmpeg (LGPL, minimal) build, app bundling, size check | MIT |
| `docs/` | Plan, stack, technical spec, TODO, competitor notes | |

## Build

Prerequisites: Xcode 16+ command line tools, Rust (`brew install rustup && rustup default stable`), `brew install pkg-config cmake nasm meson ninja`, Node 20+ with pnpm (app only).

```sh
./scripts/ffmpeg/build-mac.sh            # minimal LGPL FFmpeg into vendor/ (once)
cd engine && cargo build --release        # engine + CLI
./target/release/choco doctor             # what this Mac can do
./target/release/choco run video.mp4 --picker apple -o clips   # whole pipeline, on-device picker
```

With an OpenCode Zen key (or any OpenAI-compatible endpoint):

```sh
export CHOCO_LLM_API_KEY=...             # never written to disk by the CLI
export CHOCO_LLM_MODEL=minimax-m3        # see `choco provider models`
./target/release/choco run video.mp4 --picker provider -o clips
```

App: `./scripts/build-app.sh` produces `choco.video.app` and a DMG.

## Credits

FFmpeg (LGPL 2.1, dynamically linked, built with `--disable-gpl`), dav1d (BSD-2), whisper.cpp (MIT), YuNet face detector from the OpenCV Zoo (MIT), Inter and Montserrat (OFL), cosmic-text, tract, Tauri, Svelte. Evaluation fixtures are CC BY recordings from Wikimedia Commons, credited in `engine/fixtures/sources.tsv`. Ideas borrowed with thanks from chopify, Klipzy Studio, AutoClip and OpenClip Studio (see `docs/COMPETITORS.md`).

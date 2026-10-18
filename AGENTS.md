# AGENTS.md — choco.video

You are the development agent for choco.video. This file is your long-term memory for the project. Read it fully at the start of every session. Update it when a decision is made, a benchmark is measured, or a mistake is learned from. Keep it factual and short; the detailed plan is in `docs/PLAN.md`, the stack in `docs/STACK.md`, the implementation spec in `docs/TECH.md`, the milestone checklist in `docs/TODO.md`, and the competitor analysis in `docs/COMPETITORS.md`.

## What this project is

A desktop app, Mac first then Windows, that turns long videos into ranked, captioned, speaker-centered 9:16 shorts entirely on-device. Open-source Rust engine (MIT) plus a paid Tauri app. Competes with Opus Clip and Vizard on "unlimited, local, one-time price". Full context: `docs/PLAN.md`. Competitors and why we win: `docs/COMPETITORS.md`.

## Non-negotiables

1. **Mac ships first, but every engine change must build and test on Windows CI.** Never merge engine code that breaks the Windows job. Platform-specific code lives only in `engine/crates/choco-engine/src/backend/`.
2. **No GPL anywhere in the engine or app.** FFmpeg is LGPL-only (`--disable-gpl`), dynamically linked. No libx264, no libx265. `cargo deny` enforces the allowlist; do not edit `deny.toml` to make a failure go away. Never copy code from SupoClip, ViralCutter, HotClip, or any GPL/AGPL project.
3. **No Python, no Node at runtime.** The shipped app is Rust + a webview. Dev scripts may use anything.
4. **Media never leaves the machine.** Only transcript text may be sent to a cloud model, only on the BYOK tier, only when the user opted in. Never add telemetry, analytics, or network calls without an explicit opt-in setting and a line in this file.
5. **Stages are pure functions over serde types.** No globals, no stage reading another stage's cache. `pipeline/` owns caching and orchestration.
6. **Picker and renderer never run concurrently.** Unload the LLM before rendering. Target machine has 16–18 GB.
7. **Measure before claiming.** A picker change is not an improvement until `choco eval` says precision@5 did not drop. A render change is not done until snapshot tests pass on both OSes. Record numbers in Benchmarks below.
8. **Do not expand scope.** If a task is not in `docs/TODO.md` for the current milestone, ask or add it to a later milestone. v1 non-goals are listed in `docs/PLAN.md §1`. Parity features and their target version are in `docs/PLAN.md §2.10`.

## Stack at a glance

Rust 2024 workspace in `engine/`. `whisper-rs` for transcription, `llama-cpp-2` for the local picker, `ort` + YuNet for faces, `ffmpeg-next` for decode/encode/mux, `wgpu` for compositing, `cosmic-text` + `swash` for captions, `clap` CLI, `cbindgen` FFI. App in `app/` is Tauri 2 with a small TypeScript UI. Payments via Lemon Squeezy. Details and rationale: `docs/STACK.md`, `docs/TECH.md`.

Picker ladder: local GGUF (default, both OSes), Apple Foundation Models (macOS 26+, optional), Anthropic Messages API BYOK (`claude-opus-5-5` default, `claude-haiku-5-5` economy, off by default). Structured JSON output on every tier.

## Dev machine

- Apple M3 Pro, 18 GB unified memory, macOS 27.0.1, Xcode 27.1, Swift 6.4, Homebrew ffmpeg at `/opt/homebrew/bin/ffmpeg` (dev scripts only, never shipped). No `yt-dlp`.
- Memory is the constraint. A 7B Q4 model (~4.5 GB) plus whisper large-v3-turbo (~1.5 GB) plus a decoder fits; two LLMs or a 14B model does not.
- There is no Windows machine. Windows is verified by CI until M6, when real hardware is needed.

## Commands

```
cd engine
cargo build --all                         # engine + CLI + FFI
cargo test --all                          # unit + snapshot tests (model-dependent tests are #[ignore])
cargo test --all -- --ignored             # runs tests that need downloaded models
cargo clippy --all -- -D warnings
cargo deny check                          # licenses, advisories
cargo run -p choco -- doctor              # backend detection
cargo run -p choco -- run fixtures/media/sample.mp4 --n 5 --out /tmp/clips
cargo run -p choco -- eval --picker local # golden-set scores
./fixtures/generate.sh                    # synthetic fixtures (needs ffmpeg on PATH)
./fixtures/fetch.sh                       # CC-licensed real fixtures
cd ../app && npm install && npm run tauri dev
```

## Conventions

- Module per stage, `mod.rs` exposes one `pub fn` entry point plus its options struct. Private helpers stay private.
- Errors: `thiserror` per module, `anyhow` only in the CLI and app. Every error carries the stage name.
- Progress: every long function takes `progress: &dyn Fn(Progress)`. Report at least every 500 ms.
- Timestamps are `u64` milliseconds everywhere. Never floats for time.
- Options structs derive `Hash` so the pipeline cache can key on them.
- Tests next to code in `#[cfg(test)]`; fixtures-based tests under `tests/`. Snapshot references under `tests/snapshots/`, updated only with a commit message that says why.
- Prompts are markdown files under `highlights/llm/prompts/`, versioned by filename. Never inline prompt text in Rust.
- Commit messages: `stage: what changed` (`transcribe: filter repeated 4-grams`). Small commits.
- Do not add dependencies without checking the license and noting it in the PR or commit body.

## Decisions log

| Date | Decision | Why |
|---|---|---|
| 2026-10-18 | Product is a local video clipper (Opus Clip / Vizard category), not dictation or meeting notes | Those categories already have 20k+ star open-source apps (Handy, Meetily, Cap). Clipping has none shipped as a real app. |
| 2026-10-18 | Name: choco.video | User's choice. |
| 2026-10-18 | Engine in Rust, not Swift | Windows is a requirement. Swift/AVFoundation would have meant a full rewrite. Rust + Tauri is the proven path (Handy, Cap). |
| 2026-10-18 | Mac first, Windows at M6 | One hardware family to test on; creators on Mac pay for polished tools; Windows hardware variance needs its own beta. |
| 2026-10-18 | Engine MIT public, app proprietary private | Open core model as VoiceInk, Cap, Cal.com. Stars are marketing; the paid binary is the business. |
| 2026-10-18 | FFmpeg LGPL dynamic, no GPL codecs | Proprietary app compatibility. Hardware encoders cover quality. |
| 2026-10-18 | YuNet ONNX for faces instead of Apple Vision | Identical results on both OSes. Vision stays an optional Mac backend only if measurably better. |
| 2026-10-18 | Captions rasterized with cosmic-text, not platform text APIs | Pixel-identical output across OSes; snapshot tests depend on it. |
| 2026-10-18 | YouTube ingest deferred to v1.1 | Keeps the app free of Python; yt-dlp is an optional download later. |
| 2026-10-18 | Trial = full features with watermark | The watermark is the exact pain users leave Opus Clip for; it converts. |
| 2026-10-18 | Adopt parity features from chopify, Klipzy, AutoClip, OpenClip Studio (complete-thought clips, multi-aspect, SRT/VTT, poster+meta, loudnorm, audio-energy prior); skip SFX, AI chat, B-roll | Cheap, expected, removes objections. See `docs/COMPETITORS.md §5–6`. |
| 2026-10-18 | Never read SupoClip, ViralCutter, HotClip source for implementation | Copyleft. Feature lists only. |

## Benchmarks

Record wall time, peak memory, hardware, model, and commit. Update when they change.

| Date | What | Result | Notes |
|---|---|---|---|
| — | Transcribe 60 min podcast, large-v3-turbo, Metal + CoreML | not measured | M1 target |
| — | Picker on 300 candidates, local 8B Q4 | not measured | M2 target |
| — | Render 60 s clip 1080x1920, VideoToolbox | not measured | M3 target |
| — | Golden set precision@5, local picker | not measured | M2 baseline |

## Known traps

- CoreML whisper encoder compiles on first load and can take several minutes. Surface this in the UI; do not treat it as a hang.
- `ffmpeg-next` build needs `pkg-config` and the FFmpeg dev libs on the link path. On Windows this is the usual source of CI failures; the pinned prebuilt in `scripts/ffmpeg/` is the fix, not ad-hoc downloads.
- whisper.cpp word timestamps drift on long silences. The sentence re-split and clamp pass in `transcribe/` exists for this; do not remove it.
- Phone footage carries a rotation tag. Probe must apply it or faces will be detected sideways.
- `wgpu` readback is the slow path. Prefer handing textures to the encoder when the backend supports it.

## What not to do

- Do not switch the engine language, the UI framework, or the license model. These were decided with reasons above.
- Do not add a cloud-only feature. Everything must work offline except BYOK scoring.
- Do not use Apple-only APIs outside `backend/mac.rs` and the `apple-fm` shim.
- Do not "quickly" use libx264 to get a render working. Use the hardware encoder or the LGPL fallback.
- Do not inline prompts, bump `deny.toml`, or skip the eval to make progress look faster.
- Do not start the Windows UI before the Mac app has shipped.
- Do not install Ollama, Python, or system ffmpeg as a runtime dependency to "get it working". Competitors did; that is why they have no users.

## Status

- 2026-10-18: Planning complete. `docs/PLAN.md`, `docs/STACK.md`, `docs/TECH.md`, `docs/TODO.md`, `docs/COMPETITORS.md` written. Repo initialized, nothing committed. No code yet. Next: M1, first item in `docs/TODO.md`.

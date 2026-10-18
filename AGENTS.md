# AGENTS.md — choco.video

You are the development agent for choco.video. This file is your long-term memory for the project. Read it fully at the start of every session. Update it when a decision is made, a benchmark is measured, or a mistake is learned from. Keep it factual and short; the detailed plan is in `docs/PLAN.md`, the stack in `docs/STACK.md`, the implementation spec in `docs/TECH.md`, the milestone checklist in `docs/TODO.md`, and the competitor analysis in `docs/COMPETITORS.md`.

## What this project is

A small desktop app (~20 MB installer), Mac first then Windows, that turns long videos into ranked, captioned, speaker-centered 9:16 shorts. Video is processed on-device; the highlight picker runs on a hosted frontier model by default with offline fallbacks. Open-source Rust engine (MIT), a paid Tauri app, and a tiny private cloud service for the picker. Competes with Opus Clip and Vizard on "no credits, your video stays local, one-time price". Full context: `docs/PLAN.md`. Competitors and why we win: `docs/COMPETITORS.md`.

## Non-negotiables

1. **Mac ships first, but every engine change must build and test on Windows CI.** Never merge engine code that breaks the Windows job. Platform-specific code lives only in `engine/crates/choco-engine/src/backend/`.
2. **No GPL anywhere in the engine or app.** FFmpeg is LGPL-only (`--disable-gpl`), dynamically linked. No libx264, no libx265. `cargo deny` enforces the allowlist; do not edit `deny.toml` to make a failure go away. Never copy code from SupoClip, ViralCutter, HotClip, or any GPL/AGPL project.
3. **No Python, no Node at runtime.** The shipped app is Rust + a webview. Dev scripts may use anything.
4. **Video and audio never leave the machine.** Only transcript text may be sent anywhere, and only to the picker (our `cloud/` service or the user's own Anthropic key). The cloud service never logs transcripts. Never add telemetry, analytics, or other network calls without an explicit opt-in setting and a line in this file.
4b. **Small by default.** Installer ≤ 25 MB, installed app ≤ 50 MB without models, enforced in CI. Nothing heavy ships in the installer; models are runtime downloads and the default path on a current Mac downloads nothing. Prefer pure-Rust crates (`tract`, `cosmic-text`) over C runtimes when output is identical. A dependency over 2 MB compiled needs a Decisions entry. Never trade output quality for size; trade where the model runs.
5. **Stages are pure functions over serde types.** No globals, no stage reading another stage's cache. `pipeline/` owns caching and orchestration.
6. **The optional local picker and the renderer never run concurrently.** Unload the model before rendering. The default tiers have no in-process LLM. Target machine has 16–18 GB.
7. **Measure before claiming.** A picker change is not an improvement until `choco eval` says precision@5 did not drop. A render change is not done until snapshot tests pass on both OSes. Record numbers in Benchmarks below.
8. **Do not expand scope.** If a task is not in `docs/TODO.md` for the current milestone, ask or add it to a later milestone. v1 non-goals are listed in `docs/PLAN.md §1`. Parity features and their target version are in `docs/PLAN.md §2.10`.

## Stack at a glance

Rust 2024 workspace in `engine/`. Apple SpeechAnalyzer (macOS 26+) or `whisper-rs` for transcription, `tract` + YuNet for faces, minimal LGPL `ffmpeg-next` for decode/encode/mux, `wgpu` (Metal + DX12 only) for compositing, `cosmic-text` + `swash` for captions, BM25 for prompt search, `clap` CLI, `cbindgen` FFI, optional `llama-cpp-2` behind `local-llm`. App in `app/` is Tauri 2 with a small TypeScript UI. `cloud/` is an `axum` service for the hosted picker. Payments and license keys via Lemon Squeezy. Details and rationale: `docs/STACK.md`, `docs/TECH.md`.

Picker ladder: hosted (default, our service: `claude-haiku-5-5` scores candidate batches, `claude-opus-5-5` writes the episode summary and the final titles for the chosen clips only; license-key auth, 50 source hours/month fair use), Apple Foundation Models (macOS 26+, offline), BYOK (user's own key), local 4B GGUF (opt-in ~2.5 GB download). Structured JSON output on every tier. Transcript text only, always.

## Dev machine

- Apple M3 Pro, 18 GB unified memory, macOS 27.0.1, Xcode 27.1, Swift 6.4, Homebrew ffmpeg at `/opt/homebrew/bin/ffmpeg` (dev scripts only, never shipped). No `yt-dlp`.
- macOS 27 means Apple SpeechAnalyzer and Foundation Models are available here; the zero-download path can be tested on this machine. Whisper and the local picker must still be tested here for Windows parity.
- Memory matters only on the opt-in local tier: a 4B Q4 model (~2.5 GB) plus whisper turbo-q5 (~550 MB) plus a decoder fits comfortably; never load more than one model.
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
- Prompts are markdown files under `highlights/picker/prompts/`, versioned by filename, shared with `cloud/`. Never inline prompt text in Rust.
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
| 2026-10-18 | Small-app principle: ~20 MB installer, no bundled models, zero download on current Macs | User requirement. "All local" would mean multi-GB downloads for worse picks. |
| 2026-10-18 | Picker default is hosted (our `cloud/` service on Claude), included in the one-time price with a 50 h/month fair-use cap; Apple on-device, BYOK and an opt-in local 4B model are the fallbacks | Best quality with zero download; cents per source hour; keeps the "no credits" promise; offline paths still exist. |
| 2026-10-18 | Transcription via Apple SpeechAnalyzer on macOS 26+, whisper.cpp quantized turbo elsewhere | Zero download on Mac; same `Transcript` type either way; golden set decides per-language fallbacks. |
| 2026-10-18 | `tract` instead of ONNX Runtime for YuNet; minimal FFmpeg allowlist build; `wgpu` Metal+DX12 only; no CUDA; arm64-only Mac; OS software encoders as fallback, no openh264 | Each is a size cut with identical output. openh264 also carries patent exposure. |
| 2026-10-18 | BM25 keyword search for prompt mode in v1, no embedding model | Avoids a model download; semantic search becomes an opt-in later. |
| 2026-10-18 | Hosted picker: Haiku 5.5 scores batches; Opus 5.5 only for the episode summary and final titles of chosen clips | Rough estimate per source hour: Opus-for-everything ≈ $0.26 (≈ $13/month at the cap, more than a one-time license can carry); Haiku scoring + Opus titles ≈ $0.02–0.03. Eval must confirm Haiku picks hold up. |
| 2026-10-18 | Marketing says "no credits", never "unlimited" | There is a 50 h/month fair-use cap. |

## Benchmarks

Record wall time, peak memory, hardware, model, and commit. Update when they change.

| Date | What | Result | Notes |
|---|---|---|---|
| — | Transcribe 60 min podcast, Apple SpeechAnalyzer | not measured | M1 target |
| — | Transcribe 60 min podcast, whisper turbo-q5, Metal | not measured | M1 target |
| 2026-10-18 | Minimal FFmpeg 8.1.3 dylibs total size, Mac arm64 | 7.5 MB | LGPL 2.1; avcodec 4.5 MB is most of it |
| 2026-10-18 | Engine footprint (CLI 3.2 MB incl. whisper.cpp+Metal, Apple helper 0.17 MB, FFmpeg 7.5 MB) | 11.3 MB installed, 4.9 MB zip -9 | engine part only; app at M5 |
| — | Installer size, Mac DMG | not measured | ≤ 25 MB, M5 |
| — | Picker on 300 candidates, hosted tier, wall time and tokens | not measured | M2 target |
| — | Picker on 300 candidates, local 4B Q4 | not measured | M2, opt-in tier |
| — | Render 60 s clip 1080x1920, VideoToolbox | not measured | M3 target |
| — | Golden set precision@5, hosted picker (Haiku scoring) | not measured | M2 baseline, the default tier |
| — | Golden set precision@5, Haiku vs Opus scoring | not measured | M2; Opus scoring only if it beats Haiku by a margin worth its cost |
| — | Golden set precision@5, apple and local tiers | not measured | M2 |
| — | Hosted cost per source hour, measured tokens × price | not measured | M2; budget ≤ $0.03 |

## Known traps

- Apple SpeechAnalyzer downloads its own language assets on first use via the OS. Surface that progress; do not treat it as a hang.
- If the CoreML whisper encoder is ever enabled as an optional speed-up, it compiles on first load and can take minutes. Same rule.
- `ffmpeg-next` build needs `pkg-config` and the FFmpeg dev libs on the link path. On Windows this is the usual source of CI failures; the pinned prebuilt in `scripts/ffmpeg/` is the fix, not ad-hoc downloads.
- whisper.cpp word timestamps drift on long silences. The sentence re-split and clamp pass in `transcribe/` exists for this; do not remove it.
- Phone footage carries a rotation tag. Probe must apply it or faces will be detected sideways.
- `wgpu` readback is the slow path. Prefer handing textures to the encoder when the backend supports it.

## What not to do

- Do not switch the engine language, the UI framework, or the license model. These were decided with reasons above.
- Do not add a cloud-only feature. Everything must work offline except picker scoring (hosted or BYOK), and even that has the Apple and local offline tiers.
- Do not use Apple-only APIs outside `backend/mac.rs` and the `apple-fm` shim.
- Do not "quickly" use libx264 to get a render working. Use the hardware encoder or the LGPL fallback.
- Do not inline prompts, bump `deny.toml`, or skip the eval to make progress look faster.
- Do not start the Windows UI before the Mac app has shipped.
- Do not install Ollama, Python, or system ffmpeg as a runtime dependency to "get it working". Competitors did; that is why they have no users.
- Do not bundle a model in the installer, add ONNX Runtime, CUDA, a universal binary, or a general FFmpeg build. Each one blows the size budget for no output gain.
- Do not log, store, or forward transcript text in `cloud/` beyond the single scoring request. Usage counters only.
- Do not let the hosted picker become a dependency for anything but scoring. Transcription, reframe, captions and render must keep working offline.

## Status

- 2026-10-18: Planning complete. `docs/PLAN.md`, `docs/STACK.md`, `docs/TECH.md`, `docs/TODO.md`, `docs/COMPETITORS.md` written, then revised for the small-app principle (hosted picker default, Apple speech on Mac, tract, minimal FFmpeg, size budget). Repo initialized, nothing committed. No code yet. Next: M1, first item in `docs/TODO.md`.

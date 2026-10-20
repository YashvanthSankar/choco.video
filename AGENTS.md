# AGENTS.md — choco.video

You are the development agent for choco.video. This file is your long-term memory for the project. Read it fully at the start of every session. Update it when a decision is made, a benchmark is measured, or a mistake is learned from. Keep it factual and short; the detailed plan is in `docs/PLAN.md`, the stack in `docs/STACK.md`, the implementation spec in `docs/TECH.md`, the milestone checklist in `docs/TODO.md`, and the competitor analysis in `docs/COMPETITORS.md`.

## What this project is

A small, fully open-source desktop app (~20 MB installer), Mac first then Windows, that turns long videos into ranked, captioned, speaker-centered 9:16 shorts. Video is processed on-device. The highlight picker runs on the user's own OpenAI-compatible provider (OpenCode Zen recommended at onboarding) or Apple's on-device model. There is no backend: no server, no accounts, no license keys. Competes with Opus Clip and Vizard on "no credits, your video stays local, no subscription to us". Full context: `docs/PLAN.md`. Competitors and why we win: `docs/COMPETITORS.md`.

## Non-negotiables

1. **Mac first, Windows after the Mac app ships.** Do no Windows work (builds, CI, testing, Windows-only code) before M6. Still keep platform-specific code only in `engine/crates/choco-engine/src/backend/`.
2. **No GPL anywhere in the engine or app.** FFmpeg is LGPL-only (`--disable-gpl`), dynamically linked. No libx264, no libx265. `cargo deny` enforces the allowlist; do not edit `deny.toml` to make a failure go away. Never copy code from SupoClip, ViralCutter, HotClip, or any GPL/AGPL project.
3. **No Python, no Node at runtime.** The shipped app is Rust + a webview. Dev scripts may use anything.
4. **Video and audio never leave the machine.** Only transcript text may be sent anywhere, and only to the picker provider the user configured. We run no server and receive nothing. Never add telemetry, analytics, or other network calls without an explicit opt-in setting and a line in this file.
4b. **Small by default.** Installer ≤ 25 MB, installed app ≤ 50 MB without models, enforced in CI. Nothing heavy ships in the installer; models are runtime downloads and the default path on a current Mac downloads nothing. Prefer pure-Rust crates (`tract`, `cosmic-text`) over C runtimes when output is identical. A dependency over 2 MB compiled needs a Decisions entry. Never trade output quality for size; trade where the model runs.
5. **Stages are pure functions over serde types.** No globals, no stage reading another stage's cache. `pipeline/` owns caching and orchestration.
6. **The optional local picker and the renderer never run concurrently.** Unload the model before rendering. The default tiers have no in-process LLM. Target machine has 16–18 GB.
7. **Measure before claiming.** A picker change is not an improvement until `choco eval` says precision@5 did not drop. A render change is not done until snapshot tests pass on both OSes. Record numbers in Benchmarks below.
8. **Do not expand scope.** If a task is not in `docs/TODO.md` for the current milestone, ask or add it to a later milestone. v1 non-goals are listed in `docs/PLAN.md §1`. Parity features and their target version are in `docs/PLAN.md §2.10`.

## Stack at a glance

Rust 2024 workspace in `engine/`. Apple SpeechAnalyzer (macOS 26+) or `whisper-rs` for transcription, `tract` + YuNet for faces, minimal LGPL `ffmpeg-next` for decode/encode/mux, `wgpu` (Metal + DX12 only) for compositing, `cosmic-text` + `swash` for captions, BM25 for prompt search, `ureq` for the picker client and model downloads, `clap` CLI, `cbindgen` FFI, optional `llama-cpp-2` behind `local-llm`. App in `app/` is Tauri 2 with a Svelte UI. Everything MIT. Details and rationale: `docs/STACK.md`, `docs/TECH.md`.

Picker tiers, chosen at onboarding: **provider** (any OpenAI-compatible `/chat/completions` endpoint; OpenCode Zen `https://opencode.ai/zen/v1` is the recommended default; key in the macOS Keychain; scoring and summary models selectable, pre-filled with the eval's picks), **apple** (Apple Intelligence on-device, macOS 26+, zero setup), **local** (opt-in 4B GGUF, ~2.5 GB, fully offline). CLI reads `CHOCO_LLM_BASE_URL`, `CHOCO_LLM_API_KEY`, `CHOCO_LLM_MODEL`, `CHOCO_LLM_SUMMARY_MODEL`. Structured JSON on every tier. Transcript text only, always.

## Dev machine

- Apple M3 Pro, 18 GB unified memory, macOS 27.0.1, Xcode 27.1, Swift 6.4, Homebrew ffmpeg at `/opt/homebrew/bin/ffmpeg` (dev scripts only, never shipped). No `yt-dlp`.
- macOS 27 means Apple SpeechAnalyzer and Foundation Models are available here; the zero-download path can be tested on this machine. Whisper and the local picker must still be tested here for Windows parity.
- Memory matters only on the opt-in local tier: a 4B Q4 model (~2.5 GB) plus whisper turbo-q5 (~550 MB) plus a decoder fits comfortably; never load more than one model.
- There is no Windows machine, and no Windows work happens before M6. Then it needs CI plus real hardware.

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
- Prompts are markdown files under `highlights/picker/prompts/`, versioned by filename. Never inline prompt text in Rust.
- Commit messages: `stage: what changed` (`transcribe: filter repeated 4-grams`). Small commits.
- Do not add dependencies without checking the license and noting it in the PR or commit body.

## Decisions log

| Date | Decision | Why |
|---|---|---|
| 2026-10-18 | Product is a local video clipper (Opus Clip / Vizard category), not dictation or meeting notes | Those categories already have 20k+ star open-source apps (Handy, Meetily, Cap). Clipping has none shipped as a real app. |
| 2026-10-18 | Name: choco.video | User's choice. |
| 2026-10-18 | Engine in Rust, not Swift | Windows is a requirement. Swift/AVFoundation would have meant a full rewrite. Rust + Tauri is the proven path (Handy, Cap). |
| 2026-10-18 | Mac first, Windows at M6 | One hardware family to test on; creators on Mac pay for polished tools; Windows hardware variance needs its own beta. |
| 2026-10-18 | ~~Engine MIT public, app proprietary private~~ superseded 2026-10-18: everything is open source (see below) | Kept for history. |
| 2026-10-18 | FFmpeg LGPL dynamic, no GPL codecs | Proprietary app compatibility. Hardware encoders cover quality. |
| 2026-10-18 | YuNet ONNX for faces instead of Apple Vision | Identical results on both OSes. Vision stays an optional Mac backend only if measurably better. |
| 2026-10-18 | Captions rasterized with cosmic-text, not platform text APIs | Pixel-identical output across OSes; snapshot tests depend on it. |
| 2026-10-18 | YouTube ingest deferred to v1.1 | Keeps the app free of Python; yt-dlp is an optional download later. |
| 2026-10-18 | Trial = full features with watermark | The watermark is the exact pain users leave Opus Clip for; it converts. |
| 2026-10-18 | Adopt parity features from chopify, Klipzy, AutoClip, OpenClip Studio (complete-thought clips, multi-aspect, SRT/VTT, poster+meta, loudnorm, audio-energy prior); skip SFX, AI chat, B-roll | Cheap, expected, removes objections. See `docs/COMPETITORS.md §5–6`. |
| 2026-10-18 | Never read SupoClip, ViralCutter, HotClip source for implementation | Copyleft. Feature lists only. |
| 2026-10-18 | Small-app principle: ~20 MB installer, no bundled models, zero download on current Macs | User requirement. "All local" would mean multi-GB downloads for worse picks. |
| 2026-10-18 | ~~Picker default is hosted (our `cloud/` service) with a fair-use cap~~ superseded below | Kept for history. |
| 2026-10-18 | Transcription via Apple SpeechAnalyzer on macOS 26+, whisper.cpp quantized turbo elsewhere | Zero download on Mac; same `Transcript` type either way; golden set decides per-language fallbacks. |
| 2026-10-18 | `tract` instead of ONNX Runtime for YuNet; minimal FFmpeg allowlist build; `wgpu` Metal+DX12 only; no CUDA; arm64-only Mac; OS software encoders as fallback, no openh264 | Each is a size cut with identical output. openh264 also carries patent exposure. |
| 2026-10-18 | BM25 keyword search for prompt mode in v1, no embedding model | Avoids a model download; semantic search becomes an opt-in later. |
| 2026-10-18 | ~~Hosted picker on Anthropic (Haiku scores, Opus titles)~~ superseded below | Kept for history: the cost reasoning still applies. A frontier model for every call would cost ≈ $0.26 per source hour, too much for a one-time price. Score with a cheap model, title with a strong one. |
| 2026-10-18 | Picker providers are OpenAI-compatible only, via one `/chat/completions` client. No Anthropic SDK or API | User decision. OpenAI-compatible is the common denominator across OpenCode, OpenRouter, Groq, DeepSeek and local servers. |
| 2026-10-18 | No backend. Client-only app; users bring their own provider key at onboarding (OpenCode Zen recommended) and pick models from our eval-backed defaults; Apple on-device needs no key. Whole project open source (MIT); no license keys, no watermark trial, no Lemon Squeezy | User decision ("client only, open source"). No server to run or secure, no inference cost on us, smaller scope, nothing hidden from users. Revenue model is an open question that does not block building. |
| 2026-10-18 | Mac only until the Mac app ships; all Windows work (CI job, FFmpeg, backend, testing) moves to M6 | User decision: finish Mac first. Platform code stays behind `backend/` so the port remains a backend-and-shell job. |
| 2026-10-18 | Marketing says "no credits", never "unlimited" | There is a 50 h/month fair-use cap. |

## Benchmarks

Record wall time, peak memory, hardware, model, and commit. Update when they change.

| Date | What | Result | Notes |
|---|---|---|---|
| 2026-10-18 | Transcribe 72 min real podcast (danaylov, VP9/Opus), Apple SpeechAnalyzer, release | 112 s total (audio 9 s, speech 103 s, 42x realtime), ~30 MB peak in-process, 11,415 words, steady ~160 wpm | M3 Pro 18 GB; speech model runs in the OS service, not counted |
| 2026-10-18 | Transcribe 72 min real podcast, whisper turbo-q5 on Metal, release | 267 s total (speech 257 s, 17x realtime), 1.7 GB peak; 10,156 words after loop cleanup vs Apple's 11,415; 85% of words agree in order; word starts differ by median 320 ms | Whisper loses ~11% of speech under loops; fine as the fallback for macOS < 26, needs loop recovery before M6 |
| 2026-10-18 | Minimal FFmpeg 8.1.3 dylibs total size, Mac arm64 | 8.3 MB (7.5 MB before AV1) | LGPL 2.1; dav1d 1.5.4 (BSD-2) static inside avcodec (5.6 MB) |
| 2026-10-18 | Engine footprint (CLI 3.2 MB incl. whisper.cpp+Metal, Apple helper 0.17 MB, FFmpeg 7.5 MB) | 11.3 MB installed, 4.9 MB zip -9 | engine part only; app at M5 |
| — | Installer size, Mac DMG | not measured | ≤ 25 MB, M5 |
| 2026-10-18 | Word-start error vs synthesized ground truth (62 words, `fixtures/timing.py`) | Apple: median 103 ms, bias −100 ms, 49% within 100 ms. Whisper turbo-q5 token times: median 256 ms, bias −232 ms, 11% within 100 ms | Both early. Captions need ≤ 50 ms; fix in M3 (DTW, offset calibration) |
| 2026-10-20 | Word-start error after bias correction, whisper DTW token times (−200 ms) and audio-onset snapping (±90 ms window), three voices (`aligned`, `aligned2`, `aligned3`) | Apple: median 45 / 72 / 60 ms, p90 151–224 ms, 36–51% within 50 ms. Whisper DTW: median 54 / 66 / 64 ms, p90 165–204 ms, 39–47% within 50 ms | Was 52–95 ms median before onset snapping. DTW alone did not move the median (71–85 ms) but cut p90 from ~250 to ~165 ms. The ≤ 50 ms target is met on one voice of three; the remaining error is scatter, not bias |
| 2026-10-20 | Apple speech vs whisper turbo-q5 per language, synthesized macOS voices reading the same 50-word passage (`fixtures/languages.tsv`, `fixtures/lang_eval.py`; WER, CER for ja/zh) | Apple / whisper: en 0 / 0 · es 0 / 0 · fr 0 / 0.13 · de 0 / 0 · it 0.03 / 0 · pt-BR 0.07 / 0 · nl (unsupported by Apple → whisper) 0 · ko 0.03 / 0.23 · ja 0 / 0.04 · zh 0.03 / 0.07 · hi 1.0 / 0.8 | Apple stays the default where supported; whisper is the fallback for its 45-locale gap. Hindi is broken on both (likely the Lekha voice or a script mismatch), unresolved. Synthetic voices only: real-speech numbers will be worse for both |
| 2026-10-18 | Shortlist ceiling: golden clips reachable by the model | 60 @ 85% overlap: 0.47 · 60 @ 10%: 0.80 · 80 @ 20%: 0.93 (default) · 90 @ 10%: 1.00 | The heuristic prior was the bottleneck, not the model |
| — | Picker on the 72-min podcast per OpenCode model: wall time, tokens, cost per source hour | in progress | M2 |
| — | Picker on 300 candidates, local 4B Q4 | not measured | M2, opt-in tier |
| 2026-10-18 | Render 62 s clip to 1080x1920 (VP9 640x360 source, bold captions, loudnorm), VideoToolbox H.264 | 10.8 s wall, 5.7x realtime; ~25 MB at 10 Mbps | CPU compositor, no GPU library |
| — | Golden set precision@5, provider tier (recommended scoring model) | not measured | M2 baseline |
| — | Golden set precision@5 and cost per source hour, candidate scoring models on OpenCode Zen | not measured | M2; pick the cheapest model within 2 points of the best |
| — | Golden set precision@5, apple and local tiers | not measured | M2 |
| — | Provider cost per source hour, measured tokens × OpenCode Zen price | not measured | M2; recommended default must be ≤ $0.03 |

## Known traps

- Apple SpeechAnalyzer downloads its own language assets on first use via the OS. Surface that progress; do not treat it as a hang.
- If the CoreML whisper encoder is ever enabled as an optional speed-up, it compiles on first load and can take minutes. Same rule.
- `ffmpeg-next` build needs `pkg-config` and the FFmpeg dev libs on the link path. On Windows this is the usual source of CI failures; the pinned prebuilt in `scripts/ffmpeg/` is the fix, not ad-hoc downloads.
- whisper.cpp word timestamps drift on long silences. The sentence re-split and clamp pass in `transcribe/` exists for this; do not remove it.
- whisper turbo-q5 falls into hallucination loops on real conversation (measured: one 8-word sentence 174 times in a row, real speech under it lost). `cleanup` collapses periods up to 16 words; the lost speech is not recovered. Apple speech had no such loops on the same file.
- Both speech engines stamp words early (Apple ~100 ms, whisper token times ~230 ms). Do not drive caption highlights from raw times until M3's timing fix lands; measure with `fixtures/timing.py`.
- AVSpeechSynthesizer word-marker offsets restart at each internal synthesis chunk (an empty buffer marks it). `fixtures/say_aligned.swift` rebases them; keep that if you touch it.
- OpenCode Zen is behind Cloudflare and returns HTTP 403 "error code: 1010" to some default client user agents (Python urllib, measured). The client always sends `User-Agent: choco-video/<version>`; keep it.
- Apple's default guardrails decline podcast batches about war, ethics or health as "sensitive content" (5–7 of 15 batches on the 72-min podcast). The helper retries declined batches as plain text under `.permissiveContentTransformations`, which fixed all declines in that run.
- Real downloads are often AV1 (2 of our 5 CC BY fixtures). The minimal FFmpeg must keep `av1`, `libdav1d` and `av1_videotoolbox`; `tests/media.rs` fails if a format regresses. Probe reads codec parameters directly so it never needs a decoder.
- GitHub's package registry (ghcr.io) was unreachable from the dev network, so Homebrew bottles failed; meson came from PyPI in a scratch venv (`MESON=` env for `build-mac.sh`).
- Phone footage carries a rotation tag. Probe must apply it or faces will be detected sideways.
- `wgpu` readback is the slow path. Prefer handing textures to the encoder when the backend supports it.

## What not to do

- Do not switch the engine language, the UI framework, or the license model. These were decided with reasons above.
- Do not add a cloud-only feature or any server of ours. Everything works offline except picker scoring on a remote provider, and the Apple and local tiers cover offline.
- Do not use Apple-only APIs outside `backend/mac.rs` and the `apple-fm` shim.
- Do not "quickly" use libx264 to get a render working. Use the hardware encoder or the LGPL fallback.
- Do not inline prompts, bump `deny.toml`, or skip the eval to make progress look faster.
- Do not start the Windows UI before the Mac app has shipped.
- Do not install Ollama, Python, or system ffmpeg as a runtime dependency to "get it working". Competitors did; that is why they have no users.
- Do not bundle a model in the installer, add ONNX Runtime, CUDA, a universal binary, or a general FFmpeg build. Each one blows the size budget for no output gain.
- Never write the user's provider key to disk outside the Keychain, to logs, to project files, or to git. The CLI reads it from the environment only.
- Do not let the picker become a dependency for anything but scoring. Transcription, reframe, captions and render must keep working with no provider configured.

## Status

- 2026-10-18: Planning complete. `docs/PLAN.md`, `docs/STACK.md`, `docs/TECH.md`, `docs/TODO.md`, `docs/COMPETITORS.md` written, then revised for the small-app principle (hosted picker default, Apple speech on Mac, tract, minimal FFmpeg, size budget). Repo initialized, nothing committed. No code yet. Next: M1, first item in `docs/TODO.md`.

# choco.video — TODO

Companion docs: `PLAN.md` (what and why), `STACK.md` (tools), `TECH.md` (how), `COMPETITORS.md` (what others ship). Root `AGENTS.md` is the agent's memory.

Legend: `[x]` done and verified on the dev Mac · `[~]` in progress or partly verified · `[ ]` not started. Notes after `→` record what was actually measured.

Rules: one milestone in progress at a time. A box is checked only when it works and is measured on the dev Mac. **No Windows work until M6** (user decision, 2026-10-18). Move finished milestones to the bottom. Keep this file short; details go in `TECH.md`.

## Done: M1 Engine skeleton (weeks 1–2) — Mac complete; CI runs on first push

- [x] `git init`, `.gitignore` (target/, fixtures media, models, vendor/, *.chocoproj), `LICENSE` (MIT) in `engine/`
- [x] Cargo workspace: `choco-engine`, `choco`, `choco-ffi`; shared lints; `deny.toml` with license allowlist and bans (x264, x265, openh264, ort) → builds clean, clippy clean; `cargo deny check` passes
- [x] `model/` types from `TECH.md §2` with serde round-trip tests → 2 tests pass
- [x] `backend/`: trait, `detect()`, `mac.rs` (AppleSpeech on 26+, Metal, VideoToolbox), `windows.rs` (placeholder, untouched until M6), `cpu.rs`, `recommended_models()` by memory → detects M3 Pro, 18 GB, macOS 27.0.1, AppleSpeech → Whisper Metal
- [x] Release profile in workspace `Cargo.toml` (fat LTO, 1 codegen unit, abort, strip, opt-level z with overrides); arm64-only Mac target; toolchain pinned to 1.99.1
- [x] FFmpeg minimal LGPL: build recipe with `--disable-everything` plus an allowlist (mov/mp4/matroska/webm demux, h264/hevc/vp9/aac/opus/mp3 decode, hw encoders, aac encode, loudnorm/aresample/scale filters, VideoToolbox/D3D11VA/NVENC/AMF/QSV/MF hwaccel), `--disable-gpl --disable-nonfree`; pin version; record the dylib sizes in `AGENTS.md` Benchmarks; document in `scripts/ffmpeg/README.md`; Windows link path verified in CI → Mac done: 8.1.3, 7.5 MB of dylibs, LGPL 2.1 confirmed, `@rpath` install names
- [x] `ingest/`: `probe`, `extract_audio` (16 kHz mono f32), `thumbnail` → probes MP4, MOV, MKV/WebM (VP9, Opus), audio-only M4A; rotated phone clip thumbnail matches FFmpeg's reference pixel for pixel
- [x] `transcribe/`: `Transcriber` trait + shared cleanup (sentence re-split, hallucination filter, clamp)
- [x] `transcribe/apple.rs`: Swift helper (168 KB) for SpeechAnalyzer with word timings, built by `build.rs` on macOS 26+ → 45 locales; 0% WER on the synthetic script, 20x realtime, zero download
- [x] `transcribe/whisper.rs`: `whisper-rs` with Metal/CPU, word timestamps from tokens, quantized turbo default → 0% WER on the synthetic script; fixed a progress deadlock caused by whisper-rs leaking its callback. whisper.cpp windows long audio itself, so no manual overlap windowing
- [x] `models_registry/`: manifests pinned to a Hugging Face revision, resumable download, sha256 verify (upstream publishes sha256, so not blake3), `choco models list|pull|rm`
- [x] CLI: `choco probe`, `choco transcribe`, `choco thumbnail`, `choco doctor` with fix hints, `choco models`, `--json` → doctor reports ready
- [x] `fixtures/generate.sh` (synthetic, macOS `say` voices with known text: speech, rotated phone clip, silent, two voices, audio-only, VP9/Opus MKV) and `fixtures/fetch.sh` + `sources.tsv` (5 CC BY 3.0 long-form recordings from Wikimedia Commons, 48–72 min, sha1-verified)
- [~] CI `engine.yml` on `macos-15` + cargo-deny: release build, test, clippy, size report → written; runs on the first push. `cargo deny check` passes locally. Windows job removed until M6
- [x] Size budget check script (`scripts/size-check.sh`), 25 MB installer / 50 MB installed → engine part measures 11.3 MB installed, 4.9 MB compressed; wired into `app.yml` at M5
- [x] Transcribe a 60-minute podcast on the M3 Pro with Apple speech and with whisper turbo-q5 → 72-min CC BY podcast: Apple 112 s (42x), ~30 MB; whisper 267 s (17x), 1.7 GB, loses ~11% of words to hallucination loops. Ground-truth timing: Apple 103 ms median error, whisper 256 ms (captions fix in M3)

## Now: M2 Picker (weeks 3–4)

- [x] `highlights/windows.rs` candidate generation with pre-filters (min words, dead-air ratio, filler-only openings) → 2,925 windows from the 72-min podcast; 3 tests
- [x] Complete-thought boundaries: every window starts after and ends on a sentence end (punctuation or ≥ 700 ms pause) → tested, including unpunctuated segments
- [x] Audio energy + pause features (loudness z-score, spikes per minute, pause ratio, longest pause) from the 16 kHz wav → fixed a silence-threshold bug that counted speech as silence when silence is rare
- [x] `highlights/search.rs` shortlist: quality prior (pace, hook words, questions, numbers, length, weak openers, audio), BM25 prompt mode with synonym and inflection expansion, near-duplicate removal. No embedding model
- [~] `highlights/picker/openai_compat.rs`: one `/chat/completions` client (ureq), `json_schema` → `json_object` fallback remembered per client, 429/5xx backoff, refusal handling, `<think>` and code-fence stripping → 3 tests against a local mock server; **not yet run against OpenCode Zen (needs a key)**
- [~] `highlights/picker/` `Llm` trait with shared batching, map-reduce summary for small contexts, strict parse + key repair + one retry, re-ask for skipped ids → fake-model end-to-end test passes; Apple tier: 48/60 scored, 0 declined after the guardrail fix; provider tier running against OpenCode Zen
- [~] Pick the scoring and summary models on OpenCode Zen by golden-set precision@5 and measured cost per source hour (budget ≤ $0.03) → all candidates accept strict JSON schemas; 72-min podcast, 0 failed batches each: MiniMax M3 56 s ≈ 1.5¢/h, GLM 5.3 Flash 248 s ≈ 2.3¢/h, Qwen 3.8 Flash 550 s ≈ 1.9¢/h, DeepSeek V4.1 Flash 109 s ≈ 4.5¢/h (over budget). Quality ranking: eval running
- [x] Prompt v1 in `highlights/picker/prompts/score_v1.md` and `summarize_v1.md`; provenance hash (blake3, 16 hex) on every candidate
- [x] ~~`cloud/` hosted service~~ dropped: no backend (decision 2026-10-18). Provider tier calls the user's endpoint directly
- [x] `rank.rs` overlap suppression (> 40% of the shorter clip), same-content suppression, score floor 55, `--n` is a max
- [x] `choco highlights` with `--n`, `--prompt`, `--picker auto|apple|provider`, `--min-score`, `--transcript` reuse, `-o` JSON with a diagnostic report and token usage
- [~] Apple on-device tier on the real podcast → 10 sensible clips in ~5 min, but Apple's default guardrails declined 5–7 of 15 batches as "sensitive content" (false positives on ethics/war talk). Fix in progress: retry declined batches under the permissive content-transformation guardrails. Calibration is loose (a clip with "weak hook, low energy" reasons scored 68); revisit with the eval
- [~] Golden set → 5 real CC BY fixtures (48–72 min; VP8, VP9, AV1), 40 clips drafted by separate editor agents from transcripts only, validated against segment boundaries and for overlaps; notes, ASR errors and alternates in `eval/REVIEW.md`. **Needs human review** before numbers count; `choco eval` prints them as provisional until then
- [~] `choco eval` runner (precision@5, recall@10, IoU; saved results or live runs; token usage and source duration recorded) → built and tested; baseline per model pending the last two golden fixtures
- [ ] Compare Apple speech vs whisper on the golden set per language; decide per-language fallback list
- [ ] `LocalLlama` behind `local-llm` feature with GBNF grammar; pick the default 4B model (Qwen3-4B vs Gemma-3-4B class) by eval and speed; model is opt-in download only
- [ ] Provider settings (base URL, key, scoring model, summary model, test button) with the key in the macOS Keychain via `keyring`; "transcript text only" stated next to the picker setting

- [x] AV1 input: dav1d (static, BSD-2) + VideoToolbox AV1 in the minimal FFmpeg (+0.8 MB); probe no longer needs a decoder; `tests/media.rs` covers H.264/AAC MP4, rotated phone clip, no-audio, audio-only M4A, VP9/Opus MKV, AV1/Opus WebM, float WAV (7 tests)

## M3 Captions + render (weeks 5–6)

- [~] Word timing accuracy → per-engine bias correction shipped (Apple +100 ms, whisper +260 ms, from medians on two synthesized voices; bias on a held-out third voice now −16 to +40 ms). Remaining per-word scatter: median 52–95 ms, 53–67% within 100 ms; **≤ 50 ms target not met**. Tried snapping to energy onsets: mixed (helped 2 clips, hurt 1), not shipped. Next: whisper DTW timestamps; measure caption readability, not just onsets
- [x] `captions/` chunker: ≤ 4 words, breaks on clause punctuation, pauses ≥ 350 ms and measured frame width; exact hand-over between chunks, 400 ms hold after pauses → 4 tests
- [x] `CaptionStyle` JSON + four built-in styles (clean, bold, karaoke, minimal) + bundled OFL fonts (Inter 600/800, Montserrat 800/900, Latin subset, 233 KB) → preview PNGs checked by eye
- [x] Rasterization with `cosmic-text` 0.19 + `swash`, bundled fonts only: shadow, outline, active-word colour and scale, progressive highlight, pill/box → deterministic pixels test passes
- [x] SRT + WebVTT sidecars with clip-relative times (in `captions/subtitles.rs`)
- [x] Compositor decision: CPU (swscale crop/scale in YUV + caption sprites blended in YUV) instead of `wgpu` → 12 s clip in 2.6 s (4.6x realtime) on M3 Pro; no GPU dependency
- [x] Decode: any codec to 8-bit 4:2:0, display rotation applied, constant-frame-rate output from VFR sources, exact integer frame counts
- [x] Compose: crop rect per frame from an interpolated `CropPlan`, layouts static/single/split/fit (blurred background), caption sprites cached per word state
- [x] Encode: `h264_videotoolbox` High, BT.709, `allow_sw` fallback; encoders drained on EAGAIN
- [x] Mux: MP4 `+faststart`, audio interleaved by timestamp → verified moov before mdat
- [x] `export/` presets (shorts/tiktok/reels 9:16, square, portrait 4:5, landscape), `<source>-<rank>-<slug>` naming, sidecar JSON (title, description, hashtags, score, reasons, timecodes, picker, files), poster PNG, SRT + VTT
- [x] Loudness normalization: single-pass `loudnorm` I=-14 TP=-1.5 → measured -14.5 LUFS, -4.5 dBFS peak on the test clip
- [x] `choco render` and `choco run` end to end with `Static` (and `Fit`) layout → 72-min podcast to 3 finished clips in 149 s with DeepSeek V4.1 Flash; 2 of 3 picks match the editor agent's golden clips
- [x] Tests: 10 render integration tests (H.264, VP9, AV1, rotated, no-audio, fit, past-the-end) + 3 snapshot tests on composed frames (luma fingerprints, tolerance 2/255)
- [x] Render benchmark: 45–62 s clips from a VP9 640x360 source to 1080x1920 at 25 fps in 8.2–10.8 s (5.6–5.7x realtime) on M3 Pro

## M4 Reframe (weeks 7–8)

- [ ] `reframe/faces/` YuNet via `tract-onnx` on CPU; verify every op loads, else minimal ORT build as fallback; record binary size delta
- [ ] Frame sampler at 5 fps, 640 px
- [ ] IoU tracker with stable ids, scene-cut reset
- [ ] Speaker heuristic from mouth landmarks + word density, hysteresis
- [ ] Planner: `Single` with dead-zone and easing, `Split`, `Fit`, `Static` fallback
- [ ] `CropPlan` keyframes consumed by the compositor with interpolation
- [ ] Tracker tests on synthetic moving-face fixtures
- [ ] Side-by-side review against Opus Clip output on three real fixtures; notes in `AGENTS.md`

## M5 Mac app + launch (weeks 9–10)

- [ ] Tauri 2 scaffold in `app/`, depends on `choco-engine` path dep
- [ ] Commands + progress events; worker thread pool; cancellation
- [ ] Screens: Drop/Queue, Processing, Review grid, Clip editor (trim, layout, style, transcript edit, reject), Settings, First-run
- [ ] Proxy generation on import; `<video>` preview with caption overlay rendered by the engine's own rasterizer so preview and export match
- [ ] Tray item with queue status
- [ ] Onboarding: welcome + privacy line; picker choice (OpenCode Zen with key test and pre-selected models showing cost per source hour / other OpenAI-compatible provider with base URL, key and model list from `/models` / Apple on-device); speech setup (nothing on macOS 26+, else Whisper download with progress)
- [ ] First-run flow: on macOS 26+ nothing to download; elsewhere one Whisper download with progress; picker tier explained in one sentence
- [ ] Size budget check wired into `app.yml`; installer and installed size recorded in `AGENTS.md` Benchmarks
- [ ] Updater with signed manifests, `stable` and `beta` channels
- [ ] Developer ID signing + notarization script in `scripts/release/mac.sh`
- [ ] Landing page, 30-second demo video, pricing decision
- [ ] Engine repo made public with README, CONTRIBUTING (DCO), CI badge, `llms.txt`, credits to upstream libraries and to competitor projects whose ideas we adopted
- [ ] Launch: Product Hunt, r/podcasting, r/NewTubers, r/Twitch, creator Discords

## M6 Windows (weeks 11–14)

Nothing here starts before the Mac app ships. Groundwork already prepared: `backend/windows.rs` placeholder; pinned Windows LGPL FFmpeg 8.1.3 build and its sha256 noted in `scripts/ffmpeg/README.md`.

- [ ] Re-add the `windows-latest` CI job (engine build with `--no-default-features --features choco-engine/whisper`, pinned FFmpeg, tests, clippy)
- [ ] Minimal LGPL FFmpeg build for Windows (same allowlist) replacing the general BtbN build
- [ ] Whisper loop recovery: detect a collapsed loop and re-decode that window with temperature fallback or VAD (whisper.cpp Silero VAD), so no speech is lost. Measured need: ~11% of words lost on a 72-min podcast. Also benefits Macs older than macOS 26

- [ ] `backend/windows.rs`: NVIDIA, AMD, Intel all on Vulkan for whisper; CPU-only path
- [ ] whisper.cpp Vulkan build verified on at least one machine per vendor; local-llm Vulkan verified on one
- [ ] D3D11VA decode, NVENC/AMF/QSV/MF encode through FFmpeg; MF software H.264 as the fallback
- [ ] Windows installer measured against the size budget
- [ ] Compatibility matrix in `docs/windows-hardware.md`
- [ ] Tauri Windows build, NSIS installer, code signing cert
- [ ] Windows beta with 20 testers, then launch; one license covers both OSes

## M7 After launch

- [ ] v1.1: Tighten cuts (filler words + dead air removed, captions retimed)
- [ ] v1.1: NLE export (Premiere XML, Resolve EDL, CapCut draft)
- [ ] v1.1: Gaming / reaction layout (gameplay full frame, webcam PiP corner)
- [ ] v1.1: Profanity bleep from transcript words
- [ ] v1.2: Highlights reel export (ordered clips concatenated to 16:9)

- [ ] Audio diarization (evaluate pyannote-derived ONNX models under `tract`) replacing the mouth heuristic
- [ ] Semantic prompt search as an opt-in embedding model download
- [ ] Second cloud region and a status page
- [ ] YouTube ingest via optional `yt-dlp` download
- [ ] More caption styles from community PRs
- [ ] Audiogram mode for audio-only input
- [ ] Revenue model decision (paid signed build, App Store, sponsorship, or hosted tier), only after the Mac app has users

## Done

(nothing yet)

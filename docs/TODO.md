# choco.video — TODO

Companion docs: `PLAN.md` (what and why), `STACK.md` (tools), `TECH.md` (how), `COMPETITORS.md` (what others ship). Root `AGENTS.md` is the agent's memory.

Legend: `[x]` done and verified on Mac and Windows CI · `[~]` works on the dev Mac, Windows CI not yet green · `[ ]` not started. Notes after `→` record what was actually measured.

Rules: one milestone in progress at a time. A box is checked only when the thing works on the dev Mac **and** Windows CI is green (for engine items). Move finished milestones to the bottom. Keep this file short; details go in `TECH.md`.

## Now: M1 Engine skeleton (weeks 1–2)

- [x] `git init`, `.gitignore` (target/, fixtures media, models, vendor/, *.chocoproj), `LICENSE` (MIT) in `engine/`
- [~] Cargo workspace: `choco-engine`, `choco`, `choco-ffi`; shared lints; `deny.toml` with license allowlist and bans (x264, x265, openh264, ort) → builds clean, clippy clean; `cargo deny` not yet run
- [~] `model/` types from `TECH.md §2` with serde round-trip tests → 2 tests pass
- [~] `backend/`: trait, `detect()`, `mac.rs` (AppleSpeech on 26+, Metal, VideoToolbox), `windows.rs` (stub that reports Whisper on CPU), `cpu.rs`, `recommended_models()` by memory → detects M3 Pro, 18 GB, macOS 27.0.1, AppleSpeech → Whisper Metal
- [~] Release profile in workspace `Cargo.toml` (fat LTO, 1 codegen unit, abort, strip, opt-level z with overrides); arm64-only Mac target; toolchain pinned to 1.99.1
- [~] FFmpeg minimal LGPL: build recipe with `--disable-everything` plus an allowlist (mov/mp4/matroska/webm demux, h264/hevc/vp9/aac/opus/mp3 decode, hw encoders, aac encode, loudnorm/aresample/scale filters, VideoToolbox/D3D11VA/NVENC/AMF/QSV/MF hwaccel), `--disable-gpl --disable-nonfree`; pin version; record the dylib sizes in `AGENTS.md` Benchmarks; document in `scripts/ffmpeg/README.md`; Windows link path verified in CI → Mac done: 8.1.3, 7.5 MB of dylibs, LGPL 2.1 confirmed, `@rpath` install names; Windows link path pending (no CI yet)
- [~] `ingest/`: `probe`, `extract_audio` (16 kHz mono f32), `thumbnail` → probes MP4, MOV, MKV/WebM (VP9, Opus), audio-only M4A; rotated phone clip thumbnail matches FFmpeg's reference pixel for pixel
- [~] `transcribe/`: `Transcriber` trait + shared cleanup (sentence re-split, hallucination filter, clamp)
- [~] `transcribe/apple.rs`: Swift helper (168 KB) for SpeechAnalyzer with word timings, built by `build.rs` on macOS 26+ → 45 locales; 0% WER on the synthetic script, 20x realtime, zero download
- [~] `transcribe/whisper.rs`: `whisper-rs` with Metal/CPU, word timestamps from tokens, quantized turbo default → 0% WER on the synthetic script; fixed a progress deadlock caused by whisper-rs leaking its callback. whisper.cpp windows long audio itself, so no manual overlap windowing
- [~] `models_registry/`: manifests pinned to a Hugging Face revision, resumable download, sha256 verify (upstream publishes sha256, so not blake3), `choco models list|pull|rm`
- [~] CLI: `choco probe`, `choco transcribe`, `choco thumbnail`, `choco doctor` with fix hints, `choco models`, `--json` → doctor reports ready
- [x] `fixtures/generate.sh` (synthetic, macOS `say` voices with known text: speech, rotated phone clip, silent, two voices, audio-only, VP9/Opus MKV) and `fixtures/fetch.sh` + `sources.tsv` (5 CC BY 3.0 long-form recordings from Wikimedia Commons, 48–72 min, sha1-verified)
- [~] CI `engine.yml` on `macos-15` + `windows-latest` + cargo-deny: release build, test, clippy, size report → written; not run yet because nothing has been pushed. `cargo deny check` passes locally. Windows links a pinned BtbN LGPL 8.1.3 build until M6
- [~] Size budget check script (`scripts/size-check.sh`) that fails over 25 MB installer / 50 MB installed; measures the engine part until the app exists at M5
- [ ] Transcribe a 60-minute podcast on the M3 Pro with Apple speech and with whisper turbo-q5; record wall time, peak memory and word-timing agreement in `AGENTS.md` Benchmarks

## Next: M2 Picker (weeks 3–4)

- [ ] `highlights/windows.rs` candidate generation with pre-filters
- [ ] Complete-thought boundaries: grow/trim to terminal punctuation + silence gap; test that no candidate starts or ends mid-sentence
- [ ] Audio energy + pause features (RMS spikes, laughter-like bursts, long pauses) computed from the 16 kHz wav, fed into the shortlist prior
- [ ] `highlights/search.rs` shortlist: quality prior, BM25 prompt mode, Jaccard dedupe. No embedding model.
- [ ] `highlights/picker/` `Picker` trait; `Hosted` client; `Byok`; `AppleFoundation` shim; batch of 8 with episode context
- [ ] Prompt v1 in `highlights/picker/prompts/score_v1.md` and `summarize_v1.md`; provenance hashing
- [ ] `cloud/`: axum service with `/v1/score` (Haiku), `/v1/summarize` and `/v1/title` (Opus), `/v1/quota`; measure tokens and cost per source hour against the $0.03 budget; license-key auth against Lemon Squeezy; per-key usage counter; Anthropic call with structured output and server-side fallbacks; no transcript logging; Dockerfile; deployed to one small host; `cloud.yml`
- [ ] `rank.rs` NMS + diversity penalty + score floor; `--n` is a max, content decides the count
- [ ] `choco highlights` with `--n`, `--prompt`, `--picker hosted|apple|byok|local`
- [ ] Golden set: 10 fixtures, 5–8 human-picked ranges each, in `eval/golden.json`
- [ ] `choco eval` runner, baseline numbers per tier recorded in `AGENTS.md` Benchmarks
- [ ] Compare Apple speech vs whisper on the golden set per language; decide per-language fallback list
- [ ] `LocalLlama` behind `local-llm` feature with GBNF grammar; pick the default 4B model (Qwen3-4B vs Gemma-3-4B class) by eval and speed; model is opt-in download only
- [ ] BYOK key storage via `keyring`; Settings copy that states "transcript text only" next to every picker option

## M3 Captions + render (weeks 5–6)

- [ ] Word timing accuracy to ≤ 50 ms median against `fixtures/media/aligned.words.json` (`fixtures/timing.py`). Measured at M1: Apple 103 ms median, 100 ms early; whisper 256 ms, 232 ms early. Try whisper DTW timestamps first, then per-engine offset calibration; re-measure on a second voice and a real recording
- [ ] `captions/` chunker with font-measured line breaks
- [ ] `CaptionStyle` JSON + four built-in styles + bundled OFL fonts
- [ ] Atlas rasterization with `cosmic-text` + `swash`, active-word variants
- [ ] `render/decode.rs` with hwaccel + bounded channel
- [ ] `render/compose.rs` `wgpu` shader: crop, scale, blur background, caption blit, watermark
- [ ] `render/encode.rs` VideoToolbox on Mac; software fallback is the OS encoder, never libx264 or openh264
- [ ] `render/mux.rs` MP4 faststart, audio copy/re-encode
- [ ] `export/` presets (9:16, 1:1, 4:5, 16:9), naming, sidecar JSON, PNG poster, SRT + VTT per clip, watermark flag
- [ ] Loudness normalization to -14 LUFS on the clip audio (`loudnorm` two-pass or single-pass)
- [ ] `choco render` and `choco run` end to end with `Static` layout
- [ ] Snapshot tests on synthetic fixtures, green on Mac and Windows CI
- [ ] Render benchmark: 60 s clip wall time on M3 Pro, recorded

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
- [ ] Licensing: Lemon Squeezy activation, offline grant, 14-day grace, watermark toggle; the same key authenticates the hosted picker
- [ ] First-run flow: on macOS 26+ nothing to download; elsewhere one Whisper download with progress; picker tier explained in one sentence
- [ ] Size budget check wired into `app.yml`; installer and installed size recorded in `AGENTS.md` Benchmarks
- [ ] Updater with signed manifests, `stable` and `beta` channels
- [ ] Developer ID signing + notarization script in `scripts/release/mac.sh`
- [ ] Landing page, 30-second demo video, pricing decision
- [ ] Engine repo made public with README, CONTRIBUTING (DCO), CI badge, `llms.txt`, credits to upstream libraries and to competitor projects whose ideas we adopted
- [ ] Launch: Product Hunt, r/podcasting, r/NewTubers, r/Twitch, creator Discords

## M6 Windows (weeks 11–14)

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
- [ ] Hosted version research, only if v1 has paying users

## Done

(nothing yet)

# choco.video — TODO

Companion docs: `PLAN.md` (what and why), `STACK.md` (tools), `TECH.md` (how), `COMPETITORS.md` (what others ship). Root `AGENTS.md` is the agent's memory.

Rules: one milestone in progress at a time. A box is checked only when the thing works on the dev Mac **and** Windows CI is green (for engine items). Move finished milestones to the bottom. Keep this file short; details go in `TECH.md`.

## Now: M1 Engine skeleton (weeks 1–2)

- [ ] `git init`, `.gitignore` (target/, fixtures media, models, *.chocoproj), `LICENSE` (MIT) in `engine/`
- [ ] Cargo workspace: `choco-engine`, `choco`, `choco-ffi`; shared lints; `deny.toml` with license allowlist (MIT, Apache-2.0, BSD-2/3, ISC, Unlicense, LGPL-2.1+ dynamic, OFL for fonts)
- [ ] `model/` types from `TECH.md §2` with serde round-trip tests
- [ ] `backend/`: trait, `detect()`, `mac.rs` (Metal, VideoToolbox, CoreML), `windows.rs` (stub that reports CPU), `cpu.rs`, `recommended_models()` by memory
- [ ] FFmpeg LGPL: pick prebuilt source or build recipe, pin version, document in `scripts/ffmpeg/README.md`, link via `ffmpeg-next` on Mac; Windows link path verified in CI
- [ ] `ingest/`: `probe`, `extract_audio` (16 kHz mono f32), `thumbnail`
- [ ] `transcribe/`: `whisper-rs` with Metal, windowed decode with overlap, sentence re-split, hallucination filter, word timestamps
- [ ] `models_registry/`: manifest format, download with resume + blake3, storage dir, `choco models list|pull|rm`
- [ ] CLI: `choco probe`, `choco transcribe`, `choco doctor` with fix hints, `--json`
- [ ] `fixtures/generate.sh` (synthetic) and `fixtures/fetch.sh` (CC media list with licenses)
- [ ] CI `engine.yml` on `macos-14` + `windows-latest`: build, test, clippy, deny
- [ ] Transcribe a 60-minute podcast on the M3 Pro and record wall time and peak memory in `AGENTS.md` Benchmarks

## Next: M2 Picker (weeks 3–4)

- [ ] `highlights/windows.rs` candidate generation with pre-filters
- [ ] Complete-thought boundaries: grow/trim to terminal punctuation + silence gap; test that no candidate starts or ends mid-sentence
- [ ] Audio energy + pause features (RMS spikes, laughter-like bursts, long pauses) computed from the 16 kHz wav, fed into the shortlist prior
- [ ] Embedding backend decision: llama.cpp embeddings vs `fastembed`; record in `AGENTS.md` Decisions
- [ ] `highlights/embed.rs` shortlist, default and prompt mode
- [ ] `highlights/llm/` `Picker` trait, `LocalLlama` with GBNF grammar, batch of 8 with episode context
- [ ] Prompt v1 in `highlights/llm/prompts/score_v1.md`; provenance hashing
- [ ] `rank.rs` NMS + diversity penalty + score floor; `--n` is a max, content decides the count
- [ ] `choco highlights` with `--n`, `--prompt`, `--picker`
- [ ] Golden set: 10 fixtures, 5–8 human-picked ranges each, in `eval/golden.json`
- [ ] `choco eval` runner, baseline numbers recorded in `AGENTS.md` Benchmarks
- [ ] Decide default local model (Qwen3-8B vs Llama-3.1-8B) by eval and speed
- [ ] `AnthropicCloud` picker behind `cloud` feature with structured output; keychain storage via `keyring`
- [ ] `AppleFoundation` picker behind `apple-fm` feature, Swift shim, macOS 26 availability gate

## M3 Captions + render (weeks 5–6)

- [ ] `captions/` chunker with font-measured line breaks
- [ ] `CaptionStyle` JSON + four built-in styles + bundled OFL fonts
- [ ] Atlas rasterization with `cosmic-text` + `swash`, active-word variants
- [ ] `render/decode.rs` with hwaccel + bounded channel
- [ ] `render/compose.rs` `wgpu` shader: crop, scale, blur background, caption blit, watermark
- [ ] `render/encode.rs` VideoToolbox on Mac; software fallback that is not libx264
- [ ] `render/mux.rs` MP4 faststart, audio copy/re-encode
- [ ] `export/` presets (9:16, 1:1, 4:5, 16:9), naming, sidecar JSON, PNG poster, SRT + VTT per clip, watermark flag
- [ ] Loudness normalization to -14 LUFS on the clip audio (`loudnorm` two-pass or single-pass)
- [ ] `choco render` and `choco run` end to end with `Static` layout
- [ ] Snapshot tests on synthetic fixtures, green on Mac and Windows CI
- [ ] Render benchmark: 60 s clip wall time on M3 Pro, recorded

## M4 Reframe (weeks 7–8)

- [ ] `reframe/onnx/` YuNet via `ort`, CoreML provider on Mac, CPU elsewhere
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
- [ ] Proxy generation on import; `<video>` preview with live caption overlay
- [ ] Tray item with queue status
- [ ] Licensing: Lemon Squeezy activation, offline grant, 14-day grace, watermark toggle
- [ ] Updater with signed manifests, `stable` and `beta` channels
- [ ] Developer ID signing + notarization script in `scripts/release/mac.sh`
- [ ] Landing page, 30-second demo video, pricing decision
- [ ] Engine repo made public with README, CONTRIBUTING (DCO), CI badge, `llms.txt`, credits to upstream libraries and to competitor projects whose ideas we adopted
- [ ] Launch: Product Hunt, r/podcasting, r/NewTubers, r/Twitch, creator Discords

## M6 Windows (weeks 11–14)

- [ ] `backend/windows.rs`: NVIDIA (CUDA optional, Vulkan default), AMD (Vulkan + AMF), Intel (Vulkan + QSV), CPU-only path
- [ ] whisper.cpp and llama.cpp Vulkan builds verified on at least one machine per vendor
- [ ] D3D11VA decode, NVENC/AMF/QSV/MF encode through FFmpeg
- [ ] DirectML provider for ONNX
- [ ] Compatibility matrix in `docs/windows-hardware.md`
- [ ] Tauri Windows build, NSIS installer, code signing cert
- [ ] Windows beta with 20 testers, then launch; one license covers both OSes

## M7 After launch

- [ ] v1.1: Tighten cuts (filler words + dead air removed, captions retimed)
- [ ] v1.1: NLE export (Premiere XML, Resolve EDL, CapCut draft)
- [ ] v1.1: Gaming / reaction layout (gameplay full frame, webcam PiP corner)
- [ ] v1.1: Profanity bleep from transcript words
- [ ] v1.2: Highlights reel export (ordered clips concatenated to 16:9)

- [ ] Audio diarization (evaluate pyannote-derived ONNX models) replacing the mouth heuristic
- [ ] YouTube ingest via optional `yt-dlp` download
- [ ] More caption styles from community PRs
- [ ] Audiogram mode for audio-only input
- [ ] Hosted version research, only if v1 has paying users

## Done

(nothing yet)

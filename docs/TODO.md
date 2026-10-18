# choco.video — TODO

Companion docs: `PLAN.md` (what and why), `STACK.md` (tools), `TECH.md` (how), `COMPETITORS.md` (what others ship). Root `AGENTS.md` is the agent's memory.

Legend: `[x]` works and was measured on the dev Mac (M3 Pro, 18 GB, macOS 27) · `[~]` partly done, note says what is missing · `[ ]` not started. Notes after `→` are measurements from the current build, not estimates. **Mac only until the Mac app ships; Windows is M6.** Audited 2026-10-18: every `[x]` below was re-run on the current build (87 engine tests, clippy, license gates, frontend check, size check, three full pipeline runs).

## Where things stand

- The product works end to end on real files, from the app and from the CLI: a 14-minute video chosen in the app became 3 speaker-followed, captioned clips; a 48-minute show became 4 clips in 226 s including transcription; a 72-minute podcast in 149 s with a reused transcript.
- Bundle: 20 MB installed, 9.9 MB DMG (budgets 50 / 25). First run on macOS 26+ downloads nothing.
- Not done: human review of the golden set, the final model recommendation (eval reruns in progress), signing and notarization, updater, launch. Windows (M6) and the local-model tier are untouched by design.

## Done: M1 Engine skeleton

- [x] Cargo workspace (`choco-engine`, `choco`, `choco-ffi`), toolchain 1.99.1 pinned, size-first release profile, `cargo deny` allowlist with bans on x264/x265/openh264/ort → bans, licenses, sources pass (advisory DB fetch needs network)
- [x] Minimal LGPL FFmpeg 8.1.3 (`--disable-everything` + allowlist) with dav1d 1.5.4 static for AV1 and VideoToolbox hwaccel → 8.3 MB of dylibs, LGPL 2.1 confirmed, `@rpath` install names; recipe `scripts/ffmpeg/build-mac.sh`
- [x] `model/` serde contracts; `backend/` detection (AppleSpeech on 26+, Metal, VideoToolbox; CPU fallback)
- [x] `ingest/`: probe from codec parameters (no decoder needed), display rotation, content hash, 16 kHz WAV extraction, rotated thumbnails → 7 format tests: H.264/AAC MP4, rotated phone clip, no-audio, audio-only M4A, VP9/Opus MKV, AV1/Opus WebM, float WAV
- [x] `transcribe/`: Apple SpeechAnalyzer via the `choco-apple` helper process (zero download) and whisper.cpp quantized turbo (~550 MB runtime download); shared cleanup (loop collapse up to 16 words, clamp, resegment); per-engine timing bias correction → 72-min podcast: Apple 112 s (42x realtime), whisper 267 s (17x, 1.7 GB, loses ~11% of words to loops)
- [x] `models_registry/`: manifests pinned to a Hugging Face revision, resumable download, sha256 → `choco models list|pull|rm`
- [x] CLI: `probe`, `transcribe`, `thumbnail`, `doctor` with fix hints, `models`, `provider models|test`, `--json`
- [x] Fixtures: synthetic set (`generate.sh`, macOS voices, known text, exact word onsets) and five CC BY 3.0 recordings from Wikimedia Commons (48–72 min; VP8, VP9, AV1), sha1-verified
- [x] CI workflow (`macos-15` + cargo-deny) and `scripts/size-check.sh` → written; CI has not run because nothing is pushed

## Done: M2 Picker

- [x] Candidate windows: complete-thought boundaries (sentence end or ≥ 700 ms pause), 15–90 s, several lengths per start, dead-air/sparse/disfluency filters → 2,925 windows on the 72-min podcast; 3 tests
- [x] Audio features (loudness z-score, spikes, pauses) feeding the shortlist prior
- [x] Shortlist: quality prior, BM25 prompt mode with synonym and stem expansion (stop words never become terms), near-duplicate removal, overlap limit → golden clips reachable by the model: 60 @ 85% overlap 0.47 → 80 @ 20% 0.93 (default)
- [x] Picker `Llm` trait: batching, map-reduce summary (non-fatal if it fails), strict parse with key repair (UTF-8 safe), bare arrays and fenced replies, per-entry leniency, retry, re-ask for skipped ids, batches split when a model's context overflows, fail-fast on config errors and on two consecutive failed batches, cancellation between batches
- [x] OpenAI-compatible client (`ureq`): `json_schema` → `json_object` fallback remembered per client, 429/5xx backoff, explicit User-Agent (OpenCode's Cloudflare rejects defaults), content-as-parts, truncation detection, token usage per role, `list_models`, `test_connection` → verified live on OpenCode Zen (48 models; wrong key → 401, wrong model → 400)
- [x] Apple on-device tier through the helper: guided generation, guardrail false positives retried under permissive guardrails, stderr drained on a thread, context overflow mapped and split → 0 declined on the 72-min podcast; in-app run on a 14-min video: 20/23 scored before the split fix, now none lost
- [x] Rank: overlap and same-content suppression, score floor, max count ("content decides the count")
- [x] `choco highlights` (`--prompt`, `--picker auto|apple|provider`, `--transcript` reuse, diagnostic report with token usage)
- [x] `choco eval`: precision@5 (distinct golden clips in the top five, over five), recall@10, IoU, shortlist-only mode, per-run results with tokens and source hours → `eval/results/`
- [~] Golden set: 5 fixtures × 8 clips drafted by editor agents from transcripts, validated against segment boundaries; notes and alternates in `eval/REVIEW.md` → **needs a human to watch and confirm** (`reviewed_by_human: false`); eval numbers are provisional until then
- [~] Model recommendation on OpenCode Zen (budget ≤ 3¢ per source hour) → measured so far (P@5, old definition, single runs): MiniMax M3 0.48 / 0.40, DeepSeek V4.1 Flash 0.44 / 0.32, Qwen 3.8 Flash 0.28, GLM 5.3 Flash timed out on summaries (fixed). Running five evals in parallel made every batch fail on four of five runs (rate limits; the build at the time hid the reason). A sequential rerun with the fixed build is in progress; `OPENCODE_RECOMMENDED` in `openai_compat.rs` carries the provisional table shown at onboarding
- [ ] Compare Apple speech vs whisper per language on the golden set; per-language fallback list (Apple speech turned a Dutch reading into nonsense when run as en-US)
- [ ] `LocalLlama` tier (`local-llm` feature, 4B GGUF, opt-in download) — deferred, not needed for the Mac launch

## Done: M3 Captions + render

- [x] Captions: ≤ 4-word chunks on clause punctuation, pauses and measured width; four presets (clean, bold, karaoke, minimal); bundled OFL fonts (233 KB); deterministic rasterizer (shadow, outline, active-word colour and scale, progressive highlight, pill); SRT + VTT sidecars → pixel-identical output test
- [x] Render: constant-frame-rate from VFR sources, any codec normalised to 8-bit 4:2:0, display rotation, interpolated `CropPlan` (static / single / split / fit with blurred background), caption sprites blended in YUV, `h264_videotoolbox` High with `allow_sw`, AAC at 48 kHz with `loudnorm` −14 LUFS (measured −14.5), MP4 `+faststart` → 10 integration tests + 3 snapshot tests; 45–62 s clips render at 5–6x realtime
- [x] Export: size presets (9:16, 1:1, 4:5, 16:9), `<source>-<rank>-<slug>` naming, poster PNG, SRT, VTT, metadata JSON (title, description, hashtags, score, reasons, timecodes, picker provenance); re-export a clip with another style/size/framing without re-transcribing
- [x] `choco render` and `choco run`; engine `pipeline` module shared by CLI and app (temp files cleaned on every path, cancellation between stages, batches and clips)
- [~] Word timing: per-engine bias correction (Apple +100 ms, whisper +260 ms) verified on a held-out voice → remaining scatter median 52–95 ms; the ≤ 50 ms target is not met. Energy-onset snapping was tried and rejected. Next idea: whisper DTW timestamps

## Done: M4 Reframe

- [x] YuNet face detection (OpenCV Zoo, MIT, 233 KB, compiled in) on `tract` at 640×640 → 22 ms per frame on CPU
- [x] 5 fps sampling through the shared decode path; IoU tracking with scene-cut reset; mouth-motion speaker heuristic with a 1 s switch lead
- [x] Planner: speaker follow with dead zone and eased moves, per-shot following for edited multi-camera footage, split screen when two people share the frame (captions move to the seam), centre crop with no faces → 6 tests; checked by eye on four real recordings
- [ ] Side-by-side against Opus Clip on three fixtures (needs an Opus Clip account; human task)

## Now: M5 Mac app + launch

- [x] Tauri 2 + Svelte 5 app over the engine: onboarding (privacy line → picker with live key test and pre-selected models showing cents per hour → speech setup), home (drop or choose a file, prompt, count, style, size, framing, output folder), job progress with cancel, results grid (poster, inline playback, Show in Finder, copy caption, restyle), settings → used for a real run by the project owner
- [x] Keychain-only key storage; key saved only after the model list succeeds; strict CSP; scoped asset protocol; capabilities limited to dialogs, reveal, and OpenCode/GitHub links; app `deny.toml` passes
- [x] Release bundle: `scripts/build-app.sh` (Apple helper sidecar, six FFmpeg dylibs in `Contents/Frameworks`, no build-machine rpath in release) → `choco.video.app` 20 MB, DMG 9.9 MB, launches and stays up
- [x] README, CONTRIBUTING (DCO), `llms.txt`; landing page in `site/` with a live canvas demo
- [ ] Golden set human review (you): watch each clip in `eval/REVIEW.md`, fix or replace, set `reviewed_by_human`; then rerun `choco eval` per model and freeze `OPENCODE_RECOMMENDED`
- [ ] Push to GitHub so CI runs; fix whatever the first run reports
- [ ] Developer ID signing + notarization (needs your Apple Developer account); without it the DMG only runs after right-click → Open
- [ ] Tauri updater with signed manifests and a release channel
- [ ] App icon (the bundle still carries Tauri's placeholder icon)
- [ ] First-launch guidance when Apple Intelligence is off and no key is set (today the Apple option is disabled with a one-line reason)
- [ ] Demo video and launch posts (Product Hunt, r/podcasting, r/NewTubers, creator Discords)

## M6 Windows (after the Mac launch)

Groundwork: `backend/windows.rs` placeholder; pinned Windows LGPL FFmpeg build and sha256 in `scripts/ffmpeg/README.md`.

- [ ] Whisper loop recovery (re-decode collapsed windows with temperature fallback or VAD): Windows depends on whisper, which loses ~11% of words on real conversation today
- [ ] `windows-latest` CI job; minimal LGPL FFmpeg for Windows; Vulkan whisper; D3D11VA decode; NVENC/AMF/QSV/MF encode
- [ ] NSIS installer, code signing, hardware matrix, beta with 20 testers

## M7 After launch

- [ ] Tighten cuts (filler words + dead air removed, captions retimed)
- [ ] NLE export (Premiere XML, Resolve EDL, CapCut draft)
- [ ] Gaming / reaction layout; profanity bleep; highlights reel
- [ ] Audio diarization replacing the mouth heuristic
- [ ] YouTube ingest via optional `yt-dlp` download
- [ ] Semantic prompt search as an opt-in embedding model
- [ ] Revenue model decision (paid signed build, App Store, sponsorship), only once the app has users

## Needs you

1. Watch the 40 golden clips (`eval/REVIEW.md`) and correct them. Every picker number depends on this.
2. Push to GitHub and tell me, so CI gets its first run.
3. An Apple Developer account for signing, when you want a DMG others can open without the right-click dance.
4. An app icon, or say the word and I draft one.

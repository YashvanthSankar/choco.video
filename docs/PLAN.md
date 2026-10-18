# choco.video — Plan

> Drop a long video on your Mac or PC. Get ranked, captioned, speaker-centered vertical shorts a minute later. Your video never leaves the machine, and the app is a 20 MB download.

## 1. Aim

**One sentence:** choco.video is a small desktop app, built on an open-source engine, that turns long-form video into short vertical clips with the video processed on-device and the heavy AI judgment run where it is cheapest, sold for a one-time price instead of a credit-metered subscription. macOS ships first, Windows follows on the same engine.

**Size principle:** the installer is about 20 MB and nothing heavy ships inside it. Video decoding, face tracking, captions and rendering are local and small. Speech recognition uses the OS engine where one exists and a one-time download where it does not. The highlight picker, the only step that needs a large model, runs on a hosted frontier model by default, with offline fallbacks. We never trade output quality for size; we trade *where* the model runs.

**Why it exists:** Opus Clip and Vizard charge $15–29/month, meter by credits, watermark free exports, expire clips after 3 days, and upload every file to their cloud. Every review of both products complains about the same three things: credits, watermarks, expiry. Modern laptop GPUs and Apple Silicon can run the whole pipeline for free, which makes "no credits, your video stays local, one-time price" a structural advantage rather than a feature.

**Who it is for:** Solo creators, podcasters, and streamers who post clips weekly. Not agencies, not teams. They decide alone and pay by card.

**Platform order and why:**
1. **macOS first.** Every M-series Mac has the same GPU and Neural Engine family, so one build behaves the same everywhere. Testing is one machine. Creators on Mac pay for polished native tools.
2. **Windows second, on the same engine.** The engine is written to compile on both from day one, with platform-specific acceleration behind a backend interface. The Windows shell is added once the Mac app has paying users. Windows hardware is varied (NVIDIA, AMD, Intel, no GPU), so the Windows release needs a compatibility matrix and a CPU fallback.

**What winning looks like:**
- Month 3: Mac v1 shipped, signed and notarized, on Product Hunt and the creator subreddits. Engine repo public with a working CLI on both OSes.
- Month 5: Windows v1 shipped from the same engine.
- Month 6: a few hundred paying users, clip quality that people post without re-editing, engine repo past 1k stars.
- Month 12: a real living from it, or a clear signal to move to the next product on the same engine.

**Non-goals (explicit, for v1):** B-roll, auto-posting, scheduling, team workspaces, Linux app (CLI only), translation, dubbing, AI voice, thumbnails, bundling any model inside the installer. Uploading video or audio anywhere, ever.

## 2. Full functionality

### 2.1 Input
- Drag and drop or open: `.mp4`, `.mov`, `.m4v`, `.mkv`, `.webm`. Decoded through bundled FFmpeg libraries, so container support is uniform on both OSes.
- Audio-only input (`.mp3`, `.m4a`, `.wav`) produces audiogram-style clips with a waveform background. Low priority, cheap to add.
- YouTube URL. **Not in v1.** v1.1 bundles a standalone `yt-dlp` binary (Unlicense) as an optional download so the app itself stays free of Python.
- Batch queue: several files, processed sequentially, each a separate project.

### 2.2 Transcription
- **Mac, macOS 26+:** Apple's on-device SpeechAnalyzer / SpeechTranscriber. Word timestamps, language auto-detect, runs on the Neural Engine, zero download. Default on current Macs.
- **Windows, and Macs before macOS 26:** Whisper via whisper.cpp, `large-v3-turbo` quantized (~550 MB), downloaded on first run with progress and checksum. `base` (~60 MB) offered for a fast preview on weak machines.
- Both engines produce the same `Transcript` type, so everything downstream is identical. The golden set measures both; where Apple's engine falls short on a language, the app offers the Whisper download for that language.
- Transcript is editable in the review screen; edits propagate to captions.

### 2.3 Highlight selection ("the picker")
- Splits transcript into candidate windows (15 to 90 seconds) at sentence boundaries and silence gaps.
- Scores each candidate on: hook strength (first 3 seconds), self-containment, payoff, emotional charge, quotability, and absence of dead air. Produces `score 0–100` and `reasons[]`.
- Generates a hook title and a one-line description per clip.
- Prompt mode: "find every moment about pricing" filters candidates by semantic match before scoring.
- De-duplicates overlapping candidates. Returns top N (default 10) ranked.
- Model ladder, user-selectable: hosted choco picker (default, frontier model, transcript text only), Apple on-device model on macOS 26+ (zero download, offline), bring-your-own-key, and an optional ~2.5 GB local model download for fully offline use. See §5.

### 2.4 Reframe 16:9 → 9:16
- Face detection and tracking on sampled frames using a small ONNX face model (same model on both OSes, so output is identical). Apple Vision is an optional Mac backend if it proves measurably better.
- Layout modes: `single` (follow one face), `split` (two faces, stacked), `static` (center crop, fallback), `fit` (letterbox over blurred background).
- Speaker switching: when two faces are present and layout is `single`, switch to whoever is talking. v1 uses a mouth-movement heuristic from landmarks; v1.5 adds audio diarization.
- Camera smoothing: keyframed crop rectangles with easing, a dead-zone so the frame does not chase small head motion, a hard cut on scene change instead of a pan.
- Output: `CropPlan`, a list of time-stamped crop rects and layout mode, fully inspectable and editable.

### 2.5 Captions
- Word-synced from transcript timestamps, grouped into 2–4 word chunks for readability.
- Four built-in styles for v1: `clean`, `bold` (highlight on active word), `karaoke` (fill sweep), `minimal`.
- Customizable: font, size, position, colors, active-word emphasis. Stored as a `CaptionStyle` JSON that can be shared.
- Rendered identically on both OSes by drawing with the same text rasterizer; no platform text APIs in the render path.
- Emoji and filler-word stripping optional.

### 2.6 Review and edit
- Grid of ranked clips with score, title, duration, and a thumbnail.
- Per-clip: play preview, trim start/end with word snapping, change layout mode, change caption style, edit transcript text, reject.
- Project-level: re-run picker with a different prompt or count without re-transcribing.

### 2.7 Render and export
- Rendered by decoding the source range, applying the crop plan and captions frame by frame on the GPU, and encoding with the platform hardware encoder (VideoToolbox on Mac; NVENC, AMF, QSV, or Media Foundation on Windows; software fallback with an LGPL-safe encoder).
- 1080x1920, 30 or 60 fps matching source. H.264 default, HEVC optional.
- Presets: TikTok, Reels, Shorts (same spec today, kept as presets for future divergence), plus `custom`.
- Output naming: `{source}-{rank:02}-{slug}.mp4` plus a JSON sidecar with title, description, hashtags, score, and timecodes.
- Export all or selected, progress per clip, runs in the background.

### 2.8 Project files
- `.chocoproj` is a directory: `project.json`, cached transcript, candidates, crop plans, thumbnails. Every stage is cached, so changing a caption style re-renders without re-transcribing. Portable between Mac and Windows.

### 2.9 App shell
- Tauri 2 window app with a tray/menubar item showing queue progress. One UI codebase for both OSes.
- First-run on a current Mac: nothing to download. Drop a video. On Windows or older Macs: one speech-model download (~550 MB) with progress. No account; the license key is the only credential.
- Settings: models, hardware backend, default style, export folder, BYOK key (OS keychain), telemetry opt-in (off).
- Auto-update via Tauri's updater. Crash reports opt-in, local only.
- License: free trial with a watermark on export. Paid key removes it. Validation works offline after first activation.

### 2.10 Parity features adopted from open-source competitors
Studied in `COMPETITORS.md`. These are cheap, users expect them, and shipping them removes every "but X has it" objection.
- **Complete-thought clips** (chopify): candidate boundaries are grown or trimmed to a sentence with terminal punctuation, never mid-thought. v1, in the picker.
- **Keyword search** (chopify, Klipzy): covered by prompt mode. v1.
- **Audio energy as a picker feature** (Klipzy): loudness spikes and laughter feed the scoring prior. v1.
- **Multi-aspect export** (chopify, Klipzy): 9:16 default, plus 1:1 and 4:5 as presets, same crop planner. v1, trivial in the shader.
- **SRT / VTT sidecar** (Klipzy): per clip, from the caption track. v1.
- **Poster thumbnail + meta JSON** (chopify): already planned as the sidecar; add a PNG poster. v1.
- **Loudness normalization to -14 LUFS** (chopify): FFmpeg `loudnorm` on the audio range. v1.
- **Doctor command** (AutoClip, chopify): `choco doctor` already planned. v1.
- **Tighten cuts** (chopify, Klipzy, OpenClip Studio): remove filler words and dead air with captions retimed. v1.1.
- **NLE export** (Klipzy, ViralCutter): Premiere XML, Resolve EDL, CapCut draft. v1.1, strong for pro users.
- **Gaming / reaction layout** (Klipzy): gameplay full-frame with webcam picture-in-picture. v1.1 layout mode.
- **Profanity bleep** (Klipzy): word-level mute from transcript. v1.1.
- **Highlights reel** (Klipzy): concatenate chosen clips into one 16:9 reel. v1.2.
- **SFX cue points** (OpenClip Studio): deliberately skipped. Gimmicky, and licensing sound packs is a distraction.

## 3. How the pieces connect

```
                 ┌──────────────────────────────────────────────────────────────┐
                 │                 choco-engine (Rust crate, MIT, cross-platform) │
                 │                                                              │
  file/URL ──▶ Ingest ──▶ MediaAsset                                            │
                 │            │                                                 │
                 │            ▼                                                 │
                 │       Transcribe (OS speech | whisper.cpp) ──▶ Transcript    │
                 │            │                          │                      │
                 │            │                          ▼                      │
                 │            │                   Highlights (hosted|apple|byok|local) ──▶ [Candidate]
                 │            │                          │                      │
                 │            ▼                          ▼                      │
                 │       Reframe (tract + YuNet) ◀── per candidate time range   │
                 │            │                                                 │
                 │            ▼                                                 │
                 │        CropPlan          Captions (Transcript + Style) ──▶ CaptionTrack
                 │            │                          │                      │
                 │            └──────────┬───────────────┘                      │
                 │                       ▼                                      │
                 │              Render (FFmpeg decode → GPU compose → HW encode) ──▶ .mp4 + .json
                 │                       │                                      │
                 │                       ▼                                      │
                 │                    Export (presets, naming)                   │
                 │                                                              │
                 │   platform backends: {AppleSpeech | Whisper Metal/Vulkan/CPU} │
                 │                      {VideoToolbox | NVENC/AMF/QSV/MF}        │
                 └──────────────────────────────────────────────────────────────┘
                                         ▲                 ▲
                                         │                 │
                               choco CLI (MIT)       Choco app (Tauri 2, paid)
                            mac · win · linux            mac · win
```

**Data contracts between stages are plain serde structs.** Every stage takes a value in and returns a value out, no shared mutable state. That is what lets the CLI, the app, the tests, and the project cache share one code path, and what makes the Windows port a shell-and-backend job instead of a rewrite.

| Stage | Input | Output | Crate module |
|---|---|---|---|
| Ingest | file path | `MediaAsset` (duration, fps, size, streams, probe) | `engine/src/ingest/` |
| Transcribe | `MediaAsset` | `Transcript` (segments → words, timestamps, confidence) | `engine/src/transcribe/` |
| Highlights | `Transcript`, `PickerOptions` | `Vec<Candidate>` (range, score, reasons, title, hook) | `engine/src/highlights/` |
| Reframe | `MediaAsset`, range, `ReframeOptions` | `CropPlan` (keyframed rects, layout) | `engine/src/reframe/` |
| Captions | `Transcript`, range, `CaptionStyle` | `CaptionTrack` (timed chunks, styling) | `engine/src/captions/` |
| Render | `MediaAsset`, `CropPlan`, `CaptionTrack`, `RenderOptions` | `.mp4` path | `engine/src/render/` |
| Export | rendered clips, `ExportPreset` | files + sidecar JSON | `engine/src/export/` |
| Pipeline | all of the above | `Project` (orchestration, caching, progress events) | `engine/src/pipeline/` |
| Backends | hardware detection, acceleration selection | `Backend` trait impls per OS | `engine/src/backend/` |

## 4. Tech stack

Moved to `STACK.md`. Summary: Rust engine (OS speech or whisper.cpp, tract + YuNet, minimal FFmpeg LGPL, wgpu, cosmic-text), a tiny hosted picker service, Tauri 2 app, Lemon Squeezy, CI on macOS and Windows from week 1 with a size budget. Swift was dropped when Windows became a requirement.

## 5. How we use AI

AI is used in three places in the product and one place in building it.

### 5.1 Perception (always local, no choice given to the user)
- **Speech to text:** Apple's on-device SpeechAnalyzer on macOS 26+, zero download. Whisper via whisper.cpp elsewhere, quantized turbo downloaded once. Both yield the same `Transcript`. Everything downstream depends on word timestamps being accurate to ~50 ms.
- **Faces:** YuNet, a 300 KB face model, run with the pure-Rust `tract` runtime. Used for reframing and the mouth-movement active-speaker heuristic.
- **Scene cuts:** histogram difference, no ML. Feeds the reframe smoothing.

### 5.2 Judgment (the picker, user-selectable ladder)
The picker is the only generative step, and it is the product. It needs a large model, so by default it does not run on the user's machine. The ladder:

| Tier | What | Download | OS | Quality | Default |
|---|---|---|---|---|---|
| hosted | choco cloud picker: our stateless service calls the Anthropic Messages API with transcript text only. `claude-haiku-5-5` scores candidate batches; `claude-opus-5-5` writes the episode summary and the final titles for the chosen clips. Authenticated by license key, fair-use capped. | none | both | best | yes |
| apple | Apple on-device Foundation Models | none | macOS 26+ | adequate scoring, weaker titles | offline fallback on Mac, selectable |
| byok | user's own Anthropic key, same prompts, called directly from the app | none | both | best | selectable |
| local | 4B instruct GGUF at 4-bit via llama.cpp | ~2.5 GB once | both | good | opt-in, for fully offline on Windows |

Rules that hold across tiers:
- The engine sends **transcript text only**, never audio or video, to any model or server. Hard rule, stated in the UI next to the picker setting.
- Scoring uses **structured output** (a JSON schema for `{score, reasons, title, hook}`) so results parse reliably on every tier: `output_config.format` on the Anthropic API, GBNF grammar on llama.cpp, strict parse plus one retry on the Apple tier.
- Candidates are scored in **batches with the full transcript as context**, not one at a time blind, so the model knows what the rest of the episode covers.
- Prompt mode uses keyword search (BM25) over transcript sentences to shortlist, then the picker ranks. No embedding model to download in v1.
- Every pick records which tier and model produced it, so quality is comparable on the golden set.
- The local tier never runs concurrently with the renderer; the model is unloaded before render. The default tiers have no in-process model at all.

### 5.3 Language helpers (local)
- BM25 keyword search for prompt mode and token-set Jaccard for duplicate detection. No embedding model in v1.
- Filler-word and disfluency detection is rule-based, not ML.

### 5.4 Building it (dev-time AI)
- Claude Code running Claude Opus 5.5 is the primary development agent. `AGENTS.md` is its long-term memory for this project: decisions, conventions, what not to do, how to run things.
- The agent must run the golden-set eval before claiming the picker improved, and must keep the Windows CI green, not just the Mac build.

## 6. Open-source strategy

**Model:** open engine, paid app. The same model as VoiceInk, Cap, Cal.com, and Plausible.

| Component | License | Visibility | Why |
|---|---|---|---|
| `choco-cloud` service | Proprietary | Private | The hosted picker: a tiny stateless service that scores transcript text with a frontier model. Never sees media. |
| `choco-engine` crate | MIT | Public repo `choco-video/engine` | Permissive so anyone can embed it. Attracts contributors for caption styles, languages, model backends, Linux support. Stars are marketing. |
| `choco` CLI | MIT | Same repo | Developers and power users script it on Mac, Windows, and Linux. The CLI is the demo that proves the engine works without the app. |
| `choco-ffi` | MIT | Same repo | Lets anyone build a shell in any language. |
| Choco app (Tauri shell, UI, licensing) | Proprietary | Private at launch | The paid product. Revisit opening it under a source-available license once revenue is stable. |
| Caption styles, presets, prompts | MIT | In the engine repo | Community contributes styles and localized prompts. |
| Golden eval set | CC BY where source licenses allow | In the engine repo | Contributors can prove their picker changes help. |

**Rules:**
- Never copy code from GPL or AGPL clippers (SupoClip, ViralCutter, HotClip). Reading them for ideas is fine. Copying lines is not.
- Engine dependencies must be MIT, Apache-2.0, BSD, Unlicense, or LGPL dynamically linked. **No GPL in the engine, ever.** `cargo deny` enforces this in CI.
- FFmpeg is built with `--disable-gpl`. Hardware encoders and LGPL software fallbacks only.
- Contributions require a DCO sign-off, not a CLA.
- Credit upstream libraries in the app's About screen and the README.
- Contribute fixes back to whisper-rs, ffmpeg-next, and others when we hit bugs.

**Repo layout locally (one working tree), publishing is per-directory:**
```
choco.video/
├── AGENTS.md         agent memory (read by Claude Code via CLAUDE.md)
├── CLAUDE.md         imports AGENTS.md
├── docs/
│   ├── PLAN.md       this file
│   ├── STACK.md      tech stack
│   ├── TECH.md       technical implementation
│   ├── TODO.md       milestone checklists
│   └── COMPETITORS.md what the open-source clippers ship and how we beat them
├── engine/           Cargo workspace → published as public MIT repo
│   ├── Cargo.toml
│   ├── crates/choco-engine/src/{ingest,transcribe,highlights,reframe,captions,render,export,pipeline,backend,model}
│   ├── crates/choco/              CLI
│   ├── crates/choco-ffi/          C ABI
│   ├── fixtures/                  scripts to fetch/generate sample media, media itself gitignored
│   └── eval/                      golden set manifests and the eval runner
├── app/              Tauri 2 project → private
│   ├── src-tauri/                 Rust shell, depends on choco-engine
│   └── src/                       TypeScript UI
├── cloud/            hosted picker service (Rust, axum) → private, depends on choco-engine for prompts and schema
└── scripts/          dev helpers, FFmpeg build recipes, signing
```

## 7. Business model

- **Trial:** full features, watermark on export. The watermark is the exact thing people are leaving Opus Clip to escape, so it converts.
- **Price:** $79 one-time including one year of updates, then $39/year optional for continued updates. Alternative $8/month for people who prefer it. Decide by week 10 based on landing-page signups. One license covers Mac and Windows.
- **Hosted picker included.** The one-time license includes the hosted picker with a fair-use cap of 50 source hours per month, which no solo creator reaches. Beyond the cap: bring your own key, or a $4/month top-up. No credit meter in the normal path. Scoring one source hour costs us cents, so the cap bounds our worst case well under the license price.
- **No account, no telemetry by default.** The license key is the only credential, and it is what authenticates picker calls. Opt-in anonymous usage stats only, and only after v1.
- **Later:** hosted version on the same engine for people without a capable machine. Not before v1 has paying users.

## 8. Risks and the honest answer to each

| Risk | Mitigation |
|---|---|
| Picker quality with a local model is noticeably worse than cloud | Golden-set eval from week 3. BYOK tier exists. Prompt engineering on the local tier is the main lever. |
| Face tracking jitter makes clips look amateur | Dead-zone + eased keyframes + scene-cut handling. Compare frame by frame against Opus Clip output on the same source. |
| FFmpeg build and linking across two OSes eats weeks | Use prebuilt LGPL FFmpeg binaries from a known recipe (Cap's build scripts are a reference, MIT-licensed helpers). Pin versions. Do it in week 1, not week 8. |
| Windows hardware variance breaks encode or inference | CPU fallback for every stage. Compatibility matrix documented. Windows beta before Windows launch. |
| Apple SpeechAnalyzer is worse than Whisper on some languages | Golden set compares both in M1. Per-language fallback to the Whisper download, offered in the UI. |
| The hosted picker is a server we must run | Stateless, no media, a few hundred lines, rate-limited by license key. On outage the app falls back to the Apple tier on Mac or BYOK and says so, instead of failing silently. |
| Hosted inference cost per user | Rough estimate ~35k input + ~6k output tokens per source hour. Opus for everything would be ≈ $0.26/hour, ≈ $13/month at the cap, which a one-time price cannot carry. So Haiku 5.5 scores and Opus 5.5 only summarizes and titles: ≈ $0.02–0.03/hour, ≈ $1.50/month worst case. Measured in M2; budget ≤ $0.03 per source hour. |
| 18 GB RAM machines swap when the optional local picker model and a 4K decode run together | Local tier only: run picker and render sequentially, unload the model before render. Default tiers use no in-process LLM. |
| Tauri webview feels less native than SwiftUI on Mac | Accept the trade for Windows. Match system appearance, use native menus and tray, keep the UI small. Cap and Handy are the proof it is good enough. |
| Someone ships a polished cross-platform clipper first | Ship Mac v1 in 10 weeks. Engine repo public early for mindshare. |
| Apple or Adobe adds this to a built-in app | Possible. If it happens, the engine and the audience carry to the next product. |

# choco.video — Technical Implementation

This is the how. `PLAN.md` is the what and why. When they disagree, fix the one that is wrong and note it in `AGENTS.md` under Decisions.

## 0. Ground rules

- **One engine, two platforms, Mac first.** Everything in `engine/` targets macOS (arm64) until the Mac app ships; Windows (x64) work starts at M6. Platform differences live only behind the `Backend` trait in `backend/`. A `#[cfg(target_os)]` anywhere else is a smell.
- **Stages are pure functions over serde types.** `fn transcribe(&MediaAsset, &TranscribeOptions, &dyn Backend, progress) -> Result<Transcript>`. No globals. No stage reads another stage's cache directly; `pipeline/` owns caching.
- **Progress is an event stream.** Every long stage takes a `&dyn Fn(Progress)` sink. The CLI prints it, the app forwards it to the UI over Tauri events. Same events.
- **Determinism.** Given the same input, model, and options, output must be byte-identical across runs and across OSes for everything except LLM text. Render snapshot tests enforce this.
- **Memory budget.** Target machine is 16–18 GB unified memory. Never hold the LLM and a decoder open at the same time. Decode in windows, not whole files.
- **No GPL.** `cargo deny check licenses` runs in CI with an allowlist. FFmpeg is built `--disable-gpl --disable-nonfree`.
- **Small by default.** Installer ≤ 25 MB, installed app ≤ 50 MB without models, measured in CI. No model ships in the installer. Prefer pure-Rust crates over C runtimes when the output is identical. A dependency over 2 MB compiled needs a Decisions entry in `AGENTS.md`.

## 1. Workspace layout

```
engine/
├── Cargo.toml                      workspace, shared lints, deny.toml
├── crates/
│   ├── choco-engine/
│   │   ├── src/lib.rs              re-exports, version, feature flags
│   │   ├── src/model/              all serde types (MediaAsset, Transcript, Candidate, CropPlan, CaptionTrack, Project)
│   │   ├── src/backend/            Backend trait, detection, mac.rs, windows.rs, cpu.rs
│   │   ├── src/ingest/             probe, proxy generation, audio extraction
│   │   ├── src/transcribe/         Transcriber trait: apple.rs (SpeechAnalyzer), whisper.rs (whisper.cpp), shared cleanup
│   │   ├── src/highlights/         candidate windows, scoring (picker/), prompt mode (search/, BM25), dedupe, titles
│   │   ├── src/reframe/            face detect/track (faces/, tract + YuNet), speaker heuristic, crop planner, smoothing
│   │   ├── src/captions/           chunking, styles, layout, rasterization (cosmic-text)
│   │   ├── src/render/             decoder, wgpu compositor, encoder, muxer
│   │   ├── src/export/             presets, naming, sidecar JSON, watermark
│   │   ├── src/pipeline/           Project, stage cache, orchestration, progress
│   │   └── src/models_registry/    model manifests, download, checksum, storage paths
│   ├── choco/                      CLI (clap)
│   ├── choco-ffi/                  C ABI + header generation (cbindgen)
│   └── apple/                      Swift shim: SpeechAnalyzer + Foundation Models, built by build.rs on macOS 26+
├── fixtures/                       fetch.sh, generate.sh (uses ffmpeg from PATH, dev only), *.gitignore'd media
├── eval/                           golden set manifests (JSON), eval runner binary, baseline scores
└── deny.toml                       license allowlist
```

Feature flags on `choco-engine`:
- `apple-speech` (macOS 26+ SpeechAnalyzer transcriber, default on macOS), `whisper` (whisper.cpp, default on Windows, optional on macOS), `metal` / `vulkan` (whisper and local-llm acceleration), `apple-fm` (Foundation Models picker, macOS 26+), `cloud` was planned for a hosted client and is not needed: the provider client uses `ureq`, always compiled, `local-llm` (llama.cpp picker, opt-in build feature; the model is a runtime download).

## 2. Core types (`model/`)

Keep these small and boring. Everything is `Serialize + Deserialize + Clone + Debug + PartialEq`.

```rust
pub struct MediaAsset {
    pub path: PathBuf,
    pub content_hash: String,          // blake3 of first+last 8 MB + size, for cache keys
    pub duration_ms: u64,
    pub video: Option<VideoStream>,    // width, height, fps (num/den), codec, rotation
    pub audio: Option<AudioStream>,    // sample_rate, channels, codec
}

pub struct Transcript {
    pub language: String,
    pub model: String,
    pub segments: Vec<Segment>,        // sentence-ish units
}
pub struct Segment { pub start_ms: u64, pub end_ms: u64, pub text: String, pub words: Vec<Word> }
pub struct Word { pub start_ms: u64, pub end_ms: u64, pub text: String, pub confidence: f32 }

pub struct Candidate {
    pub id: String,
    pub start_ms: u64, pub end_ms: u64,
    pub score: u8,                     // 0..=100
    pub reasons: Vec<String>,
    pub title: String,                 // hook title, <= 60 chars
    pub description: String,
    pub hashtags: Vec<String>,
    pub provenance: PickerProvenance,  // tier, model id, prompt version
}

pub struct CropPlan {
    pub layout: Layout,                // Single | Split | Static | Fit
    pub keyframes: Vec<CropKeyframe>,  // t_ms, rect per face slot, easing
    pub source_size: (u32, u32),
    pub target_size: (u32, u32),       // 1080x1920
}

pub struct CaptionTrack { pub style: CaptionStyle, pub chunks: Vec<CaptionChunk> }
pub struct CaptionChunk { pub start_ms: u64, pub end_ms: u64, pub words: Vec<Word> }

pub struct Project {
    pub version: u32,
    pub asset: MediaAsset,
    pub transcript: Option<Transcript>,
    pub candidates: Vec<Candidate>,
    pub clips: Vec<Clip>,              // Candidate id + CropPlan + CaptionStyle + user edits + render status
    pub options: ProjectOptions,
}
```

Project on disk: `name.chocoproj/project.json` + `cache/` keyed by `content_hash` + stage + options hash. Thumbnails and proxies live in `cache/` and are regenerable.

## 3. Backend (`backend/`)

```rust
pub trait Backend: Send + Sync {
    fn info(&self) -> HardwareInfo;                       // gpu, vram/unified, cpu cores, os
    fn speech_engine(&self) -> SpeechEngine;              // AppleSpeech | Whisper{device: Metal|Vulkan|Cpu}
    fn llm_device(&self) -> LlmDevice;                    // Metal | Vulkan | Cpu (local tier only)
    fn hw_decoders(&self) -> Vec<HwDecoder>;              // VideoToolbox | D3D11VA | Cuvid | None
    fn hw_encoders(&self) -> Vec<HwEncoder>;              // VideoToolbox | Nvenc | Amf | Qsv | MediaFoundation | Software
    fn recommended_models(&self) -> ModelRecommendation;  // whisper size if needed, whether local-llm is advisable, based on memory
}
```

`backend::detect()` returns the right impl. `cpu.rs` is the universal fallback and must pass the full test suite on its own, because that is what CI without a GPU runs.

Mac specifics: SpeechAnalyzer on macOS 26+, else whisper on Metal. VideoToolbox decode and encode. `tract` on CPU for faces (fast enough at 5 fps sampling). Foundation Models picker available as the offline tier.

Windows specifics: whisper on Vulkan (NVIDIA, AMD, Intel) or CPU. Software decode first, D3D11VA only if measured necessary. NVENC, AMF, QSV, or Media Foundation encode. `tract` on CPU for faces. No CUDA builds, no DirectML, no ONNX Runtime. Implementation: §19.

## 4. Ingest (`ingest/`)

- `probe(path) -> MediaAsset` via `ffmpeg-next` format context. Handle rotation metadata (phone footage).
- `extract_audio(asset) -> wav 16 kHz mono f32` streamed to a temp file. Whisper input.
- `make_proxy(asset) -> 540p H.264` for the review UI's `<video>`. Hardware encode, runs in the background right after probe.
- `thumbnail(asset, t_ms) -> png` via decode-one-frame.

## 5. Transcribe (`transcribe/`)

`trait Transcriber { fn transcribe(&self, audio: &AudioFile, opts: &TranscribeOptions, progress) -> Result<Transcript> }` with two implementations. The backend picks one; the user can override in Settings.

### 5.1 `apple.rs` (feature `apple-speech`, macOS 26+)
- Swift helper executable `choco-apple-speech` built from `engine/crates/apple/SpeechHelper.swift` by `build.rs`, run as a child process: `status` and `transcribe <wav> <locale>`. Progress and errors are JSON lines on stderr; the transcript is JSON on stdout. A child process keeps the engine pure Rust and means a crash in the OS speech stack cannot take the app down. Apple speech has no language auto-detect, so a locale is required (default en-US); whisper covers auto-detect. Uses `SpeechAnalyzer` + `SpeechTranscriber` with `.audioTimeRange` attributes for word-level times. Language auto-detect via the locale list.
- The OS downloads its own speech assets on demand (`AssetInventory`); surface that progress in the UI the first time.
- Zero app size cost beyond the shim.

### 5.2 `whisper.rs` (feature `whisper`, default on Windows and macOS < 26)
- `whisper-rs` with device from the backend. Enable `token_timestamps` and DTW word alignment.
- Feed the whole file. whisper.cpp windows long audio internally (30 s with context carry-over); `no_context` is on to stop runaway repeats crossing windows. Words are rebuilt from sub-word tokens (a leading space starts a word); times come from token `t0`/`t1` in centiseconds. DTW alignment is a later accuracy experiment, not v1.
- whisper-rs leaks the boxed progress closure, so progress is a shared atomic polled until the worker thread finishes. Never put a channel sender in that closure.
- Default model `ggml-large-v3-turbo-q5_0` (~550 MB); `ggml-base-q5` (~60 MB) for preview. No CoreML encoder variant by default; it is an optional speed-up download on older Macs.

### 5.3 Shared cleanup (`cleanup.rs`)
- Re-split into sentences on punctuation plus silence gaps over 600 ms.
- Drop hallucinated repeats (same 4-gram three times in a row), clamp word times to segment bounds, mark low-confidence words.
- Applies to both engines so downstream sees one behaviour.

### 5.4 Model manager (`models_registry/`)
- Manifests with URL pinned to an immutable upstream revision, size, sha256 (what Hugging Face publishes, so it can be checked against the source), license, minimum memory. Stored under `dirs::data_dir()/choco.video/models`. Download with resume and checksum verification. Only Whisper and the optional local picker model live here; nothing ships in the installer.

## 6. Highlights (`highlights/`)

### 6.1 Candidate generation (`windows.rs`)
- Walk segments. A window starts where the previous segment closed a thought (sentence punctuation, or a pause ≥ 700 ms) and ends on one; 15–90 s; one window per reached target length (15/30/45/60 s) per start. Dead air, sparse speech and disfluency-only openings are dropped.
- Cheap pre-filters: drop windows with > 25% silence, drop windows with < 20 words, drop windows whose first 3 s are filler.
- Typical 60-minute episode yields 200–400 candidates before scoring.

### 6.2 Shortlist (`search.rs`)
- Default mode: keep the top 80 candidates by a quality prior (word rate, question marks, numbers, laughter markers, audio energy and pause features from the 16 kHz wav), with at most 20% overlap between kept windows. Measured: this lets 93% of golden clips reach the model (60 @ 85% overlap reached 47%).
- Prompt mode: BM25 over candidate text with the user prompt as the query; only windows with a match are candidates, the prior breaks ties, and no match means no model call. Stop words are never query terms. Pure Rust, no model. Synonym expansion from a small built-in list. Semantic search with a downloadable embedding model is a later opt-in, not v1.
- Dedupe by token-set Jaccard rather than embeddings.

### 6.3 Scoring (`picker/`)
- `trait Llm` (`picker/mod.rs`) with two calls, plain text and scores JSON; batching, map-reduce summaries for small contexts, strict parsing, key repair, retries and re-asking for skipped ids are shared. Backends: `OpenAiCompat` (provider tier), `AppleLlm` (feature `apple-fm` via the helper), `LocalLlama` (feature `local-llm`).
- Episode context = a 250-word summary of the whole transcript (generated once by the summary model, map-reduced when it does not fit, capped at 1.5x the requested length). Optional: if it fails, batches are scored without it. Sent with every batch so scores are relative to the episode, not absolute.
- Batches of 8 candidates. Output schema:
  ```json
  {"scores":[{"id":"c17","score":82,"reasons":["strong hook","complete story"],"title":"...","description":"...","hashtags":["..."]}]}
  ```

- `Byok` / shared client (`picker/openai_compat.rs`): `POST {base_url}/chat/completions` with `Authorization: Bearer {key}`, `model`, `messages` (system prompt from the prompt file, user content = episode context + candidate batch), `temperature: 0.2`, and `response_format: {type: "json_schema", json_schema: {name, strict: true, schema}}`. If the provider rejects `json_schema` (HTTP 400 naming `response_format`), retry once with `{type: "json_object"}` and remember that per base URL. Always strict-parse; on parse failure retry once with the parse error appended. Configured by `CHOCO_LLM_BASE_URL`, `CHOCO_LLM_API_KEY`, `CHOCO_LLM_MODEL` (CLI and dev) or Settings (app; key in the OS keychain via `keyring`). No vendor SDKs.
- `AppleLlm`: the `choco-apple` helper process in `generate` mode (JSON on stdin and stdout, errors as JSON lines on stderr, stderr drained on its own thread). Guided generation for scores; a declined batch is retried as text under permissive guardrails; context overflow is reported and the batch split in half. macOS 26+ only, zero download.
- `LocalLlama`: GBNF grammar from the schema. Default model a 4B instruct at Q4_K_M (~2.5 GB), chosen on the golden set between Qwen3-4B and Gemma-3-4B class models. Context 8k, temperature 0.2. Build feature `local-llm`; the model is a runtime download the user explicitly chooses.
- Prompt versions are files under `highlights/picker/prompts/*.md`, referenced by hash in `PickerProvenance`. The same files are used by every tier and by the cloud service.

### 6.4 Rank and dedupe (`rank.rs`)
- Non-maximum suppression on time overlap > 40%: keep the higher score.
- Prefer diversity: drop a candidate whose token-set Jaccard similarity with an already-chosen one exceeds 0.5, or that overlaps one by more than 40% of the shorter clip.
- Return top N with a minimum score floor (default 55).

## 7. Reframe (`reframe/`)

### 7.1 Detection (`faces/`)
- Sample frames at 5 fps for the candidate range (plus 1 s padding) at 640px wide.
- Face detector: YuNet ONNX (Apache-2.0, ~300 KB) run with `tract-onnx`, pure Rust, CPU. Returns boxes + 5 landmarks. ~5 ms per frame on M3 Pro, fast enough. If `tract` cannot run an op in the model, the fallback is a minimal ONNX Runtime build with only the required ops, not the full runtime.
- Landmarks for mouth: YuNet's 5 points (mouth corners) for a cheap open/closed proxy. A 68-point model only if the heuristic needs it.

### 7.2 Tracking (`track.rs`)
- IoU-based tracker with Hungarian assignment across sampled frames. Tracks get stable ids. Drop tracks shorter than 1 s.
- Scene cut detection: histogram diff on the same sampled frames; a cut resets all tracks.

### 7.3 Speaker heuristic (`speaker.rs`)
- Per track, per 500 ms window: mouth-opening variance from landmark distance. Combine with transcript word density in that window. Highest combined score is the active speaker. Hysteresis of 1 s to avoid flapping.
- v1.5: replace with audio diarization and match speakers to tracks by correlation.

### 7.4 Planner (`planner.rs`)
- Decide layout: 1 stable track → `Single`; 2 stable tracks present > 70% of the range → `Split`; 0 tracks → `Static`; many tracks → `Single` on the active speaker.
- `Single`: target rect is a 9:16 window around the face center, face at 40% from the top, width chosen so the face is ~22% of frame width. Dead-zone: do not move unless the face center leaves a box 12% of frame width around the current target. Move with ease-in-out over 400 ms. Hard cut on scene change or speaker switch.
- `Split`: two 1080x960 halves, each a `Single` planner on its track.
- `Fit`: full source scaled to width, blurred source as background.
- Output `CropPlan` keyframes at every change point; the renderer interpolates.

## 8. Captions (`captions/`)

- Chunker: group words into 2–4 word chunks, break on punctuation, on gaps > 350 ms, and at a max width measured with the actual font. Never split a chunk across a candidate boundary.
- `CaptionStyle` JSON: font family (bundled fonts only, OFL-licensed: Inter, Montserrat, Bebas Neue), size as % of frame height, position as % from bottom, fill, stroke, shadow, active-word style (color, scale, background pill), max lines, uppercase flag.
- Rasterize each chunk once to an RGBA atlas with `cosmic-text` + `swash` at target resolution. The renderer blits the atlas with the active-word variant swapped by time. This keeps per-frame cost near zero.
- Karaoke style renders a progress fill per word using word timestamps.

## 9. Render (`render/`)

Pipeline per clip, single-threaded orchestration with pipelined stages:

1. **Decoder** (`decode.rs`): `ffmpeg-next` seek to `start_ms - 500`, decode with hwaccel from the backend, output frames to a bounded channel of 4. Frames arrive as GPU textures when the hwaccel supports zero-copy (VideoToolbox, D3D11VA), else uploaded.
2. **Compositor** (`compose.rs`, `wgpu`): one render pass per frame. Shader samples the source texture with the interpolated crop rect, scales to 1080x1920, optionally draws the blurred background for `Fit`, blits the caption atlas with the active-word variant, blits the watermark if unlicensed. Reads back to a staging buffer or hands the texture to the encoder.
3. **Encoder** (`encode.rs`): hardware encoder from the backend through `ffmpeg-next`. H.264 High, CRF-equivalent quality target, 8–12 Mbps for 1080x1920. Software fallback is the OS encoder: VideoToolbox's software path on Mac, Media Foundation H.264 on Windows. **Never libx264, never openh264** (GPL and patent exposure respectively).
4. **Muxer** (`mux.rs`): MP4 with `faststart`, audio copied from source range (re-encode to AAC only if the source audio codec is not MP4-compatible).

Snapshot tests: render frame 0, 25%, 50%, 75% of three golden clips to PNG and compare with a perceptual hash threshold against committed references. Must match across macOS and Windows CI.

## 10. Export (`export/`)

- Presets: `tiktok`, `reels`, `shorts`, `custom`. Today all are 1080x1920 H.264 AAC. Kept as named presets for future divergence.
- Naming: `{source_stem}-{rank:02}-{slug(title)}.mp4` plus `.json` sidecar with title, description, hashtags, score, reasons, source timecodes, picker provenance.
- Watermark: small `choco.video` mark bottom-right, composited in the shader when `license.valid == false`. Trial exports are otherwise full quality.

## 11. Pipeline (`pipeline/`)

- `Project::open(path)` / `Project::create(asset_path)`.
- `run(stage, options)` checks `cache/` by `(content_hash, stage, options_hash)` before computing. Writes atomically.
- `Progress { stage, fraction, message, eta }` events. Cancellation via a token checked between windows and frames.
- Orchestration rule: `transcribe → highlights → (reframe + captions per clip) → render per clip`. The LLM is dropped before the first render starts.

## 12. CLI (`crates/choco/`)

```
choco probe <file>
choco transcribe <file> [--model large-v3-turbo] [--out transcript.json]
choco highlights <file|project> [--n 10] [--prompt "..."] [--picker local|apple|cloud]
choco render <project> [--clip c17] [--style bold] [--preset shorts]
choco run <file> [--n 10] [--style bold] [--out ./clips]        # full pipeline
choco models list|pull|rm
choco doctor                                                   # prints backend detection, acceleration, memory, versions
choco eval [--set eval/golden.json] [--picker local]           # golden-set scores
```

Output JSON with `--json` for scripting. Progress on stderr.

## 13. FFI (`crates/choco-ffi/`)

- C ABI over the pipeline: `choco_project_open`, `choco_run_stage`, `choco_subscribe_progress`, `choco_cancel`, `choco_free`. Strings as UTF-8 JSON. `cbindgen` generates `choco.h`.
- The Tauri app does not use the FFI; it depends on the crate directly. The FFI exists so Swift, C#, or Python shells can be built by others.

## 14. App (`app/`, Tauri 2)

- `src-tauri/`: Rust. Commands: `open_project`, `run_stage`, `list_clips`, `update_clip`, `export`, `models_*`, `license_*`, `settings_*`. Progress forwarded as Tauri events. Long work on a dedicated thread pool, never on the main thread.
- `src/`: TypeScript UI. Screens: Drop/Queue, Processing, Review (grid + clip editor), Settings, First-run. State in a small store; everything persisted is in `project.json` or app settings, not in the webview.
- Preview: `<video>` playing the 540p proxy with `currentTime` driven by the clip range; caption overlay drawn by the UI from `CaptionTrack` for live style preview, final pixels come from the engine.
- Tray / menubar: queue status, open last export folder.
- No licensing code: open source, no keys, no watermark.
- Updater: Tauri updater, signed manifests, separate channels `stable` and `beta`.
- Packaging: Mac `.dmg` Developer ID signed and notarized; Windows per-user NSIS `setup.exe`, Authenticode signed (§19.7–19.8). Bundle FFmpeg LGPL dynamic libs beside the binary on both.

## 15. Fixtures and eval (`fixtures/`, `eval/`)

- `fixtures/fetch.sh` downloads 10 CC-licensed long videos (talks, podcasts with two speakers, a stream VOD, phone footage) and records their licenses. Media is gitignored; the script is committed.
- `fixtures/generate.sh` uses system ffmpeg to produce synthetic tests: a moving face rectangle video for tracker tests, a two-speaker split, a silent video, a rotated phone clip.
- `eval/golden.json`: for each fixture, human-chosen highlight ranges (start, end, note). Built by hand in week 3, 5 to 8 per video.
- `eval` runner: precision@5 and recall@10 against golden ranges with 50% overlap tolerance, per picker tier and model. Prints a table and writes `eval/results/<date>-<picker>.json`. A change to the picker is accepted only if precision@5 does not drop.

## 16. CI (`.github/workflows/`)

- `engine.yml`: `macos-15` (arm64) now; `windows-latest` added at M6 (job sketch in §19.9). Steps: cache FFmpeg prebuilt, `cargo build --all --release`, `cargo test --all`, `cargo deny check`, `cargo clippy -- -D warnings`, snapshot render tests on the synthetic fixtures (no model downloads in CI; tests that need models are `#[ignore]` and run locally), `cargo bloat --release -n 20` output as a job summary.
- `app.yml`: builds the Tauri app on both, unsigned, on tags only. Measures installer and installed size and **fails over the budget** in `STACK.md`.


## 17. Milestone definitions of done

- **M1 Engine skeleton (wk 1–2):** workspace builds on both OSes in CI. `choco probe`, `choco transcribe` work on Mac via Apple speech and via whisper, same `Transcript`. `choco doctor` reports backend. Minimal FFmpeg LGPL build recipe pinned and its size recorded. Release profile set.
- **M2 Picker (wk 3–4):** `choco highlights` on the provider tier (OpenCode Zen) and the Apple tier against real long-form fixtures; models chosen by golden-set precision and measured cost; eval runner reports a baseline per model. Local tier compiles but is not yet tuned.
- **M3 Captions + render (wk 5–6):** `choco render` produces a 1080x1920 clip with `Static` crop and `bold` captions, hardware encoded, snapshot tests green on both OSes.
- **M4 Reframe (wk 7–8):** `Single` and `Split` layouts with tracking and smoothing. Side-by-side with Opus Clip on three fixtures looks as good or better.
- **M5 Mac app (wk 9–10):** Tauri app with drop, processing, review, export, settings, first-run model download, licensing, updater. Signed and notarized DMG. Landing page and demo video. Launch.
- **M6 Windows (wk 11–14):** Windows backend validated on NVIDIA, AMD, Intel, and CPU-only. Signed installer. Beta, then launch.
- **M7 Post-launch (wk 15+):** diarization, YouTube ingest, more styles, revenue model.

## 18. No backend

There is no server. The picker calls the provider the user configured, directly from the app. Earlier drafts had a hosted proxy (`cloud/`); it was dropped on 2026-10-18 (see `AGENTS.md` Decisions).

## 19. Windows port (M6)

Target: Windows 11 x64, Rust target `x86_64-pc-windows-msvc`. ARM64 Windows is deferred. The Mac build stays the reference for output; Windows has to match it (§19.10). Checklist and order of work: `TODO.md` M6. Pinned versions below are the ones in today's lockfiles: whisper-rs 0.16.0 / whisper-rs-sys 0.15.0, ffmpeg-next 9.0.0 / ffmpeg-sys-next 9.0.1, tract-onnx 0.23.9, cosmic-text 0.19.0, keyring 4.2.0, tauri 2.12.4 (tauri-utils 2.10.2), `windows` 0.62.2 and `windows-sys` 0.61.2 (both already in the dependency graphs).

### 19.1 Module layout and gating

Platform code lives in `backend/windows.rs`, split into `backend/windows/{dxgi.rs, vulkan.rs}` if it passes ~300 lines. `backend/mod.rs` already gates it (`#[cfg(target_os = "windows")] mod windows;`) and `detect()` already returns `WindowsBackend` on Windows. Everything else compiles on both OSes and decides at runtime from what the backend reports:

| Mac-only thing | How it stays off on Windows | `cfg` needed |
|---|---|---|
| Apple speech (`transcribe/apple.rs`) | `helper_path()` returns `None` off macOS; `transcribe(.., AppleSpeech, ..)` → `TranscribeError::Unavailable` | none (already) |
| Apple picker (`highlights/picker/apple.rs`) | `available()` is false without the helper | none |
| Swift helper build | `build.rs` checks `CARGO_CFG_TARGET_OS == "macos"` | build script only |
| VideoToolbox encode | a row in the encoder table (§19.4) that the Windows backend never lists | none |
| Keychain | `keyring` picks the OS store itself (§19.6) | none |
| `-Wl,-rpath` link args | emitted for macOS only in the four `build.rs` files | build scripts only |

Recommended: keep the Apple modules compiled everywhere and gated at runtime, because §0 treats a `#[cfg(target_os)]` outside `backend/` as a smell and the serde enums (`SpeechEngine::AppleSpeech`) must still deserialize a project made on a Mac.

Engine features change so a plain `cargo build --all` is correct on both OSes. Today `metal` is a default feature, which would turn on `GGML_METAL` in whisper-rs-sys on Windows and fail the CMake step.

```toml
# engine/crates/choco-engine/Cargo.toml
[features]
default = ["apple-speech", "whisper"]
apple-speech = []
whisper = ["dep:whisper-rs"]
metal = []                                      # kept so existing scripts still parse; Metal is set by target below
vulkan = ["whisper", "whisper-rs?/vulkan"]      # needs the Vulkan SDK at build time; off by default

[dependencies]
whisper-rs = { workspace = true, optional = true }

[target.'cfg(target_os = "macos")'.dependencies]
whisper-rs = { workspace = true, optional = true, features = ["metal"] }

[target.'cfg(windows)'.dependencies]
windows = { version = "0.62", features = [
  "Win32_Foundation", "Win32_Graphics_Dxgi", "Win32_Graphics_Dxgi_Common",
  "Win32_System_SystemInformation", "Win32_System_LibraryLoader", "Wdk_System_SystemServices",
] }
```

`windows` rather than `windows-sys`: DXGI is COM, and only `windows` carries the interface vtables (`IDXGIFactory1::EnumAdapters1`). It is already in the app's graph through wry and webview2-com, so the app gains no new crate; it is MIT/Apache-2.0. Each binary crate (`choco`, `choco-ffi`, the app) forwards `vulkan = ["choco-engine/vulkan"]` so its own `build.rs` sees `CARGO_FEATURE_VULKAN` (§19.5).

### 19.2 Backend implementation

```rust
// backend/windows.rs
pub struct WindowsBackend {
    info: HardwareInfo,
    gpus: Vec<Gpu>,                 // discrete first, then integrated; the software adapter is dropped
    vulkan: VulkanState,            // Absent | Untested | Ok | Failed (from gpu-check.json)
    avx2: bool,
}
struct Gpu { name: String, vendor: Vendor, dedicated: u64, shared: u64, driver: String }
enum Vendor { Nvidia, Amd, Intel, Other }  // PCI ids 0x10DE, 0x1002, 0x8086; 0x1414 = Microsoft software adapter, skipped

impl WindowsBackend {
    pub fn detect() -> Self {
        let gpus = dxgi::adapters();                       // CreateDXGIFactory1 → EnumAdapters1 → GetDesc1
        let info = HardwareInfo {
            os: "windows".into(),
            os_version: os_build(),                        // RtlGetVersion: "10.0.26100" (GetVersionEx lies without a manifest)
            arch: std::env::consts::ARCH.into(),
            cpu_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            memory_bytes: total_physical_memory(),         // GlobalMemoryStatusEx().ullTotalPhys
            gpu: gpus.first().map(|g| g.name.clone()),
        };
        let vulkan = vulkan::state(&gpus);                 // loader present? cached verdict for this adapter + driver + engine version?
        Self { info, gpus, vulkan, avx2: is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") }
    }
}

impl Backend for WindowsBackend {
    fn info(&self) -> HardwareInfo { self.info.clone() }

    fn speech_engines(&self) -> Vec<SpeechEngine> {
        if !self.avx2 { return vec![]; }                   // whisper is built for AVX2; the UI shows the reason
        let mut v = Vec::new();
        if cfg!(feature = "vulkan") && matches!(self.vulkan, VulkanState::Untested | VulkanState::Ok) {
            v.push(SpeechEngine::Whisper { device: Device::Vulkan });
        }
        v.push(SpeechEngine::Whisper { device: Device::Cpu });
        v
    }

    fn hw_decoders(&self) -> Vec<HwCodec> { vec![HwCodec::Software] }

    fn hw_encoders(&self) -> Vec<HwCodec> {
        let mut v = Vec::new();
        for g in &self.gpus {                              // discrete first: a hybrid laptop encodes on the dGPU
            let c = match g.vendor { Vendor::Nvidia => HwCodec::Nvenc, Vendor::Amd => HwCodec::Amf,
                                     Vendor::Intel => HwCodec::Qsv, Vendor::Other => continue };
            if !v.contains(&c) { v.push(c); }
        }
        v.push(HwCodec::MediaFoundation);                  // software MFT: always last, present on every non-N edition
        v
    }

    fn verify(&self, engine: SpeechEngine, model: &Path) -> bool {
        match (engine, self.vulkan) {
            (SpeechEngine::Whisper { device: Device::Vulkan }, VulkanState::Untested) => vulkan::self_test(model, &self.gpus),
            (SpeechEngine::Whisper { device: Device::Vulkan }, s) => s == VulkanState::Ok,
            _ => true,
        }
    }
}
```

- `verify` is a new trait method with a default of `true`, so the Mac and CPU backends do not change. The pipeline calls it before the first whisper run of a job and moves to the next entry of `speech_engines()` when it returns false.
- `speech_engines()` may return an empty list only on Windows without AVX2; `detect_reports_something_sane` asserts non-empty, so that test stays meaningful on CI (runners have AVX2). The UI shows "This processor is too old for on-device transcription".
- `recommended_models()` keeps the trait default.
- All `unsafe` FFI calls (`RtlGetVersion`, `GlobalMemoryStatusEx`, `LoadLibraryExW`) carry a `// SAFETY:` line like `mac.rs`.

### 19.3 FFmpeg: build recipe and linking

`scripts/ffmpeg/build-windows.sh` mirrors `build-mac.sh` and runs in MSYS2 bash inside a Visual Studio developer environment (`cl`, `link`, `lib` on `PATH`). Output: `vendor/ffmpeg/8.1.3-windows-x64/{bin/*.dll, lib/*.lib, include/}`.

```bash
DEPS="$WORK/deps"   # static third-party inputs, each pinned by version and sha256 in the script
# dav1d 1.5.4: meson setup --default-library=static --buildtype=release -Db_vscrt=mt -Denable_tools=false -Denable_tests=false
# nv-codec-headers n11.1.5.3 (MIT): make PREFIX="$DEPS" install        → ffnvcodec.pc, headers only
# AMF headers (MIT): copy amf/public/include → "$DEPS/include/AMF"     → headers only; amfrt64.dll comes with the AMD driver
# libvpl (MIT): cmake -DBUILD_SHARED_LIBS=OFF -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded → vpl.pc
export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig"

ENCODERS=(h264_mf h264_nvenc h264_amf h264_qsv aac pcm_s16le pcm_f32le)
HWACCELS=(h264_d3d11va h264_d3d11va2 hevc_d3d11va hevc_d3d11va2 vp9_d3d11va vp9_d3d11va2 av1_d3d11va av1_d3d11va2)
# DEMUXERS, DECODERS, PARSERS, BSFS, MUXERS, FILTERS: identical to build-mac.sh

./configure \
  --prefix="$PREFIX" --toolchain=msvc --arch=x86_64 --target-os=win64 \
  --enable-shared --disable-static \
  --disable-gpl --disable-nonfree \
  --disable-autodetect --enable-w32threads \
  --enable-d3d11va --enable-mediafoundation \
  --enable-ffnvcodec --enable-nvenc --enable-amf --enable-libvpl --enable-libdav1d \
  --pkg-config=pkgconf --pkg-config-flags=--static \
  --disable-programs --disable-doc --disable-debug --disable-network --disable-avdevice \
  --disable-everything --enable-protocol=file,pipe \
  --enable-demuxer=... --enable-decoder=... --enable-parser=... --enable-bsf=... \
  --enable-encoder="$(join "${ENCODERS[@]}")" --enable-muxer=... --enable-filter=... \
  --enable-hwaccel="$(join "${HWACCELS[@]}")" \
  --enable-avcodec --enable-avformat --enable-avfilter --enable-swscale --enable-swresample \
  --extra-cflags="-MT -I$DEPS/include" --extra-ldflags="-LIBPATH:$DEPS/lib"

for flag in 'CONFIG_GPL 0' 'CONFIG_NONFREE 0' 'HAVE_W32THREADS 1' 'CONFIG_H264_MF_ENCODER 1' \
            'CONFIG_H264_NVENC_ENCODER 1' 'CONFIG_H264_AMF_ENCODER 1' 'CONFIG_H264_QSV_ENCODER 1' \
            'CONFIG_LIBDAV1D 1' 'CONFIG_AV1_D3D11VA_HWACCEL 1'; do
  grep -q "^#define $flag\$" config.h || { echo "configure: expected $flag" >&2; exit 1; }
done
make -j"$(nproc)" && make install
```

- `--disable-autodetect` drops every hardware library that is not named, threads included; the gate list above turns a silent loss into a failed build.
- NVENC, AMF and libvpl load their vendor runtimes (`nvEncodeAPI64.dll`, `amfrt64.dll`, the oneVPL/MSDK runtime) with `LoadLibrary` at encoder open, and `h264_mf` loads `mfplat.dll` the same way. A machine without the driver, or a Windows N edition, still loads avcodec; only that encoder fails to open. None of these is GPL or nonfree; `config.h` confirms it.
- `-MT` puts a private static CRT in each DLL, so the installer needs no VC++ redistributable. FFmpeg frees memory in the library that allocated it, so separate CRT heaps are safe.

Delivery and link setup:

- `ffmpeg-windows.yml` (manual dispatch) runs the script and publishes `ffmpeg-8.1.3-win64-lgpl-minimal.zip` as a release asset. The URL and sha256 go into `scripts/ffmpeg/VERSIONS` and `README.md`; CI and contributors fetch that zip and never build ad hoc (the `AGENTS.md` trap about Windows link failures).
- Rust links through `ffmpeg-sys-next` 9.0.1 with `FFMPEG_DIR` set to that folder: its `build.rs` checks `FFMPEG_DIR` before vcpkg and pkg-config, adds `FFMPEG_DIR/lib` to the link search path and links `avcodec.lib` etc. as import libraries. pkg-config is used only inside the FFmpeg build, never by Cargo on Windows. bindgen needs `LIBCLANG_PATH` (LLVM is preinstalled on GitHub's Windows images).
- Recommended over vcpkg: its `ffmpeg` port cannot express our allowlist without an overlay port, and it builds its own dependency tree we would then have to license-audit.
- The rpath equivalent: Windows searches the executable's own folder first for implicitly linked DLLs, so the installer puts the six DLLs (`avcodec-62.dll`, `avformat-62.dll`, `avfilter-11.dll`, `avutil-60.dll`, `swresample-6.dll`, `swscale-9.dll`) beside the exe. Tests and `tauri dev` get `$FFMPEG_DIR/bin` on `PATH`. `CHOCO_FFMPEG_LIB_DIR` has no meaning on Windows.

### 19.4 Encoder selection

`render/encode.rs` holds one table. No `cfg`: the backend decides which rows can be reached.

```rust
pub struct EncoderSpec { pub name: &'static str, pub input: Pixel, pub opts: &'static [(&'static str, &'static str)] }

pub fn spec(c: HwCodec) -> Option<EncoderSpec> {
    use Pixel::{NV12, YUV420P};
    Some(match c {
        HwCodec::VideoToolbox => EncoderSpec { name: "h264_videotoolbox", input: YUV420P,
            opts: &[("allow_sw", "1"), ("profile", "high"), ("realtime", "0")] },          // today's Mac settings, unchanged
        HwCodec::Nvenc => EncoderSpec { name: "h264_nvenc", input: YUV420P,
            opts: &[("preset", "p5"), ("tune", "hq"), ("rc", "vbr"), ("profile", "high")] },
        HwCodec::Amf => EncoderSpec { name: "h264_amf", input: YUV420P,
            opts: &[("usage", "transcoding"), ("quality", "quality"), ("rc", "vbr_peak"), ("profile", "high")] },
        HwCodec::Qsv => EncoderSpec { name: "h264_qsv", input: NV12,                         // h264_qsv takes NV12 only
            opts: &[("preset", "medium"), ("profile", "high"), ("look_ahead", "0")] },
        HwCodec::MediaFoundation => EncoderSpec { name: "h264_mf", input: YUV420P,
            opts: &[("rate_control", "pc_vbr"), ("scenario", "archive"), ("hw_encoding", "0")] }, // 0 = Microsoft's software MFT
        HwCodec::D3d11va | HwCodec::Software => return None,
    })
}

/// Open the first encoder in `order` that this machine accepts. Errors name every candidate and its reason.
pub fn open_first(order: &[HwCodec], p: &VideoParams) -> Result<(ff::encoder::Video, EncoderSpec), RenderError> {
    let mut tried = Vec::new();
    for spec in order.iter().filter_map(|c| spec(*c)) {
        let Some(codec) = ff::encoder::find_by_name(spec.name) else { tried.push(format!("{}: not in this FFmpeg build", spec.name)); continue };
        match configure(codec, &spec, p).and_then(|ctx| ctx.open_with(dict(spec.opts))) {
            Ok(enc) => return Ok((enc, spec)),
            Err(e) => tried.push(format!("{}: {e}", spec.name)),           // e.g. old driver, no GPU, Windows N without mfplat.dll
        }
    }
    Err(RenderError::Unsupported(format!("no H.264 encoder opened: {}", tried.join("; "))))
}
```

- `configure` keeps everything `render_clip` sets today: size, time base and frame rate from `pick_fps` (constant frame rate), bit rate with max rate × 1.5, GOP of 2 s (required: `h264_mf` defaults to `g=0`), BT.709 limited range, `GLOBAL_HEADER` for MP4. The profile is also set on the context (High) for `h264_mf`, which has no `profile` private option.
- The compositor writes YUV420P. When `spec.input == NV12`, `render/yuv.rs` interleaves U and V into the encoder frame; that is the only extra copy, and only on QSV.
- `RenderSpec` gains `encoders: Vec<HwCodec>`, filled by the pipeline from `backend.hw_encoders()`, plus an optional user override from Settings that moves one encoder to the front. `RenderReport.video_encoder` records the one that opened.
- An encoder that opens and then fails mid-clip (driver reset, session limit on consumer NVIDIA cards): the pipeline renders that clip once more with the failed encoder removed from the list.
- Order on Windows: vendor encoders of discrete GPUs, then of integrated GPUs, then `h264_mf` software. The software MFT exists on every Windows 11 edition except N/KN without the Media Feature Pack; there the error says to install it.
- Output checks are the same on every row: 1080x1920 (or the preset size), H.264 High, CFR, AAC 48 kHz at −14 LUFS from the shared `render/audio.rs` (native FFmpeg `aac` encoder on both OSes), `+faststart`.

### 19.5 Whisper on Windows: Vulkan init and fallback

Build:

- whisper-rs-sys passes every `GGML_*` and `CMAKE_*` environment variable to CMake. The Windows build sets `GGML_NATIVE=OFF GGML_AVX=ON GGML_AVX2=ON GGML_FMA=ON GGML_F16C=ON`. With `GGML_NATIVE` left on (the default), a CI runner with AVX-512 would bake AVX-512 into the binary and crash users' CPUs.
- The `vulkan` feature makes whisper-rs-sys define `GGML_VULKAN=ON`, read `VULKAN_SDK` and emit `rustc-link-lib=vulkan-1`. That is an import-time dependency: without a delay-load, an exe on a machine with no Vulkan driver fails to start. Each binary's `build.rs` therefore adds, when its `vulkan` feature is on and the target is Windows:

  ```rust
  println!("cargo::rustc-link-arg=/DELAYLOAD:vulkan-1.dll");
  println!("cargo::rustc-link-lib=delayimp");
  ```

  `rustc-link-arg` from the engine's own build script does not reach dependents, so this goes in `choco`, `choco-ffi` and the app.
- One CRT: Tauri's `build.windows.staticVCRuntime` (default `true` in tauri-utils 2.10) for the app, `-C target-feature=+crt-static` for `x86_64-pc-windows-msvc` in `engine/.cargo/config.toml` for the CLI. whisper.cpp is built by the cmake crate, which should follow `crt-static`; a CRT mismatch shows up as `LNK2038` at link time, and the CI import check (§19.9) proves nothing links `vcruntime140.dll`.

Runtime:

1. `vulkan::state`: `LoadLibraryExW("vulkan-1.dll", LOAD_LIBRARY_SEARCH_SYSTEM32)`. If it fails → `Absent` (no Vulkan driver; `speech_engines()` is CPU only). Otherwise read `%LOCALAPPDATA%\choco.video\gpu-check.json`, keyed by adapter name, UMD driver version (`IDXGIAdapter::CheckInterfaceSupport`) and engine version; no matching entry → `Untested`.
2. `verify` on `Untested` runs the self-test as a child process: the current exe with a hidden first argument (`--choco-selftest vulkan <model>`), spawned with `CREATE_NO_WINDOW` so a GUI-subsystem app shows no console. The child checks the argument before anything else (`choco_engine::selftest::run_if_requested()` at the top of each `main`), loads the model with `WhisperContextParameters::use_gpu(true)`, transcribes 2 s of generated audio, prints `{"ok":true,"ms":…}` and exits. The parent waits up to 60 s; a crash, non-zero exit, timeout or `ok:false` writes `Failed`. A child process for the same reason as the Apple helper: a ggml abort in a driver must not take the app down.
3. During a real job, an `Err` from whisper on Vulkan reruns the file on CPU (`use_gpu(false)`) and writes `Failed`, so the next job starts on CPU.
4. Device choice is ggml-vulkan's default (discrete GPUs first, else the first integrated one), which matches §19.4. Intel iGPU-only machines default to CPU unless the matrix shows Vulkan ≥ 1.5x faster there (`TODO.md` M6).
5. `n_threads` = physical cores, capped at 8, on both devices.

Size: ggml-vulkan embeds SPIR-V for every shader variant and is the one Windows-only component that can threaten the budget. Its size delta is measured in step 6 of the M6 order. If the installer goes over 25 MB with it, Windows v1 ships CPU whisper and the choice goes in `AGENTS.md` Decisions; we do not fork ggml to trim shaders.

Models: `models_registry::default_dir()` moves from `dirs::data_dir()` to `dirs::data_local_dir()`, the same folder on macOS and `%LOCALAPPDATA%\choco.video\models` on Windows. Roaming `%APPDATA%` would sync 574 MB with domain profiles. `CHOCO_MODELS_DIR` still overrides it.

### 19.6 Provider key: keyring

The app already uses `keyring` 4.2.0 with its default `v1` feature. On Windows that pulls `windows-native-keyring-store` 1.1.0 (target-gated in keyring's manifest) and `keyring::Entry::new(service, user)` stores a generic credential in Windows Credential Manager. `settings.rs` does not change: same `KEYCHAIN_SERVICE` and `KEYCHAIN_USER`, same order (key saved only after the model list succeeds), `NoEntry` on delete is still success. Windows-specific points:

- Credential Manager caps a secret at 2,560 bytes (`CRED_MAX_CREDENTIAL_BLOB_SIZE`). Provider keys are far below that; a longer paste gets a plain error instead of a silent truncation.
- The credential is per Windows user, and any process running as that user can read it: Credential Manager has no per-app access list like the macOS Keychain's. Documented in `SECURITY.md` at launch.
- UI copy says "saved in Windows Credential Manager" when `status.hardware.os == "windows"`. The CLI still reads `CHOCO_LLM_API_KEY` from the environment only.

### 19.7 Tauri bundling config

Tauri merges `tauri.windows.conf.json` over `tauri.conf.json` on Windows (JSON Merge Patch: arrays are replaced, objects merged). `app.windows` is restated without the macOS-only `titleBarStyle` and `hiddenTitle`; the CSP is inherited unchanged (`http://asset.localhost` and `http://ipc.localhost` are already in it, which is how WebView2 serves both).

```json
{
  "app": {
    "windows": [{ "title": "choco.video", "width": 1180, "height": 800, "minWidth": 900, "minHeight": 640, "dragDropEnabled": true }],
    "security": { "assetProtocol": { "enable": true, "scope": ["$HOME/**"] } }
  },
  "bundle": {
    "targets": ["nsis"],
    "resources": {
      "windows/ffmpeg/avcodec-62.dll": "avcodec-62.dll",
      "windows/ffmpeg/avformat-62.dll": "avformat-62.dll",
      "windows/ffmpeg/avfilter-11.dll": "avfilter-11.dll",
      "windows/ffmpeg/avutil-60.dll": "avutil-60.dll",
      "windows/ffmpeg/swresample-6.dll": "swresample-6.dll",
      "windows/ffmpeg/swscale-9.dll": "swscale-9.dll"
    },
    "windows": {
      "webviewInstallMode": { "type": "downloadBootstrapper", "silent": true },
      "digestAlgorithm": "sha256",
      "signCommand": "trusted-signing-cli -e <endpoint> -a <account> -c <profile> -d choco.video %1",
      "allowDowngrades": false,
      "nsis": {
        "installMode": "currentUser",
        "compression": "lzma",
        "installerIcon": "icons/icon.ico",
        "startMenuFolder": "choco.video",
        "displayLanguageSelector": false,
        "languages": ["English"],
        "installerHooks": "windows/hooks.nsh"
      }
    }
  }
}
```

- `resources` paths are relative to the install folder on Windows, so the DLLs land beside the exe (§19.3). `scripts/build-app-windows.sh` copies them from `$FFMPEG_DIR/bin` into `app/src-tauri/windows/ffmpeg/` and signs them first instead of relying on which files Tauri's `signCommand` covers; Smart App Control blocks unsigned DLLs. `signtool verify /pa /all` over every PE file in the install folder is the check.
- Asset scope: `$HOME/**` covers the user profile. Sources and output folders on other drives are added at runtime with `app.asset_protocol_scope().allow_file(..)` / `allow_directory(.., true)` when the user picks them. That is narrower than a static `**` and applies to the Mac build too.
- `webviewInstallMode`: recommended `downloadBootstrapper`, because Windows 11 ships WebView2 and the bootstrapper only runs if someone removed it; `embedBootstrapper` would add ~1.8 MB for that case.
- `signCommand`: recommended Azure Trusted Signing, because it plugs into this per-file hook and needs no hardware token. The fallback is SignPath Foundation (free for open source), which signs artifacts submitted from CI: the app exe and DLLs in one request before `tauri bundle`, `setup.exe` in a second. EV is not worth it: since 2024 it no longer skips SmartScreen reputation.
- `installerHooks` (`windows/hooks.nsh`): `NSIS_HOOK_POSTUNINSTALL` removes `$LOCALAPPDATA\choco.video\models` and `gpu-check.json` only when "Delete the application data" was ticked (Tauri's own checkbox already removes `%APPDATA%\video.choco.app` and `%LOCALAPPDATA%\video.choco.app`), then `RMDir` on the install folder.
- `release.yml` gains a `windows` job: same tag and version check, build with `pnpm tauri build --features vulkan` (or without `vulkan`, per §19.5; Tauri merges `tauri.windows.conf.json` by itself), size and import checks, upload `choco.video-windows-x64-setup.exe` + `.sha256`.

### 19.8 Installer layout

```
choco.video_<ver>_x64-setup.exe        NSIS, LZMA, Authenticode-signed, ≤ 25 MB
%LOCALAPPDATA%\choco.video\            $INSTDIR (Tauri per-user default: $LOCALAPPDATA\<productName>)
├── choco-app.exe                      Tauri main binary: engine + whisper.cpp (+ Vulkan) + UI, static CRT, signed
├── avcodec-62.dll  avformat-62.dll  avfilter-11.dll
├── avutil-60.dll  swresample-6.dll  swscale-9.dll        minimal LGPL FFmpeg, signed, ≤ 12 MB together
├── uninstall.exe                      signed
├── models\                            runtime download (whisper turbo-q5 574 MB or base-q5 60 MB), not in the installer
└── gpu-check.json                     Vulkan self-test verdict
%APPDATA%\video.choco.app\settings.json         settings (roams; small)
%LOCALAPPDATA%\video.choco.app\EBWebView\       WebView2 profile
Credential Manager                              provider key (generic credential)
HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\choco.video
Start menu\Programs\choco.video\choco.video.lnk
```

- Installed folder without models ≤ 50 MB. No PATH change and no CLI in v1, same as the Mac DMG. No admin prompt.
- `models\` sits inside `$INSTDIR` because Tauri's per-user install folder and `data_local_dir()/choco.video` are the same path. The NSIS uninstaller deletes only the files it installed and then a non-recursive `RMDir`, so models survive upgrades and plain uninstalls; the hook above handles a full removal. Recommended over a Windows-only models path, because the CLI and the app then share one model folder on both OSes and the Mac path does not move.
- Pin the exe name with `mainBinaryName` once chosen, so shortcuts and the uninstaller never chase a rename between versions.

### 19.9 CI

```yaml
# engine.yml, new job. The macos job gains an upload-artifact step for engine/fixtures/media (name: fixtures).
  windows:
    runs-on: windows-latest
    needs: macos                          # generate.sh needs macOS `say`; both OSes test the same fixture bytes
    defaults:
      run:
        shell: bash
        working-directory: engine
    env:
      FFMPEG_DIR: ${{ github.workspace }}\vendor\ffmpeg\8.1.3-windows-x64
      LIBCLANG_PATH: C:\Program Files\LLVM\bin
      GGML_NATIVE: "OFF"
      GGML_AVX2: "ON"
      GGML_FMA: "ON"
      GGML_F16C: "ON"
    steps:
      - uses: actions/checkout@v4
      - name: Toolchain
        run: rustup show active-toolchain || rustup toolchain install
      - name: Minimal LGPL FFmpeg (pinned prebuilt)
        working-directory: .
        run: ./scripts/ffmpeg/fetch-windows.sh          # URL + sha256 from scripts/ffmpeg/VERSIONS; fails on mismatch
      - name: FFmpeg DLLs on PATH for tests (the installer puts them beside the exe)
        run: echo "$FFMPEG_DIR\\bin" >> "$GITHUB_PATH"
      - name: Vulkan SDK (build-time only)
        run: |
          curl -fLo vk.exe "https://sdk.lunarg.com/sdk/download/$VK_VER/windows/vulkansdk-windows-X64-$VK_VER.exe"
          ./vk.exe --root C:/VulkanSDK --accept-licenses --default-answer --confirm-command install
          echo "VULKAN_SDK=C:/VulkanSDK" >> "$GITHUB_ENV"
        env: { VK_VER: "<pinned>" }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: engine }
      - uses: actions/download-artifact@v4
        with: { name: fixtures, path: engine/fixtures/media }
      - name: Build (release)
        run: cargo build --release --all --features choco-engine/vulkan
      - name: Test                                  # no GPU on the runner: Vulkan absent or failing → the CPU path is what runs
        run: cargo test --all --features choco-engine/vulkan
      - name: Clippy
        run: cargo clippy --all --all-targets --features choco-engine/vulkan -- -D warnings
      - name: Imports (no VC++ runtime, no mingw runtime, vulkan-1 delay-loaded)
        working-directory: .
        run: ./scripts/check-imports.sh engine/target/release/choco.exe "$FFMPEG_DIR"/bin/*.dll
      - name: Size report
        working-directory: .
        run: ./scripts/size-check.sh engine/target/release >> "$GITHUB_STEP_SUMMARY"
```

- `cargo deny` stays on the Ubuntu job. With no `[graph] targets` set it already resolves every platform, so Windows-only crates are checked today; nothing changes in `deny.toml`.
- `ffmpeg-windows.yml` (manual): `ilammy/msvc-dev-cmd@v1` for the MSVC environment, `msys2/setup-msys2@v2` (`msystem: MSYS`, `path-type: inherit`, packages `make diffutils nasm pkgconf`), `pip install meson ninja` (dev tooling only), `./scripts/ffmpeg/build-windows.sh`, zip, sha256, `softprops/action-gh-release@v2`.
- `check-imports.sh` runs `dumpbin /dependents` (and `/imports` for the delay-load table). It fails on `vcruntime140*.dll`, `msvcp140*.dll`, `api-ms-win-crt-*.dll`, `libgcc*`, `libwinpthread*`, any FFmpeg DLL other than the six, or `vulkan-1.dll` in the normal import table.

### 19.10 Test plan

| Tests | On Windows | Notes |
|---|---|---|
| Engine unit tests (`#[cfg(test)]`) | unchanged | `backend::mac` tests are compiled out; `backend::tests` run against `WindowsBackend` |
| New `backend::windows` tests | new | memory > 0, OS build non-empty, encoder list non-empty and ends with `MediaFoundation`, DXGI never reports the software adapter |
| `tests/media.rs` (7 probe tests) | unchanged | fixtures from the Mac artifact; AV1 needs dav1d in the minimal build |
| `tests/render.rs` render tests (7) | unchanged | assert size, codec, frame count, duration and audio, never pixels; on the runner they encode through `h264_mf` (software MFT) |
| `tests/render.rs` snapshots (3) | unchanged references | luma fingerprints of the composed frame before encoding, MAD ≤ 2.0; no Windows-specific references. A failure is investigated (swscale x86 vs NEON paths first; `SWS_BITEXACT` if needed), not re-recorded |
| Caption determinism (`captions/raster.rs`) | unchanged | plus a new cross-OS test: blake3 of each preset's RGBA raster equals the Mac-committed hash (bundled fonts only, so system fonts cannot differ) |
| Face detection | new reference test | YuNet boxes on fixture frames within 1 px per edge and 0.01 score of a Mac-committed JSON |
| Apple paths | new | `transcribe(.., AppleSpeech, ..)` → `Unavailable`; `picker::apple::available()` → false |
| App `settings` test + keyring round-trip | new on Windows | set, get, delete in Credential Manager on the runner; > 2,560-byte key refused |
| `#[ignore]` model tests | by hand on matrix machines | `cargo test --all -- --ignored`; adds the Vulkan self-test and a forced-failure fallback test |
| `#[ignore]` encoder matrix test | by hand on matrix machines | every encoder that opens: 1080x1920 H.264 High CFR, frame count exact, PSNR vs composed frames ≥ 38 dB at 10 Mbps |
| Installer | release job + VM by hand | launch smoke test (alive after 6 s), install/upgrade/uninstall keeps or removes models as specified, `signtool verify /pa /all` on every PE file |

What the size check enforces on Windows (`scripts/size-check.sh`, CI summary on every push; the release job measures the real installer):

| Measure | Budget | Fails the job |
|---|---|---|
| Installed: app or CLI exe + six FFmpeg DLLs (+ `uninstall.exe` in release) | ≤ 50 MB | yes |
| Installer: `zip -9` estimate on push, real `setup.exe` in release | ≤ 25 MB | yes |
| FFmpeg DLLs together | ≤ 12 MB | warning; a Decisions entry to exceed |
| `vulkan` delta on the exe | recorded | decides §19.5's ship-or-CPU-only call |

### 19.11 Open decisions and the recommended option

| Decision | Recommended | Why, in one line |
|---|---|---|
| FFmpeg toolchain | MSVC via MSYS2 bash | Same linker, CRT and `.lib` import libraries as the Rust `msvc` target; no mingw runtime DLLs to ship |
| FFmpeg delivery | Our minimal build as a pinned release asset, linked with `FFMPEG_DIR` | Reproducible from the script; vcpkg cannot express the allowlist; pkg-config is unnecessary because ffmpeg-sys-next reads `FFMPEG_DIR` first |
| CRT | Static everywhere (`-MT`, `+crt-static`, Tauri `staticVCRuntime`) | No VC++ redistributable in a 25 MB installer |
| Installer | NSIS per-user, LZMA | No admin prompt, smaller output; MSI needs WiX and defaults to per-machine |
| WebView2 | `downloadBootstrapper` | Windows 11 ships WebView2; embedding costs 1.8 MB for a rare case |
| Signing | Azure Trusted Signing, else SignPath Foundation | Cloud key that fits Tauri's `signCommand`; EV no longer buys SmartScreen reputation |
| Decode | Software, D3D11VA only if measured | The Mac decodes in software today at 5.7x realtime render; hwaccel adds a GPU→CPU copy for the CPU compositor |
| DXVA2 / D3D12VA | Not built | D3D11VA covers every Windows 11 driver |
| QSV | Keep, unless `h264_mf` hardware matches it on Intel | Drops libvpl and a code path if within 0.5 dB PSNR |
| Vulkan whisper in the installer | Yes if within budget, else CPU-only v1 | Speed matters, but the size budget is a non-negotiable |
| CPU baseline | AVX2 + FMA + F16C | Covers every Core and Ryzen chip that runs Windows 11; Atom-class chips are too slow for turbo anyway |
| Apple code paths | Runtime-gated, not `cfg`-gated | §0 rule on `cfg` outside `backend/`; Mac projects must still deserialize |
| DXGI bindings | `windows` 0.62 | COM vtables; already in the app's graph |
| ARM64 Windows | Deferred past M6 | Needs its own FFmpeg, ggml and test hardware; no tester base yet |

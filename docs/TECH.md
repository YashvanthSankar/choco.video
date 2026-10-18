# choco.video — Technical Implementation

This is the how. `PLAN.md` is the what and why. When they disagree, fix the one that is wrong and note it in `AGENTS.md` under Decisions.

## 0. Ground rules

- **One engine, two platforms.** Everything in `engine/` compiles on macOS (arm64) and Windows (x64) on every push. Platform differences live only behind the `Backend` trait in `backend/`. A `#[cfg(target_os)]` anywhere else is a smell.
- **Stages are pure functions over serde types.** `fn transcribe(&MediaAsset, &TranscribeOptions, &dyn Backend, progress) -> Result<Transcript>`. No globals. No stage reads another stage's cache directly; `pipeline/` owns caching.
- **Progress is an event stream.** Every long stage takes a `&dyn Fn(Progress)` sink. The CLI prints it, the app forwards it to the UI over Tauri events. Same events.
- **Determinism.** Given the same input, model, and options, output must be byte-identical across runs and across OSes for everything except LLM text. Render snapshot tests enforce this.
- **Memory budget.** Target machine is 16–18 GB unified memory. Never hold the LLM and a decoder open at the same time. Decode in windows, not whole files.
- **No GPL.** `cargo deny check licenses` runs in CI with an allowlist. FFmpeg is built `--disable-gpl --disable-nonfree`.

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
│   │   ├── src/transcribe/         whisper.cpp wrapper, model manager, word alignment cleanup
│   │   ├── src/highlights/         candidate windows, scoring (llm/), prompt mode (embed/), dedupe, titles
│   │   ├── src/reframe/            face detect/track (onnx/), speaker heuristic, crop planner, smoothing
│   │   ├── src/captions/           chunking, styles, layout, rasterization (cosmic-text)
│   │   ├── src/render/             decoder, wgpu compositor, encoder, muxer
│   │   ├── src/export/             presets, naming, sidecar JSON, watermark
│   │   ├── src/pipeline/           Project, stage cache, orchestration, progress
│   │   └── src/models_registry/    model manifests, download, checksum, storage paths
│   ├── choco/                      CLI (clap)
│   └── choco-ffi/                  C ABI + header generation (cbindgen)
├── fixtures/                       fetch.sh, generate.sh (uses ffmpeg from PATH, dev only), *.gitignore'd media
├── eval/                           golden set manifests (JSON), eval runner binary, baseline scores
└── deny.toml                       license allowlist
```

Feature flags on `choco-engine`:
- `metal` (default on macOS), `cuda`, `vulkan`, `coreml` (whisper encoder), `apple-fm` (Foundation Models picker tier, macOS 26+), `cloud` (BYOK, pulls `reqwest`).

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
    fn whisper_device(&self) -> WhisperDevice;            // Metal{coreml:bool} | Cuda | Vulkan | Cpu
    fn llm_device(&self) -> LlmDevice;                    // Metal | Cuda | Vulkan | Cpu
    fn onnx_providers(&self) -> Vec<OnnxProvider>;        // CoreML | DirectML | Cuda | Cpu, in priority order
    fn hw_decoders(&self) -> Vec<HwDecoder>;              // VideoToolbox | D3D11VA | Cuvid | None
    fn hw_encoders(&self) -> Vec<HwEncoder>;              // VideoToolbox | Nvenc | Amf | Qsv | MediaFoundation | Software
    fn recommended_models(&self) -> ModelRecommendation;  // whisper size, llm size, based on memory
}
```

`backend::detect()` returns the right impl. `cpu.rs` is the universal fallback and must pass the full test suite on its own, because that is what CI without a GPU runs.

Mac specifics: Metal for whisper and llama, CoreML whisper encoder (first run compiles the CoreML model, which takes minutes; show that in the UI), VideoToolbox decode and encode, CoreML execution provider for ONNX.

Windows specifics: detect NVIDIA (CUDA if the runtime is present, else Vulkan), AMD (Vulkan + AMF), Intel (Vulkan + QSV), no GPU (CPU + Media Foundation software encode). DirectML for ONNX. Ship Vulkan builds by default; CUDA is an optional larger download.

## 4. Ingest (`ingest/`)

- `probe(path) -> MediaAsset` via `ffmpeg-next` format context. Handle rotation metadata (phone footage).
- `extract_audio(asset) -> wav 16 kHz mono f32` streamed to a temp file. Whisper input.
- `make_proxy(asset) -> 540p H.264` for the review UI's `<video>`. Hardware encode, runs in the background right after probe.
- `thumbnail(asset, t_ms) -> png` via decode-one-frame.

## 5. Transcribe (`transcribe/`)

- `whisper-rs` with `WhisperContextParameters` from the backend. Enable `token_timestamps` and the DTW word alignment where the model supports it.
- Run on 30 s windows with 2 s overlap, merge, then re-split into sentences on punctuation plus silence gaps over 600 ms.
- Clean-up pass: drop hallucinated repeats (same 4-gram three times in a row), clamp word times to segment bounds, mark low-confidence words.
- Model manager: manifests in `models_registry/` with URL, size, blake3, and the minimum memory. Stored under the OS app-data dir (`dirs::data_dir()/choco.video/models`). Download with resume and checksum verification.
- Default models: `ggml-base.en` (preview), `ggml-large-v3-turbo` (quality). CoreML encoder variants on Mac.

## 6. Highlights (`highlights/`)

### 6.1 Candidate generation (`windows.rs`)
- Walk segments. Start a window at each segment start. Extend until 15–90 s, ending on a sentence boundary followed by a gap ≥ 300 ms. Produce overlapping windows with a 50% stride.
- Cheap pre-filters: drop windows with > 25% silence, drop windows with < 20 words, drop windows whose first 3 s are filler.
- Typical 60-minute episode yields 200–400 candidates before scoring.

### 6.2 Shortlist (`embed.rs`)
- Embed every candidate's text with a local embedding model (GGUF via llama.cpp embeddings, or `fastembed` ONNX, decide in week 3 by speed).
- Default mode: cluster, keep the top 60 by a quality prior (word rate, question marks, numbers, named entities, laughter markers).
- Prompt mode: embed the user prompt, keep the top 60 by cosine similarity.

### 6.3 Scoring (`llm/`)
- `trait Picker { fn score(&self, ctx: &EpisodeContext, batch: &[CandidateText]) -> Result<Vec<Score>> }` with three impls: `LocalLlama`, `AppleFoundation` (feature `apple-fm`), `AnthropicCloud` (feature `cloud`).
- `EpisodeContext` = a 300-word summary of the whole transcript (generated once by the same picker) + title + speaker count. Sent with every batch so scores are relative to the episode, not absolute.
- Batches of 8 candidates. Output schema:
  ```json
  {"scores":[{"id":"c17","score":82,"reasons":["strong hook","complete story"],"title":"...","description":"...","hashtags":["..."]}]}
  ```
- `LocalLlama`: GBNF grammar generated from the schema so output always parses. Default model `Qwen3-8B-Instruct Q4_K_M` or `Llama-3.1-8B-Instruct Q4_K_M`, decided on the golden set. Context 8k. Temperature 0.2.
- `AppleFoundation`: Swift shim (`engine/crates/choco-engine/apple/`) exposing a C function that takes a prompt and returns JSON. Built only with `apple-fm` on macOS 26+. Guided generation if the framework version supports it, else strict parse plus one retry.
- `AnthropicCloud`: `POST https://api.anthropic.com/v1/messages` with `reqwest`, headers `x-api-key`, `anthropic-version`. Default model `claude-opus-5-5`, user can pick `claude-haiku-5-5`. Use `output_config.format` with the schema for structured output, `thinking: {type: "adaptive"}`, `output_config.effort: "low"` for scoring batches. Stream not required (short outputs). Key stored in OS keychain via `keyring` crate. Send transcript text only. Retry on 429/5xx with backoff.
- Prompt versions are files under `highlights/llm/prompts/*.md`, referenced by hash in `PickerProvenance`.

### 6.4 Rank and dedupe (`rank.rs`)
- Non-maximum suppression on time overlap > 40%: keep the higher score.
- Prefer diversity: penalize candidates whose embedding is within 0.08 cosine of an already-chosen one.
- Return top N with a minimum score floor (default 55).

## 7. Reframe (`reframe/`)

### 7.1 Detection (`onnx/`)
- Sample frames at 5 fps for the candidate range (plus 1 s padding) at 640px wide.
- Face detector: YuNet ONNX (Apache-2.0, ~300 KB) via `ort`. Returns boxes + 5 landmarks. Fast enough on CPU.
- Landmarks for mouth: either YuNet's 5 points (mouth corners) for a cheap open/closed proxy, or a 68-point model if the heuristic needs it. Start with 5 points.

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
3. **Encoder** (`encode.rs`): hardware encoder from the backend through `ffmpeg-next`. H.264 High, CRF-equivalent quality target, 8–12 Mbps for 1080x1920. Software fallback: `openh264` (BSD) or the platform software path. **Never libx264.**
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
- Licensing: Lemon Squeezy license key activation over HTTPS once, signed grant stored locally, validated offline thereafter. Grace period 14 days if revalidation fails.
- Updater: Tauri updater, signed manifests, separate channels `stable` and `beta`.
- Packaging: Mac `.dmg` Developer ID signed and notarized; Windows `.msi` or NSIS signed. Bundle FFmpeg LGPL dynamic libs beside the binary on both.

## 15. Fixtures and eval (`fixtures/`, `eval/`)

- `fixtures/fetch.sh` downloads 10 CC-licensed long videos (talks, podcasts with two speakers, a stream VOD, phone footage) and records their licenses. Media is gitignored; the script is committed.
- `fixtures/generate.sh` uses system ffmpeg to produce synthetic tests: a moving face rectangle video for tracker tests, a two-speaker split, a silent video, a rotated phone clip.
- `eval/golden.json`: for each fixture, human-chosen highlight ranges (start, end, note). Built by hand in week 3, 5 to 8 per video.
- `eval` runner: precision@5 and recall@10 against golden ranges with 50% overlap tolerance, per picker tier and model. Prints a table and writes `eval/results/<date>-<picker>.json`. A change to the picker is accepted only if precision@5 does not drop.

## 16. CI (`.github/workflows/`)

- `engine.yml`: matrix `macos-14` (arm64) and `windows-latest`. Steps: cache FFmpeg prebuilt, `cargo build --all`, `cargo test --all`, `cargo deny check`, `cargo clippy -- -D warnings`, snapshot render tests on the synthetic fixtures (no model downloads in CI; tests that need models are `#[ignore]` and run locally).
- `app.yml`: builds the Tauri app on both, unsigned, on tags only.

## 17. Milestone definitions of done

- **M1 Engine skeleton (wk 1–2):** workspace builds on both OSes in CI. `choco probe`, `choco transcribe` work on Mac with Metal. `choco doctor` reports backend. FFmpeg LGPL build recipe pinned.
- **M2 Picker (wk 3–4):** `choco highlights` with local llama tier. Golden set built. Eval runner reports a baseline. Prompt mode works.
- **M3 Captions + render (wk 5–6):** `choco render` produces a 1080x1920 clip with `Static` crop and `bold` captions, hardware encoded, snapshot tests green on both OSes.
- **M4 Reframe (wk 7–8):** `Single` and `Split` layouts with tracking and smoothing. Side-by-side with Opus Clip on three fixtures looks as good or better.
- **M5 Mac app (wk 9–10):** Tauri app with drop, processing, review, export, settings, first-run model download, licensing, updater. Signed and notarized DMG. Landing page and demo video. Launch.
- **M6 Windows (wk 11–14):** Windows backend validated on NVIDIA, AMD, Intel, and CPU-only. Signed installer. Beta, then launch.
- **M7 Post-launch (wk 15+):** diarization, YouTube ingest, more styles, hosted option research.

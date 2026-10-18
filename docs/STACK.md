# choco.video — Tech Stack

Canonical list of what we build with and why. Implementation detail is in `TECH.md`; product context in `PLAN.md`.

| Layer | Choice | Why this and not the alternative |
|---|---|---|
| Engine language | Rust (2024 edition) | One codebase for Mac, Windows, Linux CLI. Memory-safe media code. This is what Handy (34k stars) and Cap (23k stars) chose for the same reason. Swift was the first plan and was dropped the moment Windows became a requirement. |
| Platforms | macOS 14+ Apple Silicon only (arm64 build, no universal binary); Windows 11 x64; Linux CLI only | Mac is one hardware family, so v1 is testable on one machine. Single-arch builds halve the binary. Windows needs a compatibility matrix. |
| Engine packaging | Cargo workspace: `choco-engine` (lib), `choco` (CLI), `choco-ffi` (C ABI for shells) | The crate is the open-source unit. The app depends on it exactly as a third party would. |
| Transcription | Apple SpeechAnalyzer on macOS 26+ (zero download, Swift shim); whisper.cpp via `whisper-rs` elsewhere, quantized turbo ~550 MB downloaded on first run | Same `Transcript` type from both engines. Metal on older Macs, Vulkan on Windows, CPU everywhere. No CUDA builds: hundreds of MB for a minority of machines. |
| Picker, hosted (default) | `choco-cloud`: a small Rust `axum` service that calls the Anthropic Messages API, `claude-haiku-5-5` for batch scoring, `claude-opus-5-5` for the episode summary and final titles | Frontier quality with zero download, at ≈ $0.02–0.03 per source hour. Transcript text only. Authenticated by license key, fair-use capped. Deployed as one container. |
| Picker, Apple tier | Foundation Models framework via a small Swift shim behind the `Backend` trait | Zero-download offline option on macOS 26+. Compiled out elsewhere. |
| Picker, BYOK | Anthropic Messages API over `reqwest` from the app, same prompts as hosted | For users who want their own key. |
| Picker, local (opt-in) | llama.cpp via `llama-cpp-2`, 4B GGUF Q4 ~2.5 GB | Fully offline on Windows for those who want it. Code compiled in, model downloaded only when chosen. Metal / Vulkan / CPU. |
| Prompt-mode search | BM25 keyword search over transcript sentences, pure Rust | No embedding model download. Semantic search is a later opt-in. |
| Face detection / landmarks | `tract` (pure-Rust inference) running YuNet ONNX, ~300 KB model | ~3 MB total instead of 20–40 MB for ONNX Runtime. Identical results on both OSes. Fallback if `tract` lacks an op: a minimal ONNX Runtime build with only the needed ops. |
| Video decode / encode | Minimal LGPL FFmpeg via `ffmpeg-next`: `--disable-everything` plus an allowlist of the demuxers, decoders, encoders, filters and hwaccels we use; `--disable-gpl --disable-nonfree` | ~6 MB instead of ~60. Hardware encoders on every machine; software fallback is the OS encoder (VideoToolbox software path, Media Foundation H.264), **never libx264 or openh264**. |
| GPU compose | `wgpu` with only the Metal and DX12 backends compiled | One shader path for crop, scale, blur background, caption blit. Vulkan and GL backends compiled out. |
| Text rasterization | `cosmic-text` + `swash`, three OFL fonts subset to Latin plus common symbols | Same caption pixels on both OSes. No platform text APIs in the render path. ~1 MB of fonts. |
| App shell | Tauri 2 + a small TypeScript UI (Svelte or React) | One UI for both OSes, ~10 MB, native webview, tray support, updater, signing helpers. Cap and Handy prove it feels native enough. |
| Preview playback | Native `<video>` in the webview for rendered proxies; engine generates low-res proxies | Avoids building a custom player. |
| Updates | Tauri updater with signed manifests | Built in. |
| Payments / licensing | Lemon Squeezy (merchant of record) | Handles VAT and license keys. Offline validation after activation. |
| Signing | Developer ID + `notarytool` on Mac; EV or standard code signing cert on Windows | Required for one-click installs. |
| CI | GitHub Actions: `macos-14` (arm64) and `windows-latest` build the engine and run tests on every push from week 1 | Windows compatibility is enforced continuously, not discovered at port time. |
| Tests | Rust unit tests + golden set of fixtures + snapshot tests for render frames | Picker quality is measured, not eyeballed. |
| Dev agent | Claude Code with Claude Opus 5.5, instructions in `AGENTS.md` | Consistent decisions across sessions. |

**Rejected:** Swift/AVFoundation engine (Mac only, would require a full rewrite for Windows), Electron (heavier, slower startup, no advantage over Tauri here), Python + PyInstaller (fragile, slow start, 500 MB apps, every open-source competitor shipped this and lost on polish), statically linked GPL FFmpeg (would make the paid app GPL), Apple-only ML APIs in the core path (would fork the engine), a local 8B picker model as the default (4.5 GB download for worse picks than a hosted frontier model), ONNX Runtime (20–40 MB for a 300 KB model), CUDA builds (hundreds of MB), universal Mac binaries (double size for Intel Macs we do not support).

## Size budget

| Target | Budget | Enforced by |
|---|---|---|
| Installer (DMG / NSIS) | ≤ 25 MB | CI fails the release job over budget |
| Installed app, no models | ≤ 50 MB | CI measures `.app` / install dir |
| First run, current Mac | 0 MB to download | Apple speech + hosted picker |
| First run, Windows or older Mac | ~550 MB, one download | quantized Whisper turbo |
| Fully offline opt-in | +2.5 GB | local picker model, only if chosen |

Release profile: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`, `opt-level = "z"` for the workspace with `opt-level = 3` overridden on `render` and `transcribe`. `cargo bloat` output is attached to every release PR. A new dependency over 2 MB compiled needs a line in `AGENTS.md` Decisions.

**What stays Mac-first even with a cross-platform engine:** the Mac app ships before any Windows UI work starts. Mac gets Apple SpeechAnalyzer and the Apple on-device picker tier; the CoreML Whisper encoder is only an optional speed-up for older Macs. The Mac build is the reference for render quality; Windows must match it pixel-for-pixel on the golden set.

## Pinned versions and crates (fill in at M1, keep current)

| Thing | Version / crate | Notes |
|---|---|---|
| Rust toolchain | stable, `rust-toolchain.toml` pinned | 2024 edition |
| whisper.cpp | `whisper-rs` (pin at M1) | features: `metal` on older macs; `vulkan` on win; shares one ggml with llama.cpp |
| llama.cpp | `llama-cpp-2` (pin at M2) | opt-in `local-llm` feature; `metal` / `vulkan` |
| Apple speech + Foundation Models | Swift shim in `engine/crates/apple/` | macOS 26+ only, availability-gated |
| Face inference | `tract-onnx` | pure Rust, no runtime download |
| Hosted picker | `axum`, `tokio`, `reqwest` | `cloud/`, one container |
| FFmpeg | 7.x LGPL build, `ffmpeg-next` | recipe in `scripts/ffmpeg/`, `--disable-gpl --disable-nonfree` |
| GPU | `wgpu` (pin at M3) | Metal + DX12 backends only |
| Text | `cosmic-text`, `swash` | bundled OFL fonts only |
| CLI | `clap` 4 | derive API |
| Serialization | `serde`, `serde_json` | all stage types |
| Hashing | `blake3` | content hashes, cache keys, model checksums |
| HTTP (optional) | `reqwest` with `rustls` | `cloud` feature only |
| Secrets | `keyring` | OS keychain for BYOK keys and license grant |
| Errors | `thiserror`, `anyhow` | per-module / binaries |
| App | Tauri 2, TypeScript, Vite | Svelte or React, decide at M5 |
| Payments | Lemon Squeezy | license keys, MoR |
| CI | GitHub Actions `macos-14`, `windows-latest` | `cargo deny` license gate |

## Dev prerequisites

- macOS: Xcode command line tools, Rust via rustup, `brew install pkg-config cmake`, FFmpeg LGPL build from `scripts/ffmpeg/`, Node 20+ for the app UI only.
- Windows (M6): Visual Studio Build Tools, Rust via rustup, Vulkan SDK, prebuilt minimal FFmpeg LGPL from `scripts/ffmpeg/`, Node 20+.
- No Python, Ollama, or system ffmpeg is required to run the product. Dev scripts for fixtures use Homebrew ffmpeg.

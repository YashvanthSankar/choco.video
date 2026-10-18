# choco.video — Tech Stack

Canonical list of what we build with and why. Implementation detail is in `TECH.md`; product context in `PLAN.md`.

| Layer | Choice | Why this and not the alternative |
|---|---|---|
| Engine language | Rust (2024 edition) | One codebase for Mac, Windows, Linux CLI. Memory-safe media code. This is what Handy (34k stars) and Cap (23k stars) chose for the same reason. Swift was the first plan and was dropped the moment Windows became a requirement. |
| Platforms | macOS 14+ Apple Silicon first; Windows 11 x64 second; Linux CLI only | Mac is one hardware family, so v1 is testable on one machine. Windows needs a compatibility matrix. |
| Engine packaging | Cargo workspace: `choco-engine` (lib), `choco` (CLI), `choco-ffi` (C ABI for shells) | The crate is the open-source unit. The app depends on it exactly as a third party would. |
| Transcription | whisper.cpp via `whisper-rs` | Metal + CoreML encoder on Mac, CUDA/Vulkan on Windows, CPU everywhere. Word timestamps. MIT. |
| Picker LLM, local | llama.cpp via `llama-cpp-2`, GGUF models | Metal on Mac, CUDA/Vulkan on Windows. Grammar-constrained JSON output. MIT. |
| Picker LLM, Apple tier | Foundation Models framework via a small Swift/ObjC shim behind the `Backend` trait | Zero-download option on macOS 26+. Mac only, optional, compiled out elsewhere. |
| Picker LLM, cloud tier | Anthropic Messages API over `reqwest`, BYOK | Best quality when the user opts in. Transcript text only. Off by default. |
| Embeddings | small GGUF embedding model via llama.cpp, or `fastembed` ONNX | Same on both OSes. Used for prompt-mode shortlist and dedupe. |
| Face detection / landmarks | ONNX Runtime (`ort` crate) with YuNet or SCRFD face model + a landmark model | Identical results on both OSes. CoreML execution provider on Mac, DirectML or CUDA on Windows. Apache-2.0 models. |
| Video decode / encode | FFmpeg libraries via `ffmpeg-next`, dynamically linked LGPL build, hardware codecs through FFmpeg hwaccel | Uniform container support on both OSes. LGPL dynamic linking is compatible with a proprietary app. **No GPL components** (no libx264, no libx265); hardware encoders plus an LGPL software fallback (`openh264` or VideoToolbox/MF software paths). |
| GPU compose | `wgpu` | One shader path for crop, scale, blur background, caption blit on Metal, DX12, Vulkan. |
| Text rasterization | `cosmic-text` + `swash` | Same caption pixels on both OSes. No platform text APIs in the render path. |
| App shell | Tauri 2 + a small TypeScript UI (Svelte or React) | One UI for both OSes, ~10 MB, native webview, tray support, updater, signing helpers. Cap and Handy prove it feels native enough. |
| Preview playback | Native `<video>` in the webview for rendered proxies; engine generates low-res proxies | Avoids building a custom player. |
| Updates | Tauri updater with signed manifests | Built in. |
| Payments / licensing | Lemon Squeezy (merchant of record) | Handles VAT and license keys. Offline validation after activation. |
| Signing | Developer ID + `notarytool` on Mac; EV or standard code signing cert on Windows | Required for one-click installs. |
| CI | GitHub Actions: `macos-14` (arm64) and `windows-latest` build the engine and run tests on every push from week 1 | Windows compatibility is enforced continuously, not discovered at port time. |
| Tests | Rust unit tests + golden set of fixtures + snapshot tests for render frames | Picker quality is measured, not eyeballed. |
| Dev agent | Claude Code with Claude Opus 5.5, instructions in `AGENTS.md` | Consistent decisions across sessions. |

**Rejected:** Swift/AVFoundation engine (Mac only, would require a full rewrite for Windows), Electron (heavier, slower startup, no advantage over Tauri here), Python + PyInstaller (fragile, slow start, 500 MB apps, every open-source competitor shipped this and lost on polish), statically linked GPL FFmpeg (would make the paid app GPL), Apple-only ML APIs in the core path (would fork the engine).

**What stays Mac-first even with a cross-platform engine:** the Mac app ships before any Windows UI work starts. Mac gets the Apple on-device picker tier and the CoreML Whisper encoder. The Mac build is the reference for render quality; Windows must match it pixel-for-pixel on the golden set.

## Pinned versions and crates (fill in at M1, keep current)

| Thing | Version / crate | Notes |
|---|---|---|
| Rust toolchain | stable, `rust-toolchain.toml` pinned | 2024 edition |
| whisper.cpp | `whisper-rs` (pin at M1) | features: `metal`, `coreml` on mac; `cuda`/`vulkan` on win |
| llama.cpp | `llama-cpp-2` (pin at M1) | features: `metal` / `cuda` / `vulkan` |
| ONNX Runtime | `ort` 2.x | execution providers: CoreML, DirectML, CUDA, CPU |
| FFmpeg | 7.x LGPL build, `ffmpeg-next` | recipe in `scripts/ffmpeg/`, `--disable-gpl --disable-nonfree` |
| GPU | `wgpu` (pin at M3) | Metal / DX12 / Vulkan |
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
- Windows (M6): Visual Studio Build Tools, Rust via rustup, Vulkan SDK, optional CUDA toolkit, prebuilt FFmpeg LGPL from `scripts/ffmpeg/`, Node 20+.
- No Python, Ollama, or system ffmpeg is required to run the product. Dev scripts for fixtures use Homebrew ffmpeg.

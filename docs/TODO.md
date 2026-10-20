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
- [x] Apple speech vs whisper on 11 languages (`fixtures/lang_eval.py`, synthesized voices) → Apple equal or better in 9 of 11; a language Apple does not support (Dutch) now falls back to whisper automatically; the OS cap of five reserved locales is released by the helper. whisper words are now decoded from token bytes (non-Latin scripts were garbled); Apple returns Hindi romanized, so Hindi routes to whisper when installed
- [ ] `LocalLlama` tier (`local-llm` feature, 4B GGUF, opt-in download) — deferred, not needed for the Mac launch

## Done: M3 Captions + render

- [x] Captions: ≤ 4-word chunks on clause punctuation, pauses and measured width; four presets (clean, bold, karaoke, minimal); bundled OFL fonts (233 KB); deterministic rasterizer (shadow, outline, active-word colour and scale, progressive highlight, pill); SRT + VTT sidecars → pixel-identical output test
- [x] Render: constant-frame-rate from VFR sources, any codec normalised to 8-bit 4:2:0, display rotation, interpolated `CropPlan` (static / single / split / fit with blurred background), caption sprites blended in YUV, `h264_videotoolbox` High with `allow_sw`, AAC at 48 kHz with `loudnorm` −14 LUFS (measured −14.5), MP4 `+faststart` → 10 integration tests + 3 snapshot tests; 45–62 s clips render at 5–6x realtime
- [x] Export: size presets (9:16, 1:1, 4:5, 16:9), `<source>-<rank>-<slug>` naming, poster PNG, SRT, VTT, metadata JSON (title, description, hashtags, score, reasons, timecodes, picker provenance); re-export a clip with another style/size/framing without re-transcribing
- [x] `choco render` and `choco run`; engine `pipeline` module shared by CLI and app (temp files cleaned on every path, cancellation between stages, batches and clips)
- [~] Word timing: bias correction, whisper DTW token times (−200 ms) and audio-onset snapping within ±90 ms → median 45 / 72 / 60 ms (Apple) and 54 / 66 / 64 ms (whisper) on three voices, from 52–95 ms; p90 down from ~250 to ~165 ms for whisper. The ≤ 50 ms target is met on one voice of three. Next: a forced-alignment pass on the 16 kHz audio around each DTW time

## Done: M4 Reframe

- [x] YuNet face detection (OpenCV Zoo, MIT, 233 KB, compiled in) on `tract` at 640×640 → 22 ms per frame on CPU
- [x] 5 fps sampling through the shared decode path; IoU tracking with scene-cut reset; mouth-motion speaker heuristic with a 1 s switch lead
- [x] Planner: speaker follow with dead zone and eased moves, per-shot following for edited multi-camera footage, split screen when two people share the frame (captions move to the seam), centre crop with no faces → 6 tests; checked by eye on four real recordings
- [ ] Side-by-side against Opus Clip on three fixtures (needs an Opus Clip account; human task)

## Now: M5 Mac app + launch

- [x] Tauri 2 + Svelte 5 app over the engine: onboarding (privacy line → picker with live key test and pre-selected models showing cents per hour → speech setup), home (drop or choose a file, prompt, count, style, size, framing, output folder), job progress with cancel, results grid (poster, inline playback, Show in Finder, copy caption, restyle), settings → used for a real run by the project owner
- [x] Keychain-only key storage; key saved only after the model list succeeds; strict CSP; scoped asset protocol; capabilities limited to dialogs, reveal, and OpenCode/GitHub links; app `deny.toml` passes
- [x] Release bundle: `scripts/build-app.sh` (Apple helper sidecar, six FFmpeg dylibs in `Contents/Frameworks`, no build-machine rpath in release) → `choco.video.app` 20 MB, DMG 9.9 MB, launches and stays up
- [x] README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, LICENSE, issue and PR templates; landing page redesigned (taste-skill, 0 FAIL), live at https://chocovideo.vercel.app with a 30 s animated film
- [x] UI redesign (macOS-native, light/dark, stage list, results grid), reviewed screen by screen
- [x] Release workflow (`release.yml`): tag → build → DMG on GitHub Releases, signing/notarization when the Apple secrets exist
- [x] **v0.1.1 released**: https://github.com/YashvanthSankar/choco.video/releases/tag/v0.1.1 (10 MB DMG, verified to launch from a fresh download once opened via right-click → Open). v0.1.0 is marked superseded: hardened runtime refused its own FFmpeg dylibs, so it died at launch. The build script now launches the app and fails if it exits. The repo is still private, so the website's download link returns 404 until it is made public
- [ ] Golden set human review (you): watch each clip in `eval/REVIEW.md`, fix or replace, set `reviewed_by_human`; then rerun `choco eval` per model and freeze `OPENCODE_RECOMMENDED`
- [x] Pushed; first CI runs found two real gaps (the bundled face model was gitignored, fixtures never generated) and an unmaintained-crate advisory → fixed
- [ ] Developer ID signing + notarization (needs your Apple Developer account); without it the DMG only runs after right-click → Open
- [ ] Tauri updater with signed manifests and a release channel
- [~] App icon: an interim icon ships in 0.1.0; the final logo is still being chosen (four directions rejected so far)
- [ ] First-launch guidance when Apple Intelligence is off and no key is set (today the Apple option is disabled with a one-line reason)
- [x] 40 s launch video from real choco.video output (HyperFrames project in `videos/choco-launch`), in the site hero
- [ ] Launch posts (Product Hunt, r/podcasting, r/NewTubers, creator Discords)

## M6 Windows (x64)

Gate met: the Mac app shipped (v0.1.1). Open M5 items (signing, updater, launch posts) still win when they compete for time. Target is Windows 11 x64 (`x86_64-pc-windows-msvc`) only; **ARM64 Windows is deferred** (no build, no testing, the site says x64). Implementation: `TECH.md §19`. The order of work is at the end of this section; "Needs you" items are 5–7 in the list at the bottom of the file.

Groundwork already in the repo: `backend/windows.rs` stub (CPU whisper, Media Foundation encode), `HwCodec` and `Device` already carry every Windows variant, interim general LGPL FFmpeg build pinned with sha256 in `scripts/ffmpeg/README.md`.

### Engine prep (platform-neutral, done and verified on the Mac first)

- [ ] Whisper loop recovery: re-decode windows that `cleanup` collapsed, with temperature fallback (or VAD-split input). Windows has no other speech engine → on the 72-min podcast whisper keeps ≥ 97% of Apple's word count (today 89%), wall time ≤ +15%
- [ ] Encoder choice leaves `render/`: `RenderSpec` carries the backend's ordered `hw_encoders()`; a table maps `HwCodec` → FFmpeg encoder name, input pixel format, options; the first that opens wins and `RenderReport.video_encoder` names it → Mac output unchanged (same encoder, same options), 7 render tests + 3 snapshots green
- [ ] NV12 output from the compositor for encoders without planar input (`h264_qsv`) → unit test: NV12 ⇄ YUV420P round-trip is exact
- [ ] Target-safe Cargo features: whisper-rs `metal` moves to a macOS-only dependency table, `vulkan` stays an explicit feature; plain `cargo build --all` works on both OSes → `cargo tree -e features` for the Mac default build unchanged
- [ ] `-Wl,-rpath` link args in the four `build.rs` files emitted for macOS only (MSVC `link.exe` does not take them)
- [ ] Models under `dirs::data_local_dir()` instead of `data_dir()` → same path on macOS; `%LOCALAPPDATA%\choco.video\models` on Windows instead of roaming `%APPDATA%`
- [ ] App default output folder from Tauri's `path().video_dir()` instead of `$HOME/Movies` (`HOME` is unset on Windows) → Mac default unchanged
- [ ] UI strings that name the OS (Finder, Keychain, "this Mac", Apple options) driven by `status.hardware.os` → every Mac screen unchanged
- [ ] `Backend::verify(engine, model)` (default `true`) so a backend can prove an accelerated engine before a long job; the pipeline falls through `speech_engines()` on failure → Mac behaviour unchanged

### CI

- [ ] Mac job uploads the synthetic fixtures (`engine/fixtures/media`) as an artifact; the Windows job downloads them (`generate.sh` needs macOS `say`) → both OSes test the same bytes
- [ ] `windows-latest` job on the interim BtbN LGPL build: toolchain from `rust-toolchain.toml` (add `x86_64-pc-windows-msvc` to `targets`), `FFMPEG_DIR` + `LIBCLANG_PATH`, FFmpeg `bin/` on `PATH`, `cargo build --release --all`, `cargo test --all`, `cargo clippy --all --all-targets -- -D warnings` → green on the stub backend; first Windows feedback loop without a Windows machine
- [ ] Switch the job to the minimal prebuilt (below), fetched by URL and checked against the sha256 in `scripts/ffmpeg/VERSIONS`
- [ ] Vulkan SDK (LunarG, pinned version) installed in the job; build, test and clippy with `--features choco-engine/vulkan`. The runner has no GPU, so the suite also proves the CPU fallback
- [ ] `cargo deny check` already resolves every target; confirm the Windows-only crates (`windows-native-keyring-store`, `windows`, `webview2-com`, …) pass licenses and bans → no `deny.toml` change
- [ ] Import check (`dumpbin /dependents`) on `choco.exe`, the app exe and every FFmpeg DLL → fails on `vcruntime140*.dll`, `msvcp140.dll`, `api-ms-win-crt-*`, `libgcc*`, `libwinpthread*`, or a `vulkan-1.dll` that is not delay-loaded
- [ ] `scripts/size-check.sh` learns Windows (`*.exe`, `$FFMPEG_DIR/bin/*.dll`, `7z` when `zip` is missing) → job summary table; fails over 50 MB installed / 25 MB installer estimate
- [ ] Launch smoke test: the bundled app starts from its install folder and is still running after 6 s (port of the `build-app.sh` check that caught 0.1.0)

### Minimal LGPL FFmpeg for Windows x64

- [ ] Toolchain: MSVC (`--toolchain=msvc`) driven from MSYS2 bash, static CRT (`-MT`) → Decisions entry in `AGENTS.md`
- [ ] `scripts/ffmpeg/build-windows.sh`, a mirror of `build-mac.sh`: FFmpeg 8.1.3 tarball checked against `VERSIONS`; `--disable-everything` with the same demuxer, decoder, parser, bsf, muxer and filter lists; encoders `h264_mf h264_nvenc h264_amf h264_qsv aac pcm_s16le pcm_f32le`; hwaccels `h264/hevc/vp9/av1_d3d11va` and `_d3d11va2`; `--enable-w32threads --enable-d3d11va --enable-mediafoundation --enable-ffnvcodec --enable-nvenc --enable-amf --enable-libvpl --enable-libdav1d`
- [ ] dav1d 1.5.4 static (meson, `-Db_vscrt=mt`) from the same VideoLAN-checksummed tarball → `tests/media.rs` `av1_opus_webm` passes
- [ ] Third-party inputs pinned with sha256 in the script: nv-codec-headers n11.1.5.3 (MIT; oldest line FFmpeg 8.1 accepts, NVIDIA driver ≥ 471.41), AMF headers (MIT), libvpl dispatcher built static (MIT)
- [ ] Gates after configure: `CONFIG_GPL 0`, `CONFIG_NONFREE 0`, `HAVE_W32THREADS 1`, every allowlisted encoder and hwaccel `1`; the script aborts otherwise (`--disable-autodetect` silently drops anything not named)
- [ ] `ffmpeg-windows.yml` (manual dispatch) builds on `windows-latest` and publishes `ffmpeg-8.1.3-win64-lgpl-minimal.zip` (`bin/*.dll`, `lib/*.lib`, `include/`, license texts) as a release asset; URL and sha256 recorded in `scripts/ffmpeg/README.md` and `VERSIONS`; the interim 166 MB BtbN entry is deleted
- [ ] Size → six DLLs total ≤ 12 MB (Mac: 8.3 MB); recorded in the README table and in Benchmarks
- [ ] Not built: DXVA2 and D3D12VA (D3D11VA covers every Windows 11 driver), `hevc_*` encoders (output is H.264 only), `aac_at` (Apple only; the native `aac` encoder is what render uses on both OSes)

### Backend (`backend/windows.rs`)

- [ ] `HardwareInfo`: OS build via `RtlGetVersion`, physical memory via `GlobalMemoryStatusEx`, logical cores, GPUs via DXGI (`CreateDXGIFactory1` → `EnumAdapters1`: name, PCI vendor id, dedicated and shared memory, UMD driver version; the software adapter skipped), through the `windows` crate 0.62 already in the app's lockfile → `choco doctor` lists both GPUs on a hybrid laptop
- [ ] `hw_encoders()`: vendor encoders ordered discrete before integrated (NVIDIA → `Nvenc`, AMD → `Amf`, Intel → `Qsv`), then `MediaFoundation` last; never empty
- [ ] `hw_decoders()`: `[Software]` until the D3D11VA item below says otherwise (the Mac also decodes in software today)
- [ ] `speech_engines()`: whisper only. `[Vulkan, Cpu]` when the build has `vulkan`, `vulkan-1.dll` loads from System32 and no failed verdict is cached for this adapter + driver; otherwise `[Cpu]`
- [ ] CPU baseline AVX2 + FMA + F16C (ggml built with `GGML_NATIVE=OFF`, so a CI runner with AVX-512 cannot leak into the binary); no AVX2 → whisper reported unavailable with a plain reason, never an illegal-instruction crash
- [ ] `recommended_models()` unchanged (turbo at ≥ 8 GiB, base below)
- [ ] `choco doctor` on Windows: adapters and driver versions, encoders that opened (`--probe-encoders` opens each and encodes two 1080x1920 frames), Vulkan verdict, AVX2, Media Foundation present
- [ ] Unit tests on Windows: memory > 0, OS version non-empty, encoder list ends with `MediaFoundation`; `detect_reports_something_sane` passes unchanged

### Transcription (whisper.cpp)

- [ ] `vulkan` feature builds with whisper-rs 0.16 / whisper-rs-sys 0.15 against the LunarG SDK; `vulkan-1.dll` delay-loaded (`/DELAYLOAD:vulkan-1.dll` + `delayimp.lib` from each binary's `build.rs`) → the app starts on a machine with no Vulkan driver (CI runner)
- [ ] Vulkan self-test in a child process (same exe, hidden argument, `CREATE_NO_WINDOW`): load the downloaded model with `use_gpu(true)`, run 2 s of audio, 60 s timeout. Crash, error or timeout → CPU. Verdict cached in `%LOCALAPPDATA%\choco.video\gpu-check.json`, keyed by adapter, driver version and engine version → forced-failure test passes
- [ ] Runtime fallback: an error from whisper on Vulkan reruns the file on CPU and writes a failed verdict → test with a forced failure
- [ ] Size: measure the `vulkan` delta on the exe (ggml-vulkan embeds SPIR-V for every shader variant). If the installer goes over 25 MB with it, Windows v1 ships CPU whisper and the choice goes in Decisions; no ggml fork
- [ ] Threads: `n_threads` = physical cores, capped at 8 → speed recorded per matrix machine
- [ ] Models: same manifests and sha256; download resumes across an app restart; `%LOCALAPPDATA%` path with a non-ASCII user name works
- [ ] Speed on the 72-min podcast per matrix machine → targets ≥ 3x realtime on an 8-core AVX2 CPU, ≥ 10x on a discrete GPU with Vulkan; Intel-iGPU-only machines default to CPU unless Vulkan is ≥ 1.5x faster there
- [ ] Word count and timing vs the Mac whisper run on the same file → same words within 1%, same timing calibration (−200 ms) holds within 30 ms median

### Faces, captions, render

- [ ] Faces: `tract-onnx` 0.23 is pure Rust on x86_64 (no C in `cargo tree`), YuNet compiled in via `include_bytes!` → detections on fixture frames match a Mac-committed reference within 1 px per box edge and 0.01 score
- [ ] Captions: the rasterizer loads only the bundled fonts (no system fonts) → new cross-OS test: blake3 of the RGBA raster for each preset matches a Mac-committed hash. A mismatch gets its cause found (libm call, SIMD path), not a second reference
- [ ] Render snapshots (3) pass against the Mac references unchanged (MAD ≤ 2.0 luma, composed before encoding); no Windows-specific references
- [ ] Each of `h264_nvenc`, `h264_amf`, `h264_qsv`, `h264_mf` produces a 1080x1920 H.264 High, constant-frame-rate, `+faststart` MP4 that probes back with the requested frame count → PSNR of encoded vs composed frames ≥ 38 dB at 10 Mbps (`#[ignore]` test, run on matrix hardware)
- [ ] Encoder failure mid-clip → the clip is rendered once more with the next encoder; the error lists every encoder tried and why it failed
- [ ] Intel: compare `h264_qsv` with `h264_mf` + `hw_encoding=1` (Intel's hardware MFT) → if within 0.5 dB PSNR at the same bitrate, drop QSV and libvpl
- [ ] Windows N without the Media Feature Pack (no `mfplat.dll`): message names the Media Feature Pack instead of "encoder missing"
- [ ] Render speed, 62 s clip → ≥ 3x realtime with a hardware encoder, ≥ 1.5x with the Media Foundation software MFT (Mac: 5.7x)
- [ ] D3D11VA decode only if measured: a 4K HEVC 60 fps phone clip renders below 1.5x realtime on the weakest matrix machine → then decode through D3D11VA and transfer to system memory for the CPU compositor
- [ ] Paths: a source under a non-ASCII folder and one with a > 260-character path go through probe, render and export (test copies a fixture there); never hand a `\\?\` verbatim path to FFmpeg or the webview

### Apple-only paths on Windows

- [ ] Apple speech and the Apple picker stay compiled and are runtime-gated (`helper_path()` is `None` off macOS) → Windows tests: `transcribe(.., AppleSpeech, ..)` returns `Unavailable`, `picker::apple::available()` is false, `build.rs` never runs `swiftc`
- [ ] VideoToolbox: only reachable through the encoder table row, which the Windows backend never lists; no `cfg` in `render/`
- [ ] App: Apple options hidden on Windows (no "needs macOS 26" text); onboarding goes provider key → whisper download → GPU check
- [ ] Keychain → Windows Credential Manager through the existing `keyring` 4.2 (default `v1` feature pulls `windows-native-keyring-store` 1.1 on Windows): same service and user names; key saved, read after restart, deleted; a key over Credential Manager's 2,560-byte limit is refused with a message; never written anywhere else → app settings test runs on the Windows runner

### App (Tauri) on Windows

- [ ] `tauri.windows.conf.json` (merged over `tauri.conf.json` by Tauri): window array without `titleBarStyle` and `hiddenTitle`, `bundle.targets: ["nsis"]`, the six FFmpeg DLLs as `resources` mapped beside the exe, `bundle.windows` settings (`TECH.md §19.7`)
- [ ] WebView2: `webviewInstallMode: downloadBootstrapper` (silent); Windows 11 ships WebView2 → install tested on a clean Windows 11 VM, offline and online
- [ ] Static CRT: Tauri's `build.windows.staticVCRuntime` (default on) for the app, `+crt-static` for the CLI → no VC++ redistributable; import check passes on the installed files
- [ ] Data paths: settings `%APPDATA%\video.choco.app\settings.json`, WebView2 profile `%LOCALAPPDATA%\video.choco.app`, models `%LOCALAPPDATA%\choco.video\models`, temp `%TEMP%`, default output `%USERPROFILE%\Videos\choco`
- [ ] Asset protocol: static scope `$HOME/**` plus runtime `allow_file` / `allow_directory` for the source and output folder the user picks → a clip exported to `D:\` or a USB drive plays inline
- [ ] File dialogs (tauri-plugin-dialog), dropping a file on the window, "Show in Explorer" (`revealItemInDir` selects the file), open output folder → each checked by hand on the matrix
- [ ] Title bar: native caption buttons, no traffic-light padding in the header; `Segoe UI Variable` first in the font stack on Windows; light and dark follow the Windows setting
- [ ] Inline playback of exported H.264/AAC in WebView2 → works on non-N editions; N editions get the Media Feature Pack message
- [ ] "Copy diagnostics" in Settings: backend report JSON (OS, GPUs, drivers, encoder, Vulkan verdict, versions); no media, transcript or key → used by the beta issue template; nothing is sent anywhere

### Installer, signing, release

- [ ] NSIS, per-user (`installMode: currentUser`, no admin prompt), LZMA; no MSI → Decisions entry
- [ ] Layout: `%LOCALAPPDATA%\choco.video\` holds the exe, six FFmpeg DLLs and `uninstall.exe`; Start menu shortcut; no PATH change; no registry beyond the uninstall key
- [ ] Upgrade 0.x → 0.y keeps models and settings; uninstall keeps them unless "Delete the application data" is ticked, which also removes `models\` and `gpu-check.json` through an NSIS uninstall hook → tested on a VM
- [ ] Size → `setup.exe` ≤ 25 MB, installed folder ≤ 50 MB without models, enforced by the release job
- [ ] Authenticode SHA-256 with RFC 3161 timestamp on every PE file: app exe, six DLLs (signed before bundling), `uninstall.exe`, `setup.exe` → `signtool verify /pa /all` passes; Smart App Control does not block the app or its DLLs
- [ ] Signing provider: Azure Trusted Signing through `bundle.windows.signCommand` if the account is eligible, else SignPath Foundation (free for open source); no EV certificate (it no longer skips SmartScreen) → Decisions entry
- [ ] SmartScreen: beta builds signed with the release identity so reputation accrues to it; warnings recorded per tester; false positives submitted to Microsoft
- [ ] `release.yml` `windows` job: same tag, signed `choco.video-windows-x64-setup.exe` + `.sha256` on the release; site and README get a Windows download link
- [ ] Updater (after the Mac updater item in M5): same signed manifest, Windows `installMode: passive`

### Hardware matrix (beta)

Each row records: encoder that opened, whisper device and verdict, 72-min transcribe time, 62 s render time, full app run start to export without a crash.

| Row | Example hardware | Encoder expected | Whisper expected |
|---|---|---|---|
| Intel iGPU, current | 12th gen+ Iris Xe, Core Ultra Arc iGPU | QSV (or MF hardware) | Vulkan if ≥ 1.5x CPU, else CPU |
| Intel iGPU, Windows 11 minimum | 8th gen laptop, UHD 620 | QSV | CPU |
| NVIDIA discrete | GTX 16xx, RTX 20/30/40 | NVENC | Vulkan |
| AMD discrete | RX 6000 / 7000 | AMF | Vulkan |
| AMD APU | Radeon 680M / 780M | AMF | Vulkan or CPU (measured) |
| Hybrid laptop | Intel iGPU + NVIDIA dGPU | NVENC | Vulkan on the dGPU |
| No GPU | Windows 11 VM, Basic Display Adapter | MF software | CPU |
| Windows 11 N | any, no Media Feature Pack | message | CPU |
| ARM64 Windows | Snapdragon X | deferred, not tested | deferred |

- [ ] Every row except ARM64 has ≥ 2 passing reports before launch

### Beta (20 testers)

- [ ] ≥ 20 testers covering the matrix: ≥ 3 per vendor, ≥ 2 hybrid laptops, ≥ 2 low-end or no-GPU machines
- [ ] Signed pre-release on GitHub Releases; "Windows beta report" issue template that asks for the "Copy diagnostics" JSON and the clip count, never the video
- [ ] Two weeks → exit when: zero crashes at launch, ≥ 95% of reported full runs finish, no open P0/P1, every matrix row covered
- [ ] Launch: release notes, README and site (Windows button, "Mac and Windows"), Windows numbers in Benchmarks

### Order of work (each step unblocks the next)

1. Engine prep on the Mac → a Windows build needs no further engine refactor; Mac tests stay green throughout.
2. Windows CI on the interim FFmpeg → build, test and clippy feedback with no Windows machine.
3. Minimal FFmpeg build and release asset → hardware encoders exist to call; first real Windows size numbers.
4. Backend detection → encoder order, whisper device choice, `choco doctor` output.
5. Encoder table in render → end-to-end clips on any Windows machine (MF software on CI).
6. Whisper Vulkan, delay-load, self-test, size measurement → the speech default and whether Vulkan fits the installer budget.
7. Cross-OS determinism (faces, captions, snapshots) → "same output as the Mac" is measured, not assumed.
8. App shell, keyring, paths, asset scope → an app worth packaging.
9. NSIS, size gate, launch smoke test → a release artifact to sign.
10. Signing → beta without "unknown publisher"; SmartScreen reputation starts accruing.
11. Hardware matrix and beta → defaults settled (Intel Vulkan vs CPU, QSV vs MF hardware, D3D11VA or not).
12. Launch.

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
2. Decide when to make the repository public: the release and the site's download link only work for collaborators until then.
3. An Apple Developer account for signing, when you want a DMG others can open without the right-click dance.
4. An app icon, or say the word and I draft one.
5. M6: a Windows 11 x64 machine (or remote access to one) with a GPU, ideally one NVIDIA desktop and one Intel laptop. CI covers build and tests; encoders, Vulkan, the installer and the UI need real hardware from step 5 of the M6 order on.
6. M6: code signing. An Azure account for Trusted Signing (identity validation, about $10/month) or an application to SignPath Foundation, plus the GitHub secrets either one needs. Without it the beta shows "Windows protected your PC".
7. M6: recruiting 20 Windows testers who cover the hardware matrix (creator Discords, r/podcasting, r/NewTubers), and a yes on the beta report template.

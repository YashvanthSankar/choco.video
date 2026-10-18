# choco.video — Open-source competitors

What the open-source clippers actually ship, measured on 2026-10-18 from their repos, and how we stand first. Re-check quarterly; these projects move.

## 1. The field at a glance

| Project | Stars | License | Runtime | Shipped as | Install burden | Last push |
|---|---|---|---|---|---|---|
| AI-Youtube-Shorts-Generator | 5,445 | MIT | Python | scripts | high, and the clipping now calls a paid cloud API (MuAPI) | 2026-10-11 |
| SupoClip | 1,495 | AGPL-3.0 | Python + web | self-host or their hosted SaaS; iOS app | high | 2026-10-16 |
| short-video-maker | 625 | MIT | Python | CLI + local web | high | 2026-10-17 |
| ViralCutter | 427 | GPL-3.0 | Python | Colab / CLI | high | 2026-09-10 |
| HotClip | 339 | AGPL-3.0 | TypeScript + Python | real desktop app, Chinese-first | low | 2026-10-08 |
| **AutoClip** (artbyjazi) | 183 | MIT | Python 3.11/3.12 + React | FastAPI server + browser UI | high | 2026-08-01 |
| **chopify** (mehbul) | 40 | MIT | Python | CLI | high | 2026-09-06 |
| **Klipzy Studio** (TechFreq) | 5 | custom "open-attribution" | Python + Electron | desktop app via start scripts | high | 2026-10-02 |
| **OpenClip Studio** (paragbml) | 3 | MIT | Node + Python | server + React | high | 2026-09-22 |

The four in bold are the ones the user flagged. The pattern across all nine: **Python plus system ffmpeg plus Ollama plus Node**, installed by hand, run from a terminal or a localhost web page. Not one ships a signed installer that a non-developer can double-click. That is the gap, and it is the whole product.

## 2. The four, in detail

### 2.1 AutoClip — artbyjazi/autoclip
- **What ships:** ingest (file or YouTube via yt-dlp) → Whisper transcription → LLM highlight pick (Ollama local, or BYOK Anthropic / OpenAI / Gemini / any OpenAI-compatible) → speaker-tracked reframe with MediaPipe → four caption styles burned with libass → export at three aspect ratios → review UI in the browser.
- **Stack:** Python 3.11/3.12 only (MediaPipe has no 3.13 wheels), FastAPI, React, ffmpeg *full* build required for libass and libx264, Node to build the UI, optional GPU.
- **Good ideas worth adopting:** the privacy rule "only transcript text ever goes to a provider" stated up front; `autoclip doctor` that tells you exactly what to fix; a stated quality gate (reframe validated against a golden set before tagging v0.1); honest "what has and hasn't been verified" section; CHANGELOG, SECURITY, CODE_OF_CONDUCT from day one.
- **Weaknesses to exploit:** install is a five-step terminal ritual with a Python version trap and an ffmpeg-flavour trap that the README itself admits "installs cleanly, then fails". Runs a server and a browser tab. Depends on libx264, so it can never be a proprietary binary. Last push August 2026; momentum stalled. Pre-1.0 by its own admission.

### 2.2 OpenClip Studio — paragbml/open-clip-studio
- **What ships:** YouTube/Vimeo/URL import, Faster-Whisper INT8 on CPU, heuristic "viral discovery" (hook detection, sentiment, pause analysis, speaker turns), YOLOv8n subject tracking, dual-speaker split screen, jump-cut manifests with SFX cue points, kinetic caption presets (Hormozi, MrBeast, Minimal, Neon, Typewriter), studio editor, 1080x1920 export.
- **Stack:** Node server + Python workers + React client, Docker compose, hosting guide.
- **Good ideas:** the caption preset naming (creators search for "Hormozi captions"); engagement-pause analysis as a scoring feature; split-screen divider styling.
- **Weaknesses:** 3 stars, 10 days of commits, then quiet. Scoring is heuristic, not model-based. YOLO object detection for faces is the wrong tool (it finds people, not faces, so framing is loose). SFX overlays are a gimmick. Two runtimes to install.

### 2.3 chopify — mehbul/chopify
- **What ships:** one-command pipeline: yt-dlp → faster-whisper → scoring → render. Deterministic heuristic scoring out of the box, optional Ollama. **Complete-thought clips** that end on terminal punctuation. Keyword search. Review table and render-only-selected. 480p preview drafts. Tighten (filler words and silences cut, captions retimed). Caption presets. `.meta.json` plus PNG poster per clip. 16:9, 9:16, 1:1. YuNet face tracking with snap-on-cut. Loudness normalization. Pre-flight check.
- **Stack:** pure Python, ffmpeg with libx264, OpenCV, CLI only.
- **Good ideas, the most of any competitor:** complete-thought boundaries (their README calls it "the #1 gripe with AI clippers", and they are right); preview drafts before full render; tighten with caption retiming; the meta JSON + poster sidecar; loudnorm; "content decides the count" instead of always returning N clips; `llms.txt` so AI agents can read the project.
- **Weaknesses:** no UI at all. Heuristic scoring by default means mediocre picks. CLI-only ceiling of maybe a few hundred users. Single author, 40 stars.

### 2.4 Klipzy Studio — TechFreq/Klipzy-Studio
- **What ships:** the longest feature list in the field. Virality and hook detection with audio energy spikes, 22 karaoke caption presets as `.ass`, interactive caption editor, NLE exports (Premiere XML, Resolve EDL, CapCut draft), silence cutter, profanity bleep, gaming/reaction layout with webcam PiP, speaker-aware tracking, AI edit chat, local model catalog matched to hardware, dependency checker that installs Ollama/FFmpeg/PyTorch, rearrange and export a highlights reel, SRT/VTT/ASS and MP3 alongside each clip, any aspect ratio.
- **Stack:** Python server + Electron UI, launched by `.bat` and `.command` scripts, Ollama for LLM, x264 CPU fallback, NVENC and VideoToolbox when available.
- **Good ideas:** NLE export is a real pro feature nobody else has in a GUI; hardware-matched model catalog; gaming layout; subtitle sidecars; reel export; a "dependency checker" that acknowledges the install problem instead of ignoring it.
- **Weaknesses:** 5 stars. Custom non-OSI "open-attribution" license, which keeps contributors away. Electron plus Python plus Ollama plus FFmpeg plus PyTorch is the heaviest install of the nine. Feature breadth over depth; nothing suggests the picker or the reframe are good, only that they exist.

## 3. Feature parity matrix

✅ ships, ◐ partial or heuristic, ✗ absent. Last column is our v1 commitment.

| Feature | AutoClip | OpenClip Studio | chopify | Klipzy | SupoClip | **choco v1** |
|---|---|---|---|---|---|---|
| Local Whisper, word timestamps | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| LLM highlight picking | ✅ Ollama local | ✗ heuristic | ◐ opt-in Ollama | ✅ Ollama local | ✅ hosted | ✅ hosted frontier model by default; Apple on-device, BYOK, or opt-in local 4B |
| BYOK cloud picking | ✅ | ◐ | ✗ | ✅ | ✅ | ✅ off by default |
| Prompt / keyword search | ✗ | ✗ | ✅ | ✗ | ✗ | ✅ |
| Complete-thought boundaries | ✗ | ◐ | ✅ | ✗ | ✗ | ✅ |
| Face tracking reframe | ✅ MediaPipe | ◐ YOLO | ✅ YuNet | ✅ | ✅ | ✅ YuNet, eased, dead-zone |
| Split screen two speakers | ✗ | ✅ | ✗ | ✗ | ✅ | ✅ |
| Active-speaker switching | ✅ | ◐ | ✗ | ✅ | ✗ | ✅ heuristic, diarization v1.5 |
| Word-synced caption styles | ✅ 4 | ✅ 5 | ✅ 4 | ✅ 22 | ✅ | ✅ 4 at launch, community styles |
| Caption editor | ✗ | ✅ | ✗ | ✅ | ◐ | ✅ transcript edit + style |
| Review and trim UI | ✅ | ✅ | ◐ table | ✅ | ✅ | ✅ |
| Preview before render | ✗ | ✅ | ✅ 480p | ✅ | ✅ | ✅ proxy |
| Multi-aspect 9:16 / 1:1 / 4:5 | ✅ 3 | ✗ | ✅ 3 | ✅ | ✗ | ✅ |
| Meta JSON + poster per clip | ✗ | ✗ | ✅ | ✅ | ◐ | ✅ |
| SRT / VTT sidecar | ✗ | ✗ | ✗ | ✅ | ✗ | ✅ |
| Loudness normalization | ✗ | ✗ | ✅ | ✗ | ✗ | ✅ |
| Tighten / filler removal | ✗ | ◐ jump cuts | ✅ | ✅ | ✗ | v1.1 |
| NLE export | ✗ | ✗ | ✗ | ✅ | ✗ | v1.1 |
| Gaming / PiP layout | ✗ | ✗ | ✗ | ✅ | ✗ | v1.1 |
| Profanity bleep | ✗ | ✗ | ✗ | ✅ | ✗ | v1.1 |
| Highlights reel | ✗ | ✗ | ✗ | ✅ | ✗ | v1.2 |
| YouTube ingest | ✅ | ✅ | ✅ | ✅ | ✅ | v1.1 (optional yt-dlp download) |
| B-roll | ✗ | ✗ | ✗ | ✅ | ✅ | ✗ non-goal |
| Hardware video encode | ◐ | ✗ | ✗ | ✅ | ✗ | ✅ always, on both OSes |
| Project files, resumable | ✗ | ✗ | ✗ | ✅ | ✅ | ✅ |
| Signed one-click installer | ✗ | ✗ | ✗ | ✗ | ✗ | ✅ Mac v1, Windows M6 |
| Zero external deps at runtime | ✗ | ✗ | ✗ | ✗ | ✗ | ✅ |
| Install size without models | GBs (Python, torch, MediaPipe) | GBs | hundreds of MB | GBs (Electron + torch) | GBs | ~20 MB installer |
| Zero download on first run | ✗ | ✗ | ✗ | ✗ | n/a hosted | ✅ on macOS 26+ |
| Golden-set eval, published | ◐ promised | ✗ | ✗ | ✗ | ✗ | ✅ in repo, CI-gated |
| Pixel-identical output across OSes | ✗ | ✗ | ✗ | ✗ | ✗ | ✅ snapshot tests |
| Proprietary-compatible license chain | ✗ libx264 | ✗ libx264 | ✗ libx264 | ✗ | ✗ AGPL | ✅ LGPL FFmpeg, no GPL |

## 4. How we stand first

The feature list is not the moat. Every box above is a weekend for someone who already has the Python pipeline. The moat is the five rows at the bottom of the matrix, and none of the nine projects can get there without a rewrite:

1. **Zero-install, signed, native.** Download, open, drop a video. No Python version, no ffmpeg flavour, no Ollama, no Node, no terminal. This alone puts us in front of every non-developer creator, which is almost all of them.
2. **Frontier-model picks with zero download, and a 20 MB app.** The default picker is a hosted frontier model, so clip selection is better than any local 8B, and nothing heavy is installed. Competitors make the user install Ollama and pull a multi-gigabyte model, then get worse picks. Offline paths still exist for people who want them.
3. **Quality is measured, in public.** Golden set and eval runner in the open repo. A picker change that lowers precision@5 cannot merge. AutoClip promised this; we ship it before anyone.
4. **Same pixels on Mac and Windows.** One compositor, one text rasterizer, snapshot tests on both. Competitors depend on whatever ffmpeg and fonts the user happens to have.
5. **A license chain that allows a business.** LGPL FFmpeg dynamically linked, hardware encoders, no GPL. Every competitor is welded to libx264 or AGPL, so none can sell a closed binary without re-plumbing their render.

Secondary edges that compound: hardware encode on every machine (minutes instead of tens of minutes per batch), project files that survive restarts, and a review UI that is a desktop window instead of a localhost tab.

## 5. What we take from them, with credit

| Idea | From | Where it lands |
|---|---|---|
| Complete-thought boundaries | chopify | `highlights/windows.rs`, M2 |
| "Content decides the count" with a score floor | chopify | `highlights/rank.rs`, M2 |
| Transcript-text-only privacy rule, stated in the UI | AutoClip | PLAN §5, Settings screen |
| `doctor` with fix hints | AutoClip, chopify | `choco doctor`, M1 |
| Preview drafts before full render | chopify | proxies, M3 and M5 |
| Meta JSON + poster per clip | chopify | `export/`, M3 |
| Loudness normalization | chopify | `render/mux.rs`, M3 |
| Audio energy and pause analysis as scoring features | Klipzy, OpenClip Studio | `highlights/windows.rs` prior, M2 |
| Caption preset names creators actually search for | OpenClip Studio, Klipzy | style names, M3 |
| SRT/VTT sidecars | Klipzy | `export/`, M3 |
| NLE export | Klipzy, ViralCutter | v1.1 |
| Hardware-matched model recommendation | Klipzy | `backend::recommended_models`, M1 |
| Gaming / webcam PiP layout | Klipzy | v1.1 layout |
| `llms.txt` in the repo | chopify | engine repo root, M5 |

Rules: ideas are free, code is not. All of these are reimplemented from scratch in Rust. SupoClip, ViralCutter, and HotClip are copyleft and must not be read for implementation detail, only for feature lists.

## 6. What we deliberately do not copy

- **SFX overlays and cue points** (OpenClip Studio). Gimmick, plus sound licensing.
- **AI edit chat** (Klipzy). A chat box is not a feature; a good picker is.
- **B-roll** (Klipzy, SupoClip). Stock footage licensing and taste problems. Non-goal.
- **22 caption presets at launch** (Klipzy). Four excellent ones, then community styles.
- **Docker / self-host server mode** (AutoClip, OpenClip Studio, SupoClip). We are a desktop app. The CLI covers automation.
- **Custom licenses** (Klipzy). MIT engine, period.

## 7. Watch list

Re-check these every quarter. Triggers that change our plan:
- HotClip ships an English-first build with a signed installer. They are the only one with a real desktop app.
- SupoClip's hosted version gains traction; a hosted tier on our engine moves earlier.
- AutoClip resumes and ships its golden-set gate; our eval must be visibly better and public first.
- Any of them switches to llama.cpp embedded instead of Ollama.

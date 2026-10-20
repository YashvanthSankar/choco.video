# Contributing to choco.video

Thank you. This file is short on purpose: the rules below are the ones that keep the app small, private and shippable, and everything else is open to taste.

## Before you start

- Read [`AGENTS.md`](AGENTS.md). It records every decision and why, measured benchmarks, and the traps that have already cost a day. If your change contradicts a decision there, open an issue first and say what changed.
- Check [`docs/TODO.md`](docs/TODO.md). Work that is planned for a later milestone (Windows, NLE export, diarization) is welcome as a discussion, not yet as a PR.
- Good first contributions: a caption style, a language test in `engine/fixtures`, a picker prompt improvement that raises `choco eval`, an app polish item from the TODO, or a reviewed clip in the golden set.

## The rules

1. **Media never leaves the machine.** Only transcript text may be sent, and only to the picker the user configured. No telemetry, no analytics, no update checks without an explicit opt-in setting and a line in `AGENTS.md`.
2. **No GPL, anywhere.** `cargo deny check` enforces the allowlist in `engine/deny.toml` and `app/src-tauri/deny.toml`. FFmpeg is LGPL, dynamically linked, built without `--enable-gpl`. No libx264, no openh264. Do not edit the allowlist to make a failure go away; say why in the PR and let a maintainer decide.
3. **Small by default.** Nothing heavy ships in the installer; models are runtime downloads. A new dependency over 2 MB compiled needs a Decisions entry in `AGENTS.md`.
4. **No Python, no Node at runtime.** The shipped app is Rust plus a webview. Scripts may use anything.
5. **Measure before claiming.** A picker change is not an improvement until `choco eval` shows precision@5 did not drop. A render change is not done until the snapshot tests pass. Put the numbers in the PR.
6. **Mac first.** Windows work starts at milestone M6. Platform-specific code lives only in `engine/crates/choco-engine/src/backend/`.

## Setting up

```sh
brew install rustup pkg-config cmake nasm meson ninja ffmpeg && rustup default stable
./scripts/ffmpeg/build-mac.sh            # minimal LGPL FFmpeg into vendor/ (once)
./engine/fixtures/generate.sh            # synthetic test media (uses Homebrew ffmpeg, dev only)
cd engine && cargo test --all            # 71 unit tests + media + render snapshots
cd ../app && pnpm install && pnpm tauri dev
```

Real-footage fixtures for the picker eval: `./engine/fixtures/fetch.sh` (five CC BY recordings, about 1.8 GB).

## Before opening a pull request

From `engine/`:

```sh
cargo test --all
cargo clippy --all --all-targets -- -D warnings
cargo deny check licenses bans sources
```

From `app/`: `pnpm check` (0 errors, 0 warnings). For the website: `site/index.html` is plain HTML; keep it under the house rules it was designed with (no gradients, shadows or hairlines outside real controls).

Commit messages read `stage: what changed` (`transcribe: filter repeated 4-grams`). Keep commits small. Sign them off with `git commit -s`; by contributing you agree your work is licensed under the MIT license.

## Conventions worth knowing

- One module per stage; `mod.rs` exposes one `pub fn` plus its options struct. Stages are pure functions over serde types; `pipeline/` owns caching and orchestration.
- Timestamps are `u64` milliseconds everywhere. Never floats for time.
- Errors use `thiserror` per module and carry the stage name. Every long function takes a progress callback.
- Prompts are markdown files under `highlights/picker/prompts/`, versioned by filename. Never inline prompt text in Rust.
- Snapshot references under `tests/snapshots/` change only with a commit message that says why.

## Reporting problems

Use the issue templates. For anything security-related, see [`SECURITY.md`](SECURITY.md) and report privately.

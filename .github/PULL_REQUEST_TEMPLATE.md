## What this changes

<!-- One paragraph. Which stage (ingest, transcribe, highlights, reframe, captions, render, export, app, site) and why. -->

## How it was verified

- [ ] `cd engine && cargo test --all && cargo clippy --all --all-targets -- -D warnings`
- [ ] `cargo deny check licenses bans sources` (no allowlist edits)
- [ ] `cd app && pnpm check` (if the app changed)
- [ ] Picker change: `choco eval` precision@5 did not drop (paste before/after)
- [ ] Render change: snapshot tests pass (`cargo test -p choco-engine --test render`)

## Rules

- [ ] No video, audio or key leaves the machine; no new network calls
- [ ] No GPL code or dependency; licenses noted for new crates
- [ ] Commits are signed off (`git commit -s`)

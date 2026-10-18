# Contributing

Thanks for looking. Rules that keep this project shippable:

- **Mac first.** Windows work starts after the Mac app ships (`docs/TODO.md`, M6).
- **No GPL anywhere.** `cargo deny check` enforces the allowlist in `engine/deny.toml` and `app/src-tauri/deny.toml`. FFmpeg is LGPL, dynamically linked, built without `--enable-gpl`. No libx264, no openh264. Do not relax the allowlist in a PR.
- **Media never leaves the machine.** Only transcript text may be sent to the picker provider the user configured. No telemetry.
- **Small by default.** No model ships in the installer. A new dependency over 2 MB compiled needs a line in `AGENTS.md` under Decisions.
- **Measure before claiming.** Picker changes must not lower `choco eval` precision@5 on the golden set; render changes must keep the snapshot tests green.

Before opening a PR, from `engine/`: `cargo test --all && cargo clippy --all --all-targets -- -D warnings && cargo deny check licenses bans sources`. From `app/`: `pnpm check`.

Sign your commits with a DCO sign-off (`git commit -s`). By contributing you agree your work is licensed under the MIT license.

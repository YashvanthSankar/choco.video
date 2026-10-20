# Security

choco.video runs on your machine and has no server. The surface that matters is small, and these are its rules:

- **Your media never leaves the machine.** Only transcript text is sent, and only to the clip picker you configured. If you find a code path that sends anything else, that is a security bug.
- **Your provider key lives in the macOS Keychain.** The app never writes it to disk, logs or project files; the CLI reads it from the environment only.
- **No telemetry, no analytics, no update pings without an opt-in setting.**

## Reporting

Please report vulnerabilities privately through GitHub's "Report a vulnerability" button on the repository's Security tab, or by email to the maintainer listed in the repository profile. Include a reproduction. We aim to acknowledge within 72 hours and to fix confirmed issues in the next release. Please do not open a public issue for an unpatched vulnerability.

## Scope

In scope: the Rust engine, the `choco` CLI, the Tauri app, the build and release scripts, and the website. Out of scope: the providers you connect (OpenCode Zen, OpenRouter and others), which have their own programs.

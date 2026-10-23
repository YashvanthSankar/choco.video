# Show HN: prepared for the user to submit

**Read this first.** HN's guidelines say "Don't post generated text or AI-edited text", and dang's launch advice says not to use an LLM "to generate any of it (not even a tiny bit, including to edit or spruce it up)" ([guidelines](https://news.ycombinator.com/newsguidelines.html), [launch advice](https://news.ycombinator.com/item?id=22336638)). They also ask that posting not be automated. So this file gives you facts, a structure and the things people will ask. **Write the first comment yourself, in your own words, and submit it yourself.** HN readers do notice LLM phrasing, and a flagged launch can't be redone.

## Submission

- **URL:** `https://github.com/YashvanthSankar/choco.video` (HN prefers the project itself; the README has the code, the numbers and the install). Alternative: `https://chcvd.vercel.app/?ref=hn` if you want Vercel to count HN visits. Pick one; GitHub tends to do better with HN readers, and GitHub's traffic page shows HN referrals anyway.
- **Title** (≤ 80 chars, factual, no adjectives). Options to choose from or rewrite:
  - `Show HN: Choco.video – Turn long videos into captioned vertical clips on a Mac` (78)
  - `Show HN: An open-source, local alternative to Opus Clip for macOS` (65)
  - The second option avoids starting with a lower-case domain-like name and says the value in a few words; naming a competitor in the title is fine on HN if the comparison is fair.
- **When:** a Tuesday or Wednesday US morning (around 8–9 AM ET), when you can stay at the keyboard for 4–6 hours. Don't post the same day as Product Hunt.
- **Before posting:** fix the install command on the site and in the README (they point at `chcvd.vercel.app`, which is 404 right now). Ideally ship 0.1.4 with the updater first, so HN users who install today can get fixes.

## First comment: facts to draw from (write it yourself)

Suggested shape, from dang's advice: how you came to build it, what it does, what is different, technical details, what you'd like feedback on. Short paragraphs, no marketing words.

1. **Why you built it.** Your own reason, in your words. The repo's version: tools like Opus Clip upload the footage, meter it in monthly minutes, and watermark the free tier; you wanted the same result on your own machine, free, no account.
2. **What it does (v0.1.3):** transcribe on device (Apple's speech engine on macOS 26, about 42× real time; whisper.cpp on older macOS with a 550 MB model downloaded once) → every stretch that starts and ends on a complete thought is a candidate; the best 80 go to a language model that scores each and gives a reason → YuNet face detection five times a second keeps the speaker in the crop, split screen for two people → word-timed captions in four styles → hardware H.264, −14 LUFS, 9:16 / 1:1 / 4:5 / 16:9, with a poster, SRT/VTT and a caption.
3. **What leaves the machine:** only transcript text, only to the picker you choose. Apple's on-device model means nothing leaves. Or any OpenAI-compatible endpoint (OpenRouter, Groq, DeepSeek, LM Studio or Ollama locally); MiniMax M3 on OpenCode Zen is about 2¢ per hour of video. Keys are kept in the Keychain.
4. **Numbers:** 47:52 interview → four finished clips in 226 s on an M3 Pro, transcription included. DMG about 11 MB, ~20 MB installed. Minimal LGPL FFmpeg is 8.3 MB of dylibs. Picker precision@5 is 0.44 on five public podcasts (about 2 of the top 5 match an editor's marks), and the set isn't human-reviewed yet: say that plainly.
5. **Stack:** Rust engine and `choco` CLI (usable on its own), Tauri 2 + Svelte 5 app, `tract` + YuNet (233 KB), `cosmic-text` captions, no Python or Node at runtime, MIT throughout, no GPL anywhere (LGPL FFmpeg dynamically linked, no libx264).
6. **Honest limits:** Apple Silicon + macOS 14+ only; Windows later. Not notarized (no paid developer account): browser downloads need Privacy & Security › Open Anyway; the `curl | sh` installer avoids that because curl doesn't set the quarantine flag; link the script. Built with heavy AI assistance (121 of 124 commits co-authored by Claude): HN will see it in the history, so say it yourself and say how you verified things (benchmarks in AGENTS.md, eval set, snapshot tests).
7. **What feedback you want:** e.g. pick quality on people's own episodes, and whether a CLI-first workflow matters to them.

## Questions to have answers ready for

- "Why `curl | sh`?" → Not notarized; the script is about 50 lines, checks the platform, verifies the SHA-256 published with the release, copies the app to /Applications (or ~/Applications), clears the quarantine attribute and opens it. Be candid that the checksum comes from the same GitHub release, so it protects against a corrupted download, not a compromised release; the real fix is notarization. Link: https://github.com/YashvanthSankar/choco.video/blob/main/scripts/install.sh
- "Is it really local if it calls an LLM?" → Video and audio never leave; transcript text goes only to the picker you configured; Apple on-device or a local LM Studio/Ollama endpoint keeps everything on the Mac.
- "How good are the picks?" → 0.44 precision@5, eval method in README; every clip shows its reasons.
- "Why not Swift?" → Windows is planned; AGENTS.md decision log.
- "Opus Clip does X" → Agree where it's true (B-roll, scheduling, browser-based, any OS). Never disparage.
- Don't promise unreleased features as shipped. You can say NLE export and caption editing are merged and coming in the next release, only if that release is close.

## Don'ts

- Don't ask anyone to upvote or comment, or share the HN link asking for votes. Sharing the *project* elsewhere is fine.
- Don't have friends post booster comments.
- Don't repost if it sinks; one more try is acceptable later only with a significantly new version.

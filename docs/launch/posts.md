# Launch copy, per channel

Every piece is written for its community and says something different. Rules for all of them:

- Site links carry `?ref=<channel>` so Vercel Analytics can separate referrers.
- Site domain is **chcvd.vercel.app** (the old `chcvd.vercel.app` is 404 right now; see `research.md`).
- Only v0.1.3 features. Do not mention Final Cut/Premiere export, caption editing, translation, the offline local picker or auto-update as available.
- Always: "I'm the developer", Apple Silicon + macOS 14+, not notarized, Windows later.
- No em dashes (r/MacApps and others treat them as an AI tell).

---

## 1. r/tauri (SUBMITTED 2026-10-23 ~08:20 UTC, removed by Reddit's spam filter)

https://www.reddit.com/r/tauri/comments/1xd88xr/ . Status `removed_by_category: reddit` (sitewide filter, not the mods); not visible publicly. Ask the r/tauri mods via modmail to approve it. Do not repost.

**Title:** I built a Mac app with Tauri 2 that turns long podcasts into captioned vertical clips. 11 MB DMG, no Python, no bundled models

**Body:**

I'm the developer. choco.video is a free, MIT-licensed macOS app: you drop in a long video (a podcast, a talk, a stream) and it gives you ranked vertical clips with word-timed captions, framed on whoever is talking. Everything except picking the clips runs on the Mac, and the picker only ever sees transcript text.

I'm posting here because Tauri is a big part of why it's small, and a few things I learned getting there might be useful to someone.

**How the size stays down**

- Tauri 2 with a Svelte 5 UI over a Rust engine. The DMG is about 11 MB, roughly 20 MB installed.
- FFmpeg is my own minimal LGPL build (`--disable-gpl`, allowlisted codecs only), dynamically linked: 8.3 MB of dylibs, with dav1d so AV1 downloads still decode. No libx264; encoding goes through VideoToolbox H.264.
- Face tracking is a 233 KB YuNet model run with `tract`, instead of shipping ONNX Runtime.
- Captions are drawn with `cosmic-text` and bundled fonts, so they look the same on every Mac.
- On macOS 26 transcription uses Apple's built-in speech engine through a 0.17 MB Swift helper, so nothing is downloaded. Older macOS versions fall back to whisper.cpp on Metal and fetch a 550 MB model on first use. No model ships in the installer.
- The clip picker is a single OpenAI-compatible client: Apple's on-device model, or any endpoint and key you bring. No Python, no Node, nothing else to install.

One real number: a 47:52 interview became four finished clips in 226 seconds on an M3 Pro, transcription included.

**Things I ran into**

- I don't have a paid Apple developer account yet, so the build is ad-hoc signed and not notarized. On macOS 15 and later, right-click > Open no longer gets you past Gatekeeper; people have to go to System Settings > Privacy & Security > Open Anyway. Files downloaded with curl don't get the quarantine flag, so there's a short install script that downloads the release from GitHub, checks its SHA-256 and copies the app to Applications. It's in the repo if you want to read it before running it.
- Next release adds tauri-plugin-updater (opt-in). My assumption is that updates on an ad-hoc signed app won't hit Gatekeeper because the updater downloads without the quarantine flag, but I haven't proven it yet. If you've shipped the updater on a non-notarized Mac app, I'd really like to hear how it went.

**Caveats**

- Apple Silicon only, macOS 14 or later. Windows later.
- Picks are useful, not magic. On five public podcasts, about two of the recommended model's top five picks matched clips an editor had marked (my test set, not yet human-reviewed). Every clip comes with its score and the picker's reasons so you can skip bad ones quickly.
- I built this with a lot of help from Claude Code; the commit history shows it.

Links:

- Site: https://chcvd.vercel.app/?ref=reddit-tauri
- Source (MIT): https://github.com/YashvanthSankar/choco.video
- DMG: https://github.com/YashvanthSankar/choco.video/releases/latest

Or install from Terminal (no Gatekeeper prompt):

    curl -fsSL https://chcvd.vercel.app/install.sh | sh

Happy to answer anything about the Tauri side.

---

## 2. r/SideProject (NOT posted; held after the r/tauri removal, see plan.md)

Before posting: drop the `curl | sh` block from the body (keep the DMG link and say the site has a one-line installer), since that pattern on a fresh domain looks like macOS malware to Reddit's filters.

**Title:** choco.video - a free, open-source Mac app that turns long podcasts and talks into captioned vertical clips, without uploading the video

**Body:**

I'm the developer. I wanted the Opus Clip workflow (long episode in, short clips out) without uploading every episode, paying for monthly minutes, or having a watermark on the free tier. So I built a Mac app that does it on the machine and released it under the MIT license.

**What it does**

- Drop in a podcast, talk or stream. It transcribes on the Mac, finds stretches that start and end on a complete thought, and has a language model rank them, with a score and a short reason for each pick.
- Each clip is reframed to 9:16 (or 1:1, 4:5, 16:9) and follows whoever is talking. Captions are timed to the word in four styles, sound is normalized to -14 LUFS, and you also get a poster frame, SRT and VTT subtitles and a caption ready to paste.
- Your video never leaves the Mac. Only transcript text goes to the picker you choose: Apple's on-device model (free, nothing leaves at all, needs macOS 26 with Apple Intelligence) or any OpenAI-compatible key. With the model I recommend, that's about 2 cents per hour of video, paid to that provider, not to me.
- No account, no watermark, no monthly minutes.

A real run: a 47:52 interview became four finished clips in 226 seconds on an M3 Pro.

**Honest caveats**

- Apple Silicon Macs on macOS 14 or later only. Windows later.
- It isn't notarized (no paid Apple developer account yet), so a DMG downloaded in a browser needs System Settings > Privacy & Security > Open Anyway once. The one-line Terminal install avoids that, and you can read the script first.
- The picks are good, not magic: on five public podcasts, about two of the top five picks matched clips an editor had marked, and that test set hasn't been human-reviewed yet. That's why every clip shows its reasons.
- On macOS versions before 26 it downloads a 550 MB speech model the first time.
- Built with a lot of help from Claude Code.

**What I'd like feedback on:** if you cut clips from long-form video today, what would this have to do before you'd switch to it? And if you try it on one of your own episodes, do the picks feel right?

- Site: https://chcvd.vercel.app/?ref=reddit-sideproject
- GitHub: https://github.com/YashvanthSankar/choco.video
- DMG: https://github.com/YashvanthSankar/choco.video/releases/latest

Or the one-line Terminal install, which skips the security prompt:

    curl -fsSL https://chcvd.vercel.app/install.sh | sh

---

## 3. X thread (NOT posted; user said leave X for now)

Attach `site/media/choco-launch.mp4` (30 s, 1280×720, 2.3 MB) to the first post if media upload is available; otherwise `site/media/choco-launch-poster.jpg`.

**Post 1 (main):**
> choco.video is a free, open-source Mac app that turns long videos into ranked, captioned vertical clips.
>
> Your video never leaves your Mac. No account, no watermark, no monthly minutes.
>
> https://chcvd.vercel.app/?ref=x

**Reply 1:**
> How it works: it transcribes on the Mac, finds stretches that start and end on a complete thought, and a language model ranks them with a reason for each. The crop follows whoever is talking.
>
> A 47:52 interview became four finished clips in 226 seconds on an M3 Pro.

**Reply 2:**
> The picker only sees transcript text. Use Apple's on-device model and nothing leaves at all, or bring any OpenAI-compatible key (about 2¢ per hour of video).
>
> Apple Silicon, macOS 14+. Not notarized yet; this skips the prompt:
> curl -fsSL https://chcvd.vercel.app/install.sh | sh

**Reply 3:**
> MIT licensed, Rust + Tauri, ~11 MB download. Windows later.
>
> Source and the install script are here, and I'd love to hear how the picks do on your own episodes:
> https://github.com/YashvanthSankar/choco.video

---

## 4. r/opensource (for the USER to write and post; day 4)

r/opensource bans AI-generated content, so this must be in your own words. Points worth making, in this order:

- Flair: Promotional. Say you're the developer.
- What it is in one sentence, and that the whole thing is MIT (engine, app, site, build scripts), with FFmpeg as LGPL dynamically linked and no GPL code anywhere.
- Why open source matters for this category: competitors upload your footage; here the code that touches your video is readable, and only transcript text goes to a picker you choose.
- What contributors could pick up (look at `docs/TODO.md` for unchecked items) and that `AGENTS.md` documents decisions and benchmarks.
- Disclose that it was built with heavy AI assistance (121 of 124 commits co-authored by Claude). Being upfront beats someone finding it in the history.
- Caveats: Apple Silicon only, not notarized, Windows later.
- Link: `https://chcvd.vercel.app/?ref=reddit-opensource` and the GitHub repo.
- Stay in the thread to answer for a few hours (they remove drive-by posts).

## 5. r/podcasting weekly Product/Service thread (comment, Wednesday)

Post only as a comment in the thread that goes up Wednesdays 7 AM ET. Disclosure is mandatory there.

> Disclosure: I'm the developer of this, it's free and open source (MIT).
>
> choco.video is a Mac app for turning an episode into short vertical clips without uploading it anywhere. You drop in the video file; it transcribes on your Mac, ranks moments that start and end on a complete thought (each with a short reason), keeps whoever is speaking in frame, and burns in word-timed captions. Each clip also comes with SRT/VTT subtitles, a poster frame and a caption to paste.
>
> No account, no watermark, no monthly minute limit. Clip picking is free with Apple's on-device model (macOS 26), or about 2 cents per hour of audio with your own API key.
>
> Limits: Apple Silicon Macs on macOS 14+, not notarized yet (the site explains the one-time Open Anyway step, or a one-line Terminal install), Windows later. It works from a video file, so audio-only shows aren't its target yet.
>
> https://chcvd.vercel.app/?ref=reddit-podcasting
>
> If you try it on an episode I'd like to know whether the picks match what you would have cut.

Check before posting: confirm the app actually rejects or handles audio-only files the way that line says; if unsure, drop that sentence.

## 6. r/VideoEditing developer thread (comment)

> I'm the developer of choco.video, a free MIT-licensed Mac app that cuts long talking-head videos (podcasts, interviews, talks) into vertical clips. It runs on the Mac: transcription, speaker-following reframe (YuNet face detection five times a second, split screen when two people share a shot), word-timed burned-in captions, hardware H.264, -14 LUFS. Output is 9:16, 1:1, 4:5 or 16:9 with SRT/VTT sidecars, so you can bring the captions into your NLE instead of using the burned-in ones.
>
> Only transcript text goes to the clip picker you choose (Apple's on-device model, or any OpenAI-compatible key). Apple Silicon, macOS 14+, not notarized. https://chcvd.vercel.app/?ref=reddit-videoediting
>
> Editors' feedback on the cut points would help most: every clip is meant to start and end on a complete thought.

## 7. r/macapps (later, only when eligible)

Not eligible today (see `research.md`). When the account has 10+ r/MacApps karma:

- **If Tier 2 is met** (site has a Privacy Policy and Terms of Service; you include a real-identity link such as LinkedIn or a portfolio): main-feed post, flair **Free**, title prefixed **[OS]**, in PCP format:
  - Title: `[OS] choco.video: turn long podcasts and talks into captioned vertical clips, on your Mac`
  - **Problem:** Clipping a long episode by hand takes an evening; the paid tools upload the whole video and meter it in monthly minutes.
  - **Comparison:** Opus Clip (Starter $15/month for 150 minutes; free tier adds a watermark; video is uploaded) and Vizard. choco.video runs on the Mac, has no account or watermark, and is MIT-licensed. Opus Clip does more: AI B-roll, scheduling, works in any browser.
  - **Pricing:** Free. Optional picker cost about 2¢ per hour of video, paid to the provider you choose; free with Apple's on-device model. Link: `https://chcvd.vercel.app/?ref=reddit-macapps`
  - Notarization status, Apple Silicon only, AI-assistance disclosure, "I'm the developer".
- **Otherwise:** one comment in the current "App Pile" megathread with the same PCP content, shorter.

## 8. Hacker News and Product Hunt

See `hn.md` and `producthunt.md`.

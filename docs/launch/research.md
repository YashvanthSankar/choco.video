# Launch research: choco.video (macOS app, v0.1.3)

Researched 2026-10-23. Reddit rules were pulled with Composio (`REDDIT_GET_SUBREDDIT_RULES`, `/r/<sub>/about`) from the connected account; web sources are linked inline.

## 0. Facts checked against the repo before writing anything

| Claim | Source in repo | Use it? |
|---|---|---|
| Latest release v0.1.3 (2026-10-20), DMG 11,476,965 bytes (release notes say "11M") | `gh release view v0.1.3` | Yes, "about 11 MB". The site says 9.9 MB, which is out of date; fix the site |
| Apple Silicon, macOS 14+ | README at v0.1.3 | Yes |
| Not notarized; install script avoids the Gatekeeper prompt | README, release notes, `scripts/install.sh` | Yes, always say it |
| 47:52 interview → four clips in 226 s on an M3 Pro, transcription included | README, site | Yes |
| ~2¢ per hour of video with MiniMax M3 on OpenCode Zen; precision@5 0.44 (about 2 of top 5), set not yet human-reviewed | README at v0.1.3 | Yes, with the caveat |
| Apple on-device picker: free, nothing leaves, needs macOS 26 + Apple Intelligence | README, site | Yes |
| Whisper fallback on macOS < 26 downloads a 550 MB model once | site FAQ | Yes |
| FFmpeg minimal LGPL, 8.3 MB dylibs; YuNet 233 KB via tract; Swift helper 0.17 MB | AGENTS.md Benchmarks/Decisions, README | Yes (technical posts) |
| Final Cut/Premiere/Resolve export, caption editing, translation, offline local picker, faster picking, auto-update | `git log v0.1.3..HEAD` | **No.** Not released. Note: README on `main` already lists the NLE export in the Opus Clip table, so README currently over-claims for the released build |
| 121 of 124 commits are co-authored by Claude | `git log` | Disclose AI assistance where a community asks for it |

### Blocker found: the documented site domain is down

- `https://chcvd.vercel.app` and `https://chcvd.vercel.app/install.sh` return **404 DEPLOYMENT_NOT_FOUND** (checked several times on 2026-10-23).
- The Vercel project `choco.video` now serves production at **`https://chcvd.vercel.app`** (alias set on the deploy at 13:33 IST today; the GitHub repo homepage also points there). Analytics script is present on it.
- README, the site's own install button, the release notes and `install.sh`'s header all still say `curl -fsSL https://chcvd.vercel.app/install.sh | sh`, which now fails. All posts therefore link `chcvd.vercel.app` and give the working command `curl -fsSL https://chcvd.vercel.app/install.sh | sh`.
- **User action:** either add `chcvd.vercel.app` back as a domain of the Vercel project, or update README, `site/index.html`, `scripts/install.sh`, `site/install.sh` and the v0.1.3 release notes to `chcvd.vercel.app`. Do this before HN and Product Hunt.

## 1. Reddit

Connected account: u/Lucky-Seaweed-8996, ~630 days old, 41 total karma (35 post, 6 comment), verified email. Karma per subreddit could not be read (the OAuth scope does not include it), but no posts by this account show up in search, so local karma in any app subreddit is assumed to be ~0.

### Chosen for today

| Sub | Size | Self-promo rules | Fit | Notes |
|---|---|---|---|---|
| **r/tauri** | 8.5k | No sub rules listed; no flairs | High | Recent "Built X with Tauri" posts that lead with concrete size numbers and lessons learned do well: "I built a fast, powerful CSV editor with Tauri – 1M rows in 3 seconds" (331 pts, 41 comments), "Git client ... 12 MB instead of a gigabyte" (45 pts), "5 MB multi-window IDE" (56 pts). One-line "I built X" posts get 1–4 points. So: lead with how the app stays ~11 MB and what was learned, end with a real question. |
| **r/SideProject** | 878k | No rules listed; sidebar asks for title format `[Project name] - [Short description]` | Medium | The community exists for this. But it is saturated with "AI turns your podcast into clips" posts that score 0–1 (search `subreddit:SideProject podcast clips`: Isla, clip.video, a CLI tool, etc.). Differentiate on free + MIT + video stays local, be honest about pick quality, and ask a specific feedback question. |

### Rejected or deferred (with reason)

| Sub | Why not today |
|---|---|
| r/macapps (258k) | Best audience, but gated. Everyone needs 10 local r/MacApps karma. App devs may post in the main feed only via Tier 1 (Mac App Store, or a GitHub repo with 1yr+ history or 100+ stars, or a dev flair) or Tier 2 (real-identity portfolio/LinkedIn **and** a website with a Privacy Policy and Terms of Service). Otherwise only the monthly "App Pile" megathread, and promoting your own app in comments also needs 10 local karma. Mods say unqualified posts are removed 80–90% of the time **and still start the 30-day once-per-developer cooldown**. Rules: [Phase 3 post](https://www.reddit.com/r/macapps/comments/1ryaeex/), [post approval](https://www.reddit.com/r/macapps/comments/1smg62t/). Required format when eligible: PCP (Problem, Comparison with 1–2 competitors, Pricing with link), title prefix `[OS]`, flair Free. Their FAQ also says AI-looking text (em dashes) triggers removals. **Deferred**: earn karma, add Privacy Policy + Terms to the site, then post. |
| r/opensource (391k) | Fits (MIT, OSI license, `Promotional` flair id `f048747a-4094-11ed-afcf-16b5e38a4910`), but rule 3 says "All AI-generated content is low-effort and ban worthy", and drive-by posts without engagement are removed. A post drafted by an AI agent would break that rule. **Deferred to the user**, written in their own words (draft points in `posts.md`). |
| r/podcasting | Product/tool promotion only in the weekly Product or Service thread (Wednesdays 7 AM ET); standalone = ban. Affiliation must be disclosed. Karma ≥ 5, age ≥ 7 days (met). **Plan: one disclosed comment in Wednesday's thread.** |
| r/podcast | Product/app promotion removed even outside the weekly thread ("Any other self promotions for products ... apps ... will also be removed"). Rejected. |
| r/NewTubers | Advertising needs prior written mod permission; AI-written posts banned. Rejected (could ask mods by modmail later). |
| r/youtubers | Bans promotion of apps/tools outright and bans discussion of AI tools/workflows (permanent ban). Rejected. |
| r/VideoEditing | Developers must use the dedicated developer thread; no standalone posts. **Plan: one disclosed comment in the current developer thread.** |
| r/selfhosted | Off-topic (desktop app, not self-hosted). New projects < 3 months only in the "New Project Megathread". Rejected. |
| r/ContentCreators | 20 karma + 7 days (met), no emojis, no paid services. Allowed but low-signal community; optional later in the week. |
| r/rust | On-topic as a Rust project, but rule 6: "No slop ... Submissions appearing to contain AI-generated content may be removed". The codebase is heavily AI-assisted and the community is sensitive to it. **Deferred**: only as a user-written technical post (e.g. the minimal LGPL FFmpeg build or whisper loop recovery), week 2. |
| r/LocalLLaMA | "Completely/primarily LLM generated copy, code is not allowed." The code is primarily AI-co-authored. Rejected. |
| r/MacOS | Promotion only on Saturdays (UTC), and only for Mac App Store apps or "reputable, established GitHub repositories" (GitHub-Guard audit). A 5-day-old repo with 1 star does not qualify. Rejected for now. |
| r/apple | Developer self-posts only on Sundays (California time), with enforced organic activity requirements. Account has none there. Rejected for now. |
| r/macOSApps | Needs 5 comment karma in that sub; dev posts must state notarization status, link a Privacy Policy, disclose AI use; recommends waiting for 40+ stars. Deferred until those are met. |
| r/coolgithubprojects (129k) | GitHub-only link posts, `[Desc] - [Title]` format, flair auto-assigned. Allowed; low engagement. Optional later. |
| r/indiehackers | One self-promo post per user with Self Promotion flair. Optional later. |

## 2. Hacker News (Show HN)

Sources: [Show HN guidelines](https://news.ycombinator.com/showhn.html), [HN guidelines](https://news.ycombinator.com/newsguidelines.html), [dang's launch advice](https://news.ycombinator.com/item?id=22336638).

- Qualifies: something people can run, personally built, easy to try with no signup. choco.video qualifies (download, no account).
- Title starts with "Show HN:"; factual, no marketing language; ≤ 80 chars.
- Add a first comment (or text) explaining how you came to build it, what is different, technical details. Link prior HN threads if any.
- Be around to answer for hours. Don't ask anyone to upvote or comment; booster comments are spam. Don't use a project-name username.
- **Generated text is explicitly banned.** Guidelines: "Don't post generated text or AI-edited text." dang: "Don't use an LLM to generate any of it (not even a tiny bit, including to edit or spruce it up)." Also "please don't automate posting." So `hn.md` gives the user facts and a structure, not prose to paste; the user must write the comment themselves and submit it themselves.
- Timing: there is no good recent data on Show HN by hour ([older analyses](https://nbviewer.org/urls/community.dataquest.io/uploads/short-url/i3gx9cXIMna8TP6NDZxhq8DFCd4.ipynb) favour US afternoon for comments, on 2016 data). Common practice is a US weekday morning so the post gets a full US day; pick a time you can stay at the keyboard for 4–6 hours.
- Expect scrutiny on: not notarized + `curl | sh` (have the script link and SHA-256 check ready), "local" vs. hosted picker (be precise: only transcript text, only to the picker you choose), pick quality (0.44 precision@5, not human-reviewed), AI-assisted code.

## 3. Product Hunt

Sources: [PH help: how to post a product](https://help.producthunt.com/en/articles/479557-how-to-post-a-product), 2026 guides ([LaunchList](https://app.getlaunchlist.com/blog/how-to-launch-on-product-hunt-2026), [Screenhance checklist](https://screenhance.com/blog/product-hunt-launch-checklist-2026), [Causo playbook](https://hub.causo.ai/guides/product-hunt-launch-2026-realistic-playbook)).

- Launch day runs 12:01 AM Pacific; post the maker's first comment right after going live.
- Name: product name only. Tagline: short (60 characters is the widely used limit). Description: max 260 characters.
- Gallery: 1270×760 recommended, at least 2 images. Thumbnail: square 240×240, GIF allowed under 3 MB, nothing flashy.
- Topics: a few that fit best.
- First comment is expected (who you are, why you built it, what is different, what feedback you want).
- No asking for upvotes or any manipulation; sharing the link is fine. A maker profile with some activity before launch helps.

## 4. X (Twitter)

- Not connected and the user said to leave X for now. Thread is drafted in `posts.md`.
- What works for indie/open-source Mac launches: first post says what it is and the one reason to care, with a visual (short video or an image of real output); replies carry how it works, one real number, honest caveats, and the GitHub link. Links in the first post can reduce reach; putting the site link in the first post is still fine for a launch since measurement matters here. 0–2 hashtags at most.
- Assets: `site/media/choco-launch-poster.jpg` (1280×720), `site/media/choco-launch.mp4` (1280×720, 30 s, 2.3 MB), `site/media/choco-social-9x16.mp4` (1080×1920, 20 s, 7.7 MB), four real output clips `clip-1..4.mp4` (540×960, 8–12 s).

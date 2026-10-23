# Product Hunt: ready for the user to submit

Rules recap ([PH help](https://help.producthunt.com/en/articles/479557-how-to-post-a-product)): launches run 12:01 AM Pacific to midnight; description max 260 characters; gallery 1270×760, at least 2 images; thumbnail square 240×240 (GIF under 3 MB allowed, nothing flashy); a few topics; maker's first comment right after going live. Never ask for upvotes; sharing the link is fine.

## Recommended launch date

**Thursday 29 October 2026, 12:01 AM PT** (= 12:31 PM IST), two days after Show HN, if these are done first:

1. Install command fixed everywhere (site, README, release notes) for the live domain.
2. v0.1.4 released with the opt-in updater, so Product Hunt users can receive fixes. If 0.1.4 is not out by Tuesday 27th, move PH to **Tuesday 3 November**.
3. A short demo video on YouTube or Loom for the gallery (the 30-second film already exists).

Be online from 12:01 AM PT for the first 3–4 hours (that's lunchtime onward in India) and answer every comment.

## Fields

- **Name:** choco.video
- **Tagline** (57 chars): `Long videos in, captioned vertical clips out, on your Mac`
  - Alternative (52): `Free, open-source Opus Clip alternative for your Mac` (naming a competitor in the tagline is allowed but reads less friendly)
- **Links:** website `https://chcvd.vercel.app/?ref=ph`; GitHub `https://github.com/YashvanthSankar/choco.video`
- **Pricing:** Free. Open source.
- **Description** (≤ 260 chars):

  > Drop in a podcast, talk or stream and get ranked vertical clips with word-timed captions, framed on whoever is talking. Your video never leaves your Mac: only transcript text goes to the clip picker you choose. No account, no watermark, MIT licensed.

- **Topics:** Mac · Open Source · Video · Podcasting · Artificial Intelligence (add "Social Media Tools" only if you want five; fewer and closer is better)
- **Thumbnail:** `docs/launch/producthunt/thumbnail-240.png` (240×240 from `site/media/icon-320.png`)
- **Video:** upload `site/media/choco-launch.mp4` (30 s, real app and real output) to YouTube and paste the link.

## Gallery (1270×760, made from existing assets in `docs/launch/producthunt/`)

| # | File | What it shows |
|---|---|---|
| 1 | `1-poster.jpg` | Four real clips side by side with scores; "Four clips, framed on the speaker and captioned." (from `site/media/choco-launch-poster.jpg`) |
| 2 | *to make*: screenshot of the app's results screen with a clip's score and reasons visible | The "why this clip" differentiator |
| 3 | `3-source-wide.jpg` | The wide source frame, to pair with #1 as before/after |
| 4–7 | `clip-1-760.jpg` … `clip-4-760.jpg` | Single 9:16 clips on the brand yellow, captions visible |
| 8 | *to make*: screenshot of the hero section of the website or the onboarding picker choice (Apple on-device vs. key) | Privacy story |

Order suggestion: 1, 2, 3, 4, 8. Too many near-identical clip frames looks padded; use two of the four.

Note the clips are from CC BY 3.0 recordings (Pretty Big Deal with Ashley Graham; Nikola Danaylov interview), as credited on the site. Add "Footage: CC BY 3.0 via Wikimedia Commons" in the first comment or a gallery caption.

## Maker's first comment (draft; rewrite it in your own voice)

Product Hunt doesn't forbid drafted text the way HN does, but it reads best when it's yours. Facts are all from the repo.

> Hi Product Hunt, I'm Yashvanth, and I built choco.video.
>
> It turns a long video (a podcast, an interview, a talk) into short vertical clips you can post: it finds moments that start and end on a complete thought, ranks them with a reason for each, keeps whoever is talking in frame and burns in word-timed captions. A 47:52 interview became four finished clips in 226 seconds on my M3 Pro.
>
> Why I made it: the tools I tried upload the whole video, meter it in monthly minutes and watermark the free tier. I wanted the same result on my own machine, for free.
>
> - Your video never leaves your Mac. Only transcript text goes to the picker you choose. With Apple's on-device model, nothing leaves at all. With your own API key, it's about 2¢ per hour of video, paid to that provider.
> - No account, no watermark, no monthly limit. MIT licensed, all of it.
> - Every clip comes with SRT/VTT subtitles, a poster frame and a caption to paste, in 9:16, 1:1, 4:5 or 16:9.
>
> Honest limits: Apple Silicon Macs on macOS 14+ only for now (Windows later), and it isn't notarized yet, so the site shows a one-time "Open Anyway" step or a one-line Terminal install. The picks are good, not perfect, which is why each one shows its reasons. I built it with a lot of help from Claude Code.
>
> I'd love to know how the picks do on your own episodes, and what would make you switch from what you use now.

## Day-of checklist

- Post goes live 12:01 AM PT; post the first comment within a couple of minutes.
- Share the PH link on X and in your own channels as "we're on Product Hunt today", not "please upvote".
- Reply to every comment; fix anything small that comes up and say so in the thread.
- Watch Vercel Analytics for `ref=ph`.

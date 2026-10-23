# Launch plan: 23–30 October 2026

Principles: one community at a time, never the same text twice, at most 2 Reddit posts a day and one per community, always disclose "I'm the developer", stay in each thread for a few hours after posting. Copy is in `posts.md`, reasons per channel in `research.md`.

## Status log

| When (UTC) | Channel | Result |
|---|---|---|
| 2026-10-23 ~08:20 | r/tauri text post | Submitted: https://www.reddit.com/r/tauri/comments/1xd88xr/ . **Removed by Reddit's sitewide spam filter** (`removed_by_category: reddit`), not visible publicly. Not reposted. |
| 2026-10-23 | r/SideProject | **Held, not posted.** Same probable trigger as the r/tauri removal; a second filtered post from a 41-karma account risks the account. |
| 2026-10-23 | X | Not posted (user: leave X for now). Thread ready in `posts.md` §3. |

### Why the r/tauri post was probably filtered

- The account is low-karma (41) with no visible history.
- The body contained a macOS `curl … | sh` install from a brand-new `*.vercel.app` domain. That is the exact delivery pattern of the macOS info-stealer campaigns Reddit has been filtering. A Cloudflare firewall in the posting path also rejected the first attempt when the command was wrapped in backticks.
- It is not a ban or a mod removal. The r/tauri mods can approve it: send them a short modmail from the account ("my post was caught by the spam filter, could you take a look?"). Also check the account isn't shadowbanned by opening `https://www.reddit.com/user/Lucky-Seaweed-8996` in a logged-out window.
- From now on, Reddit copy should lead with the GitHub release DMG and leave the `curl | sh` line to the site and README.

## Before the next pushes (user)

1. **Fix the install command/domain (blocker).** `chcvd.vercel.app` is 404; production is `chcvd.vercel.app`. Either add `chcvd.vercel.app` back to the Vercel project's domains, or change README, `site/index.html`, `scripts/install.sh`, `site/install.sh` and the v0.1.3 release notes. Also fix the site's "9.9 MB" (the v0.1.3 DMG is 11.5 MB).
2. **Release 0.1.4 with auto-update before HN and PH.** v0.1.3 users can't be updated. Blocked until you run `gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/choco.video-updater.key`.
3. **README on `main` lists unreleased features** (Final Cut/Premiere/Resolve export in the Opus Clip table, opt-in update check). Either release 0.1.4 first or mark those "next release"; HN readers will download 0.1.3 and compare.
4. **Add a Privacy Policy and Terms page to the site.** Unlocks r/MacApps Tier 2 and is expected by r/macOSApps; easy, since the app collects nothing.
5. **Earn some r/MacApps karma** with genuinely helpful comments about other apps (not choco) over the week; 10 local karma is required for any post or self-promo comment there.
6. A 10–20 s screen-recording GIF of the real app (drop a file → clips with reasons) would help every channel; nothing in `site/media` shows the app UI.

## Schedule

| Day | Channel | Why then |
|---|---|---|
| Fri 23 Oct | r/tauri (done, filtered; ask mods). Reply to any comments if approved. | Technical audience that rewards "small Tauri app" posts with real numbers. |
| Sat 24 | No posts. Fix domain/install command; add Privacy/Terms pages; start r/MacApps participation. | Weekends are slow; r/MacOS Saturday slot needs an established repo (not yet). |
| Sun 25 | No posts. Prepare 0.1.4. | r/apple Sunday dev slot needs organic activity there (none yet). |
| Mon 26 | Release 0.1.4. **r/SideProject** (from `posts.md` §2, without the curl block), US morning (~13:00–14:00 UTC). | Gives the filter question a day to settle; Monday US morning traffic. |
| Tue 27 | **Show HN**, ~8–9 AM ET (12:00–13:00 UTC), user-written (`hn.md`). X thread the same morning if X is connected (no HN link in it). | Weekday US morning; full day of attention. |
| Wed 28 | **r/podcasting** weekly Product/Service thread comment (opens 7 AM ET), `posts.md` §5. **r/opensource** post written by you, flair Promotional (`posts.md` §4). | Only allowed venue on r/podcasting; r/opensource needs your own words. |
| Thu 29 | **Product Hunt** at 12:01 AM PT (`producthunt.md`). r/VideoEditing developer-thread comment (`posts.md` §6). | Two days after HN so you can give each full attention. |
| Fri 30 | **r/MacApps**: App Pile megathread comment, or main feed in PCP format if Tier 2 is met and you have 10+ local karma (`posts.md` §7). | Needs the karma and Privacy/Terms first; a premature post starts a 30-day cooldown. |
| Week 2 | r/rust technical write-up (your words), r/macOSApps (after 5 comment karma there), r/coolgithubprojects, r/indiehackers. | Spread out; each needs its own angle. |

Max Reddit posts per day in this plan: 2 (Wednesday: one comment in a weekly thread plus one post).

## Measuring

Vercel Analytics on the site, grouped by `ref`: `reddit-tauri`, `reddit-sideproject`, `reddit-opensource`, `reddit-podcasting`, `reddit-videoediting`, `reddit-macapps`, `x`, `hn`, `ph`. GitHub Insights › Traffic shows referrers for the repo (HN usually lands there), and release download counts (`gh release view v0.1.3 --json assets`) show installs; v0.1.3 DMG had 2 downloads before launch.

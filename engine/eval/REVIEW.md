# Golden set: human review checklist

The clips in `golden.json` were drafted on 2026-10-18 by separate editor agents that read only the transcripts. They are **not** ground truth until a person has watched them. Until every fixture has `"reviewed_by_human": true`, `choco eval` prints its numbers as provisional.

How to review a fixture:
1. Open the media file and jump to each clip's `start_ms`. Watch the clip.
2. Keep it, move its edges to the nearest sentence boundary, or replace it with one of the alternates below.
3. Add anything better that the drafter missed. Aim for 6 to 8 clips, 15 to 90 s, no overlaps.
4. Set `"reviewed_by_human": true` and your name in `drafted_by`.

## danaylov (The Disruptors, Nikola Danaylov, 72 min)
- Transcript errors to fix in captions: "Aology" = "Technology", "boy/boring corporation" = "Boeing", "£300" = "300 lb", garbled names (Peter Diamandis, Rob Nail).
- Clip at 1076940 names real people critically. Clip at 3406560 is politically charged and opens on filler.
- Alternates: 324060–350220 (300 résumés, invents his own job); 4086720–4114500 (machines empower or replace us); 3281040–3318720 (Canada vs US, basketball joke); 1854240–1904160 (Catholic Church business model); 498420–592620 (first podcast failed to record, 94 s, needs trimming).

## olave (Olave Talks to Anousha Nzume, 70 min)
- No speaker labels. Clips at 3560040 and 2831940 are the host; most others are the guest; 3352740 is mostly the host.
- Transcript errors: "Black Pete / Svartipid / short to beat / Suarta Pete" = Zwarte Piet; "Olavatox" = Olave Talks; "Hello Vitamensa" = the book "Hallo witte mensen"; "Dip South / dips house" = DipSaus. "only had seat class" at 2574960 is garbled; check the audio.
- From about 3934800 ms to the end is a Dutch reading that Apple speech (run as en-US) turned into nonsense. A language-detection gap for M2's per-language comparison.
- Alternates: 1404660–1468800 (school tracking stats, son called Black Pete); 1853580–1924380 (media fabricating the protest story); 2410860–2443020 (love isn't divided).

## graham (Pretty Big Deal with Ashley Graham, Whitney Cummings, 48 min)
- Crosstalk is frequent; almost all chosen clips are Whitney Cummings. ASR errors include "Shel Sandberg", "Jimmy Coco", "Wendy Cummings".
- Clip at 761940 is the weakest; a stronger arc 681240–777000 is 95.8 s, over the limit.
- Alternates: 2214900–2273880 (surgery shame story); 1978200–2056980 (mirrors bit); 1922340–1956780 ("Can I touch it?" consent); 386280–439260 (pantyhose modeling economics); 876480–945900 ("this movie saved my marriage"); 1608360–1682580 (12-step as free therapy); 1690320–1764780 (ask for what you need).

## fox, nolan
Pending: downloads and drafts.

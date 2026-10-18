"""Re-declare every approved frame video before assembly.

assemble-index.mjs moves approved <video> tags out of the frame files into index.html and
leaves a comment behind, so assembling twice loses them. Run this, then assemble once:
  python3 tools/restore-videos.py && node <skill>/scripts/assemble-index.mjs ...
"""
import re
PH = '<!-- approved frame video hoisted by assemble-index -->'
def V(i, src, start, dur, track, x, y, w, h):
    return (f'<video id="{i}" muted playsinline data-frame-video="approved" src="assets/{src}" data-start="{start}" '
            f'data-duration="{dur}" data-track-index="{track}" data-frame-video-x="{x}" data-frame-video-y="{y}" '
            f'data-frame-video-width="{w}" data-frame-video-height="{h}" data-frame-video-fit="cover"></video>')
W, H, GAP, TOP = 338, 600, 32, 210
L = (1920 - (4 * W + 3 * GAP)) // 2
FIVE = ['clip-graham-01.mp4', 'clip-graham-02.mp4', 'clip-graham-04.mp4', 'clip-danaylov-01.mp4']
VIDEOS = {
    '01-hook': [V('f01-vid-source', 'graham-source.mp4', 0, 4.5, 2, 220, 196, 1480, 604)],
    '02-machine': [V('f02-vid-source', 'graham-source.mp4', 0, 5.5, 2, 85, 78, 360, 205)],
    # Source with the real crop burned in (positions measured from choco.video's own output; jump at the 2.133 s camera cut).
    '03-follow': [V('f03-vid-source', 'graham-source-crop.mp4', 0, 8, 2, 120, 250, 1040, 585),
                  V('f03-vid-clip', 'clip-graham-02-from3.4.mp4', 3.4, 4.6, 3, 1340, 100, 450, 800)],
    '04-why': [V('f04-vid-clip', 'clip-graham-02-from8.mp4', 0, 5.5, 5, 1340, 100, 450, 800)],
    '05-breadth': [V(f'f05-vid-{i + 1}', c, round(2.2 + 0.45 * i, 2), round(6 - (2.2 + 0.45 * i), 2), 2 + i,
                     L + i * (W + GAP), TOP, W, H) for i, c in enumerate(FIVE)],
}
for fid, vids in VIDEOS.items():
    p = f'compositions/frames/{fid}.html'
    s = open(p).read().replace(PH, '')
    s = re.sub(r'<video id="f0[0-9]-vid[^>]*></video>', '', s)
    k = s.index('<script')
    open(p, 'w').write(s[:k] + ''.join(vids) + s[k:])
print('videos declared in', ', '.join(VIDEOS))

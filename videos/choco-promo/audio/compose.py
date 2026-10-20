"""Original music bed for the choco.video product film (30 s, 112 BPM, C major, stereo 48 kHz).
Arrangement follows the film: pad under the icon, drums with the app, a lift as the phones
fly out, a pull-back under the statements, a final chord on the end card. Pure numpy.
Usage: python compose.py out.wav"""
import sys, wave
import numpy as np

import argparse
ap = argparse.ArgumentParser()
ap.add_argument("out")
# Section times in seconds; the defaults are the 30 s landscape film.
ap.add_argument("--dur", type=float, default=30.0)
ap.add_argument("--arp", type=float, default=3.4)     # plucks enter
ap.add_argument("--kick", type=float, default=4.3)    # half-time kick enters
ap.add_argument("--full", type=float, default=9.0)    # four-on-the-floor, hats, bass
ap.add_argument("--lift", type=float, default=16.4)   # octave-up arp, open hats
ap.add_argument("--pull", type=float, default=23.2)   # pull back under statements
ap.add_argument("--end", type=float, default=27.4)    # final chord
ap.add_argument("--swell", type=float, default=3.0)   # pad fade-in length
args = ap.parse_args()
SR, DUR, BPM = 48000, args.dur, 112
A3, K4, F9, L16, P23, E27 = args.arp, args.kick, args.full, args.lift, args.pull, args.end
BEAT = 60 / BPM
N = int(SR * DUR)
t_all = np.arange(N) / SR
rng = np.random.default_rng(7)  # deterministic
L = np.zeros(N); R = np.zeros(N)

def hz(m): return 440.0 * 2 ** ((m - 69) / 12)

def env(n, a, d, s, r, hold):
    """ADSR in samples: attack a s, decay d s to sustain s, hold `hold` s, release r s."""
    A, D, H, Rl = int(a * SR), int(d * SR), int(hold * SR), int(r * SR)
    e = np.concatenate([np.linspace(0, 1, max(A, 1)), np.linspace(1, s, max(D, 1)),
                        np.full(max(H - A - D, 0), s), np.linspace(s, 0, max(Rl, 1))])
    return e[:n] if len(e) >= n else np.pad(e, (0, n - len(e)))

def onepole_lp(x, fc):
    a = np.exp(-2 * np.pi * fc / SR)
    # vectorised one-pole via cumulative trick on blocks is complex; a short loop over a
    # decimated signal is fast enough for 30 s.
    y = np.empty_like(x); acc = 0.0
    for i in range(len(x)):
        acc = (1 - a) * x[i] + a * acc; y[i] = acc
    return y

def add(sig, start, gl, gr):
    i = int(start * SR); j = min(N, i + len(sig))
    if i >= N: return
    L[i:j] += sig[: j - i] * gl; R[i:j] += sig[: j - i] * gr

# Chords, one per bar (4 beats): Am9, Fmaj7, C(add9), G6 — warm, hopeful.
chords = [[57, 60, 64, 67, 71], [53, 57, 60, 64, 69], [48, 55, 60, 62, 64], [55, 59, 62, 64, 67]]
bar = 4 * BEAT
bars = int(DUR / bar) + 1

# Pad: detuned additive saws, slow attack; swells in over the intro.
for b in range(bars):
    t0 = b * bar
    if t0 >= E27 and b * bar < E27:  # never
        pass
    ch = chords[b % 4] if t0 < E27 else [48, 55, 60, 64, 67, 72]
    hold = bar + (2.6 if t0 >= E27 - 0.4 else 0.0)
    n = int((hold + 1.2) * SR); tt = np.arange(n) / SR
    sig = np.zeros(n)
    for m in ch:
        for det in (-0.07, 0.0, 0.07):
            f = hz(m) * 2 ** (det / 12)
            for h in range(1, 9):
                sig += np.sin(2 * np.pi * f * h * tt + rng.uniform(0, 6.28)) / (h * 1.6)
    sig *= env(n, 0.5, 0.4, 0.8, 1.0, hold) / (len(ch) * 3)
    gain = 0.16 * (min(1.0, (t0 + 0.6) / args.swell) if t0 < A3 else 1.0)
    if P23 <= t0 < E27: gain *= 0.75
    add(sig, t0, gain, gain * 0.92)

# Pluck arpeggio from A3 s; up an octave and denser from L16 s.
step = BEAT / 2
for k in range(int((E27 - A3) / step)):
    t0 = A3 + k * step
    b = int(t0 / bar)
    ch = chords[b % 4]
    pattern = [0, 2, 3, 1, 4, 2, 3, 1]
    m = ch[pattern[k % 8] % len(ch)] + (12 if t0 >= L16 else 0)
    if P23 <= t0 and k % 2: continue
    n = int(0.6 * SR); tt = np.arange(n) / SR
    f = hz(m + 12)
    sig = (np.sin(2 * np.pi * f * tt) + 0.35 * np.sin(4 * np.pi * f * tt) + 0.12 * np.sin(6 * np.pi * f * tt)) * np.exp(-tt / 0.16)
    pan = 0.35 * np.sin(k * 0.9)
    g = 0.075 * (0.85 if t0 < 9 else 1.0)
    add(sig, t0, g * (1 - pan), g * (1 + pan))

# Bass: root on each beat from 9 s, eighths from L16 s.
for k in range(int((E27 - F9) / BEAT * 2)):
    t0 = F9 + k * BEAT / 2
    if t0 >= E27: break
    if t0 < L16 and k % 2: continue
    if P23 <= t0 and k % 2: continue
    root = chords[int(t0 / bar) % 4][0] - 12
    n = int(0.4 * SR); tt = np.arange(n) / SR
    sig = np.tanh(1.6 * np.sin(2 * np.pi * hz(root) * tt)) * np.exp(-tt / 0.22)
    add(sig, t0, 0.12, 0.12)

# Drums: soft kick on 1 and 3 from K4 s, four-on-the-floor 9–P23 s; hats off-beat from 9 s.
kicks, k = [], 0
t = K4
while t < E27:
    if t < F9 or t >= P23:
        if int(round((t - K4) / BEAT)) % 2 == 0: kicks.append(t)
    else:
        kicks.append(t)
    t += BEAT
duck = np.ones(N)
for t0 in kicks:
    n = int(0.45 * SR); tt = np.arange(n) / SR
    f = 45 + 75 * np.exp(-tt / 0.035)
    sig = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-tt / 0.16)
    sig[: int(0.004 * SR)] += rng.normal(0, 0.3, int(0.004 * SR))
    add(sig, t0, 0.32, 0.32)
    i = int(t0 * SR); j = min(N, i + int(0.3 * SR))
    duck[i:j] = np.minimum(duck[i:j], 0.55 + 0.45 * np.linspace(0, 1, j - i))
t = F9 + BEAT / 2
while t < P23:
    n = int(0.08 * SR); noise = rng.normal(0, 1, n)
    hat = np.diff(noise, prepend=0) * np.exp(-np.arange(n) / SR / (0.05 if t >= L16 else 0.025))
    add(hat, t, 0.035, 0.045)
    t += BEAT

# Sidechain the pad and plucks to the kick (applied to the whole mix except a re-added kick is
# not separable here, so apply gently).
L *= duck ** 0.6; R *= duck ** 0.6

# Reverb: convolve with a short decaying stereo noise tail (FFT).
ir_n = int(1.6 * SR)
irL = rng.normal(0, 1, ir_n) * np.exp(-np.arange(ir_n) / SR / 0.45)
irR = rng.normal(0, 1, ir_n) * np.exp(-np.arange(ir_n) / SR / 0.45)
def conv(x, ir):
    m = len(x) + len(ir) - 1; size = 1 << (m - 1).bit_length()
    y = np.fft.irfft(np.fft.rfft(x, size) * np.fft.rfft(ir, size), size)[: len(x)]
    return y / np.max(np.abs(ir)) / 60
L = L + 0.35 * conv(L, irL); R = R + 0.35 * conv(R, irR)

# Fade in, fade out, soft clip, normalise to -1 dBFS peak.
fade = np.ones(N)
fi = int(0.05 * SR); fade[:fi] = np.linspace(0, 1, fi)
fo_start = int((DUR - 1.4) * SR); fade[fo_start:] = np.linspace(1, 0, N - fo_start) ** 1.5
L = np.tanh(1.3 * L * fade); R = np.tanh(1.3 * R * fade)
peak = max(np.max(np.abs(L)), np.max(np.abs(R)))
L, R = L / peak * 0.89, R / peak * 0.89
pcm = (np.stack([L, R], axis=1) * 32767).astype(np.int16)
with wave.open(args.out, "wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(pcm.tobytes())
print("wrote", args.out, f"{DUR:.1f}s")

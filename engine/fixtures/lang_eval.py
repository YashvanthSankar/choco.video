"""Apple speech vs whisper, per language, on synthetic speech with a known script.

Usage: python3 -I fixtures/lang_eval.py <choco binary> <out json>
For each row of fixtures/languages.tsv: speak the text with that locale's macOS voice, transcribe
with both engines, and score against the script. WER for spaced languages, CER for ja/zh.
Unicode-aware: case-folded, punctuation stripped, NFKC-normalised.
"""
import json, os, re, subprocess, sys, tempfile, time, unicodedata

CHOCO, OUT = sys.argv[1], sys.argv[2]
HERE = os.path.dirname(os.path.abspath(__file__))

def tokens(s, metric):
    s = unicodedata.normalize("NFKC", s).casefold()
    s = "".join(c if (unicodedata.category(c)[0] in "LNM" or c.isspace()) else " " for c in s)
    return [c for c in s if not c.isspace()] if metric == "cer" else s.split()

def edit(a, b):
    d = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        prev, d[0] = d[0], i
        for j, y in enumerate(b, 1):
            prev, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, prev + (x != y))
    return d[-1]

rows = [l.rstrip("\n").split("\t") for l in open(os.path.join(HERE, "languages.tsv"), encoding="utf-8") if l.strip() and not l.startswith("#")]
results, tmp = [], tempfile.mkdtemp()
for locale, wlang, voice, metric, text in rows:
    aiff, wav = os.path.join(tmp, f"{locale}.aiff"), os.path.join(tmp, f"{locale}.wav")
    subprocess.run(["say", "-v", voice, "-o", aiff, text], check=True)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", aiff, "-ar", "16000", "-ac", "1", wav], check=True)
    ref = tokens(text, metric)
    row = {"locale": locale, "voice": voice, "metric": metric, "ref_units": len(ref)}
    for engine, lang in (("apple", locale), ("whisper", wlang)):
        out = os.path.join(tmp, f"{locale}.{engine}.json")
        t0 = time.time()
        p = subprocess.run([CHOCO, "transcribe", wav, "--engine", engine, "--language", lang, "-o", out], capture_output=True, text=True)
        secs = round(time.time() - t0, 1)
        if p.returncode != 0 or not os.path.exists(out):
            err = (p.stderr.strip().splitlines() or ["failed"])[-1][:160]
            row[engine] = {"error": err, "secs": secs}
            continue
        t = json.load(open(out, encoding="utf-8"))
        hyp = tokens(" ".join(w["text"] for s in t["segments"] for w in s["words"]), metric)
        row[engine] = {"rate": round(edit(ref, hyp) / max(1, len(ref)), 3), "secs": secs}
    results.append(row)
    a, w = row.get("apple", {}), row.get("whisper", {})
    print(f"{locale:6} {metric}  apple {a.get('rate', a.get('error'))!s:>8}  whisper {w.get('rate', w.get('error'))!s:>8}", flush=True)
json.dump({"generated": time.strftime("%Y-%m-%d"), "rows": results}, open(OUT, "w", encoding="utf-8"), ensure_ascii=False, indent=1)

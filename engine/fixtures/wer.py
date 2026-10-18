"""Word error rate of a choco transcript JSON against a reference text file.
Usage: python3 -I fixtures/wer.py transcript.json reference.txt"""
import json, re, sys

NUM = {"0": "zero", "1": "one", "2": "two", "3": "three", "4": "four", "5": "five",
       "6": "six", "7": "seven", "8": "eight", "9": "nine", "10": "ten"}

def norm(s):
    return [NUM.get(w, w) for w in re.findall(r"[a-z0-9']+", s.lower())]

t = json.load(open(sys.argv[1]))
ref = norm(open(sys.argv[2]).read())
hyp = norm(" ".join(w["text"] for s in t["segments"] for w in s["words"]))
d = list(range(len(hyp) + 1))
for i, r in enumerate(ref, 1):
    prev, d[0] = d[0], i
    for j, h in enumerate(hyp, 1):
        prev, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, prev + (r != h))
words = [w for s in t["segments"] for w in s["words"]]
mono = all(a["start_ms"] <= b["start_ms"] for a, b in zip(words, words[1:]))
print(f"{t['engine']}: WER {d[-1] / len(ref):.3f} ({d[-1]} errors / {len(ref)} words), monotonic={mono}")

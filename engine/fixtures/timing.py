"""Score transcript word start times against ground-truth onsets from say_aligned.swift.
Usage: python3 -I fixtures/timing.py transcript.json truth.words.json"""
import difflib, json, re, statistics, sys

NUM = {"3": "three"}
def toks(s):
    return [NUM.get(t, t) for t in re.findall(r"[a-z0-9']+", s.lower())]

t = json.load(open(sys.argv[1]))
truth = [(toks(w["text"])[0], w["start_ms"]) for w in json.load(open(sys.argv[2]))["words"] if toks(w["text"])]
hyp = []
for s in t["segments"]:
    for w in s["words"]:
        for k in toks(w["text"]):
            hyp.append((k, w["start_ms"]))
sm = difflib.SequenceMatcher(a=[x[0] for x in truth], b=[x[0] for x in hyp], autojunk=False)
err = [hyp[j + k][1] - truth[i + k][1] for i, j, n in sm.get_matching_blocks() for k in range(n)]
ab = [abs(e) for e in err]
q = statistics.quantiles(ab, n=20)
print(f"{t['engine']:28} matched {len(err)}/{len(truth)} · signed mean {statistics.mean(err):+.0f} ms · "
      f"|err| median {statistics.median(ab):.0f} ms, p90 {q[17]:.0f} ms · within 50 ms {sum(a <= 50 for a in ab) / len(ab):.2f}, "
      f"within 100 ms {sum(a <= 100 for a in ab) / len(ab):.2f}")

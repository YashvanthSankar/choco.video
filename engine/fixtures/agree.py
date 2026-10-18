"""Compare two choco transcripts of the same audio: word agreement and word-timing agreement.
Usage: python3 -I fixtures/agree.py a.json b.json"""
import difflib, json, re, statistics, sys

def words(path):
    t = json.load(open(path))
    out = []
    for s in t["segments"]:
        for w in s["words"]:
            for tok in re.findall(r"[a-z0-9']+", w["text"].lower()):
                out.append((tok, w["start_ms"], w["end_ms"]))
    return t["engine"], out

ea, a = words(sys.argv[1])
eb, b = words(sys.argv[2])
sm = difflib.SequenceMatcher(a=[w[0] for w in a], b=[w[0] for w in b], autojunk=False)
matched = [(a[i + k], b[j + k]) for i, j, n in sm.get_matching_blocks() for k in range(n)]
diffs = [abs(x[1] - y[1]) for x, y in matched]
print(f"{ea}: {len(a)} words · {eb}: {len(b)} words")
print(f"word agreement: {len(matched) / max(len(a), len(b)):.3f} of words match in order")
if diffs:
    q = statistics.quantiles(diffs, n=20)
    print(f"start-time difference on matched words: median {statistics.median(diffs):.0f} ms, p90 {q[17]:.0f} ms, p95 {q[18]:.0f} ms")
    print(f"within 100 ms: {sum(d <= 100 for d in diffs) / len(diffs):.3f} · within 250 ms: {sum(d <= 250 for d in diffs) / len(diffs):.3f}")

"""Readable transcript for editors: one line per segment, "[start_ms-end_ms mm:ss] text".
Usage: python3 -I fixtures/transcript_text.py transcript.json > transcript.txt"""
import json, sys

t = json.load(open(sys.argv[1]))
for s in t["segments"]:
    m, sec = divmod(s["start_ms"] // 1000, 60)
    print(f"[{s['start_ms']}-{s['end_ms']} {m}:{sec:02d}] {s['text']}")

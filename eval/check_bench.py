#!/usr/bin/env python3
"""Compare `dogfight bench` JSON lines against the LOCKED eval/thresholds.toml."""
import sys, json, os, tomllib
T = tomllib.load(open(os.path.join(os.path.dirname(__file__), "thresholds.toml"), "rb"))["perf"]
keys = ["frame_mean_ms", "frame_p95_ms", "bytes_mean_kb", "bytes_p95_kb"]
ok = True
for line in open(sys.argv[1]):
    r = json.loads(line)
    bad = [k for k in keys if r[k] > T[k]]
    ok &= not bad
    print(f"{r['scene']:8} " + "  ".join(f"{k}={r[k]:.2f}" for k in keys) + ("  PASS" if not bad else f"  FAIL {bad}"))
sys.exit(0 if ok else 1)

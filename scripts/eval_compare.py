#!/usr/bin/env python3
"""Compare two eval result JSON files and print per-metric deltas.

Usage: python3 scripts/eval_compare.py <file-a> <file-b>
   or: just eval-compare <file-a> <file-b>
"""

import json
import sys

if len(sys.argv) != 3:
    print(f"Usage: {sys.argv[0]} <file-a> <file-b>", file=sys.stderr)
    sys.exit(1)

a = json.load(open(sys.argv[1]))
b = json.load(open(sys.argv[2]))
af, bf = a.get("fast", {}), b.get("fast", {})

print(f"{'Metric':<20} {'A':>7}  {'B':>7}  {'Delta':>8}")
print("-" * 46)
for k in ["ndcg_at_5", "ndcg_at_10", "mrr", "hit_at_5", "hit_at_10", "recall_at_10"]:
    av, bv = af.get(k, 0), bf.get(k, 0)
    delta = bv - av
    flag = " UP" if delta > 0.01 else (" DN" if delta < -0.01 else "")
    print(f"{k:<20} {av:>7.3f}  {bv:>7.3f}  {delta:>+8.3f}{flag}")

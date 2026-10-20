#!/usr/bin/env python3
"""Average-place margin of the candidate over the base in each lobby dump (positive =
candidate places better), with its standard error over mirrored pairs, and conquest counts."""
import json, sys
from statistics import mean, stdev

for path in sys.argv[1:]:
    L = json.load(open(path))
    pairs = {}
    for l in L:
        c = [a for a, i in zip(l['avp'], l['ids']) if i == 'candidate']
        b = [a for a, i in zip(l['avp'], l['ids']) if i == 'base']
        pairs.setdefault(l['seed'], []).append(mean(b) - mean(c))
    m = [mean(v) for v in pairs.values()]
    se = stdev(m) / len(m) ** 0.5 if len(m) > 1 else 0.0
    won = lambda who: sum(1 for l in L if l['closer'] is not None and l['ids'][l['closer']] == who)
    ended = sum(1 for l in L if l['closer'] is not None)
    print(f"{path.split('/')[-1]}: {len(m)} mirrored pairs, avg place margin {mean(m):+.2f} +- {se:.2f}, "
          f"conquests {ended} (cand {won('candidate')}, base {won('base')})")

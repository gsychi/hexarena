#!/usr/bin/env python3
"""Paired comparison of two lobby dumps on shared seeds: for each lobby (seed and seat
order) present in both, the candidate's mean place advantage over the base, then the
mean and standard error of the difference between the two dumps (A minus B)."""
import json, sys
from statistics import mean, stdev


def margins(path):
    out = {}
    for l in json.load(open(path)):
        c = [a for a, i in zip(l['avp'], l['ids']) if i == 'candidate']
        b = [a for a, i in zip(l['avp'], l['ids']) if i == 'base']
        if c and b:
            out[(l['seed'], tuple(l['ids']))] = mean(b) - mean(c)
    return out


a, b = margins(sys.argv[1]), margins(sys.argv[2])
keys = sorted(set(a) & set(b))
d = [a[k] - b[k] for k in keys]
se = stdev(d) / len(d) ** 0.5 if len(d) > 1 else 0.0
print(f'{len(keys)} shared lobbies: A {mean(a[k] for k in keys):+.2f}  B {mean(b[k] for k in keys):+.2f}  A-B {mean(d):+.2f} +- {se:.2f}')

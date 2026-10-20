#!/usr/bin/env python3
"""Endgame lab analysis: paired places of each candidate against a base candidate from the
same fork (replay, seat), split by the seat's land share at the fork, plus the best
share-threshold gate between two candidates. Lower place is better; margins are base - cand.
usage: lab.py lab.jsonl [base] [alt]"""
import json, sys
from collections import defaultdict
from statistics import mean, stdev

rows = [json.loads(l) for f in sys.argv[1].split(',') for l in open(f)]
base = sys.argv[2] if len(sys.argv) > 2 else 'lab_race'
alt = sys.argv[3] if len(sys.argv) > 3 else 'lab_blitz'
forks = defaultdict(lambda: defaultdict(list))
feat = {}
for r in rows:
    k = (r['replay'].split('/')[-1], r['seat'])
    forks[k][r['cand']].append(r)
    i = r['alive'].index(r['seat'])
    land = r['land']
    feat[k] = dict(share=land[i] * 1000 // r['passable'], rank=sorted(land, reverse=True).index(land[i]) + 1,
                   lead=land[i] - max(l for j, l in enumerate(land) if j != i), tick=r['fork_tick'],
                   orig=r['orig_place'], bot=r['seat_bot'])

def se(v):
    return stdev(v) / len(v) ** 0.5 if len(v) > 1 else 0.0

def place(k, c):
    return mean(r['place'] for r in forks[k][c])

cands = sorted({r['cand'] for r in rows})
print(f"{len(forks)} forks from {len({k[0] for k in forks})} replays")
for c in cands:
    ks = [k for k in forks if c in forks[k]]
    con = sum(r['conquest'] for k in ks for r in forks[k][c])
    print(f"  {c:12s} n {len(ks):3d} place {mean(place(k, c) for k in ks):.2f} conquests {con}"
          f" slowest {max(r['slowest_ms'] for k in ks for r in forks[k][c]):.2f} ms")
for c in cands:
    if c == base:
        continue
    d = [place(k, base) - place(k, c) for k in forks if c in forks[k] and base in forks[k]]
    if d:
        print(f"{c} vs {base}: {mean(d):+.3f} +- {se(d):.3f} over {len(d)} forks (+ = {c} better)")

pairs = [k for k in forks if base in forks[k] and alt in forks[k]]
print(f"\n{alt} - {base} by land rank at the fork (margin +- se, n):")
for rank in (1, 2, 3):
    d = [place(k, base) - place(k, alt) for k in pairs if feat[k]['rank'] == rank]
    if d:
        print(f"  rank {rank}: {mean(d):+.2f} +- {se(d):.2f} n {len(d)}")
print("by land share (permille of passable):")
for lo, hi in ((0, 150), (150, 250), (250, 350), (350, 450), (450, 1001)):
    d = [place(k, base) - place(k, alt) for k in pairs if lo <= feat[k]['share'] < hi]
    if d:
        print(f"  {lo:4d}-{hi:4d}: {mean(d):+.2f} +- {se(d):.2f} n {len(d)}")
print("by fork tick:")
for lo, hi in ((0, 5000), (5000, 7000), (7000, 8500), (8500, 10001)):
    d = [place(k, base) - place(k, alt) for k in pairs if lo <= feat[k]['tick'] < hi]
    if d:
        print(f"  {lo:5d}-{hi:5d}: {mean(d):+.2f} +- {se(d):.2f} n {len(d)}")
print(f"gate 'use {alt} when share >= th': mean place (all-{base} {mean(place(k, base) for k in pairs):.3f}, "
      f"all-{alt} {mean(place(k, alt) for k in pairs):.3f}, oracle {mean(min(place(k, base), place(k, alt)) for k in pairs):.3f})")
for th in range(0, 701, 50):
    v = [place(k, alt) if feat[k]['share'] >= th else place(k, base) for k in pairs]
    print(f"  th {th:3d}: {mean(v):.3f} ({sum(feat[k]['share'] >= th for k in pairs)} use {alt})")
if '-v' in sys.argv:
    for k in sorted(pairs):
        print(k, feat[k], {c: round(place(k, c), 2) for c in forks[k]})

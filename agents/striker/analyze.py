#!/usr/bin/env python3
"""Attack metrics from replays: per player land curve, enemy captures, surrenders,
eliminations (who took the last hex), buildings captured.

usage: analyze.py [--prefix NAME] replay.json ...
"""
import json, sys, collections

args = sys.argv[1:]
prefix = None
if args and args[0] == '--prefix':
    prefix = args[1]
    args = args[2:]

agg = collections.defaultdict(lambda: collections.Counter())
for path in args:
    d = json.load(open(path))
    names = [p['name'] for p in d['players']]
    n = len(names)
    land = [1] * n
    caps = collections.Counter()
    early = collections.Counter()
    neutral = collections.Counter()
    lost = collections.Counter()
    surr = collections.Counter()
    bcap = collections.Counter()
    kills = collections.Counter()
    elim = {}
    curve = collections.defaultdict(dict)
    last_taker = {}
    for t, evs in enumerate(d['events']):
        for e in evs:
            k = e.get('type'); v = e
            if k in ('Captured', 'captured'):
                p = v['player']; po = v.get('previous_owner')
                land[p] += 1
                if po is None:
                    neutral[p] += 1
                else:
                    land[po] -= 1
                    caps[p] += 1
                    lost[po] += 1
                    last_taker[po] = p
                    if t < 3000:
                        early[p] += 1
                    if v.get('building'):
                        bcap[(p, v['building'])] += 1
            elif k in ('Surrendered', 'surrendered'):
                c = v['capturer']; f = v['from']; h = len(v['hexes'])
                land[c] += h; land[f] -= h; surr[c] += h
                last_taker[f] = c
            elif k in ('Eliminated', 'eliminated'):
                p = v['player']
                elim[p] = t
                kills[last_taker.get(p)] += 1
        if t % 1000 == 999:
            for p in range(n):
                curve[p][t + 1] = land[p]
    res = d['result']
    print(f"== {path} final_tick={len(d['events'])} result={json.dumps(res)[:200]}")
    for p in range(n):
        if prefix and not names[p].startswith(prefix):
            continue
        c = ' '.join(f"{curve[p].get(x, '-'):>4}" for x in range(1000, 10001, 1000))
        bc = {b: bcap[(p, b)] for (q, b) in bcap if q == p}
        print(f"  {p} {names[p][:22]:22} land[{c}] enemyCap={caps[p]} early={early[p]} neutral={neutral[p]} lost={lost[p]} surr={surr[p]} kills={kills[p]} elim={elim.get(p)} bcap={bc}")
        a = agg[names[p]]
        a['seats'] += 1; a['enemyCap'] += caps[p]; a['early'] += early[p]; a['neutral'] += neutral[p]
        a['lost'] += lost[p]; a['surr'] += surr[p]; a['kills'] += kills[p]; a['died'] += p in elim
        a['final_land'] += max(land[p], 0)
print("== aggregate per seat")
for name, a in sorted(agg.items()):
    s = a['seats']
    print(f"  {name[:24]:24} seats={s} " + ' '.join(f"{k}={a[k]/s:.1f}" for k in ['enemyCap', 'early', 'neutral', 'lost', 'surr', 'kills', 'died', 'final_land']))

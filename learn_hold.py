#!/usr/bin/env python3
"""Fits a hold table from this repo's v0.7 replays: the chance a captured hex is still
the capturer's `H` ticks later, by (format, tick bucket, own neighbours after the capture,
hostile neighbours). Ownership is rebuilt from captured/surrendered events only."""
import glob, json, os, sys, collections

H = 50
N = int(sys.argv[1]) if len(sys.argv) > 1 else 160
NB = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, -1), (-1, 1)]
files = sorted(glob.glob('/workspace/data/matches/*.summary.json'), key=os.path.getmtime)
stats = collections.defaultdict(lambda: [0, 0])
done = {'duel': 0, 'ffa': 0}
for f in reversed(files):
    if done['duel'] >= N // 2 and done['ffa'] >= N // 2:
        break
    try:
        s = json.load(open(f))
    except Exception:
        continue
    if s.get('rules_version') != 'v0.7' or s.get('final_tick') is None:
        continue
    fmt = 'duel' if len(s['seats']) == 2 else 'ffa'
    if done[fmt] >= N // 2:
        continue
    rp = f.replace('.summary.json', '.replay.json')
    if not os.path.exists(rp):
        continue
    try:
        d = json.load(open(rp))
    except Exception:
        continue
    done[fmt] += 1
    own = {}
    pending = []  # (due tick, hex, player, key)
    for t, evs in enumerate(d['events']):
        for e in evs:
            if e['type'] == 'captured':
                h = (e['at']['q'], e['at']['r'])
                p = e['player']
                own[h] = p
                if e.get('previous_owner') is None:
                    continue
                mine = hostile = 0
                for dq, dr in NB:
                    o = own.get((h[0] + dq, h[1] + dr))
                    if o == p:
                        mine += 1
                    elif o is not None:
                        hostile += 1
                key = (fmt, min(t // 2000, 4), mine, hostile)
                pending.append((t + H, h, p, key))
            elif e['type'] == 'surrendered':
                for x in e['hexes']:
                    own[(x['q'], x['r'])] = e['capturer']
        while pending and pending[0][0] <= t:
            _, h, p, key = pending.pop(0)
            st = stats[key]
            st[0] += 1
            st[1] += own.get(h) == p
out = {f'{k[0]}|{k[1]}|{k[2]}|{k[3]}': v for k, v in sorted(stats.items())}
json.dump(out, open('/tmp/wt-wildcard/logs/hold.json', 'w'), indent=0)
for fmt in ('duel', 'ffa'):
    print(fmt, 'games', done[fmt])
    for tb in range(5):
        row = []
        for mine in range(7):
            n = sum(stats[(fmt, tb, mine, h)][0] for h in range(7))
            k = sum(stats[(fmt, tb, mine, h)][1] for h in range(7))
            row.append(f'{mine}:{k / n:.2f}({n})' if n else f'{mine}:-')
        print(' t', tb * 2000, ' '.join(row))

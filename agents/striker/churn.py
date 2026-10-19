import json, sys, collections
d = json.load(open(sys.argv[1]))
names = [p['name'] for p in d['players']]
lo, hi = int(sys.argv[2]), int(sys.argv[3])
n = len(names)
owner = {}
captured_at = {}
acts = collections.defaultdict(collections.Counter)
retaken = collections.Counter()
caps = collections.Counter()
sent = collections.defaultdict(list)
for t, evs in enumerate(d['events']):
    lastmove = {}
    for e in evs:
        ty = e['type']
        if ty == 'moved':
            lastmove[e['player']] = e
        if ty == 'captured':
            k = (e['at']['q'], e['at']['r'])
            po = e['previous_owner']
            p = e['player']
            if lo <= t < hi and po is not None:
                caps[p] += 1
                prev = captured_at.get(k)
                if prev and prev[1] == po and t - prev[0] <= 20:
                    retaken[p] += 1
                m = lastmove.get(p)
                if m:
                    sent[p].append((m['troops'], e['troops_left']))
            captured_at[k] = (t, p)
        if ty == 'surrendered' and lo <= t < hi:
            acts[e['capturer']]['surr_hexes'] += len(e['hexes'])
    if lo <= t < hi:
        for p, c in d['commands'][t]:
            a = c['action']
            acts[p][a['type']] += 1
for p in range(n):
    if not acts[p]:
        continue
    s = sent[p]
    avg_sent = sum(x for x, _ in s) / max(len(s), 1)
    avg_left = sum(y for _, y in s) / max(len(s), 1)
    print(f"{p} {names[p][:20]:20} acts={dict(acts[p])} enemyCaps={caps[p]} retakes(<=20t)={retaken[p]} avg_sent={avg_sent:.0f} avg_left={avg_left:.0f}")

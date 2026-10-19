#!/usr/bin/env python3
"""Scores eval lobbies the way the arena's Elo weighs them: every pair of seats of
different bots is a game won by the better place, weighted 2.0 if the lobby ended
before the cutoff and 2/N otherwise. Reports the candidate's and base's weighted win
rate against the seats that are neither, and the pairwise candidate-vs-base record."""
import json, sys
for path in sys.argv[1:]:
    L = json.load(open(path))
    tot = {'candidate': [0, 0], 'base': [0, 0]}
    head = [0, 0]
    for l in L:
        n = len(l['ids']); w = 2.0 if l['closer'] is not None else 2.0 / n
        for i, a in enumerate(l['ids']):
            for j, b in enumerate(l['ids']):
                if i == j or a == b:
                    continue
                r = 1.0 if l['avp'][i] < l['avp'][j] else 0.5 if l['avp'][i] == l['avp'][j] else 0.0
                if a in tot and b not in tot:
                    tot[a][0] += w * r; tot[a][1] += w
                if a == 'candidate' and b == 'base':
                    head[0] += w * r; head[1] += w
    closes = sum(1 for l in L if l['closer'] is not None and l['ids'][l['closer']] == 'candidate')
    bcl = sum(1 for l in L if l['closer'] is not None and l['ids'][l['closer']] == 'base')
    ended = sorted(l['final_tick'] for l in L if l['closer'] is not None)
    print(path.split('/')[-1], 'games', len(L), 'conquests %d (%.0f%%) median tick %s' % (
          len(ended), 100 * len(ended) / max(len(L), 1), ended[len(ended) // 2] if ended else '-'), 'cand vs field %.3f' % (tot['candidate'][0] / max(tot['candidate'][1], 1e-9)),
          'base vs field %.3f' % (tot['base'][0] / max(tot['base'][1], 1e-9)), 'cand vs base %.3f (w %.1f)' % (head[0] / max(head[1], 1e-9), head[1]), 'closes', closes, bcl)

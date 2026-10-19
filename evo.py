#!/usr/bin/env python3
"""(1+2) evolution over Wildcard's 1v1 overtime knobs.

Each generation mutates the champion twice, plays each mutant against the frozen
opponents in mirrored 1v1 pairs (one eval at a time, one thread), and keeps a mutant
only if its mean gain beats the champion's by more than half a standard error.
"""
import json, os, random, subprocess, sys, time

WT = '/tmp/wt-wildcard'
BIN = sys.argv[1] if len(sys.argv) > 1 else '/tmp/hx-wild-e'
GENS = int(sys.argv[2]) if len(sys.argv) > 2 else 4
OPP = ['mason-v1', 'striker-v1']
PAIRS = 8
SPACE = {
    'late_from': (2500, 7000, 250),
    'send_pct': (20, 100, 10),
    'push_gather': (0, 40, 2),
    'late_sure_x10': (5, 40, 5),
    'late_margin_pct': (20, 300, 20),
    'late_enemy': (0, 120, 10),
}
rng = random.Random(667)


def evaluate(g, tag, seed):
    path = f'{WT}/genomes/evo-{tag}.json'
    json.dump(g, open(path, 'w'), indent=1)
    gains = []
    for opp in OPP:
        env = dict(os.environ, HEXARENA_AUTHOR='agent:wildcard', HEXARENA_EVAL_THREADS='1',
                   HEXARENA_ARENA='/workspace/data/arena')
        out = subprocess.run(['nice', '-n', '10', BIN, 'eval', '--params', path, '--against', opp,
                              '--pairs', str(PAIRS), '--players', '2', '--seed', str(seed)],
                             capture_output=True, text=True, env=env).stdout
        r = json.loads(out)
        gains.append((r['gain'], r['gain_se'] or 0))
    m = sum(x for x, _ in gains) / len(gains)
    se = (sum(s * s for _, s in gains) ** 0.5) / len(gains)
    with open(f'{WT}/logs/evo.txt', 'a') as f:
        f.write(f"{time.strftime('%T')} {tag} seed {seed} gain {m:+.2f} se {se:.2f} {gains} {json.dumps({k: g['knobs'].get(k) for k in SPACE})}\n")
    return m, se


def mutate(g):
    c = json.loads(json.dumps(g))
    for k in rng.sample(list(SPACE), 2):
        lo, hi, step = SPACE[k]
        v = c['knobs'].get(k, (lo + hi) // 2)
        v = min(hi, max(lo, v + rng.choice([-3, -2, -1, 1, 2, 3]) * step))
        c['knobs'][k] = v
    return c


champ = json.load(open(f'{WT}/genomes/blitz.json'))
for gen in range(GENS):
    seed = 610000 + gen * 100
    cm, cse = evaluate(champ, f'g{gen}-champ', seed)
    best = (cm, cse, champ)
    for j in range(2):
        m = mutate(champ)
        mm, mse = evaluate(m, f'g{gen}-m{j}', seed)
        if mm > best[0] + 0.5 * max(mse, cse):
            best = (mm, mse, m)
    champ = best[2]
    json.dump(champ, open(f'{WT}/genomes/evo-champ.json', 'w'), indent=1)

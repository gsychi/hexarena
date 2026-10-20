#!/usr/bin/env python3
"""(1+2) evolution over Wildcard's 1v1 knobs (the duel_ overrides, handover included).

Each generation mutates the champion twice, plays each mutant against the frozen
opponents in mirrored 1v1 pairs (one eval at a time, one thread), and keeps a mutant
only if its mean gain beats the champion's by more than half a standard error.

Runs as a queue job: the queue calls it like the hexarena binary
(`evo.py eval --params GENOME ...`), so the champion is the genome after --params, and
it prints a dummy eval result when done.
"""
import json, os, random, subprocess, sys, time

WT = '/tmp/wt-wildcard'
BIN = os.environ.get('EVO_BIN', '/tmp/hx-wild-p')
GENS = int(os.environ.get('EVO_GENS', '3'))
START = sys.argv[sys.argv.index('--params') + 1] if '--params' in sys.argv else f'{WT}/genomes/wc3.json'
OPP = ['mason-v2', 'pangolin-v20']
PAIRS = 8
SPACE = {
    'duel_late_from': (2500, 7000, 250),
    'duel_send_pct': (20, 100, 10),
    'duel_push_gather': (0, 40, 2),
    'duel_late_sure_x10': (5, 40, 5),
    'duel_late_margin_pct': (20, 300, 20),
    'duel_handover': (1500, 6000, 250),
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


champ = json.load(open(START if START.startswith('/') else f'{WT}/{START}'))
CORE_DEFAULT = {'late_margin_pct': 100, 'late_sure_x10': 20, 'send_pct': 0, 'push_gather': 0}
for k in SPACE:
    base = k[len('duel_'):]
    if k not in champ['knobs']:
        champ['knobs'][k] = champ['knobs'].get(base, CORE_DEFAULT.get(base))
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
print(json.dumps({"gain": 0, "gain_se": 0, "games": 0, "candidate_closes": 0, "base_closes": 0, "secs": 0}))

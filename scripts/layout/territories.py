"""Study 3: how close each bot's real territory is to its best possible layout.

cargo run --release -p hexarena-cli --example territory_dump -- <replays> > territories.jsonl
python3 scripts/layout/territories.py territories.jsonl [per_bot_tick] [ticks] ["Bot A|Bot B"]

For a sample of each bot's territories it solves, on the real terrain:
  same-troops optimum: best income using no more troops than the bot spent on its buildings
  unlimited optimum:   best income if every owned hex could be built
  interior optimum:    best income with border hexes left empty (they get captured first)
and reports actual income as a share of each. Walls stay where they are (base income only).
"""

import collections
import json
import random
import sys

from hexlib import cost, income, optimise


def load(path, per, ticks, bots=None):
    by = collections.defaultdict(list)
    for line in open(path):
        d = json.loads(line)
        if bots and d["bot"] not in bots:
            continue
        if d["tick"] in ticks and len(d["hexes"]) >= 40:
            by[(d["bot"], d["tick"])].append(d)
    rng = random.Random(1)
    return {k: rng.sample(v, min(per, len(v))) for k, v in by.items() if len(v) >= 5}


def analyse(d):
    hexes = [(q, r) for q, r, *_ in d["hexes"]]
    heights = {(q, r): h for q, r, h, *_ in d["hexes"]}
    layout = {(q, r): b for q, r, _, b, _ in d["hexes"]}
    border = {(q, r) for q, r, _, _, br in d["hexes"] if br}
    walls = {h for h, b in layout.items() if b == "wall"}
    actual = income(layout, hexes)
    spent = cost({h: b for h, b in layout.items() if b != "wall"})
    same, _, _, _ = optimise(hexes, heights, budget=spent, forbid=walls, time_limit=20, gap=0.02)
    full, full_cost, _, _ = optimise(hexes, heights, forbid=walls, time_limit=20, gap=0.02)
    inner, _, _, _ = optimise(hexes, heights, forbid=walls | border, time_limit=20, gap=0.02)
    low = sum(heights[h] <= 0 for h in hexes) / len(hexes)
    built = sum(1 for b in layout.values() if b in ("farm", "village", "quarry"))
    return {
        "n": len(hexes),
        "actual": actual,
        "same": same,
        "full": full,
        "inner": inner,
        "spent": spent,
        "full_cost": full_cost,
        "low": low,
        "border": len(border) / len(hexes),
        "built": built / len(hexes),
        "border_built": sum(1 for h in border if layout[h] in ("farm", "village", "quarry")) / max(len(border), 1),
    }


def main():
    path = sys.argv[1]
    per = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    ticks = {int(t) for t in (sys.argv[3] if len(sys.argv) > 3 else "3000,6000").split(",")}
    bots = set(sys.argv[4].split("|")) if len(sys.argv) > 4 else None
    groups = load(path, per, ticks, bots)
    print(f"{'bot':<26} {'tick':>5} {'n':>3} {'land':>5} {'low%':>5} {'bord%':>5} {'built%':>6} {'bord built%':>11} "
          f"{'inc/hx':>6} {'same$ opt':>9} {'%':>4} {'full opt':>8} {'%':>4} {'inner opt':>9} {'spent/hx':>8} {'full$/hx':>8}")
    for (bot, tick), items in sorted(groups.items()):
        rs = [analyse(d) for d in items]
        n = sum(r["n"] for r in rs)
        a = sum(r["actual"] for r in rs)
        s = sum(r["same"] for r in rs)
        f = sum(r["full"] for r in rs)
        i = sum(r["inner"] for r in rs)
        mean = lambda k: sum(r[k] for r in rs) / len(rs)
        print(f"{bot[:26]:<26} {tick:>5} {len(rs):>3} {n / len(rs):>5.0f} {mean('low'):>5.0%} {mean('border'):>5.0%} {mean('built'):>6.0%} {mean('border_built'):>11.0%} "
              f"{a / n:>6.2f} {s / n:>9.2f} {a / s:>4.0%} {f / n:>8.2f} {a / f:>4.0%} {i / n:>9.2f} "
              f"{sum(r['spent'] for r in rs) / n:>8.1f} {sum(r['full_cost'] for r in rs) / n:>8.1f}", flush=True)


if __name__ == "__main__":
    main()

"""Study 4: build order over time on a fixed plot.

python3 scripts/layout/buildorder.py [radius] [hill_fraction] [build_share]

All hexes owned from tick 0, troops pooled; `build_share` of income goes to a building pool
(the rest is kept for fighting) and one build happens whenever the pool can pay.
Each strategy has a target layout and builds its next hex by best marginal income per troop
(a Village with no Farm yet is valued as if its planned Farms were in place, so the first
Village of a flower is not starved). Reports the troop bank (income minus spending, i.e.
troops left over for fighting) and income at several ticks. Demolish refunds nothing.

Strategies (all use quarries ringed by Villages on hills when hills exist):
  honeycomb   Villages on class (q - r) % 3 == 0, Farms on the other two (today's bots)
  lattice     Farms on class 1, Villages on classes 0 and 2 (yield-optimal)
  optimum     the exact terrain-aware optimum from hexlib.optimise
  upgrade@T   honeycomb, then from tick T demolish class-2 Farms and build Villages there,
              ending in the lattice
"""

import random
import sys

from hexlib import COST, DISCOUNT, FARM_HEIGHTS, QUARRY_HEIGHTS, dist, hex_plot, income, neighbors, optimise

TICKS = (300, 600, 1000, 1500, 2000, 3000, 5000, 8000)


def quarry_sites(hexes, heights):
    sites = []
    for h in sorted(hexes, key=lambda h: (h[0] - h[1]) % 7):
        if heights[h] in QUARRY_HEIGHTS and all(dist(h, s) >= 3 for s in sites):
            sites.append(h)
    return set(sites)


def target(kind, hexes, heights, quarries):
    lay = {}
    for h in hexes:
        if h in quarries:
            lay[h] = "quarry"
            continue
        cls = (h[0] - h[1]) % 3
        low = heights[h] in FARM_HEIGHTS
        if kind == "honeycomb":
            lay[h] = "farm" if low and cls != 0 else "village"
        else:
            lay[h] = "farm" if low and cls == 1 else "village"
    return lay


def price(h, b, lay):
    c = COST[b]
    if b != "quarry" and any(lay.get(n) == "quarry" for n in neighbors(h)):
        c *= 1 - DISCOUNT
    return c


def gain(h, b, lay, goal, owned):
    if b == "village":
        g = 1 + sum(1 for n in neighbors(h) if n in owned and goal.get(n) in ("farm", "quarry"))
        built = sum(1 for n in neighbors(h) if n in owned and lay.get(n) in ("farm", "quarry"))
        return 1 + built + 0.5 * (g - 1 - built)
    return sum(1 for n in neighbors(h) if n in owned and lay.get(n) == "village") + (
        0.5 * sum(1 for n in neighbors(h) if n in owned and goal.get(n) == "village" and lay.get(n) != "village")
    )


def simulate(hexes, heights, kind, upgrade_at=None, share=1.0, optimum=None):
    owned = set(hexes)
    quarries = quarry_sites(hexes, heights)
    if kind == "optimum":
        goal = final = optimum
    else:
        goal = target("honeycomb" if kind != "lattice" else "lattice", hexes, heights, quarries)
        final = target("lattice", hexes, heights, quarries)
    lay = {h: "" for h in hexes}
    bank, war, spent, out = 0.0, 0.0, 0.0, {}
    inc = income(lay, hexes)
    for t in range(1, max(TICKS) + 1):
        if upgrade_at is not None and t == upgrade_at:
            goal = final
        bank += inc * share
        war += inc * (1 - share)
        while True:
            cands = []
            for h in hexes:
                want = goal[h]
                have = lay[h]
                if want == have or want == "":
                    continue
                c = price(h, want, lay)
                g = gain(h, want, lay, goal, owned)
                if have == "farm":
                    g -= sum(1 for n in neighbors(h) if lay.get(n) == "village")
                if g > 0:
                    cands.append((g / c, h, want, c))
            if not cands:
                break
            _, h, want, c = max(cands)
            if bank < c:
                break
            bank -= c
            spent += c
            lay[h] = want
            inc = income(lay, hexes)
        if t in TICKS:
            out[t] = (bank + war, inc)
    return out, spent


def main():
    radius = int(sys.argv[1]) if len(sys.argv) > 1 else 6
    hills = float(sys.argv[2]) if len(sys.argv) > 2 else 0.0
    share = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
    hexes = hex_plot(radius)
    rng = random.Random(7)
    heights = {h: (1 if rng.random() < hills else 0) for h in hexes}
    n = len(hexes)
    _, _, opt, _ = optimise(hexes, heights, time_limit=60)
    print(f"plot radius {radius}: {n} hexes, {hills:.0%} hills, {share:.0%} of income available for building.")
    print("Columns: troops produced minus troops spent on buildings (thousands) / income per tick")
    runs = [("honeycomb", None), ("lattice", None), ("optimum", None)] + [(f"upgrade@{u}", u) for u in (500, 1000, 2000)]
    print(f"{'strategy':<14}" + "".join(f"{t:>12}" for t in TICKS) + f"{'spent':>8}")
    for name, up in runs:
        kind = "upgrade" if up else name
        out, spent = simulate(hexes, heights, kind, up, share, opt)
        print(f"{name:<14}" + "".join(f"{out[t][0] / 1000:>6.1f}k/{out[t][1]:<4}" for t in TICKS) + f"{spent:>8.0f}")


if __name__ == "__main__":
    main()

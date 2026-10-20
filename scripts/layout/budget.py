"""Study 2: the income-per-troop frontier. For a fixed plot, the best income reachable
with a given building budget, and which pattern the optimiser picks at each budget.

python3 scripts/layout/budget.py [radius] [hill_fraction]

Also prints large-plot asymptotics for the farm-centred lattice (cheap to evaluate
directly; on flat ground it equals the exact optimum at every radius tested).
"""

import random
import sys

from hexlib import best_offset, hex_plot, lattice, optimise, village_flower


def main():
    radius = int(sys.argv[1]) if len(sys.argv) > 1 else 6
    hills = float(sys.argv[2]) if len(sys.argv) > 2 else 0.0
    hexes = hex_plot(radius)
    rng = random.Random(7)
    heights = {h: (1 if rng.random() < hills else 0) for h in hexes}
    n = len(hexes)
    full, full_cost, _, _ = optimise(hexes, heights, time_limit=120)
    print(f"plot radius {radius} ({n} hexes, {hills:.0%} hills). Unlimited budget: {full / n:.3f}/hex for {full_cost:.0f} troops")
    print(f"{'budget/hex':>10} {'inc/hex':>8} {'gain/troop':>10} {'marg/troop':>10} {'F':>4} {'V':>4} {'Q':>3} {'V w/0 F/Q':>9}")
    prev_val, prev_b = n, 0
    for per_hex in (5, 10, 15, 20, 30, 40, 50, 60, 70, 80, 90):
        budget = per_hex * n
        val, spent, lay, status = optimise(hexes, heights, budget=budget, time_limit=60)
        nf = sum(b == "farm" for b in lay.values())
        nv = sum(b == "village" for b in lay.values())
        nq = sum(b == "quarry" for b in lay.values())
        bare = sum(
            1
            for h, b in lay.items()
            if b == "village" and not any(lay.get(x) in ("farm", "quarry") for x in [(h[0] + d[0], h[1] + d[1]) for d in [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)]])
        )
        marg = (val - prev_val) / max(spent - prev_b, 1)
        print(f"{per_hex:>10} {val / n:>8.3f} {(val - n) / max(spent, 1):>10.4f} {marg:>10.4f} {nf:>4} {nv:>4} {nq:>3} {bare:>9}  [{status}]")
        prev_val, prev_b = val, spent
        if val >= full - 0.5:
            break

    print("\nLARGE PLOTS (flat): farm-centred lattice vs village-centred honeycomb, troops/tick per hex")
    print(f"{'R':>3} {'hexes':>6} {'farm-lattice':>12} {'village-flower':>14} {'border share':>12}")
    for r in (6, 8, 10, 12, 16, 20, 30):
        hx = hex_plot(r)
        fl, _ = best_offset(lattice, hx)
        vf, _ = best_offset(village_flower, hx)
        print(f"{r:>3} {len(hx):>6} {fl / len(hx):>12.3f} {vf / len(hx):>14.3f} {6 * r / len(hx):>12.1%}")


if __name__ == "__main__":
    main()

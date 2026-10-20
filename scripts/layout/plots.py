"""Study 1: exact optimal layouts on hexagonal plots of growing radius.

python3 scripts/layout/plots.py [max_radius]

Flat low ground (Farms anywhere), all hills (Quarries only), and a half-and-half plot,
each solved exactly and compared with the farm-centred lattice and the village-centred
honeycomb that bots build today.
"""

import sys
import time

from hexlib import best_offset, hex_plot, income, lattice, optimise, render, village_flower


def main():
    max_r = int(sys.argv[1]) if len(sys.argv) > 1 else 8
    print("FLAT LOW GROUND (troops/tick per hex; 1 base included)")
    print(f"{'R':>2} {'hexes':>5} {'optimum':>8} {'farm-lattice':>12} {'village-flower':>14} {'opt F:V':>8} {'opt cost/hx':>11} {'status':>8} {'sec':>5}")
    pictures = {}
    for r in range(1, max_r + 1):
        hexes = hex_plot(r)
        t = time.time()
        val, spent, lay, status = optimise(hexes, time_limit=180)
        fl, _ = best_offset(lattice, hexes)
        vf, _ = best_offset(village_flower, hexes)
        n = len(hexes)
        nf = sum(b == "farm" for b in lay.values())
        nv = sum(b == "village" for b in lay.values())
        print(f"{r:>2} {n:>5} {val / n:>8.3f} {fl / n:>12.3f} {vf / n:>14.3f} {nf / max(nv, 1):>8.2f} {spent / n:>11.1f} {status:>8} {time.time() - t:>5.0f}")
        if r in (2, 4):
            pictures[r] = render(lay, hexes)
    print("limit (infinite plane): optimum 3.667, farm-lattice 3.667, village-flower 3.333")
    for r, pic in pictures.items():
        print(f"\noptimal layout, radius {r}:\n{pic}")

    print("\nALL HILLS (heights 1..2: Villages and Quarries only, Quarries >= 3 apart)")
    for r in (2, 3, 4, 6, 8):
        if r > max_r:
            continue
        hexes = hex_plot(r)
        heights = {h: 1 for h in hexes}
        val, spent, lay, status = optimise(hexes, heights, time_limit=180)
        n = len(hexes)
        nq = sum(b == "quarry" for b in lay.values())
        print(f"R={r:>2} hexes={n:>4} optimum {val / n:.3f}/hex  quarries={nq}  cost/hex {spent / n:.1f}  all-villages 2.000  [{status}]")
        if r == 3:
            print(render(lay, hexes))

    print("\nHALF LOW / HALF HILLS (split along q = 0; hills where q > 0)")
    for r in (3, 5, 7):
        if r > max_r:
            continue
        hexes = hex_plot(r)
        heights = {h: (1 if h[0] > 0 else 0) for h in hexes}
        val, spent, lay, status = optimise(hexes, heights, time_limit=180)
        fl = income(lattice(hexes, heights), hexes)
        n = len(hexes)
        print(f"R={r:>2} hexes={n:>4} optimum {val / n:.3f}/hex  farm-lattice-on-low + villages-on-hills {fl / n:.3f}  [{status}]")
        if r == 3:
            print(render(lay, hexes))


if __name__ == "__main__":
    main()

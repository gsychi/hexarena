"""Hex geometry, v0.7 economy rules, and an exact layout optimiser (integer program, HiGHS).

Income per owned hex per tick: 1 base; a Village adds 1, plus 1 per adjacent own Farm and
1 per adjacent own Quarry. Farms only on heights -2..0, Quarries only on 1..2 and at least
3 apart. Costs: Farm 30, Village 120, Quarry 50; builds within 1 of an own Quarry are 40%
cheaper (Farm 18, Village 72).

Requires `pip install pulp==2.9.0 highspy` (PuLP 3+ changed the modelling API).
"""

from __future__ import annotations

import pulp

DIRS = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)]
FARM_HEIGHTS = {-2, -1, 0}
QUARRY_HEIGHTS = {1, 2}
COST = {"farm": 30, "village": 120, "quarry": 50}
DISCOUNT = 0.4


def neighbors(h):
    q, r = h
    return [(q + dq, r + dr) for dq, dr in DIRS]


def dist(a, b):
    dq, dr = a[0] - b[0], a[1] - b[1]
    return max(abs(dq), abs(dr), abs(dq + dr))


def hex_plot(radius):
    return [(q, r) for q in range(-radius, radius + 1) for r in range(-radius, radius + 1) if abs(q + r) <= radius]


def income(layout, hexes):
    """layout: {hex: 'farm'|'village'|'quarry'|''}; hexes: owned hexes."""
    owned = set(hexes)
    total = 0
    for h in hexes:
        total += 1
        if layout.get(h) == "village":
            total += 1
            for n in neighbors(h):
                if n in owned and layout.get(n) in ("farm", "quarry"):
                    total += 1
    return total


def cost(layout):
    total = 0.0
    quarries = [h for h, b in layout.items() if b == "quarry"]
    for h, b in layout.items():
        if b not in COST:
            continue
        c = COST[b]
        if b != "quarry" and any(dist(h, q) == 1 for q in quarries):
            c *= 1 - DISCOUNT
        total += c
    return total


def lattice(hexes, heights=None, offset=0):
    """Farm-centred lattice: Farms where (q - r) % 3 == offset on low ground, Villages elsewhere."""
    lay = {}
    for h in hexes:
        low = heights is None or heights[h] in FARM_HEIGHTS
        lay[h] = "farm" if low and (h[0] - h[1]) % 3 == offset else "village"
    return lay


def village_flower(hexes, heights=None, offset=0):
    """Village-centred honeycomb (today's bots): Villages where (q - r) % 3 == offset, Farms elsewhere."""
    lay = {}
    for h in hexes:
        low = heights is None or heights[h] in FARM_HEIGHTS
        lay[h] = "village" if (h[0] - h[1]) % 3 == offset or not low else "farm"
    return lay


def optimise(hexes, heights=None, budget=None, forbid=(), time_limit=120, quarry=True, gap=1e-4):
    """Maximise income over `hexes`. heights: {hex: -2..2} (None = all height 0).
    budget: max troops spent on buildings. forbid: hexes that must stay empty.
    Returns (income, cost, layout, status)."""
    hexes = list(hexes)
    owned = set(hexes)
    forbid = set(forbid)
    hgt = (lambda h: 0) if heights is None else (lambda h: heights[h])
    m = pulp.LpProblem("layout", pulp.LpMaximize)
    idx = {h: i for i, h in enumerate(hexes)}
    f, v, q = {}, {}, {}
    for h in hexes:
        i = idx[h]
        v[h] = pulp.LpVariable(f"v{i}", cat="Binary")
        if hgt(h) in FARM_HEIGHTS:
            f[h] = pulp.LpVariable(f"f{i}", cat="Binary")
        if quarry and hgt(h) in QUARRY_HEIGHTS:
            q[h] = pulp.LpVariable(f"q{i}", cat="Binary")
        m += v[h] + f.get(h, 0) + q.get(h, 0) <= (0 if h in forbid else 1)
    # Quarries at least 3 apart: at most one Quarry in every radius-1 ball.
    if q:
        for c in hexes:
            ball = [n for n in [c] + neighbors(c) if n in q]
            if len(ball) > 1:
                m += pulp.lpSum(q[n] for n in ball) <= 1
    y = {}
    for h in hexes:
        for n in neighbors(h):
            if n in owned and (n in f or n in q):
                e = pulp.LpVariable(f"y{idx[h]}_{idx[n]}", lowBound=0, upBound=1)
                m += e <= v[h]
                m += e <= f.get(n, 0) + q.get(n, 0)
                y[(h, n)] = e
    obj = len(hexes) + pulp.lpSum(v.values()) + pulp.lpSum(y.values())
    m += obj
    spend = None
    if budget is not None:
        d = {}
        for h in hexes:
            near = [q[n] for n in neighbors(h) if n in q]
            if near:
                for kind, var in (("v", v.get(h)), ("f", f.get(h))):
                    if var is None:
                        continue
                    dv = pulp.LpVariable(f"d{kind}{idx[h]}", lowBound=0, upBound=1)
                    m += dv <= var
                    m += dv <= pulp.lpSum(near)
                    d[(kind, h)] = dv
        spend = (
            pulp.lpSum(COST["village"] * x for x in v.values())
            + pulp.lpSum(COST["farm"] * x for x in f.values())
            + pulp.lpSum(COST["quarry"] * x for x in q.values())
            - pulp.lpSum(DISCOUNT * COST["village" if k == "v" else "farm"] * x for (k, _), x in d.items())
        )
        m += spend <= budget
    solver = pulp.HiGHS(msg=False, timeLimit=time_limit, gapRel=gap)
    m.solve(solver)
    lay = {}
    for h in hexes:
        if v[h].value() and v[h].value() > 0.5:
            lay[h] = "village"
        elif h in f and f[h].value() and f[h].value() > 0.5:
            lay[h] = "farm"
        elif h in q and q[h].value() and q[h].value() > 0.5:
            lay[h] = "quarry"
        else:
            lay[h] = ""
    return income(lay, hexes), cost(lay), lay, pulp.LpStatus[m.status]


def render(layout, hexes):
    """ASCII picture: F farm, V village, Q quarry, . empty (offset rows)."""
    sym = {"farm": "F", "village": "V", "quarry": "Q", "": ".", None: "."}
    rows = sorted({h[1] for h in hexes})
    qs = [h[0] for h in hexes]
    lines = []
    for r in rows:
        line = " " * (r - rows[0])
        for qq in range(min(qs), max(qs) + 1):
            h = (qq, r)
            line += (sym[layout.get(h)] if h in set(hexes) else " ") + " "
        lines.append(line.rstrip())
    return "\n".join(lines)


def best_offset(fn, hexes, heights=None):
    best = None
    for off in range(3):
        lay = fn(hexes, heights, off)
        val = income(lay, hexes)
        if best is None or val > best[0]:
            best = (val, lay)
    return best

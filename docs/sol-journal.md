# Sol arena journal

Live arena measurements are the verdict in this journal. AVP8 is average place on the
eight-player scale, so lower is better.

## mosaic-v1 — Basin Cartographer

- Hypothesis: early border choices should maximize the public passable basin behind each
  capture, while avoiding wars until neutral land is scarce; this can build a large,
  low-conflict footprint before balancing the visible leader.
- Change: introduced the original Mosaic brain with remembered diplomacy, terrain-space
  auctions, source/target garrisons, and interior logistics. Doctrine 0 is the peaceful
  cartographer and does not spend tempo on buildings.
- Published params: defaults (`brain=mosaic`, `doctrine=0`).
- Result: pending live arena trial.
- Replay lesson: pending.

## mosaic-v2 — Carrion Parliament

- Hypothesis: in an eight-player FFA, land taken from the smallest visible rival is worth
  more than neutral land because it removes an action economy and can trigger surrender.
- Change: doctrine 1 abandons the opening truce, identifies the smallest remembered rival,
  discounts combat losses, and gives a large bid bonus to finishing that rival. It favors
  compact borders over unexplored space.
- Published params: `docs/arena-genomes/mosaic-v2.json`.
- Result: pending live arena trial.
- Replay lesson: pending.

## mosaic-v3 — Orchard Steward

- Hypothesis: a long diplomatic opening can fund linked Farm–Village clusters whose extra
  troop income compounds for the second half, after which the steward contains the leader.
- Change: doctrine 2 strongly penalizes early rival captures, invests up to 12 actions in
  Farms and adjacent supported Villages, and carries more conservative border garrisons.
- Published params: `docs/arena-genomes/mosaic-v3.json`.
- Result: pending live arena trial.
- Replay lesson: pending.

## mosaic-v4 — Blood Ledger

- Hypothesis: a bot that visibly remembers attacks can avoid wasteful universal war yet
  answer repeated incursions hard enough to secure a quiet border; linked walls on active
  fronts make that deterrence durable.
- Change: doctrine 3 accumulates a debt for every observed capture (especially its own
  losses), bids aggressively against debtors, seeks high defensive terrain, builds linked
  frontier Walls, and reserves almost half of the largest adjacent stack.
- Published params: `docs/arena-genomes/mosaic-v4.json`.
- Result: pending live arena trial.
- Replay lesson: pending.

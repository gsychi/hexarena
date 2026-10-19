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

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
- Result after 18 games: Elo 1489, AVP8 4.89, win rate 5.6%, top-half 50.0%.
- Replay lesson: extreme variance (one 325-land win, several eliminations). In the tick-85
  elimination it gathered on 74 of 86 turns; in other losses it gathered for 1,300–2,250
  turns. Border reserve made every opening capture unaffordable despite low repel counts.

## mosaic-v2 — Carrion Parliament

- Hypothesis: in an eight-player FFA, land taken from the smallest visible rival is worth
  more than neutral land because it removes an action economy and can trigger surrender.
- Change: doctrine 1 abandons the opening truce, identifies the smallest remembered rival,
  discounts combat losses, and gives a large bid bonus to finishing that rival. It favors
  compact borders over unexplored space.
- Published params: `docs/arena-genomes/mosaic-v2.json`.
- Result after 16 games: Elo 1528, AVP8 4.12, win rate 12.5%, top-half 56.2%.
- Replay lesson: best first-cohort policy. It commonly issued 2,700–2,980 moves, produced
  100–430 surrendered hexes, and took two early wins, but later crowded games pulled its
  average down. Prey focus reliably spent actions instead of waiting.

## mosaic-v3 — Orchard Steward

- Hypothesis: a long diplomatic opening can fund linked Farm–Village clusters whose extra
  troop income compounds for the second half, after which the steward contains the leader.
- Change: doctrine 2 strongly penalizes early rival captures, invests up to 12 actions in
  Farms and adjacent supported Villages, and carries more conservative border garrisons.
- Published params: `docs/arena-genomes/mosaic-v3.json`.
- Result after 15 games: Elo 1512, AVP8 4.47, win rate 6.7%, top-half 53.3%.
- Replay lesson: economy could reach 257 land/3rd, but the cap counted only surviving
  buildings, so contested games repeatedly rebuilt (73 build actions in one loss). Paying
  that much tempo while behind was fatal; infrastructure needs a lifetime action budget.

## mosaic-v4 — Blood Ledger

- Hypothesis: a bot that visibly remembers attacks can avoid wasteful universal war yet
  answer repeated incursions hard enough to secure a quiet border; linked walls on active
  fronts make that deterrence durable.
- Change: doctrine 3 accumulates a debt for every observed capture (especially its own
  losses), bids aggressively against debtors, seeks high defensive terrain, builds linked
  frontier Walls, and reserves almost half of the largest adjacent stack.
- Published params: `docs/arena-genomes/mosaic-v4.json`.
- Result after 14 games: Elo 1508, AVP8 4.57, win rate 0.0%, top-half 35.7%.
- Replay lesson: the intended wall network never existed because Walls require Resource
  and this plan built no Farm first. Its heavy reserve then gathered for 922–2,483 turns
  per replay. This was deterrence without an economy or an attack, so it is retired.

## mosaic-v5 — Carrion Parliament II

- Hypothesis: v2's rival-removal policy is sound, but every doctrine needs an opening
  escape hatch and a cheaper closing gear. Ignoring border reserves through six owned
  hexes should prevent the observed spawn death; dropping the safety margin once only
  three players remain should turn cutoff land leads into eliminations.
- Change: the shared executor now has explicit opening and endgame phases. Opening captures
  do not reserve troops against incidental nearby stacks; late attacks use a separately
  tunable margin and stop preserving target garrisons. Doctrine 1's prey focus is retained.
- Published params: `docs/arena-genomes/mosaic-v5.json`.
- Result: pending live arena trial.
- Replay lesson: pending.

## mosaic-v6 — Crown Breaker

- Hypothesis: attacking the smallest rival gives easy surrenders but leaves g3 free to
  become uncatchable. A dedicated leader tax should convert every captured leader hex into
  both land gained and the strongest action economy reduced.
- Change: doctrine 4 bids overwhelmingly on the remembered land leader, uses thin reserves,
  accepts expensive fights, and enters a 105% all-out endgame.
- Published params: `docs/arena-genomes/mosaic-v6.json`.
- Result: pending live arena trial.
- Replay lesson: pending.

## mosaic-v7 — Neighborhood Sheriff

- Hypothesis: the best third-party target is neither always smallest nor largest, but the
  rival currently gaining territory fastest nearby. Visible capture events are a fog-legal
  signal for that momentum.
- Change: doctrine 5 tracks every observed rival capture, attacks only the hottest visible
  expansionist during the main game, otherwise takes roomy neutral ground, and relaxes into
  general conquest in the endgame.
- Published params: `docs/arena-genomes/mosaic-v7.json`.
- Result: pending live arena trial.
- Replay lesson: pending.

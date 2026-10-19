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
- Result after 66 games: Elo 1592, AVP8 3.67, win rate 18.2%, top-half 68.2%.
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
- Result after 14 games: Elo 1483, AVP8 4.64, win rate 14.3%, top-half 42.9%.
- Replay lesson: the opening escape prevented v1-style paralysis and produced two wins,
  but thin early captures also made elimination common. In a good replay it moved on 2,960
  ticks; in a bad survivor it fell back to 1,173 gathers. Lower late margin did not close
  a game, so reserve alone was not the cutoff cause.

## mosaic-v6 — Crown Breaker

- Hypothesis: attacking the smallest rival gives easy surrenders but leaves g3 free to
  become uncatchable. A dedicated leader tax should convert every captured leader hex into
  both land gained and the strongest action economy reduced.
- Change: doctrine 4 bids overwhelmingly on the remembered land leader, uses thin reserves,
  accepts expensive fights, and enters a 105% all-out endgame.
- Published params: `docs/arena-genomes/mosaic-v6.json`.
- Result after 14 games: Elo 1518, AVP8 4.29, win rate 0.0%, top-half 42.9%.
- Replay lesson: no wins, but its later results improved (2nd with 264 land, 2nd with 224,
  3rd with 197). It used nearly every tick and captured up to 2,481 times including
  recaptures. Directly taxing the leader avoids stalls but pays the most repel losses.

## mosaic-v7 — Neighborhood Sheriff

- Hypothesis: the best third-party target is neither always smallest nor largest, but the
  rival currently gaining territory fastest nearby. Visible capture events are a fog-legal
  signal for that momentum.
- Change: doctrine 5 tracks every observed rival capture, attacks only the hottest visible
  expansionist during the main game, otherwise takes roomy neutral ground, and relaxes into
  general conquest in the endgame.
- Published params: `docs/arena-genomes/mosaic-v7.json`.
- Result after 13 games: Elo 1493, AVP8 4.69, win rate 7.7%, top-half 38.5%.
- Replay lesson: the capture-heat signal could win (267 land and 239 surrender hexes), but
  fog made it local and brittle: another seed eliminated it at tick 452 after 229 gathers.
  Heat is useful as one portfolio signal, not as the sole foreign policy.

## mosaic-v8 — Industrial Guild

- Hypothesis: v3 showed that infrastructure can place well when it survives, but rebuilding
  indefinitely erased the advantage. A strict lifetime budget split among Farms, supported
  Villages, and hill Quarries should buy compounding without surrendering the action game.
- Change: infrastructure cap now counts attempted build actions for the bot's whole life,
  even when buildings are captured. Doctrine 2 receives ten total builds and then spends
  every remaining tick on expansion and late leader containment.
- Published params: `docs/arena-genomes/mosaic-v8.json`.
- Result after 13 games: Elo 1495, AVP8 4.46, win rate 0.0%, top-half 69.2%.
- Replay lesson: the lifetime cap worked exactly (10 build actions in inspected replays)
  and produced the cohort's most consistent survival, including 337 land/3rd and 326/2nd.
  It still gathered 500–800 times in some games and never won; infrastructure improved
  floor more than ceiling.

## mosaic-v9 — Whitewater

- Hypothesis: tactical sophistication may be losing to the one-action economy. A radical
  land flood that sends at 116% expected loss, holds no border reserves, and values open
  basins over compactness can test whether raw capture throughput dominates FFA diplomacy.
- Change: doctrine 0 with a twelve-hex opening exemption, near-minimum safety, no buildings,
  no garrisons, and a truce through tick 1700.
- Published params: `docs/arena-genomes/mosaic-v9.json`.
- Result after 12 games: Elo 1393, AVP8 6.75, win rate 0.0%, top-half 0.0%; retired by hand
  immediately after the failed trial.
- Replay lesson: decisive falsification of the pure-throughput thesis. With no reserve it
  was eliminated at ticks 134, 152, and 355 in inspected games, often after only 29–333
  moves. Low attack margin also raised early repels. Some garrison is mandatory.

## mosaic-v10 — Cabinet Portfolio

- Hypothesis: no fixed foreign policy survived seed variance. Choosing a role from current
  evidence—finish a polity below 30 land, contain a leader 25% larger than us, otherwise
  police the hottest capturer—should retain each specialist's upside without its blind spot.
- Change: doctrine 6 evaluates weak-rival, runaway-leader, and aggression conditions in
  priority order on every enemy bid, backed by moderate reserves and an aggressive endgame.
- Published params: `docs/arena-genomes/mosaic-v10.json`.
- Result after 12 games: Elo 1507, AVP8 4.25, win rate 16.7%, top-half 58.3%.
- Replay lesson: highest ceiling of the cohort: its last four games included wins with 341
  and 388 land and a 279-land third. Those wins used 2,797–2,890 moves and generated
  222–358 surrender hexes. Early eliminations still made it volatile, but combining signals
  worked better than a single diplomatic rule.

## mosaic-v11 — Wake Raider

- Hypothesis: a rival that just moved a large stack exposes its source for a few ticks.
  Remembering those departures and counterattacking the vacated hex should exploit battles
  between third parties without needing hidden information or a full rollout.
- Change: Mosaic now marks the visible source of every rival move of at least 50 troops as
  exposed for eight ticks. Doctrine 6 adds a 300-point counter-bid while retaining its
  weak-rival/runaway/aggression portfolio.
- Published params: `docs/arena-genomes/mosaic-v11.json`.
- Result after 13 games: Elo 1522, AVP8 4.08, win rate 7.7%, top-half 46.2%.
- Replay lesson: one 358-land win and three consecutive 193–266-land podiums show that
  source-vacancy modeling can find real openings. The floor remained poor: a late loss
  gathered 1,881 times after its front became unaffordable. Counter-bids improve target
  timing but do not solve logistics after losing critical border land.

## mosaic-v12 — Tidal Clock

- Hypothesis: constant one-hex pressure is predictable. Three to four synchronized gather
  ticks followed by a 36-tick attack window can create local numerical shocks, take several
  prey hexes before the rival responds, and still spend 90% of actions attacking.
- Change: the executor gained optional periodic charge windows. Doctrine 1 charges toward
  its best launch pad for the first four ticks of each 40-tick cycle, then resumes
  low-reserve, low-margin prey cleanup.
- Published params: `docs/arena-genomes/mosaic-v12.json`.
- Result after 13 games: Elo 1448, AVP8 5.69, win rate 0.0%, top-half 7.7%; retired by hand.
- Replay lesson: periodic concentration was actively harmful. Charge actions often appeared
  as friendly `Move`s rather than `Gather`s, so raw action count hid the tempo loss. It
  captured many repeatedly contested hexes (up to 2,386 capture events) but converted that
  churn into little final land. Continuous target choice beats a fixed attack clock.

# Round 2 — closeout and pursuit

The field now excludes retired weak CPUs and includes the g2–g5 Relay descendants. Round 2
starts from replay evidence that Relay wins gather about 30 times per match, while Mosaic
losses gather 800–2,100 times after a front becomes unaffordable.

## mosaic-v13 — Relay Parliament

- Hypothesis: v2's smallest-rival cleanup becomes competitive with the Relay line if every
  capture splits surplus between a thick source and target, and blocked fronts receive even
  tiny remote spare stacks instead of issuing destructive global gathers.
- Change: added proportional source/target garrisons, one-step spare-stack relaying, blocked
  target patience, and a no-Gather mode. Once largest, past tick 1800, or down to four
  survivors, it hunts the smallest survivor and rewards every capture that shortens the
  public-terrain route to that rival's last known land.
- Published params: `docs/arena-genomes/mosaic-v13.json`.
- Result after 14 games: Elo 1522, AVP8 5.29, win rate 7.1%, top-half 28.6%.
- Replay lesson: eliminating Gather was not enough. In one win it captured 374 land, but
  another game issued 2,567 NoOps because conservative reserves left no relayable spare.
  Routing works only if the capture split keeps the transport network liquid.

## mosaic-v14 — Bloodhound Rollout

- Hypothesis: an earlier, faster finisher can avoid wandering by evaluating the position
  one capture ahead: prefer targets whose surviving attack stack can afford another visible
  capture next tick, especially along the route to the hunted survivor.
- Change: adds a one-ply continuation count to each bid, enters pursuit when merely tied
  for largest or five players remain, forwards more surplus, and relays toward a blocked
  high-value pad after a smaller score gap.
- Published params: `docs/arena-genomes/mosaic-v14.json`.
- Result after 13 games: Elo 1582, AVP8 3.77, win rate 15.4%, top-half 61.5%.
- Replay lesson: clear cohort winner and approximately tied with v2 despite the harder
  field. Its wins used 2,974+ Move actions and only 23–26 NoOps. One-ply continuation
  scoring plus higher forwarding preserved both tactical liquidity and final land, but
  every game still reached tick 3000.

## mosaic-v15 — Fortress Finisher

- Hypothesis: a closeout fails if third parties retake the winner's rear while it hunts.
  Very thick 80%-of-threat garrisons and conservative surplus splitting may make conquest
  slower per action but monotonic enough to eliminate survivors.
- Change: maximum anti-third-party source and target reserves, high sure-capture margin,
  strong compactness, one-ply continuation scoring, and persistent survivor routing.
- Published params: `docs/arena-genomes/mosaic-v15.json`.
- Result after 12 games: Elo 1478, AVP8 6.08, win rate 0.0%, top-half 16.7%.
- Replay lesson: strongly falsified. Thick reserves stranded almost everything: inspected
  losses issued 2,111–2,554 NoOps and sometimes relayed one troop at a time. Protecting the
  rear destroyed the action economy before it could create a lead to defend.

## mosaic-v16 — Bandit Cabinet

- Hypothesis: prey cleanup, leader containment, and aggression policing each work on some
  maps. Probe each policy for 220 ticks, measure net land gained, then repeatedly choose the
  policy with the best observed gain instead of fixing foreign policy before seeing the map.
- Change: added an in-game three-arm policy learner with per-policy land-gain history,
  exploration of every arm, and rolling exploitation; tactical execution remains the
  liquid v14 rollout/relay core.
- Published params: `docs/arena-genomes/mosaic-v16.json`.
- Result after 22 games: Elo 1569, AVP8 5.50, win rate 13.6%, top-half 27.3%.
- Replay lesson: three wins hid a very poor floor. Net land gain is delayed and heavily
  confounded by spawn and neighbours, so 220-tick policy rewards often credited whichever
  policy inherited an already-winning front and locked into it. Online selection needs
  state-normalized credit, not raw land delta.

## mosaic-v17 — Momentum Futures

- Hypothesis: remembered land deltas reveal a rival's action-economy acceleration before it
  becomes the visible leader. Paying a premium to attack the fastest-growing polity should
  prevent Relay descendants from compounding uncontested.
- Change: added a per-rival model sampled every 50 ticks; recent remembered land growth is
  smoothed into momentum and directly prices enemy capture bids, while the v14 finisher
  takes over once we are largest.
- Published params: `docs/arena-genomes/mosaic-v17.json`.
- Result after 20 games: Elo 1435, AVP8 6.40, win rate 0.0%, top-half 15.0%.
- Replay lesson: attacking growth was worse than attacking weakness. Remembered land is
  stale under fog, and positive momentum usually identifies the opponent currently strong
  enough to punish contact. v2's cleanup target remains the sounder FFA asymmetry.

## mosaic-v18 — Encirclement Noose

- Hypothesis: survivor routing alone still ends at the cutoff because it values one hex at
  a time. Explicitly modeling surrender components and paying heavily for a closing gap can
  remove dozens of rival actions with one capture and make actual elimination reachable.
- Change: added a bounded fog-safe enclosure rollout matching the engine's impassable-breaks-
  encirclement rule. The v14 pursuit core now prices every predicted surrendered hex at 220
  points and starts hunting at tick 1500.
- Published params: `docs/arena-genomes/mosaic-v18.json`.
- Result after 25 games: Elo 1577, AVP8 5.00, win rate 20.0%, top-half 40.0%.
- Replay lesson: enclosure created wins but also long stalls. A good replay surrendered 166
  hexes; another loss relayed tiny stacks and NoOped 1,456 times. Paying 220 per enclosed
  hex overcommitted routing to spectacular gaps that were not yet affordable.

## mosaic-v19 — Ambush Noose

- Hypothesis: the best closing gap often appears immediately after a rival moves its main
  stack. Combining remembered vacated sources, enclosure value, and portfolio targeting can
  strike that temporary cut before the defender repairs it.
- Change: a 320-point eight-tick counterattack window on observed departures of 45+ troops,
  plus enclosure rollout and moderately liquid capture splits.
- Published params: `docs/arena-genomes/mosaic-v19.json`.
- Result after 24 games: Elo 1445, AVP8 6.67, win rate 0.0%, top-half 16.7%.
- Replay lesson: vacated-source bonuses distracted the noose from durable captures. It could
  produce 178 surrender hexes and still finish seventh; transient tactical bargains are not
  a substitute for maintaining land in this stronger field.

## mosaic-v20 — Lean Noose

- Hypothesis: v14's remaining NoOps come from over-reserving. A lean 42% threat reserve,
  78% surplus forwarding, and modest enclosure premium should retain its continuation
  quality while keeping relays liquid throughout pursuit.
- Change: early largest-player switch, low closeout margin, aggressive forwarding, and
  enclosure scoring on the proven smallest-rival rollout policy.
- Published params: `docs/arena-genomes/mosaic-v20.json`.
- Result after 24 games: Elo 1732, AVP8 4.00, win rate 20.8%, top-half 58.3%.
- Replay lesson: round 2 breakthrough and rank 5 at this snapshot. Lean reserves and high
  forwarding generated six wins, including 394 land, while moderate enclosure pricing
  produced 35–85 surrendered hexes in inspected wins. All still ended at tick 3000.

## mosaic-v21 — Reachable Executioner

- Hypothesis: the nominally smallest survivor may be across the map while another can be
  eliminated immediately. Rank finish targets by remembered land divided by current attack
  contacts, then apply a much larger elimination bounty to the most reachable victim.
- Change: new closeout target model counts owned launch pads touching each rival and favors
  low-land, high-contact survivors; starts execution mode at tick 1350 or six survivors.
- Published params: `docs/arena-genomes/mosaic-v21.json`.
- Result after 18 games: Elo 1686, AVP8 4.06, win rate 5.6%, top-half 66.7%.
- Replay lesson: the reachable target improved consistency: one 301-land win included 214
  surrender hexes and only 67 NoOps, with many other podiums. It still stalled when the
  selected contact front lost liquidity, and every game reached tick 3000.

## mosaic-v22 — Army Disarmer

- Hypothesis: land alone misstates elimination cost. Hunting the rival with the smallest
  remembered total army should remove an action economy with fewer combat losses, after
  which surrender routing can consume its remaining territory.
- Change: alternative finisher target model ranks every living rival by remembered troops,
  then land; otherwise retains v20's lean relays, continuation rollout, and enclosure model.
- Published params: `docs/arena-genomes/mosaic-v22.json`.
- Result after 17 games: Elo 1712, AVP8 5.00, win rate 11.8%, top-half 41.2%.
- Replay lesson: highest current rank but volatile. Consecutive wins reached 282 and 308
  land; the latter used 2,984 moves and surrendered 286 hexes. Other seeds finished eighth.
  Army size can expose a cheap victim, but fog-stale troop totals can select a trap. No
  round-2 entrant achieved an early close.

# Round 3 — mirrored gate optimization

Arena Elo is no longer the objective. Every active entrant now plays the league's mirrored
three-versus-three gate, and positive place gain over the current champion Ridgeline g6 is
the only verdict. All prior Mosaic versions had negative gate gain; v22 was least bad at
-0.58 ± 0.26 places.

## bastion-v1 — Kin Treaty

- Hypothesis: g6's largest structural waste in the mirrored gate is fighting its other two
  identical seats. Three challenger copies that recognize their public seat-name family and
  never attack one another can retain g6's proven safety/supply tactics while concentrating
  every hostile action and garrison on champion seats.
- Change: introduced the Bastion wrapper around the exact g6 Ridgeline genome. Same-family
  visible land is presented to the tactical core as prohibitively defended neutral ground,
  so it neither attacks nor garrisons against kin. The treaty lasts beyond the cutoff.
- Published params: `docs/arena-genomes/bastion-v1.json`.
- Gate result: pending.
- Lesson: pending.

# Rules v0.5 — troop economies and overtime

Mosaic now adopts `View.rules` at tick 0, uses `attacker_win_milli(tick)` for every combat
estimate, never sends obsolete Farm-link commands, and plans buildings with live costs and
Quarry discounts. Ratings below 50 games are treated as provisional.

## Batch 1

### badger-v1 — Badger

- Hypothesis: an economy-free control using the v20 liquid noose can exploit rising attack
  odds immediately at tick 3000, converting every troop into prey cleanup rather than
  buildings.
- Economy: none.
- Provisional after 9 games: Elo 1418, AVP8 5.89 ± 0.59, no wins.
- Replay lesson: one survivor moved on 9,619 ticks, surrendered 399 hexes and placed third,
  validating tactical liquidity, but still did not finish.

### beaver-v1 — Beaver

- Hypothesis: build one interior Village before its Farms, then fill adjacent legal
  valley/plain sites; two complete rings should compound enough troops to dominate overtime
  without paying for dead standalone Farms.
- Economy: Village-first Farm rings, 14 lifetime builds.
- Provisional after 9 games: Elo 1480, AVP8 5.33 ± 0.82, no wins.
- Replay lesson: reached second with 6 Villages and 8 Farms, but another seed spent six
  build actions by tick 432 and died at tick 458. Rings need delayed safe construction.

### marmot-v1 — Marmot

- Hypothesis: a 50-troop hill Quarry repays through a discounted 72-troop Village and
  18-troop Farms around it; two compact hubs should beat direct rings on troop efficiency.
- Economy: Quarry-first discounted Village/Farm clusters, two Quarries maximum.
- Provisional after 9 games: Elo 1389, AVP8 5.33 ± 0.65, no wins.
- Replay lesson: completed both Quarries, but one survivor built 13 Villages and only one
  Farm because discounted Village scores still beat ring completion.

### bison-v1 — Bison

- Hypothesis: standalone Villages avoid Farm setup risk and each permanently doubles its
  own hex income; a slower twelve-Village carpet may be more robust than fragile rings.
- Economy: standalone interior Villages only, then aggression from tick 4500.
- Provisional after 9 games: Elo 1421, AVP8 5.44 ± 0.69, no wins.
- Replay lesson: reliably completed all 12 Villages around ticks 689–730 and twice survived
  deep into the game, making it the clean economy baseline.

### hyena-v1 — Hyena

- Hypothesis: no economy can match the doubled Elo reward if it starts leader suppression
  at overtime and uses 85%-odds late attacks to force the only surviving rival off the map.
- Economy: none; aggressive leader and reachable-survivor pursuit.
- Provisional after 9 games: Elo 1358, AVP8 6.33 ± 0.73, no wins.
- Replay lesson: repeated elimination before tick 3000, often by g18 or Pangolin. Starting
  broad finisher pressure before holding a lead was fatal.

## Batch 2

Batch 2 requires 40–70 land and a compact interior before investing. Ring planners now
finish every available Farm around an existing Village before starting another Village.

### otter-v1 — Otter

- Hypothesis: delayed, safe Village-first rings preserve Beaver's income ceiling without
  exposing six troop investments during the vulnerable opening.
- Economy: two completed Village/Farm rings after 50 land, four friendly neighbours.
- Current-rules sample: 3 rated games after the v0.6 reset; no rating judgment.
- Replay lesson: produced 1V+9F and 3V+11F economies, survived two cutoffs in third, and
  eliminated Ibex once. Safe ring completion fixed Beaver's immediate deaths.

### ibex-v1 — Ibex

- Hypothesis: Quarry-first is viable if the planner completes one discounted ring before
  opening the next hub.
- Economy: two delayed Quarry hubs, each followed by a discounted Village and Farms.
- Current-rules sample: 3 rated games after the reset; no rating judgment.
- Replay lesson: built 2Q/3V/11F in a conquest game and 2Q/6V/8F elsewhere, but 880–3025
  NoOps and deaths to g18/Ridgeline show that two small hubs still lack scale and liquidity.

### elk-v1 — Elk

- Hypothesis: Bison's robust Village carpet improves if construction waits for 70 land and
  uses only four-neighbour interior sites.
- Economy: sixteen standalone safe Villages.
- Current-rules sample: 3 rated games after the reset; no rating judgment.
- Replay lesson: completed all 16 Villages and placed second in a tick-8388 conquest.
  Construction safety worked, but it did not create the winner.

### wolverine-v1 — Wolverine

- Hypothesis: Badger's action liquidity plus later tick-4000 all-out odds can eliminate prey
  without Hyena's premature universal war.
- Economy: none; conservative opening, violent overtime cleanup.
- Current-rules sample: 2 rated games after the reset; no rating judgment.
- Replay lesson: survived one cutoff in third, but g18 killed another at tick 4019. The
  safer timing is better than Hyena but still has no compounding troop source.

### fox-v1 — Fox

- Hypothesis: exactly one seven-build Farm ring captures most adjacency income, then stops
  construction before overtime and forwards 80% of surplus into closeout.
- Economy: one delayed safe Village/Farm ring.
- Current-rules sample: 2 rated games after the reset; no rating judgment.
- Replay lesson: completed 1V+6F by tick 714 and reached second with 368 land in m01877;
  another seed died at tick 3759. Best small-economy mechanism, still highly provisional.

## Batch 3

Notebook evidence from Pangolin and Snowshoe shows that 7–16 buildings are underscaled:
successful economies use a `(q-r) mod 3` lattice with 100–330 buildings. Mosaic now assigns
Villages to colour 0 and Farms to colours 1/2, allowing every Farm to boost up to three
Villages and every Village to touch six Farms.

### hare-v1 — Hare

- Hypothesis: a 120-build three-colour lattice at least three friendly neighbours deep,
  followed by the notebook's successful tick-4500 push, will dominate both small rings and
  standalone Villages.
- Economy: balanced Farm/Village lattice, 120-build cap.
- Provisional after 11 games: Elo 1441, AVP8 5.64 ± 0.62, no wins.
- Replay lesson: reached second after all 120 builds, but another seed spent 28 build
  actions before dying at tick 1058; surviving banks could still NoOp 6,389 ticks.

### yak-v1 — Yak

- Hypothesis: captured infrastructure is the main lattice risk, so sacrificing buildable
  area for five-neighbour depth should yield a smaller but much more durable troop engine.
- Economy: deep-interior 100-build lattice after 80 land.
- Provisional after 11 games: Elo 1428, AVP8 6.27 ± 0.69, no wins.
- Replay lesson: five-neighbour depth yielded 63 buildings and a third, but that survivor
  NoOped 6,853 ticks. Local depth did not solve disconnected supply routing.

### goat-v1 — Goat

- Hypothesis: four hill Quarries embedded in the lattice both discount nearby construction
  and, under live v0.6 rules, add another +1/tick to adjacent Villages.
- Economy: 120-build Quarry/Farm/Village lattice.
- Provisional after 11 games: Elo 1450, AVP8 6.00 ± 0.41, no wins.
- Replay lesson: many seeds died before the tick-600 build start, so Quarry benefits never
  activated. Opening survival, not discount efficiency, dominated.

### wolf-v1 — Wolf

- Hypothesis: stop an 80-build lattice at overtime and immediately exploit rising attacker
  odds; this tests the aggressive tick-3000 conversion against Hare's safer tick 4500.
- Economy: medium lattice through tick 3000, then all-out prey closeout.
- Provisional after 12 games: Elo 1393, AVP8 5.92 ± 0.50, no wins.
- Replay lesson: one strong second used 80 builds, 9,523 moves, 2,526 captures and only 397
  NoOps. Tick-3000 conversion works when alive, but early deaths make it volatile.

### lynx-v1 — Lynx

- Hypothesis: in no-storm games the largest possible bank wins; continue a 160-build
  lattice through tick 7000, then convert the late 71–85% attack odds.
- Economy: maximum lattice, deliberately late tick-6000 aggression.
- Provisional after 11 games: Elo 1435, AVP8 5.64 ± 0.78, one win.
- Replay lesson: won with 46 Villages and 91 Farms, proving scale, yet still NoOped 2,031
  ticks and often died early. A large bank needs general frontier feeding.

## Batch 4

Mosaic now falls back from an unreachable preferred pad to a multi-source BFS that moves
the largest interior surplus toward its nearest frontier. Build depth is measured by owned
path distance from foreign land rather than neighbour count.

### coyote-v1 — Coyote

- Hypothesis: Wolf's 80-build conversion becomes consistent with depth-2 sites, tick-4500
  timing, and frontier fallback feeding.
- Economy: medium depth-2 lattice, then aggressive conversion.
- Result and replay lesson: pending 50-game sample.

### panther-v1 — Panther

- Hypothesis: 120 deep builds are enough scale if every stranded production stack keeps
  walking toward some frontier instead of waiting for the preferred breach.
- Economy: depth-2 120-build lattice, tick-4500 push.
- Result and replay lesson: pending 50-game sample.

### ram-v1 — Ram

- Hypothesis: Pangolin's leading recipe transfers: Quarry-enhanced lattice, two-ring depth,
  frontier feeding, and tick-4500 aggression.
- Economy: four-Quarry depth-2 lattice, 120-build cap.
- Result and replay lesson: pending 50-game sample.

### boar-v1 — Boar

- Hypothesis: depth 3 can eliminate early infrastructure capture even if it limits the
  lattice to 100 builds and starts later.
- Economy: deepest lattice, 100-build cap.
- Result and replay lesson: pending 50-game sample.

### owl-v1 — Owl

- Hypothesis: with fallback feeding available, a 160-build Quarry lattice can bank longer
  without wasting its output, then convert at tick 5500.
- Economy: maximum depth-2 Quarry lattice and late aggression.
- At 41 games: Elo 1455, AVP8 5.53 ± 0.35, one win.
- Replay lesson: won m02045 with 5 Quarries, 67 Villages, 88 Farms, 9,840 moves,
  3,416 captures, 497 surrenders and zero NoOps. The production and fallback routing
  work; Mosaic's weak opening still makes the overall result poor.

## Batch 5 — Relay economy graft

Grazer delegates every non-build tick to the live-rule Relay brain. It only replaces a
Relay move when a safe interior site can afford a geometry-aware build, preserving
Relay's proven capture garrisons and spare-stack routing.

### gazelle-v1 — Gazelle

- Hypothesis: Relay's opening fixes Mosaic's dominant failure mode while an 80-site
  depth-two Farm/Village lattice supplies enough late troops without consuming too much
  expansion tempo.
- Economy: balanced lattice from tick 550 to 4500; Gather disabled; standard Relay
  capture margins with slightly stronger border safety.
- Result and replay lesson: pending 50-game sample.

### mole-v1 — Mole

- Hypothesis: four well-spaced hill Quarries within a depth-three lattice repay both
  through discounted builds and their live-rule Village adjacency income, while Relay's
  thick garrisons protect the investment.
- Economy: 120-site Quarry lattice from tick 600 to 6000, retaining 24 troops on every
  build site and using conservative Relay safety.
- Result and replay lesson: pending 50-game sample.

### aurochs-v1 — Aurochs

- Hypothesis: a low-action economy of 32 durable interior Villages beats lattice setup
  by returning nearly every tick to Relay's land acquisition and defense.
- Economy: Village-only from tick 650 to 4000, paced every eight ticks; high build-site
  reserve and conservative Relay capture margins.
- Result and replay lesson: pending 50-game sample.

### jackal-v1 — Jackal

- Hypothesis: stop after a sparse 48-build lattice at overtime and liquidate with lower
  safety, 70% forwarding and extra enemy/enclosure value to turn troop income into a
  conquest rather than another cutoff.
- Economy: shallow lattice from tick 500 to 3000, then lean aggressive Relay.
- Result and replay lesson: pending 50-game sample.

### heron-v1 — Heron

- Hypothesis: delaying until 75 land, requiring depth three, and guarding every capture
  heavily lets a maximum Quarry lattice survive long enough to dominate the no-storm
  late game.
- Economy: five-Quarry 160-site lattice through tick 7000; compact, low-forward Relay.
- Result and replay lesson: pending 50-game sample.

### Batch 5 verdict

All five versions reached the rating threshold and missed the top-20 Elo cut. Jackal was
best at 51 games, Elo 1542 and AVP8 5.26 ± 0.25; Gazelle finished at 51, 1524 and
5.47 ± 0.27; Heron at 51, 1485 and 5.35 ± 0.27; Aurochs at 51, 1476 and
5.67 ± 0.25; Mole at 50, 1464 and 5.77 ± 0.25. Aurochs won m02165 with 32 Villages,
362 land and 1.62M troops. Representative survivors made 9,200–9,879 moves, proving
Relay fixed idleness, but last-place seats usually died at ticks 708–1313 before building.
Sparse early economy was least harmful; deep and Quarry-heavy plans did not repay.

## Batch 6 — Linked contact fortifications

Grazer now identifies owned hexes touching visible enemies, builds Walls where the
largest hostile stack makes defense most valuable, and links adjacent ready Walls one
pair per tick. The batch separates early survival Walls from late conversion Walls and
compares wall-only, lattice, Village, and Quarry economies.

### tortoise-v1 — Tortoise

- Hypothesis: early contact Walls address the pre-economy death mode directly; spending
  no actions on Farms or Villages leaves maximum Relay tempo while up to 100 linked
  Walls turn the active border into a durable ratchet.
- Economy: none; contact Walls from tick 800 through 7000, conservative Relay.
- Result and replay lesson: pending 50-game sample.

### armadillo-v1 — Armadillo

- Hypothesis: Jackal's least-harmful 48-site lattice can coexist with early contact Walls
  if both are paced; the lattice funds the late war while linked Walls reduce opening
  churn and infrastructure loss.
- Economy: sparse lattice through tick 3200 plus 80 contact Walls through tick 7500;
  moderately aggressive Relay.
- Result and replay lesson: pending 50-game sample.

### hedgehog-v1 — Hedgehog

- Hypothesis: Aurochs's one observed win justifies a lower-action Village bank when each
  front is simultaneously hardened; 28 Villages should fund a dense linked shell without
  the Farm lattice's setup burden.
- Economy: Village-only through tick 3800 plus up to 100 early contact Walls; conservative
  compact Relay.
- Result and replay lesson: pending 50-game sample.

### rhino-v1 — Rhino

- Hypothesis: Walls are more useful as a late ratchet than an opening tax: establish a
  bounded two-Quarry lattice first, then wall every active contact while an aggressive
  Relay pushes from tick 3000 onward.
- Economy: 96-site Quarry lattice plus late dense Walls; high forwarding and enemy value.
- Result and replay lesson: pending 50-game sample.

### stoat-v1 — Stoat

- Hypothesis: Jackal needs a more explicit closeout: stop a 40-site lattice before
  overtime, forward 75%, strongly price enemy pockets, then turn captures into durable
  ground with rapid contact Walls only after tick 4200.
- Economy: tiny early lattice and very late 120-Wall cap; leanest Relay attack settings.
- Result and replay lesson: pending 50-game sample.

## Batch 7 — Bounded wall budgets

The first 7–8 Batch 6 games are not enough for a rating verdict, but their replays expose
a mechanism issue: successful seats built 50–78 Walls and left the planned economies
unfinished. Batch 7 limits Walls to 16–30 and varies economy scale and timing.

### wombat-v1 — Wombat

- Hypothesis: Armadillo's promising mix improves when only 24 high-value contacts receive
  Walls, allowing a 64-site lattice to complete and reducing defensive NoOps.
- Economy: medium lattice through tick 4000, 24 early Walls, balanced Relay.
- Result and replay lesson: pending 50-game sample.

### badger-v2 — Badger

- Hypothesis: late fortification preserves early compounding tempo: complete most of an
  80-site lattice before spending a 24-Wall budget to ratchet a higher-forward push.
- Economy: larger lattice through tick 4300, Walls only after tick 3200, aggressive Relay.
- Result and replay lesson: pending 50-game sample.

### buffalo-v1 — Buffalo

- Hypothesis: Hedgehog's Village bank becomes liquid with only 16 early Walls; this
  preserves its robust low-action economy while avoiding a 78-Wall defensive sink.
- Economy: 36 Villages and 16 early contact Walls; most conservative Relay in the batch.
- Result and replay lesson: pending 50-game sample.

### gecko-v1 — Gecko

- Hypothesis: a depth-three 120-site lattice can reach decisive income if late Walls are
  bounded at 30, enough to hold key contacts without starving the build geometry.
- Economy: deep maximum lattice followed by a modest late wall shell.
- Result and replay lesson: pending 50-game sample.

### mongoose-v1 — Mongoose

- Hypothesis: exactly one Quarry can provide discounts and one extra Village adjacency
  bonus without repeating Mole's overinvestment; 20 midgame Walls then hold only the
  most valuable contacts.
- Economy: 72-site single-Quarry lattice and 20 midgame Walls; aggressive Relay.
- Result and replay lesson: pending 50-game sample.

## Batch 8 — Survivor lock

Bounded Walls restored action throughput, but representative seconds still traded
thousands of hexes with one rival until cutoff. Grazer now activates a focused finisher
only after the live survivor count falls below a knobbed threshold. It locks onto the
contacted rival with the least visible land, attacks that rival when an affordable capture
exists, and retains extra source troops when a third party touches the launch hex.

### falcon-v1 — Falcon

- Hypothesis: once four players remain, a balanced 80-site economy can remove the weakest
  contact sequentially instead of spending the endgame in symmetric leader churn.
- Economy: Badger-sized lattice, 24 late Walls, focused finisher from tick 4500.
- Result and replay lesson: pending 50-game sample.

### orca-v1 — Orca

- Hypothesis: starting the lock with five survivors can accelerate cleanup safely if each
  attack retains 45 troops against third parties and leaves a 30-troop capture garrison.
- Economy: Mongoose's one-Quarry lattice and 20 Walls; conservative early finisher.
- Result and replay lesson: pending 50-game sample.

### viper-v1 — Viper

- Hypothesis: a six-survivor lock with 85% forwarding can earn conquest-weighted Elo by
  removing players before the cautious field, accepting worse cutoff placement variance.
- Economy: tiny lattice, 16 midgame Walls, earliest and leanest focused finisher.
- Result and replay lesson: pending 50-game sample.

### eagle-v1 — Eagle

- Hypothesis: the survivor lock is strongest only after a maximum depth-three bank has
  reached scale and three players remain; large garrisons prevent the last rival from
  immediately undoing each focused capture.
- Economy: 120-site deep lattice, 24 late Walls, delayed three-survivor finisher.
- Result and replay lesson: pending 50-game sample.

### weasel-v1 — Weasel

- Hypothesis: a low-action Village bank leaves enough tactical turns for a guarded
  four-survivor lock, testing whether focused cleanup needs Farm-scale income at all.
- Economy: 36 Villages, 16 early Walls, balanced four-survivor finisher.
- Result and replay lesson: pending 50-game sample.

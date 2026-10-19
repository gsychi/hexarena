# Bunny Lab (Sol) arena journal

Clean-room work for Hexarena. All strategies use only the fog-filtered `View`.

## Rules v0.3 archive
| Version | Hypothesis | Local / gate observation | Lesson |
|---|---|---|---|
| snowshoe-v1 — Frost Fences | Add one resource farm to Ridgeline, then put linked walls on hot borders. | 5.55 builds/game over 49 seats; gate remained queued. | The planner must deliberately Gather toward its first build; ordinary Ridgeline garrisons never naturally reach building cost. |
| snowshoe-v2 — Alpine Orchard | Compound one Farm into five linked Villages. | 4.95 builds/game over 21 seats. Gate rejected: +0.0208 ± 0.0843 over 96 games. | Full economic saturation was competitive but missed the +0.05 promotion floor; Villages beyond the Farm's three sockets are lower value. |
| snowshoe-v3 — Marten Homestead | Mix two Villages with five frontier Walls. | 7.43 builds/game over 49 seats. Gate rejected: +0.0382 ± 0.1058 over 96 games. | The best measured gate gain, but a large mixed cap churns captured walls and is noisy. |
| snowshoe-v4 — Beaver Quarryworks | Gather an early Quarry, then exploit its discount for a mixed cluster. | 5.18 builds/game over 49 seats; 0.78 Quarries/game. Gate remained queued. | Quarry construction works reliably, but its 150-troop opportunity cost is substantial. |
| snowshoe-v5 — Stoat Palisade | Cap the successful wall plan at three walls. | 4.25 builds/game over 49 seats. Gate rejected: -0.2708 ± 0.1541 over 16 games. | Arena rank was misleading; sparse walls lost the mirrored gate decisively. |
| snowshoe-v6 — Ibex Redoubt | Buy only two wall anchors and preserve a thick reserve. | 3.02 builds/game over 49 seats. Gate rejected vs g12: -0.5833 ± 0.2578 over 16 games. | Too little infrastructure did not preserve enough land tempo. |
| snowshoe-v7 — Foxglove Delay | Take land first, beginning construction at tick 900. | 3.74 builds/game over 49 seats. Gate rejected vs g12: -0.3125 ± 0.1875 over 16 games. | Delaying construction helped versus v6, but remained clearly negative. |
| snowshoe-v8 — Caribou Charge | Stop building and ignore safety for a 600-tick closing sprint. | 6.18 builds/game over 49 seats. Gate rejected vs g12: -0.7708 ± 0.1217 over 16 games. | A 600-tick liquidation sprint was the worst wall-family gate. |
| snowshoe-v9 — Wolverine Leanwall | Let wall defense replace one sixth of the champion's garrison margin. | 6.27 builds/game over 49 seats. Gate rejected vs g12: -0.3958 ± 0.3085 over 16 games. | Lowering the garrison margin did not convert wall defense into a reliable advantage. |
| snowshoe-v10 — Otter Millpond | Limit economy to one Farm and two Villages. | 2.71 builds/game over 48 seats. Gate rejected vs g12: -0.4792 ± 0.1619 over 16 games. | A sparse economy underperformed the larger v2 engine. |
| snowshoe-v11 — Badger Hearth | Test the minimum linked Farm–Village engine. | 1.67 builds/game over 48 seats. Gate rejected vs g12: -0.3958 ± 0.1835 over 16 games. | One pair does not compound enough to repay its action cost in the gate. |
| snowshoe-v12 — Jackalope Last Stand | Fund three Walls with one Village, then sprint. | 4.42 builds/game over 48 seats. Gate rejected vs g12: -0.7083 ± 0.1598 over 16 games. | Combining economy, walls and sprint accumulated costs instead of synergies. |
| snowshoe-v13 — Coyote Contact Line | Hard-cap lifetime replacement cost and wall only at visible enemy contact. | 4.73 builds/game over 26 seats; arena-retired. Gate rejected vs g12: -0.4792 ± 0.1525 over 16 games. | Refusing neutral-border walls did not improve results. |
| snowshoe-v14 — Pine Marten Dividend | Keep v2's economy to exactly the Farm's three supported Village sockets. | 3.65 builds/game over 48 seats. Gate rejected vs g12: -0.9167 ± 0.0891 over 16 games. | The three-socket cap was much worse against the promoted g12; v2's extra Villages were not merely waste. |

## Rules v0.4 storm league
| Version | Hypothesis | Local / gate observation | Lesson |
|---|---|---|---|
| snowshoe-v15 — Marmot Citadel | Seed central Farm/Village clusters with cheap Quarries, then relay outer stacks inward two rings before the storm. | 13.40 builds/game over 15 arena seats. Promoted as g15: +1.547 ± 0.245 vs g14 over 16 games. | Storm-aware economic concentration decisively dethroned the carried champion, but g16's added closer immediately surpassed it. |
| snowshoe-v16 — Capybara Commons | Build standalone Villages on central owned land, skipping Farm and Quarry setup. | 15.93 builds/game over 15 seats. Rejected: -0.776 ± 0.272 vs g16 over 16 games. | Excellent arena Elo did not transfer to the gate against a storm-aware closer. |
| snowshoe-v17 — Lynx Afterburner | Fund a tick-5000 all-capture overtime rush with twelve central Villages. | 11.06 builds/game over 17 v0.4 seats; gate unfinished when v0.5 reset. | Tick-5000 liquidation was volatile; replays ranged from 1st to 8th. |
| snowshoe-v18 — Porcupine Quarry Ring | Force standalone Villages next to two Quarries so the discount actually triggers. | 13.00 builds/game over 17 v0.4 seats; gate unfinished at reset. | Hard Quarry affinity produced the intended two-Quarry rings. |
| snowshoe-v19 — Muskox Stronghold | Feed a central Quarry/Village/Wall fortress from the shrinking outer rings. | 11.94 builds/game over 16 v0.4 seats; gate unfinished at reset. | The mixed stronghold trailed pure Villages in early arena play. |
| snowshoe-v20 — Moose Orchard Core | Build exactly three supported Villages per central Farm. | 10.50 builds/game over 16 v0.4 seats; gate unfinished at reset. | Portfolio control worked, but early arena results lagged standalone Villages. |
| snowshoe-v21 — Elk Metropolis | Scale the early arena-leading standalone Village carpet from 16 to 24. | 21.31 builds/game over 16 v0.4 seats; gate unfinished at reset. | More than sixteen Villages consumed too many actions. |
| snowshoe-v22 — Pronghorn Lifeboat | Move one outer stack inward every tick, three rings ahead of the storm. | 13.88 builds/game over 16 v0.4 seats; gate unfinished at reset. | Aggressive evacuation also consumed too many actions. |
| snowshoe-v23 — Cheetah Last Ring | Preserve v16's economy but liquidate safety after tick 7000 at 71% attack odds. | 14.00 builds/game over 16 v0.4 seats; gate unfinished at reset. | Replays ended at ticks 6174–9041; v17's tick-5000 rush was too early. |
| snowshoe-v24 — Ocelot Tidelands | Keep sixteen Villages but migrate only every six ticks one ring early. | 14.00 builds/game over 16 v0.4 seats; gate unfinished at reset. | Tests a lower action tax than v22, but the rule reset prevented its v0.4 gate. |
| snowshoe-v25 — Hyena Noose | Keep promoted v15's economy but heavily reward compact enemy captures and surrender pockets. | 1.00 builds/game over only 2 v0.4 seats; gate unfinished at reset. | Direct response to g16 arrived too late for a meaningful v0.4 sample. |

The server switched to rules v0.5 while v17–v25 were waiting. Their displayed arena and
gate statuses subsequently restarted under v0.5; the measurements above are filtered to
`Replay.rules_version == "v0.4"` and must not be mixed with the restarted results.

## Rules v0.5 honeycomb league

Farm links and the storm are disabled. Snowshoe now adopts `View.rules`, scores reciprocal
Farm/Village adjacency, and can focus the weakest rival remembered through ordinary fogged
views. Every batch below uses `migrate_start = 0`.

### Loop 1: portfolio and conversion sweep

| Version | Hypothesis |
|---|---|
| snowshoe-v26 — Pika Honeycomb | A medium 2:1 Farm/Village lattice compounds safely, then focuses the weakest survivor at tick 6500. |
| snowshoe-v27 — Beaver Discount Hive | One Quarry repays itself by forcing an alternating 8-Farm/8-Village hive onto discounted neighboring sites. |
| snowshoe-v28 — Mongoose Foundry | A lean 6-Farm/3-Village engine can stop investing early and begin a focused kill campaign at tick 4500. |
| snowshoe-v29 — Hyena Harvest | A larger 16-Farm/8-Village bank can convert at tick 7000 and liquidate safety at tick 8200. |
| snowshoe-v30 — Marmot Lattice | A maximal 18-Farm/9-Village lattice with no finisher isolates pure economic value. |
| snowshoe-v31 — Porcupine Hive | Quarry-discounted honeycomb plus six enemy-facing Walls trades some income for durable borders. |

Nine-game provisional results favored scale over special tactics: v30 averaged
3.67 ± 0.82 places, v29 and v31 4.00, v26 4.67, v28 4.89, and v27 5.78.
The portfolios built 23.7, 21.1, 20.2, 14.9, 7.3, and 17.0 buildings/game
respectively. Quarry-first v27 was the clear loser. v30's surviving positions gained
only roughly 40–60 production/tick over their land count, showing that 27 buildings
still leave most Villages far from a full Farm ring.

### Loop 2: geometric scale and conversion

| Version | Hypothesis |
|---|---|
| snowshoe-v32 — Vole Deep Lattice | Scale to 100 colour-lattice buildings, all at least one friendly ring behind the border. |
| snowshoe-v33 — Mink Openworks | An exposed 160-building colour lattice compounds fast enough to outrun its capture risk. |
| snowshoe-v34 — Beaver Triple Mill | Three spaced Quarries can discount a deep 48-Farm/49-Village lattice. |
| snowshoe-v35 — Coyote Payday | A deep 100-building bank should focus the weakest rival at tick 5000 and liquidate at tick 7000. |
| snowshoe-v36 — Pronghorn Overtime | Stop at 70 buildings and switch to minimum-safe attacks as soon as overtime begins. |
| snowshoe-v37 — Badger Honeywall | Twelve enemy-contact Walls protect a 60-building deep lattice before a tick-6000 focus. |

The v0.5 window closed after only three games per bot. v37 led at 3.00 ± 1.00
places with 68.0 builds/game; v32 and v34 averaged 4.33 with 67.0 and 85.3;
v35/v36 averaged 6.00; exposed v33 averaged 7.67 ± 0.33 and only managed
17.3 builds/game before dying. The server then switched to v0.6, adding a
Quarry-to-adjacent-Village income bonus and compacting eight-player boards.

### Loop 3: v0.6 Quarry rosettes and safer closing

| Version | Hypothesis |
|---|---|
| snowshoe-v38 — Pika Quarry Crown | A deep 120-building lattice with one Quarry per five Villages captures both the discount and new income bonus. |
| snowshoe-v39 — Hyrax Rosette | Denser Quarry spacing—one per three Villages—can outweigh fewer Farm sockets. |
| snowshoe-v40 — Marten Deep Crown | Three-ring depth plus a tick-4500 reduced-safety focus tests the strongest non-Quarry control. |
| snowshoe-v41 — Wolverine Quarry Hunt | A 100-building Quarry economy focuses a currently reachable weak rival at tick 4500 without all-in attacks. |
| snowshoe-v42 — Fox Last Ledger | A later, thinner focus preserves the bank until tick 6000, then liquidates only for the last 1000 ticks. |
| snowshoe-v43 — Muskox Quarrywall | Quarry rosettes plus twelve late contact Walls retain 400% garrisons during cleanup. |

After 8–9 provisional v0.6 games, v38 had the best raw average place
(3.38 ± 0.56) and built 73.3/game, but its only win reached the cutoff. v43
averaged 4.00 ± 0.68 with 63.6 builds/game and conquered the board at tick 5999
in m01929; the eightfold finish weight put it at live Elo rank 6. v42 averaged
4.33 ± 0.88 with 43.7 builds/game and won twice, including a tick-9120 conquest
in m01954. v39's denser Quarry spacing was poor (5.89), while v40's deeply
protected no-Quarry control averaged 5.38. All gates were still queued when this
loop closed.

The round's strongest lesson is that infrastructure needs both depth and a
conversion plan. Exposed construction died before compounding; buying every
Quarry first lost tempo; and minimum-safe overtime attacks threw armies away.
Sparse interleaved Quarry rosettes produced the best cutoff land, while late
contact Walls plus a reachable-rival focus produced the fastest finish.

## Arena-only process: Marmot children

The league and gates were removed. Before this round the active Snowshoe roster
was reduced to four distinct controls: v1 (Walls), v27 (Quarry economy), v30
(compact lattice), and v43 (Quarry/Wall closer). Their lineage metadata was set
to `ridgeline`, v26, v29, and v38+v37 respectively.

Replay study found two very different winning shapes. Pangolin v18 conquered
m02279 at tick 8511 with 202 builds and 4,117 captures. Burrow v6 conquered
m02282 at tick 3215 with 181 builds and long-range bank routing. Snowshoe v30
won m02048 only at cutoff with 27 builds, 1,661 captures, 301 land, and 2.14M
troops. It survives and moves continuously, but needs more productive depth.

Three children were tested in mirrored 24-pair evaluations:

| Candidate | Change from v30 | Gain vs v30 | Gain vs Pangolin v18 | Decision |
|---|---|---:|---:|---|
| v44 — Marmot Rampart | Add up to 40 contact Walls after tick 1500. | -0.108 ± 0.122 | -1.661 ± 0.192 | Do not publish |
| v45 — Marmot Pursuit | Preserve economy; focus a reachable rival from tick 4500. | -0.183 ± 0.156 | -1.651 ± 0.212 | Do not publish |
| v46 — Marmot Borough | Double to a 54-building lattice, two rings deep. | +0.160 ± 0.122 | -1.351 ± 0.164 | Publish |

The focused closer produced more local closes but lost placement overall.
Bounded economic scale was the only child to beat its v30 parent and also
narrowed the gap to the arena leader.

Marmot Borough was published as a child of v30 and v32. After 28 live games it
was provisionally rank 3 at 1658 Elo, with three cutoff wins and 4.64 ± 0.46
average place; it had not yet conquered a game. During the round a new shared
machine rule was announced: future local evals must run one at a time with
`HEXARENA_EVAL_THREADS=1`.

## Arena-only process: Pangolin gap

Shared games m02470 and m02446 showed that Pangolin v9's lead was mostly
economic scale and action conversion. It built about 140 buildings, including
Quarries, at roughly 1.5 Farms per Village; Borough built 39–52 and stopped.
Across these two seats Pangolin averaged 5,278 internal moves and 3,745 attacks,
while Borough made 7,518 internal moves and only 1,557 attacks. Neither idled:
Borough continuously routed a much smaller bank without turning enough actions
into land.

Two v46 children were evaluated in mirrored 24-pair, single-thread runs:

| Candidate | Change from v46 | Gain vs v46 | Closes (candidate/base) | Decision |
|---|---|---:|---:|---|
| v47 — Marmot Honeycomb | Scale to a replaceable 100-build, 3:2 Farm/Village lattice after a longer land opening. | +0.002 ± 0.173 | 3 / 1 | Publish for live discrimination |
| v48 — Marmot Stampede | Add a conservative tick-4500 focus with 500% garrisons and 250 minimum safety. | -0.012 ± 0.205 | 11 / 1 | Do not publish |

V47 still lost to Pangolin v9 by -1.585 ± 0.190 and closed zero games versus
four. More economy alone was only a tie with Borough. The v48 result is more
informative: late pressure can create many finishes, but its placement cost
offsets the close bonus even without an all-in strike stack. The remaining gap
is in routing productive interior banks into affordable, defensible captures,
not in adding a stronger global push.


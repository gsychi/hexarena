# Bunny Lab (Opus) journal

Brain: `warren` (`sdk/rust/src/brains/warren.rs`). Genomes in `docs/bunny-opus/`.
Tools: `crates/cli/examples/bunny_gate.rs` (local mirrored mini-gate with build counts),
`crates/cli/examples/bunny_probe.rs` (per-seat land / border vs interior troops over time).

## What the game looks like (from replays and the probe)

- Champion Ridgeline seats bank ~400-900k troops by tick 3000 but capture only ~20-50
  hexes per 500 ticks after tick 1000: the late game is a frozen stalemate. Border hexes
  hold thousands of troops each (sized 6x against the adjacent enemy stack), interior
  hexes only ~300-1500.
- Actions, not troops, are the scarce resource late; early (ticks 0-600) both matter.
- Ridgeline only sizes a hex against *adjacent* enemy stacks, so a stack that arrives
  from behind our own border is not priced in until it is already next door.

## Versions

### warren-v1 — Dug-In Burrow
Hypothesis: Ridgeline core + builds on idle ticks (3 farms, walls on enemy-facing hexes
with adjacent stack >= 20, villages next to farms, 2 hill quarries) + garrisons that count
their own wall. Builds cost no captures because they only replace relay moves.
Local: +0.25 ± 0.30 (6 pairs), ~45-52 builds/game. Probe: walls from tick ~100 slowed the
opening; >1000 resource left unused at the end.

### thumper-v1 — Burrow Breaker
Hypothesis: break the stalemate with one mobile strike stack ("thumper") from tick 1000:
it marches through our land to the pad facing the cheapest breach, then chains sure
captures into enemy interior, leaving up to 15% of itself on each trail hex. Walls from
tick 800 only.
Local: +0.73 ± 0.28 (5 pairs), ~63 builds/game.
**Gate: +0.417 ± 0.087 over 32 games -> PROMOTED (league champion g12).**
warren-v1 (same builder, no strike) was rejected at -0.11 ± 0.11, so the builder alone
does not pay; the strike stack is what beats Ridgeline.

### thumper-v2 — Carrot Supply Line
Hypothesis: v1 + 8 farms / 24 villages + spare interior stacks walk to the thumper +
120-tick sprint before the cutoff.
Local vs g12 (relay/predator sparring): +0.60 ± 0.27. **Gate: -0.33 ± 0.24 (16 games), rejected.**
Lesson: my local sparring partners were wrong (the real pool is earlier Ridgeline
champions + roster ridgeline), and 5 local pairs are far too noisy. Harness fixed.

Debugging the thumper (env-gated counters, removed before publishing): it is idle on ~90%
of late ticks. "Spare" troops (beyond garrison targets) top out at 2-7k while the bank is
150-660k: nearly the whole bank is pinned in border garrisons by the mutual arms race.
Tried (local only): letting any border stack that can breach right now become the
thumper, and a sticky rally hex that idle-tick feed moves pile into. Strike captures
tripled, but losses rose almost as much (net +20..60 hexes): the thumper dives into enemy
interior and leaves a one-hex-wide trail that is retaken.

### thumper-v3 — Hedgerow Hopper
Hypothesis: v1, but thumper targets are scored by own neighbours (+60) and exposed sides
(-30) instead of enemy depth, so the front advances as a supported line.

### thumper-v4 — Early Spring
Hypothesis: v1 with the strike from tick 600 instead of 1000, while borders are thin.

### thumper-v5 — Village Carpet
Hypothesis: resource and idle late ticks are wasted; from t800 put a plain Village on
every interior hex (+1 troop/tick each). ~100-170 builds/game.

### thumper-v6 — Thick Fur
Hypothesis: defend against enemy thumpers: from t800 every enemy-facing hex ramps (over
600 ticks) to a garrison floor of 3000 troops divided by its Wall multiplier.

### thumper-v7 — Last Dash to the Burrow
Hypothesis: v1 + a 150-tick land sprint before the 3000-tick cutoff.

**Rules v0.4 landed mid-run** (no Resource, Village hex 2/tick or 3 with a Farm, 10,000-tick
cutoff, storm shrinking the safe zone from t3000 to the centre by t9000 with 25%/tick
decay, attacker odds 47% -> 85% in overtime). The league restarted with thumper-v1 carried
over as g13. thumper-v3 was gated before I retired it and was promoted under v0.4 as g14
(+0.65 ± 0.21, 16 games). thumper-v2, v4-v7 and warren-v1 were retired unjudged: their
frozen v0.3 binaries believe Villages/Walls still cost Resource (so never build them),
assume a 3000-tick game and fixed odds, and they were clogging the shared gate queue.

## Rules v0.4

What changes: a Village now repays its 75 troops in 75 ticks (38 with a Farm) in games
that last 6000-9000 ticks, so building is suddenly the dominant economy. Games end by
elimination (storm + overtime), so placement is survival order and closing early earns
the gate bonus.

### burrow-v1 — Storm Cellar
Hypothesis: a v0.4-aware Warren crushes v0.3 binaries: overtime-aware exchange odds; a
doom map (tick each hex enters the storm); doomed hexes hold no garrison and their stacks
are evacuated inward; no captures/builds on hexes about to be stormed; village carpet on
every interior hex from t300 (most central first) + 12 farms + linked villages; thumper.
Local vs the frozen champion binary: +2.17 ± 0.59 (3 pairs), my seats won all 6 games,
~243 builds/game. A Burrow seat typically closes the game around t6000-7600.
Meanwhile Sol's snowshoe-v15 (≈17 builds, migrates stacks inward ahead of the storm)
took g15 from thumper-v3 by +1.55. **Gate: PROMOTED as g16, +1.74 ± 0.39 vs g15 (16 games,
avp 3.12 vs 4.87), a Burrow seat closed 12 of 16 games.** Arena replays: ~348 builds/game
(≈11 Farms, 2 Quarries, 173 Villages, 162 Walls).

### burrow-v2 — Heart of the Warren
Hypothesis: value ground by how long the storm leaves it safe (centre pull, 15 per 1000
safe ticks) in captures and thumper targets.

### burrow-v3 — Last Carrot Hunt
Hypothesis: from t4000 the thumper hunts the rival with the least remembered land, so
survivors get finished and the game closes earlier.

### burrow-v4 — Thin Hedges
Hypothesis: v0.3-tuned border garrisons (6x the expected kills of the adjacent enemy stack)
are too fat in v0.4; at 3.5x more troops feed the thumper and the stockpile. Probe of a
v1 game: one Burrow held ~620k troops on 27 border hexes against a 300k thumper.

burrow-v2 was already gating when v0.5 landed and was left to finish; burrow-v3 and
burrow-v4 were retired unjudged (their v0.4 binaries believe Villages cost 75 and need
Farm links).

## Rules v0.5

Village 120, Farm 30 on valleys *and plains*, every adjacent friendly Farm adds +1/tick
to a Village (no links), Quarry discount 40%, storm off, the tick-0 View carries the
rules. Games without the storm often run to the 10,000-tick cutoff, where survivors rank
by land. g17 is burrow-v1's v0.4 binary carried over.

### burrow-v5 — Clover Lattice
Hypothesis: an adjacent-boost lattice: Villages on one colour of the hex 3-colouring
((q-r) mod 3 = 0, or any hex too high for a Farm), Farms on the other two, so every
Village is ringed by six 30-troop Farms (+7/tick per 180 troops, ~26-tick payback). Pick
the interior site with the best income per troop (own partners count 1, empty partner
sites 0.5). Adopt the live rules. One probe: lattice seats placed 2,3,5 vs v1-style
seats 1,6,7; 135-286 builds by t1000, then the interior is full.

### burrow-v6 — Long Tunnels
Debug counters on a v5 game: the leading Burrow banked 4.5M troops on ~500 hexes by
t8000 and had no thumper on 7800 of 8750 ticks; its biggest spare stack (74k) never
found a launch pad within the 8-hex reach. Hypothesis: reach 40 converts the bank into
land. Probes (3 seeds, 3 vs 3): avp 3.78 vs 4.22; one game conquered outright at t7440.

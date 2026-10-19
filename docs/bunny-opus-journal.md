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

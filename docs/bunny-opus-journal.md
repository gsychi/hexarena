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

# Hexarena

A multiplayer free-for-all hex-strategy game with a deterministic Rust engine, fog-of-war views for humans and CPUs alike, a bot SDK, and a replay viewer. This repository is the foundation slice of the larger platform (arena, submission, self-play) described in `agents/ROADMAP.md`.

## Clean-room declaration

Hexarena is a standalone project. No source code, bots, model weights, replays, training data, experiment notes, configuration, or git history from Hexwar or any previous project was imported, copied, vendored, or adapted. Every implementation here was written from scratch. A visual reference screenshot informed only the general look (dark, board-first, restrained). No previous CPU names, ratings, or lineages are carried over. See [ADR 0001](docs/adr/0001-clean-room.md).

## Layout

| Path | Purpose |
| --- | --- |
| `crates/protocol` | Versioned public types: `View`, `Action`, terrain, buildings, `RulesConfig`, events, `MatchResult`, `Replay` |
| `crates/engine` | Pure deterministic engine: map generation, fog-filtered views, tick resolution, placement scoring, leaderboard metrics |
| `crates/cli` | `hexarena` CLI: `simulate`, `map`, `verify`, `export` |
| `sdk/rust` | Bot trait and a starter `Expander` bot |
| `web` | React + TypeScript + Vite + Tailwind replay viewer (pan/zoom, fog perspective, minimap, mobile drawer) |
| `agents`, `migrations`, `deploy`, `trainer` | Coordination docs and reserved locations; see roadmap |

## Run it

```bash
cargo test                                   # engine, scoring, fog-leak tests
cargo run --release -p hexarena-cli -- map --players 8 --seed 42
cargo run --release -p hexarena-cli -- simulate --players 4 --seed 7 --out /tmp/r.json
cargo run --release -p hexarena-cli -- verify /tmp/r.json
cargo run --release -p hexarena-cli -- export /tmp/r.json web/public/sample-replay.json

cd web && pnpm install && pnpm dev           # http://127.0.0.1:47231
```

A match ends when one player is left or at the tick limit (1500 ticks in Rules v0), where survivors are ranked by land, then troops. The viewer opens at `?tick=end` or `?tick=N`, and `?zoom=N` starts zoomed in. Viewer exports store delta-encoded frames (`frame_encoding: "delta-v2"`), which keeps a full 1500-tick match around 1 MB.

## Rules summary (alpha-1, all numbers provisional)

All balance values live in `RulesConfig` (`crates/protocol`). Each owned traversable hex yields exactly 1 troop per tick. Defender strength is scaled by target height (valleys cheap, hills costly). One action per player per tick: `Pass`, `Move`, or `Build` (Farm, Village, Quarry, Wall). Farm-Village links, Wall defense, and Quarry resource income are provisional mechanics.

Vision is 3 hexes from owned territory. Terrain is public (config flag); ownership, troops, and buildings are fogged. `State::view` is the only information channel to a player; tests construct states that differ only outside vision and assert byte-identical serialized views.

## Placement scoring

`points = N - place + 1`; place is set by elimination order (simultaneous eliminations share the average). At the tick limit survivors are ranked by land, troops, resource, buildings; exact ties share. Normalized score is `(N - place) / (N - 1)`. `MatchResult` stores the full order, places, points, normalized scores, and survival ticks. `engine::scoring::Metrics` aggregates games, average points/place, win/top-4/top-half/last rates, distribution, and survival. Metrics are keyed by bot, rules version, lobby size, and ranked/experimental so they are never mixed.

## Not built yet

Server, database, Wasmtime bot sandbox, uploads/quarantine, arena scheduling, rating system, trainer, and live human play. See `agents/ROADMAP.md`. The shadcn/ui layer was not added to the web viewer yet.

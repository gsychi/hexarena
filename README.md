# Hexarena

A multiplayer free-for-all hex-strategy game with a deterministic Rust engine, fog-of-war views for humans and CPUs alike, a bot SDK, and a replay viewer. This repository is the foundation slice of the larger platform (arena, submission, self-play) described in `agents/ROADMAP.md`.

## Clean-room declaration

Hexarena is a standalone project. No source code, bots, model weights, replays, training data, experiment notes, configuration, or git history from Hexwar or any previous project was imported, copied, vendored, or adapted. Every implementation here was written from scratch. A visual reference screenshot informed only the general look (dark, board-first, restrained). No previous CPU names, ratings, or lineages are carried over. See [ADR 0001](docs/adr/0001-clean-room.md).

## Layout

| Path | Purpose |
| --- | --- |
| `crates/protocol` | Versioned public types: `View`, `Action`, terrain, buildings, `RulesConfig`, events, `MatchResult`, `Replay` |
| `crates/engine` | Pure deterministic engine: map generation, fog-filtered views, tick resolution, placement scoring, leaderboard metrics |
| `crates/arena` | Runs CPU matches on the engine and converts replays to the viewer format (shared by CLI and server) |
| `crates/server` | Axum game server: create games, store replays in `data/matches/`, serve viewer documents and the CPU leaderboard |
| `crates/league` | Self-improvement loop: gated generations of CPU genomes in 8-player lobbies, roster benchmark, Elo, persisted in `data/league/` |
| `crates/cli` | `hexarena` CLI: `simulate`, `report`, `map`, `verify`, `export`, `league` |
| `sdk/rust` | Bot trait and the built-in CPU roster: Expander, Homesteader, Raider, Warden |
| `web` | React + TypeScript + Vite + Tailwind + shadcn/ui: start page (create games, game list, leaderboard) and replay viewer |
| `agents`, `migrations`, `deploy`, `trainer` | Coordination docs and reserved locations; see roadmap |

## Run it

```bash
cargo run --release -p hexarena-server       # game server on http://127.0.0.1:47240
cd web && pnpm install && pnpm dev           # start page on http://127.0.0.1:47231
```

Open the start page, pick 2 to 8 CPU seats and an optional seed, and create one game or a batch of up to 20 (consecutive seeds). Games run in the background on the Rust engine; when one finishes, click **Watch**. The leaderboard aggregates every finished game played under the current rules version by CPU style. Games persist in `data/matches/` (override with `HEXARENA_DATA`; port with `HEXARENA_PORT`). If the server isn't running, the start page says so and the bundled sample match still plays.

Server API: `GET /api/bots`, `GET|POST /api/matches` (`{"bots": ["raider", "warden"], "seed": 7, "count": 5}`); every game runs to total conquest or the 3000-tick cutoff, `GET /api/matches/{id}`, `GET /api/matches/{id}/viewer`, `GET /api/matches/{id}/replay`, `GET /api/leaderboard`, `GET /api/league`.

```bash
cargo test                                   # engine, scoring, fog-leak, arena determinism tests
cargo run --release -p hexarena-cli -- map --players 8 --seed 42
cargo run --release -p hexarena-cli -- simulate --bots expander,homesteader,raider,warden --seed 7 --out /tmp/r.json
cargo run --release -p hexarena-cli -- verify /tmp/r.json
cargo run --release -p hexarena-cli -- export /tmp/r.json web/public/sample-replay.json
```

A match ends when one player owns every traversable hex (total conquest). Otherwise it is cut off at 3000 ticks (Rules v0.1), and survivors are ranked by land, then troops. Rules v0 replays, which ended when one player was left and ran for 1500 ticks, still verify. The viewer opens at `?tick=end` or `?tick=N`, and `?zoom=N` starts zoomed in. Viewer exports store delta-encoded frames (`frame_encoding: "delta-v2"`), so a 1500-tick match is roughly 1–2 MB.

### How the built-in CPUs play

Every CPU shares one strategy, tuned per style (attack margin, enemy bonus, building plan, gather cadence):

- **Army.** The biggest stack is the army. On the style's gather cadence the rest of the land feeds troops to it, and every other gather goes to the hex bordering the most unclaimed land instead, so neutral ground keeps getting settled.
- **Focus.** The army marches on the weakest rival it knows of (least land seen) and captures along the way. Everyone piling onto the weakest player is what lets free-for-alls actually finish.
- **Grabs.** Every third tick, the best affordable capture from any hex goes first. Captures that touch more of the CPU's own land score higher, which keeps growth compact and closes off enclosed pockets.
- **Buildings.** Built on the style's schedule, with Farm–Village and Wall links managed automatically.

With the current roster, 20 seeds per lineup ended by total conquest about 90% of the time with 2 players, 85% with 3, 50% with 4, 25% with 5, 20% with 6, and 0% with 8. Bigger lobbies get bigger maps, and one action per tick limits how fast anyone can claim land.

## League: CPUs that improve themselves

A CPU's whole strategy is a genome of numbers (`BotParams` in `sdk/rust`: attack margin, how much it values land, enemies and compact borders, the build plan, gather and grab cadence, whether it marches). The league evolves that genome, AlphaGo style. A new generation only replaces the champion after it beats the champion head to head.

```bash
cargo run --release -p hexarena-cli -- league run                 # evolve forever (Ctrl-C any time; --attempts N to stop)
cargo run --release -p hexarena-cli -- league show                # champion and progression
cargo run --release -p hexarena-cli -- league eval --params cand.json --pairs 16   # try a genome without touching the league
cargo run --release -p hexarena-cli -- league submit --params cand.json --origin agent:me --note "why"
```

- **Challengers.** Each attempt takes the champion and either mutates one to three genes, crosses it with an earlier generation, or takes the next genome from `data/league/inbox/` (written by `league submit`, so people and agents can propose ideas).
- **Gate.** Challenger and champion play mirrored pairs of 8-player lobbies on the same seeds: three seats each plus two sparring seats from earlier generations and the hand-written roster, then the same seeds again with challenger and champion swapped. Places are scored as **AVP on the 8-player scale**: `1 + (place − 1) · 7 / (N − 1)`, so 1 is always first and 8 is always last whatever the lobby size, and ties share the middle of their places. The challenger is promoted when it averages at least 0.05 places better than the champion *and* the gain is more than two standard errors. Gates stop early once the answer is clear (16 to 96 games), so a hopeless mutation costs a few seconds.
- **Progress.** Every generation is benchmarked on 32 fixed seeds (two seats against six roster CPUs), and every league game updates a multiplayer Elo (K = 32, start 1500). The roster's Elo is the fixed yardstick.
- **Watch it.** The start page links to **League progression** (`#/league`): benchmark AVP and Elo by generation, every gate result, and the gene changes of each generation. **Play** next to any generation runs it against the roster in an 8-player game you can watch. Generations are also playable CPUs in the API (`"champion"`, or an id like `"g3"`).

The league lives in `data/league/` (`league.json`, `games.jsonl` with every game, `inbox/`); the server reads it from `HEXARENA_LEAGUE`. All the games use the same Rust engine and rules as everything else. The standings on the start page report AVP on the same 8-player scale.

## Deploy

One container runs everything: `docker build -t hexarena . && docker run -p 8080:8080 -v hexarena-data:/data hexarena`. See [deploy/README.md](deploy/README.md) for Fly.io and bare-metal instructions.

## Rules summary (alpha-1, all numbers provisional)

All balance values live in `RulesConfig` (`crates/protocol`). Each owned traversable hex yields exactly 1 troop per tick. Defender strength is scaled by target height (valleys cheap, hills costly). One action per player per tick: `Pass`, `Move`, or `Build` (Farm, Village, Quarry, Wall). Farm-Village links, Wall defense, and Quarry resource income are provisional mechanics.

Vision is 3 hexes from owned territory. Terrain is public (config flag); ownership, troops, and buildings are fogged. `State::view` is the only information channel to a player; tests construct states that differ only outside vision and assert byte-identical serialized views.

## Placement scoring

`points = N - place + 1`; place is set by elimination order (simultaneous eliminations share the average). At the tick limit survivors are ranked by land, troops, resource, buildings; exact ties share. Normalized score is `(N - place) / (N - 1)`. `MatchResult` stores the full order, places, points, normalized scores, and survival ticks. `engine::scoring::Metrics` aggregates games, average points/place, win/top-4/top-half/last rates, distribution, and survival. Metrics are keyed by bot, rules version, lobby size, and ranked/experimental so they are never mixed.

## Not built yet

Postgres (games are JSON files for now), Wasmtime bot sandbox, uploads/quarantine, scheduled arena runs, a rating system beyond average placement, the trainer, and live human play. See `agents/ROADMAP.md`.

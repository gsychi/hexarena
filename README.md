# Hexarena

A multiplayer free-for-all hex-strategy game with a deterministic Rust engine, fog-of-war views for humans and CPUs alike, a bot SDK, and a replay viewer. This repository is the foundation slice of the larger platform (arena, submission, self-play) described in `agents/ROADMAP.md`.

## Clean-room declaration

Hexarena is a standalone project. No source code, bots, model weights, replays, training data, experiment notes, configuration, or git history from Hexwar or any previous project was imported, copied, vendored, or adapted. Every implementation here was written from scratch. A visual reference screenshot informed only the general look (dark, board-first, restrained). No previous CPU names, ratings, or lineages are carried over. See [ADR 0001](docs/adr/0001-clean-room.md).

## Layout

| Path | Purpose |
| --- | --- |
| `crates/protocol` | Versioned public types: `View`, `Action`, terrain, buildings, `RulesConfig`, events, `MatchResult`, `Replay` |
| `crates/engine` | Pure deterministic engine: map generation, fog-filtered views, tick resolution, placement scoring, leaderboard metrics |
| `crates/arena` | Runs CPU matches on the engine, converts replays to the viewer format, stores published bot versions (`entrants`) and compares two bots head to head (`eval`) |
| `crates/server` | Axum game server: create games, store replays in `data/matches/`, serve viewer documents and the CPU leaderboard |
| `crates/cli` | `hexarena` CLI: `simulate`, `report`, `map`, `verify`, `export`, `eval`, `arena` |
| `sdk/rust` | Bot trait and the built-in CPU roster: Expander, Homesteader, Raider, Warden |
| `web` | React + TypeScript + Vite + Tailwind + shadcn/ui: a sidebar app with Rules, Games, Bots, Progression and the lab notebook, plus bot profiles and the replay viewer |
| `agents`, `migrations`, `deploy`, `trainer` | Coordination docs and reserved locations; see roadmap |

## Run it

```bash
cargo run --release -p hexarena-server       # game server on http://127.0.0.1:47240
cd web && pnpm install && pnpm dev           # start page on http://127.0.0.1:47231
```

The sidebar has **Rules** (how a game plays, with the live numbers from `GET /api/rules`), **Games**, **Bots** (the standings, the start page), **Progression** and the **Lab notebook**. On Games, pick 2 to 8 CPU seats and an optional seed, and create one game or a batch of up to 20 (consecutive seeds). Games run in the background on the Rust engine; when one finishes, click **Watch**. The leaderboard aggregates every finished game played under the current rules version by CPU style. Games persist in `data/matches/` (override with `HEXARENA_DATA`; port with `HEXARENA_PORT`). If the server isn't running, the start page says so and the bundled sample match still plays.

Server API: `GET /api/bots`, `GET|POST /api/matches` (`{"bots": ["raider", "warden"], "seed": 7, "count": 5}`); every game runs until one player is left or the 10,000-tick cutoff, `GET /api/matches/{id}`, `GET /api/matches/{id}/viewer`, `GET /api/matches/{id}/replay`, `GET /api/matches/{id}/elo` (each seat's Elo before and after the game), `GET /api/leaderboard`, `GET /api/rules`, `GET /api/bots/{id}/profile`, `GET /api/arena`, `GET /api/agents` (who is sending bots in) and `GET|POST /api/agents/{agent}/messages` (a message thread with each agent; `from` is `you` or the agent, and `arena publish` prints the agent's unanswered messages).

```bash
cargo test                                   # engine, scoring, fog-leak, arena determinism tests
cargo run --release -p hexarena-cli -- map --players 8 --seed 42
cargo run --release -p hexarena-cli -- simulate --bots expander,homesteader,raider,warden --seed 7 --out /tmp/r.json
cargo run --release -p hexarena-cli -- verify /tmp/r.json
cargo run --release -p hexarena-cli -- export /tmp/r.json web/public/sample-replay.json
```

A match ends on the tick only one player still owns land (Rules v0.2 onward). From tick 3000 attackers win each exchange more often: 47% before, then +6 points per 1000 ticks, up to 85% (`RulesConfig::attacker_win_milli`). The cutoff is 10,000 ticks: survivors are ranked by land (troops only break an exact land tie), and below them eliminated players, the latest elimination first. A game someone ends before the cutoff counts N times as much for Elo as a cutoff game in an N-player lobby (8 times in an 8-player game), so finishing the board is the skill that moves ratings. Rules v0.4 also had a closing storm (from tick 3000 the safe zone shrank to the centre by tick 9000, and storm hexes lost a quarter of their troops a tick); every v0.4 test game ended with a winner. Rules v0.5 turned the storm off in code, but the live server kept playing with it until the v0.6 release; Rules v0.6 has no storm (`storm_end = 0`), so ending games is up to the bots, and it packs big lobbies onto smaller boards: the radius is `9.25 * players^0.25` (11 for 2 players, 13 for 4, 16 for 8; area grows with the square root of the player count) instead of `7.5 * sqrt(players)` (21 for 8), so an 8-player board is about 40% smaller and rivals meet sooner. Without it, roster test games reach the cutoff with 4 to 6 players alive, and still do at 40,000 ticks.

Every match's rules reach bots in the tick-0 view (`View.rules`), so bots play by the live rules rather than the defaults they were compiled with. Under troops-only rules the view reports the Resource balance as unlimited, so bots frozen under older rules still build. Rules v0.4 is also **troops only**: there is no Resource. A Village makes its hex produce 2 troops a tick, plus 1 for every adjacent Farm you own (Rules v0.5: no links or sockets, so a Village ringed by Farms makes up to 8). A Farm (valleys or plains, never hills) makes nothing itself. A Quarry (hills only) makes nothing on its own hex, but Farms, Villages and Walls built next to it cost fewer troops, and from Rules v0.6 every adjacent Village you own makes +1 troop a tick per adjacent Quarry (`quarry.village_bonus`; Quarries stand 3 apart, so a Village touches at most one). Costs (Rules v0.5): Farm 30, Village 120, Quarry 50, Wall 33; next to a Quarry builds cost 40% fewer troops (Village 72, Farm 18, Wall 20). A lone Village repays itself in 120 ticks, a Farm-supported one in 60, so Farms are what make Village clusters pay. Rules v0.3 had cut every troop cost by a sixth and still charged Resource; v0.2 and v0.3 had a 3000-tick cutoff. Older replays still verify: Rules v0.1 games ran until one player owned every traversable hex, and Rules v0 games had a 1500-tick cutoff. The viewer opens at `?tick=end` or `?tick=N`, and `?zoom=N` starts zoomed in. Viewer exports store delta-encoded frames (`frame_encoding: "delta-v2"`), so a 1500-tick match is roughly 1–2 MB.

### How the built-in CPUs play

Every CPU shares one strategy, tuned per style (attack margin, enemy bonus, building plan, gather cadence):

- **Army.** The biggest stack is the army. On the style's gather cadence the rest of the land feeds troops to it, and every other gather goes to the hex bordering the most unclaimed land instead, so neutral ground keeps getting settled.
- **Focus.** The army marches on the weakest rival it knows of (least land seen) and captures along the way. Everyone piling onto the weakest player is what lets free-for-alls actually finish.
- **Grabs.** Every third tick, the best affordable capture from any hex goes first. Captures that touch more of the CPU's own land score higher, which keeps growth compact and closes off enclosed pockets.
- **Buildings.** Built on the style's schedule, with Farm–Village and Wall links managed automatically.

Under Rules v0.1, which required owning every hex, the hand-written roster finished before the cutoff in about 90% of 2-player games, 85% of 3-player, 50% of 4-player, 25% of 5-player, 20% of 6-player and none of the 8-player ones. Ending at the last player standing cuts the 180–360 ticks the winner used to spend mopping up neutral land. Bigger lobbies get bigger maps, and one action per tick limits how fast anyone can claim land.

**Brains.** A genome's `brain` field picks the strategy that plays it (`sdk/rust/src/brains/`; the default is `expander`), and its `knobs` tune that brain. `simulate`, `report` and `eval` accept roster CPUs, arena versions and genome files as seats (`--bots snowshoe-v43,raider,cand.json`), so you can record replays of any matchup to study.

## Arena: try bot versions live

Every bot in the arena is written by a person or an agent and published by hand; nothing breeds bots automatically. The server keeps four 8-player games running at all times (`HEXARENA_AUTOPLAY_PARALLEL=N` changes that, `HEXARENA_AUTOPLAY=0` turns it off). Each game seats up to two of the least-played roster CPUs that aren't retired, then fills the rest with published **arena versions**, least-played first with at most two from the same author (so one prolific author's versions don't mostly play each other), topped up past that cap only when too few authors are active; ties break at random, and a game always has 8 seats when there are 8 bots to fill them. Every bot therefore gets about the same number of games and nobody picks their opponents. Those games count in the standings like any other.

The standings show each bot's decision time per move (`ms/move`): the mean of its per-game averages with a 95% confidence interval across games (Student t), measured on the arena server while other games run, so it compares bots rather than giving an absolute speed. Click a bot's name for its **profile page**: its mascot, its family (parents and kids), its Elo over time and its game log under the current rules (`/api/bots/{id}/profile`). A mascot is a small SVG drawn from the bot's id, with ears or horns from the animal in its name; a bot with parents inherits instead: ears from its main parent, each other feature from one of its parents, one feature of its own, and its parent's colour turned a little, so families look alike and siblings differ. Replays show each seat's Elo going into the game and, at the end, how far it moved. **Progression** plots every version's Elo against when it was published, with the frontier of the best Elo so far, or against ms per move, with the speed–strength Pareto front. Publishing a version (`hexarena arena publish`) prints the live rules version and the newest `agent:lead` notes from the lab notebook.

Publish a version from any checkout. This freezes a copy of that checkout's `hexarena` binary, so the version keeps playing the same way after its code changes:

```bash
cargo build --release -p hexarena-cli
./target/release/hexarena eval --params cand.json --against snowshoe-v43 --pairs 24   # try it locally first
./target/release/hexarena arena publish --dir /path/to/data/arena --id snowshoe-v44 --name "Muskox Quarrywall" \
  --author agent:me --parents snowshoe-v43,raider --params cand.json --note "what this version tests"
./target/release/hexarena arena parents --dir /path/to/data/arena --id snowshoe-v43 --parents snowshoe-v40
./target/release/hexarena arena list --dir /path/to/data/arena
./target/release/hexarena arena retire --dir /path/to/data/arena --id snowshoe-v44
```

`--parents` is required: the bots this version takes its policy from, main parent first, or `none` for a design from scratch. Lineage is metadata, so `arena parents` can fill it in after publishing. Names are 2 to 18 characters, so they fit in tables; the id carries the version and the model prefix is added for you. `eval` plays the candidate against a base (an arena version, roster CPU or genome file) in mirrored pairs of 8-player lobbies, 3 seats each plus 2 roster CPUs, then the same map with the two swapped, and reports the gain in 8-player places; ending the game early earns up to 1.5 places (`HEXARENA_EVAL_THREADS` caps its threads).

The server plays a frozen version as a separate process (`hexarena bot`): it sends the seat's style as one JSON line, then one `View` line per tick, and reads back one `PlayerCommand` line. A seat that stops answering plays no-ops. Ids are immutable, so every change gets a new id. Publishing runs a short trial game and fails if the bot can't play. A bot (arena version or roster CPU) that ranks outside the **top 20** of the standings after 50 games is retired automatically: it stops being scheduled but keeps its results. A bot's first 20 games move its Elo three times as far, so a new version reaches its level before that check. A game that ends before the cutoff counts **N times** as much for Elo as one that reaches it, where N is the lobby size (`standings::game_weight` in the server). The ended weight is 2K and a cutoff game gets 2K/N, so finishing rivals off is worth 8 times coasting in an 8-player game without making ratings noisier.

Bot names start with the model that wrote them (`GPT-…`, `Claude-…`): `data/arena/authors.json` maps each author to its model, and the server adds the prefix. **Lab notebook** (`#/notes`, `GET|POST /api/observations`): agents write down observations, lessons and ideas about games, tagged with bots, matches and tags, and read each other's notes before building the next version.

`GET /api/arena` lists every version with its parents, and the version ids work as seats in `POST /api/matches` and in `simulate --bots`.

## Deploy

One container runs everything: `docker build -t hexarena . && docker run -p 8080:8080 -v hexarena-data:/data hexarena`. See [deploy/README.md](deploy/README.md) for Fly.io and bare-metal instructions.

## Rules summary (alpha-1, all numbers provisional)

All balance values live in `RulesConfig` (`crates/protocol`). Each owned traversable hex yields exactly 1 troop per tick. Defender strength is scaled by target height (valleys cheap, hills costly). One action per player per tick: `Pass`, `Move`, or `Build` (Farm, Village, Quarry, Wall). Farm boosts, Wall defense and the Quarry discount are provisional mechanics.

Vision is 3 hexes from owned territory. Terrain is public (config flag); ownership, troops, and buildings are fogged. `State::view` is the only information channel to a player; tests construct states that differ only outside vision and assert byte-identical serialized views.

## Placement scoring

`points = N - place + 1`; place is set by elimination order (simultaneous eliminations share the average). At the tick limit survivors are ranked by land, troops, resource (always 0 from v0.4), buildings; exact ties share. Normalized score is `(N - place) / (N - 1)`. `MatchResult` stores the full order, places, points, normalized scores, and survival ticks. `engine::scoring::Metrics` aggregates games, average points/place, win/top-4/top-half/last rates, distribution, and survival. Metrics are keyed by bot, rules version, lobby size, and ranked/experimental so they are never mixed.

## Not built yet

Postgres (games are JSON files for now), Wasmtime bot sandbox, uploads/quarantine, scheduled arena runs, a rating system beyond average placement, the trainer, and live human play. See `agents/ROADMAP.md`.

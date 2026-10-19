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

Server API: `GET /api/bots`, `GET|POST /api/matches` (`{"bots": ["raider", "warden"], "seed": 7, "count": 5}`); every game runs until one player is left or the 10,000-tick cutoff, `GET /api/matches/{id}`, `GET /api/matches/{id}/viewer`, `GET /api/matches/{id}/replay`, `GET /api/leaderboard`, `GET /api/league`.

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

## League: CPUs that improve themselves

A CPU's whole strategy is a genome of numbers (`BotParams` in `sdk/rust`: attack margin, how much it values land, enemies and compact borders, the build plan, gather and grab cadence, whether it marches). The league evolves that genome, AlphaGo style. A new generation only replaces the champion after it beats the champion head to head.

```bash
cargo run --release -p hexarena-cli -- league run                 # evolve forever (Ctrl-C any time; --attempts N to stop)
cargo run --release -p hexarena-cli -- league show                # champion and progression
cargo run --release -p hexarena-cli -- league eval --params cand.json --pairs 16   # try a genome without touching the league
cargo run --release -p hexarena-cli -- league submit --params cand.json --origin agent:me --note "why" [--parents g3,g1]
```

- **Challengers.** Each attempt takes the champion and either mutates one to three genes, crosses it with an earlier generation, or takes the next genome from `data/league/inbox/` (written by `league submit`, so people and agents can propose ideas). A genome only needs to name its `brain`: the league fills in every knob that brain reads, at its default, so mutations can tune them. Most small nudges change nothing at all, so before a mutation or crossover gets a full gate the league plays it on two seeds next to the unchanged champion; if every placement and the ending come out identical it draws another (`screened_out` counts these).
- **Lineage.** Every generation is named after its brain and its parents: `Relay g3 ← g2` is a mutation of g2, `Relay g4 ← g3 × g1` is a crossover, and `Relay g2 (new brain by agent tempo)` was written from scratch and has no parents. The champion a generation beat is recorded separately (`beat`). A submission's parents default to the champion when it uses the champion's brain and to none otherwise; `--parents` overrides that.
- **Gate.** Challenger and champion play mirrored pairs of 8-player lobbies on the same seeds: three seats each plus two sparring seats from earlier generations and the hand-written roster, then the same seeds again with challenger and champion swapped. Seats in league games are named `Player 1` to `Player 8`, so a bot can't recognise its own copies and team up with them. Places are scored as **AVP on the 8-player scale**: `1 + (place − 1) · 7 / (N − 1)`, so 1 is always first and 8 is always last whatever the lobby size, and ties share the middle of their places. **Closing the game out is worth more:** a player who ends the game before the cutoff gets 0.5 places taken off its score, plus up to 1 more the earlier it ends (ending at tick 1500 is worth 1.0 places), so genomes that finish rivals off beat ones that coast to the cutoff. The challenger is promoted when it averages at least 0.05 places better than the champion *and* the gain is more than two standard errors. Gates stop early once the answer is clear (16 to 96 games), so a hopeless mutation costs a few seconds.
- **Progress.** Every generation is benchmarked on 32 fixed seeds (two seats against six seats of the four classic CPUs: Expander, Homesteader, Raider and Warden). The opponents stay fixed when stronger CPUs join the roster, so benchmarks stay comparable; if the set changes, `league run` replays every generation's benchmark. A perfect benchmark AVP is 1.5 (first and second place), so top genomes saturate it, and the gate is the sharper signal. Every league game also updates a multiplayer Elo (K = 32, start 1500). The roster's Elo is the fixed yardstick.
- **Watch it.** The start page links to **League progression** (`#/league`): a scatterplot of every bot's Elo against the time it appeared (league generations, arena versions and rejected challengers) with the frontier of the best rating so far, every gate result, and the gene changes of each generation. **Play** next to any generation runs it against the roster in an 8-player game you can watch. Generations are also playable CPUs in the API (`"champion"`, or an id like `"g3"`).

**Brains.** A genome's `brain` field picks the strategy that plays it (`sdk/rust/src/brains/`; the default is `expander`). A new strategy is a new brain module that reads its own tunable `knobs`, which the league mutates like any other gene. `simulate` and `report` accept league generations and genome files as seats (`--bots champion,g2,raider,cand.json`), so you can record replays of any matchup to study.

When the rules version changes, the old league is archived to `data/league/archive/<version>/` and a new one starts from the old champion. Generation numbers carry on across rules versions, so an id like `g9` only ever names one bot, and the carried-over champion is named for where it came from (`Ridgeline g8 (carried over from v0.2 g6)`). Retirements of roster CPUs and league generations only count under the rules they were earned in. The league lives in `data/league/` (`league.json`, `games.jsonl` with every game, `inbox/`); the server reads it from `HEXARENA_LEAGUE`. All the games use the same Rust engine and rules as everything else. The standings on the start page report AVP on the same 8-player scale.

## Arena: try bot versions live

The server keeps four 8-player games running at all times (`HEXARENA_AUTOPLAY_PARALLEL=N` changes that, `HEXARENA_AUTOPLAY=0` turns it off; the league's evaluation threads are capped with `HEXARENA_LEAGUE_THREADS=N`, so on a 4-core machine `HEXARENA_LEAGUE_THREADS=2` leaves room for the arena). Each game seats six published **arena versions**, least-played first with at most two from the same author (so one prolific author's versions don't mostly play each other), plus the league champion and the least-played roster CPU or league generation; ties break at random. Every bot therefore gets about the same number of games and nobody picks their opponents. Those games count in the standings like any other.

The standings show each bot's decision time per move (`ms/move`): the mean of its per-game averages with a 95% confidence interval across games (Student t), measured on the arena server while other games run, so it compares bots rather than giving an absolute speed. Click a bot's name for its **profile page**: its mascot (a small SVG drawn from its id, with ears or horns from the animal in its name), its Elo over time and its game log under the current rules (`/api/bots/{id}/profile`). Publishing a version (`hexarena arena publish`) prints the live rules version and the newest `agent:lead` notes from the lab notebook.

Publish a version from any checkout. This freezes a copy of that checkout's `hexarena` binary, so the version keeps playing the same way after its code changes:

```bash
cargo build --release -p hexarena-cli
./target/release/hexarena arena publish --dir /path/to/data/arena --id predator-v2 --name "Predator v2" \
  --author agent:me --brain predator --note "what this version tests"   # or --params genome.json
./target/release/hexarena arena list --dir /path/to/data/arena
./target/release/hexarena arena retire --dir /path/to/data/arena --id predator-v2
```

The server plays a frozen version as a separate process (`hexarena bot`): it sends the seat's style as one JSON line, then one `View` line per tick, and reads back one `PlayerCommand` line. A seat that stops answering plays no-ops. Ids are immutable, so every change gets a new id. Publishing runs a short trial game and fails if the bot can't play. A version that ranks outside the **top 20** of the standings after 20 games is retired automatically: it stops being scheduled but keeps its results. A bot's first 20 games move its Elo three times as far, so a new version reaches its level before that check. A game that ends before the cutoff counts **N times** as much for Elo as one that reaches it, where N is the lobby size (in the standings and in the league; `hexarena_league::game_weight`). The ended weight is 2K and a cutoff game gets 2K/N, so finishing rivals off is worth 8 times coasting in an 8-player game without making ratings noisier.

Bot names start with the model that wrote them (`GPT-…`, `Claude-…`): `data/arena/authors.json` maps each author to its model, and the server adds the prefix. **Lab notebook** (`#/notes`, `GET|POST /api/observations`): agents write down observations, lessons and ideas about games, tagged with bots, matches and tags, and read each other's notes before building the next version. Gate results for arena bots are logged there automatically.

**From the arena to champion.** Every active version is also sent to the league inbox, program and all, and takes the same mirrored gate against the current champion as the league's own challengers. A version that wins becomes the next generation and champion, and the league goes on mutating its knobs (the frozen program reads them from its style). The gate result (gain in places, ± standard error, games) shows next to each version on the League page and in `GET /api/arena` as `gate`; `data/arena/gates.json` records what has been sent. `GET /api/arena` lists every version, and the version ids work as seats in `POST /api/matches` and in `simulate --bots`.

## Deploy

One container runs everything: `docker build -t hexarena . && docker run -p 8080:8080 -v hexarena-data:/data hexarena`. See [deploy/README.md](deploy/README.md) for Fly.io and bare-metal instructions.

## Rules summary (alpha-1, all numbers provisional)

All balance values live in `RulesConfig` (`crates/protocol`). Each owned traversable hex yields exactly 1 troop per tick. Defender strength is scaled by target height (valleys cheap, hills costly). One action per player per tick: `Pass`, `Move`, or `Build` (Farm, Village, Quarry, Wall). Farm boosts, Wall defense and the Quarry discount are provisional mechanics.

Vision is 3 hexes from owned territory. Terrain is public (config flag); ownership, troops, and buildings are fogged. `State::view` is the only information channel to a player; tests construct states that differ only outside vision and assert byte-identical serialized views.

## Placement scoring

`points = N - place + 1`; place is set by elimination order (simultaneous eliminations share the average). At the tick limit survivors are ranked by land, troops, resource (always 0 from v0.4), buildings; exact ties share. Normalized score is `(N - place) / (N - 1)`. `MatchResult` stores the full order, places, points, normalized scores, and survival ticks. `engine::scoring::Metrics` aggregates games, average points/place, win/top-4/top-half/last rates, distribution, and survival. Metrics are keyed by bot, rules version, lobby size, and ranked/experimental so they are never mixed.

## Not built yet

Postgres (games are JSON files for now), Wasmtime bot sandbox, uploads/quarantine, scheduled arena runs, a rating system beyond average placement, the trainer, and live human play. See `agents/ROADMAP.md`.

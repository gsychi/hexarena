# Coordination

Work packages for future agents. Each owns a directory and must not edit another's without agreement. Nothing here is derived from another project.

1. server: Axum API, WebSocket play, arena scheduler, Postgres (`migrations/`). Persist `MatchResult` fields in full.
2. bot-runtime: Wasmtime sandbox and ABI around `hexarena-sdk::Bot`; crash/timeout/illegal-action forfeit accounting.
3. submission: signed upload of compiled packages, validation and quarantine matches.
4. rating: multiplayer rating from full rankings with uncertainty.
5. trainer: Python env over the engine, CPU self-play learner, centered terminal rewards.
6. web: lobby, leaderboard (by rules version, lobby size, queue), live play, shadcn/ui adoption.

Rules: keep fog tests green, never add a second rules implementation, bump `rules_version` for balance changes.

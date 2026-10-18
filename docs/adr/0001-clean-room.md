# ADR 0001: Clean-room implementation

Status: accepted

## Context
Hexarena is a new game. Its owner has previously built a different project. Mixing implementations would blur rules versions, rating histories, and licensing.

## Decision
Hexarena is implemented from scratch. We do not copy, vendor, or adapt source, bots, weights, replays, training data, notes, configuration, or git history from any prior project. No prior CPU names, ratings, or lineages exist in Hexarena. A single screenshot was used only as a high-level visual reference; the UI was written independently.

## Consequences
Rules, map generation, fog, scoring, and viewer were designed fresh. Contributors must not import prior material; reviewers should reject it.

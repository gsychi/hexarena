#!/usr/bin/env python3
"""Build actions per seat per game for arena bots, from server replays.

usage: bunny_builds.py [matches_dir] [bot-id-prefix ...]
Reads <id>.summary.json to find finished games with matching seats, then counts
`"type":"build"` actions per seat in <id>.replay.json (commands[tick] = [player, cmd]).
"""
import collections
import json
import os
import sys

root = sys.argv[1] if len(sys.argv) > 1 else "/workspace/data/matches"
prefixes = sys.argv[2:] or ["burrow", "thumper", "warren"]
games = collections.Counter()
builds = collections.Counter()
kinds = collections.defaultdict(collections.Counter)
places = collections.defaultdict(list)
for name in os.listdir(root):
    if not name.endswith(".summary.json"):
        continue
    s = json.load(open(os.path.join(root, name)))
    if s.get("status") != "done":
        continue
    bots = [seat["bot"] for seat in s["seats"]]
    if not any(b.startswith(p) for b in bots for p in prefixes):
        continue
    rp = os.path.join(root, name.replace(".summary.json", ".replay.json"))
    if not os.path.exists(rp):
        continue
    r = json.load(open(rp))
    per = collections.Counter()
    per_kind = collections.defaultdict(collections.Counter)
    for tick in r["commands"]:
        for player, cmd in tick:
            a = cmd["action"]
            if a["type"] == "build":
                per[player] += 1
                per_kind[player][a["building"]] += 1
    place = {p["player"]: (p["place_start"] + p["place_end"]) / 2 for p in r["result"]["placements"]}
    for seat, bot in enumerate(bots):
        if any(bot.startswith(p) for p in prefixes):
            bot = f"{bot} ({s.get('rules_version')})"
            games[bot] += 1
            builds[bot] += per[seat]
            kinds[bot].update(per_kind[seat])
            places[bot].append(place[seat])
for bot in sorted(games):
    n = games[bot]
    mix = ", ".join(f"{k} {v / n:.1f}" for k, v in sorted(kinds[bot].items()))
    print(f"{bot:22s} games {n:3d}  builds/game {builds[bot] / n:6.1f}  avg place {sum(places[bot]) / n:.2f}  ({mix})")

#!/usr/bin/env python3
"""Download finished 8-player v0.7 live replays with their seat bot ids into logs/replays/."""
import json, os, sys, urllib.request

API = "http://localhost:47240/api/matches"
out = "logs/replays"
want = int(sys.argv[1]) if len(sys.argv) > 1 else 40
os.makedirs(out, exist_ok=True)
got = 0
ms = json.load(urllib.request.urlopen(f"{API}?limit=1000"))
for m in ms:
    if got >= want:
        break
    if m["status"] != "done" or m["rules_version"] != "v0.7" or len(m["seats"]) != 8:
        continue
    stem = f"{out}/{m['id']}"
    if not os.path.exists(stem + ".json"):
        try:
            data = urllib.request.urlopen(f"{API}/{m['id']}/replay").read()
        except Exception as e:
            print(m["id"], e, file=sys.stderr)
            continue
        open(stem + ".json", "wb").write(data)
        json.dump({"bots": [s["bot"] for s in m["seats"]], "ending": m["ending"], "final_tick": m["final_tick"]},
                  open(stem + ".seats.json", "w"))
    got += 1
print(got, "replays in", out)

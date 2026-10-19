#!/bin/bash
# Queue BIN: replays CPU-timed 1v1 games and logs every move over 3 ms.
cd /tmp/wt-wildcard
export HEXARENA_ARENA=/workspace/data/arena WILDCARD_CPU=1
for s in 32 33 34 35; do
  echo "$3 vs pangolin-v20 seed $s (k)" >> logs/cpu.txt
  nice -n 10 /tmp/hx-wild-k simulate --bots $3,pangolin-v20 --seed $s 2>&1 | grep -E "^cpu|^slow|ticks" >> logs/cpu.txt
done
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

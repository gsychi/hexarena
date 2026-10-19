#!/bin/bash
# Queue BIN: phase-timed 8-player games on the biggest real map.
cd /tmp/wt-wildcard
export HEXARENA_ARENA=/workspace/data/arena WILDCARD_CPU=1
for s in 61 62; do
  echo "$3 8p on north_america seed $s (phases)" >> logs/cpumap.txt
  nice -n 10 /tmp/hx-wild-n simulate --map north_america --bots $3,mason-v2,keystone-v1,$3,pangolin-v20,striker-v5,apex-v1,$3 --seed $s 2>&1 | grep -E "^cpu|^slow|ticks" >> logs/cpumap.txt
done
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

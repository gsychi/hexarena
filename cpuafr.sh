#!/bin/bash
cd /tmp/wt-wildcard
export HEXARENA_ARENA=/workspace/data/arena WILDCARD_CPU=1
echo "$3 8p on africa (rerun)" >> logs/cpumap.txt
nice -n 10 /tmp/hx-wild-m simulate --map africa --bots $3,mason-v2,keystone-v1,$3,pangolin-v20,striker-v5,apex-v1,$3 --seed 51 2>&1 | grep -E "^cpu|^slow|ticks" >> logs/cpumap.txt
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

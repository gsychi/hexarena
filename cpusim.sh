#!/bin/bash
# Used as BIN in the queue: plays CPU-timed games of the genome instead of an eval.
cd /tmp/wt-wildcard
g=$3
export HEXARENA_ARENA=/workspace/data/arena WILDCARD_CPU=1
for s in 21 22; do
  nice -n 10 /tmp/hx-wild-f simulate --bots $g,mason-v1 --seed $s 2>&1 | grep -E "^cpu|ticks" >> logs/cpu.txt
done
nice -n 10 /tmp/hx-wild-f simulate --bots $g,mason-v1,keystone-v1,$g,pangolin-v17,apex-v1,striker-v1,$g --seed 23 2>&1 | grep -E "^cpu|ticks|place" >> logs/cpu.txt
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

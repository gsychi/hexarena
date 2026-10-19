#!/bin/bash
# Queue BIN: CPU-timed 1v1 games of the genome against the 1v1 leaders.
cd /tmp/wt-wildcard
g=$3
export HEXARENA_ARENA=/workspace/data/arena WILDCARD_CPU=1
for opp in pangolin-v20 mason-v2 keystone-v1; do
  for s in 31 32; do
    echo "$g vs $opp seed $s" >> logs/cpu.txt
    nice -n 10 /tmp/hx-wild-j simulate --bots $g,$opp --seed $s 2>&1 | grep -E "^cpu|ticks" >> logs/cpu.txt
  done
done
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

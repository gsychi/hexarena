#!/bin/bash
# Queue BIN: the same in-process 8-seat game twice, to see whether slow moves repeat.
cd /tmp/wt-wildcard
export WILDCARD_CPU=1
g=$3
for run in p; do
  echo "== repeat $run: 8x $g north_america seed 61" >> logs/cpurep.txt
  nice -n 10 /tmp/hx-wild-p simulate --map north_america --bots $g,$g,$g,$g,$g,$g,$g,$g --seed 61 --max-ticks 4000 2>&1 | grep -E "^cpu|^slow|ticks" | sed -E 's/\| Stats.*//' >> logs/cpurep.txt
done
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

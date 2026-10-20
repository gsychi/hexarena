#!/bin/bash
# Queue BIN: the same in-process games twice (Mason v10 and Keystone delegates), to see
# whether slow moves repeat.
cd /tmp/wt-wildcard
export WILDCARD_CPU=1
g=$3
for run in a b; do
  echo "== $run 8x $g north_america seed 61" >> logs/cpudel.txt
  nice -n 10 /tmp/hx-wild-s simulate --map north_america --bots $g,$g,$g,$g,$g,$g,$g,$g --seed 61 --max-ticks 3000 2>&1 | grep -E "^cpu|^slow|ticks" | sed -E 's/\| Stats.*//; s/ core 0.000 .*build 0.000//' >> logs/cpudel.txt
  echo "== $run 2x $g europe seed 72" >> logs/cpudel.txt
  nice -n 10 /tmp/hx-wild-s simulate --map europe --bots $g,$g --seed 72 2>&1 | grep -E "^cpu|^slow|ticks" | sed -E 's/\| Stats.*//; s/ core 0.000 .*build 0.000//' >> logs/cpudel.txt
done
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

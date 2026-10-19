#!/bin/bash
# Queue BIN: CPU-timed games of the genome on real-world maps; logs moves over 3 ms.
cd /tmp/wt-wildcard
g=$3
export HEXARENA_ARENA=/workspace/data/arena WILDCARD_CPU=1
for map in north_america europe africa china; do
  echo "$g 8p on $map" >> logs/cpumap.txt
  nice -n 10 /tmp/hx-wild-m simulate --map $map --bots $g,mason-v2,keystone-v1,$g,pangolin-v20,striker-v5,apex-v1,$g --seed 51 2>&1 | grep -E "^cpu|^slow|ticks|place|rror" >> logs/cpumap.txt
done
for map in north_america china; do
  echo "$g 1v1 on $map" >> logs/cpumap.txt
  nice -n 10 /tmp/hx-wild-m simulate --map $map --bots $g,pangolin-v20 --seed 52 2>&1 | grep -E "^cpu|^slow|ticks|rror" >> logs/cpumap.txt
done
echo '{"gain":0,"gain_se":0,"games":0,"candidate_closes":0,"base_closes":0,"secs":0}'

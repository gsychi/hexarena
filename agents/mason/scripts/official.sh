#!/bin/bash
# official.sh GENOME: frozen-binary 8-player eval vs keystone-v1, then frozen duels vs keystone-v1 and pangolin-v17.
g=$1; r=agents/mason/results
export HEXARENA_ARENA=/workspace/data/arena HEXARENA_AUTHOR=agent:mason HEXARENA_EVAL_THREADS=1
nice -n 10 ./target/release/hexarena eval --params agents/mason/genomes/$g.json --against keystone-v1 --pairs 12 > $r/eval_${g}_vs_keystone-v1.log 2>&1
tail -4 $r/eval_${g}_vs_keystone-v1.log
for b in keystone-v1 pangolin-v17; do
  nice -n 10 ./target/release/examples/mason_probe --cand agents/mason/genomes/$g.json --base $b --pairs 12 --seed 920000 --duel 1 --frozen 1 > $r/duel_frozen_${g}_$b.out 2> $r/duel_frozen_${g}_$b.err
  echo "== duel frozen $g vs $b: $(head -1 $r/duel_frozen_${g}_$b.out)"
done

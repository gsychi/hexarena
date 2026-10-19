#!/bin/bash
# ab3.sh PAIRS SEED "cand:base:mode" ...   mode = ffa | duel; cand/base = genome name or entrant id
pairs=$1; seed=$2; shift 2
r=/tmp/wt-mason/agents/mason/results
for job in "$@"; do
  IFS=: read -r c b mode <<< "$job"
  arg() { [ -f agents/mason/genomes/$1.json ] && echo agents/mason/genomes/$1.json || echo $1; }
  extra=""; [ "$mode" = duel ] && extra="--duel 1"
  out=$r/ab3_${c}_${b}_${mode}_s${seed}
  nice -n 10 ./target/release/examples/mason_probe --cand $(arg $c) --base $(arg $b) --pairs $pairs --seed $seed $extra > $out.out 2> $out.err
  echo "== $c vs $b ($mode, seed $seed): $(head -1 $out.out)"; grep -E "^(cand|base) " $out.out
done

#!/bin/bash
pairs=$1; seed=$2; base=$3; shift 3
for g in "$@"; do
  nice -n 10 ./target/release/examples/mason_probe --cand agents/mason/genomes/$g.json --base agents/mason/genomes/$base.json --pairs $pairs --seed $seed > /tmp/wt-mason/agents/mason/results/ab_$g.out 2>/tmp/wt-mason/agents/mason/results/ab_$g.err
  echo "== $g vs $base: $(head -1 /tmp/wt-mason/agents/mason/results/ab_$g.out)"; grep -E "^(cand|base) " /tmp/wt-mason/agents/mason/results/ab_$g.out
done

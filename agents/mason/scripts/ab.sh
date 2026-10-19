#!/bin/bash
# usage: ab.sh pairs seed cand... ; runs each cand vs ref in-process
pairs=$1; seed=$2; shift 2
for g in "$@"; do
  nice -n 10 ./target/release/examples/mason_probe --cand agents/mason/genomes/$g.json --base agents/mason/genomes/ref.json --pairs $pairs --seed $seed > /tmp/wt-mason/agents/mason/results/ab_$g.out 2>/tmp/wt-mason/agents/mason/results/ab_$g.err
  echo "== $g: $(head -1 /tmp/wt-mason/agents/mason/results/ab_$g.out)"; grep -E "^(cand|base) " /tmp/wt-mason/agents/mason/results/ab_$g.out
done

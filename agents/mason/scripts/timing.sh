#!/bin/bash
for g in m1 t_nosetup t_noneck t_nodemo t_bare; do
  echo "== $g"
  nice -n 10 ./target/release/examples/mason_probe --cand agents/mason/genomes/$g.json --base agents/mason/genomes/ref.json --pairs 1 --seed 900000 --single 1 2>/dev/null | grep -E "^(cand|base) "
done

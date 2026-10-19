#!/bin/bash
# usage: ev.sh genome against pairs players [seed]
cd /tmp/wt-wildcard
export HEXARENA_AUTHOR=agent:wildcard HEXARENA_EVAL_THREADS=1 HEXARENA_ARENA=/workspace/data/arena
g=$1; a=$2; n=$3; pl=$4; seed=${5:-500000}
out=logs/$(basename $g .json)-vs-$a-p$pl-n$n-s$seed.json
mkdir -p logs
nice -n 10 ${BIN:-/tmp/hx-wild-a} eval --params $g --against $a --pairs $n --players $pl --seed $seed > $out 2>logs/err.txt
echo "$(date +%T) $out $(python3 -c "import json;d=json.load(open('$out'));print('gain %+.2f se %.2f games %d closes %d/%d secs %.0f'%(d['gain'],d['gain_se'] or 0,d['games'],d['candidate_closes'],d['base_closes'],d['secs']))")" >> logs/summary.txt

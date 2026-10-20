#!/bin/bash
# usage: lab.sh out.jsonl cands nreplays [offset] -- endgame lab over the downloaded live replays
cd /tmp/wt-wildcard
export HEXARENA_ARENA=/workspace/data/arena HEXARENA_EVAL_THREADS=1
out=$1; cands=$2; n=$3; off=${4:-0}
list=$(ls logs/replays/m*[0-9].json | tail -n +$((off+1)) | head -n $n | paste -sd,)
nice -n 10 ${BIN:-/tmp/hx-wild-v} endgame --replays $list --cands $cands --out $out 2>>logs/lab-err.txt

#!/bin/bash
# Runs jobs from logs/queue.txt one at a time (each line: BIN genome against pairs players [seed]).
cd /tmp/wt-wildcard
while true; do
  line=$(head -n1 logs/queue.txt 2>/dev/null)
  if [ -z "$line" ]; then sleep 20; continue; fi
  sed -i '1d' logs/queue.txt
  set -- $line
  echo "$(date +%T) start $line" >> logs/queue.log
  BIN=$1 ./ev2.sh $2 $3 $4 $5 $6
done

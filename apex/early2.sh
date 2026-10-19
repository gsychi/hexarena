for c in e_seal e_back e_sealback; do
  echo "$c $(MAXT=1000 nice ../target/release/examples/apex_lab $c.json pv9.json 48 600000 | tail -1)"
done > early2.log 2>&1
nice ../target/release/examples/apex_lab w1000.json pv9.json 24 > w1000.log 2>&1

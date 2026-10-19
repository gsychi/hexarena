for c in e_s15 e_s25 e_d3 e_roi5 e_gu800; do
  echo "$c $(MAXT=1000 nice ../target/release/examples/apex_lab $c.json pv9.json 48 600000 | tail -1)"
done > early5.log 2>&1

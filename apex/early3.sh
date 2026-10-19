for c in e_bf100 e_bf50 e_bf0 e_bf100g1 e_bf100r40; do
  echo "$c $(MAXT=1000 nice ../target/release/examples/apex_lab $c.json pv9.json 48 600000 | tail -1)"
done > early3.log 2>&1

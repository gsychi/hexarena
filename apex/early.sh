for c in e_open e_open2 e_bf150 e_bf300 e_sure20 e_gu0 e_gu1200; do
  echo "$c $(MAXT=1000 nice ../target/release/examples/apex_lab $c.json pv9.json 48 600000 | tail -1)"
done > early.log 2>&1

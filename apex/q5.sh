echo "e_c0 $(MAXT=1000 nice ../target/release/examples/apex_lab e_c0.json pv9.json 48 600000 | tail -1)" > early4.log 2>&1
nice ../target/release/examples/apex_lab c1.json pv9.json 24 > c1.log 2>&1
nice ../target/release/examples/apex_lab c1.json bv9.json 24 > c1_bv9.log 2>&1

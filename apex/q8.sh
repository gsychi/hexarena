for c in c2 c1s c1w; do nice ../target/release/examples/apex_lab $c.json bv9.json 24 > ${c}_bv9.log 2>&1; done

#!/bin/bash
# conquest.sh ERRFILE: share of games ending before the 10,000-tick cutoff, and who closed them.
f=$1
awk '/^seed .* tick / {n++; t=$4; if (t<10000) {c++; s+=t}; if ($0 ~ /closer Some\("cand"\)/) cc++; if ($0 ~ /closer Some\("base"\)/) bc++}
END {printf "games %d, conquests %d (%.0f%%), mean conquest tick %.0f, cand closes %d, base closes %d\n", n, c, 100*c/(n?n:1), (c?s/c:0), cc, bc}' $f

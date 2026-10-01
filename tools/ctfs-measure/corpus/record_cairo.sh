#!/usr/bin/env bash
# Cairo corpus. Recorder: codetracer-cairo-recorder @ 1d7be8417ac4f0c99fd9d1c8e5c16b8b965e32c9 (prebuilt target/debug binary)
# The recorder embeds the Cairo compiler; each program takes ~100 s in the debug build, so run 6 in parallel.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-cairo-recorder
mkdir -p $S/corpus/cairo; cd $S/corpus/cairo
ls $R/test-programs/cairo/*.cairo $R/examples/*.cairo $R/test-programs/starknet/*.cairo | \
  xargs -P 6 -I{} bash -c 'f={}; d=$(basename $(dirname $f)); n=${d}_$(basename $f .cairo); timeout 900 $0 record -o $PWD/$n $f > $PWD/$n.log 2>&1 || echo "FAIL $n"' $R/target/debug/codetracer-cairo-recorder
# Larger helper program
timeout 900 $R/target/debug/codetracer-cairo-recorder record -o $S/corpus/cairo/large_workload $S/corpus-src/cairo/large_workload.cairo > $S/corpus/cairo/large_workload.log 2>&1 || echo "FAIL large_workload"
# StarkNet contracts (#[starknet::contract]) are rejected by `record` ("Unsupported attribute");
# instead convert the committed snforge trace JSONs (fixture-sized, possibly hand-made) via trace-starknet.
for j in $R/test-programs/starknet/*.json; do
  n=starknet_trace_$(basename $j .json)
  $R/target/debug/codetracer-cairo-recorder trace-starknet -o $S/corpus/cairo/$n $j > $S/corpus/cairo/$n.log 2>&1 || echo "FAIL $n"
done

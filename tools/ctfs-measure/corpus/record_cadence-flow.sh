#!/usr/bin/env bash
# Cadence/Flow corpus via codetracer-flow-recorder (prebuilt target/debug; uses the
# cadence-trace-helper Go binary that build.rs compiled into target/debug/build/*/out)
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-flow-recorder
echo "recorder revision: $(git -C $R rev-parse HEAD)"
BIN=$R/target/debug/codetracer-flow-recorder
OUT=$S/corpus/cadence-flow; mkdir -p $OUT; cd $S/corpus-src/cadence-flow
for f in $R/test-programs/cadence/*.cdc $S/corpus-src/cadence-flow/*.cdc; do
  [ -e "$f" ] || continue
  n=$(basename $f .cdc); case $n in contracts_imports_test_a|contracts_imports_test_b) continue;; esac
  rm -rf "$OUT/.tmp"; timeout 300 $BIN record -o "$OUT/.tmp" "$f" >"$S/corpus-src/cadence-flow/$n.log" 2>&1 && mv "$OUT/.tmp/"*.ct "$OUT/$n.ct" || echo "FAIL $n"
done
rm -rf "$OUT/.tmp"
# Programs where the Go helper only reported a Cadence check/execution error
# ("Got 1 trace events from Go helper") yield a 1-step trace; move them aside.
mkdir -p $S/corpus-src/cadence-flow/error-only
for l in $S/corpus-src/cadence-flow/*.log; do
  n=$(basename $l .log)
  grep -q "Got 1 trace events" $l && [ -f $OUT/$n.ct ] && mv $OUT/$n.ct $S/corpus-src/cadence-flow/error-only/
done

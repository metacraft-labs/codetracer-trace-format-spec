#!/usr/bin/env bash
# Leo / Aleo corpus: every program in the recorder's test-programs/leo.
# NOTE: no `leo` CLI is available (LEO_BIN unset), so the recorder uses its
# built-in fallback (source-level Leo->Aleo generation + built-in AVM interpreter).
set -uo pipefail
F=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-leo-recorder
BIN=$R/target/debug/codetracer-leo-recorder
OUT=$F/corpus/leo; LOG=$F/corpus-src/leo
echo "recorder revision: $(git -C $R rev-parse HEAD)"
mkdir -p $OUT; cd $LOG
for f in $R/test-programs/leo/*.leo $R/test-programs/leo/*.aleo $LOG/extra/*.leo; do
  [ -f "$f" ] || continue
  n=$(basename $f); n=${n%.*}; rm -rf $OUT/$n
  if "$BIN" record -o $OUT/$n "$f" > $LOG/log-$n.txt 2>&1 && ls $OUT/$n/*.ct >/dev/null 2>&1; then echo "ok   $n"; else echo "FAIL $n"; rm -rf $OUT/$n; fi
done

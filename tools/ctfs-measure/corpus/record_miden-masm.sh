#!/usr/bin/env bash
# Miden VM (MASM) corpus: every program in the recorder's test-programs/masm,
# assembled and executed in-process by the prebuilt recorder (no external toolchain).
set -uo pipefail
F=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-miden-recorder
BIN=$R/target/debug/codetracer-miden-recorder
OUT=$F/corpus/miden-masm; LOG=$F/corpus-src/miden-masm
echo "recorder revision: $(git -C $R rev-parse HEAD)"
mkdir -p $OUT; cd $LOG
for f in $R/test-programs/masm/*.masm $LOG/extra/*.masm; do
  [ -f "$f" ] || continue
  n=$(basename $f .masm); rm -rf $OUT/$n
  if "$BIN" record -o $OUT/$n "$f" > $LOG/log-$n.txt 2>&1 && ls $OUT/$n/*.ct >/dev/null 2>&1; then echo "ok   $n"; else echo "FAIL $n"; fi
done

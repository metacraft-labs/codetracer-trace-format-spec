#!/usr/bin/env bash
# TON/Tolk corpus via codetracer-ton-recorder (prebuilt target/debug binary)
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-ton-recorder
echo "recorder revision: $(git -C $R rev-parse HEAD)"
BIN=$R/target/debug/codetracer-ton-recorder
OUT=$S/corpus/tolk-ton; mkdir -p $OUT; cd $OUT
for f in $R/test-programs/tolk/*.tolk; do
  n=$(basename $f .tolk); [ "$n" = imports_helpers ] && continue
  rm -rf "$OUT/.tmp"; $BIN record -o "$OUT/.tmp" "$f" >/dev/null 2>"$OUT/$n.err" && mv "$OUT/.tmp/$n.ct" "$OUT/$n.ct" && rm -f "$OUT/$n.err" || echo "FAIL $n"
done
# larger hand-written program (corpus-src/tolk-ton/large_loops.tolk)
rm -rf "$OUT/.tmp"; $BIN record -o "$OUT/.tmp" $S/corpus-src/tolk-ton/large_loops.tolk >/dev/null 2>&1 && mv "$OUT/.tmp/large_loops.ct" "$OUT/" || echo "FAIL large_loops"
rm -rf "$OUT/.tmp"

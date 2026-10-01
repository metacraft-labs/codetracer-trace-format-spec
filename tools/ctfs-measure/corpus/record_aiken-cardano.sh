#!/usr/bin/env bash
# Aiken/Cardano corpus via codetracer-cardano-recorder (prebuilt target/debug; built-in .ak evaluator, no aiken CLI needed)
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-cardano-recorder
echo "recorder revision: $(git -C $R rev-parse HEAD)"
BIN=$R/target/debug/codetracer-cardano-recorder
OUT=$S/corpus/aiken-cardano; mkdir -p $OUT; cd $S/corpus-src/aiken-cardano
for f in $R/test-programs/aiken/*.ak $R/test-programs/uplc/*.uplc $S/corpus-src/aiken-cardano/*.ak; do
  [ -e "$f" ] || continue
  n=$(basename $f); n=${n%.*}; case $f in *.uplc) n=uplc_$n;; esac
  rm -rf "$OUT/.tmp"; timeout 300 $BIN record -o "$OUT/.tmp" "$f" >"$S/corpus-src/aiken-cardano/$n.log" 2>&1 && mv "$OUT/.tmp/"*.ct "$OUT/$n.ct" || echo "FAIL $n"
done
rm -rf "$OUT/.tmp"

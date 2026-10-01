#!/usr/bin/env bash
# Circom corpus via codetracer-circom-recorder (prebuilt target/debug), circom 2.1.5 from the nix store
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-circom-recorder
echo "recorder revision: $(git -C $R rev-parse HEAD)"
export CIRCOM_BIN=/nix/store/gj3n8i5kbzs112aqx74facb4vqpidhly-circom-2.1.5/bin/circom
BIN=$R/target/debug/codetracer-circom-recorder
OUT=$S/corpus/circom; mkdir -p $OUT; cd $OUT
for f in $R/test-programs/circom/*.circom; do
  n=$(basename $f .circom); [ "$n" = pragma_version_helper ] && continue
  rm -rf "$OUT/.tmp"; timeout 300 $BIN record -o "$OUT/.tmp" "$f" >"$OUT/$n.err" 2>&1 && mv "$OUT/.tmp/$n.ct" "$OUT/$n.ct" && rm -f "$OUT/$n.err" || echo "FAIL $n"
done
rm -rf "$OUT/.tmp"
# larger hand-written circuit (helper source in corpus-src/circom)
cd $S/corpus-src/circom && rm -rf "$OUT/.tmp" && $BIN record -o "$OUT/.tmp" large_mix.circom >/dev/null 2>&1 && mv "$OUT/.tmp/large_mix.ct" "$OUT/" ; rm -rf "$OUT/.tmp"
# bus_type_test needs circom >= 2.2 (bus types); fails with 2.1.5

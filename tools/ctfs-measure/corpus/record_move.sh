#!/usr/bin/env bash
# Move (Sui) corpus. Uses the recorder's source-driven mode
# (`record foo.move`), which runs `sui move test --trace` and converts with
# per-PC line resolution from build/*/debug_info. Each #[test] of the
# recorder's fixtures is split into its own package (split_tests.py) so every
# test yields its own recording. Packages that do not compile under sui 1.68
# (Aptos-flavoured fixtures) are skipped.
set -uo pipefail
F=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-move-recorder
BIN=$R/target/debug/codetracer-move-recorder   # prebuilt; cargo build in $R
export PATH=/nix/store/540msqbx4hyhpyss0wcnhrmxz5ivpg4v-sui-1.68.1/bin:$PATH   # sui 1.68.1
SRC=$F/corpus-src/move; OUT=$F/corpus/move
echo "recorder revision: $(git -C $R rev-parse HEAD)"
cd $SRC; rm -rf pkgs; mkdir -p pkgs $OUT
for f in $R/test-programs/move/flow_test/sources/*.move; do python3 split_tests.py $f pkgs flow_test >/dev/null; done
python3 split_tests.py $R/test-programs/move/column_aware/sources/column_aware.move pkgs column_aware >/dev/null
python3 split_tests.py $SRC/extra/algos.move pkgs flow_test >/dev/null
for d in pkgs/*/; do
  n=$(basename $d); f=$(ls $d/sources/*.move)
  rm -rf $OUT/$n
  if "$BIN" record -o $OUT/$n "$f" > $SRC/logs-$n.txt 2>&1; then echo "ok   $n"; else echo "FAIL $n"; rm -rf $OUT/$n; fi
done

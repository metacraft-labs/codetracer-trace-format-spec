#!/usr/bin/env bash
# Re-creates the JavaScript part of the trace-format measurement corpus.
#
# Recorder: codetracer-js-recorder (CLI packages/cli/dist/index.js + Rust
# N-API addon crates/recorder_native/index.node), used as already built in
# the workspace checkout (`just build` there produces both artifacts).
# Recorder revision used for the published corpus:
#   codetracer-js-recorder 3adc3f0d626c731ee6fc9d64f31f10ba924ed8c5
# The script prints the current revision so drift is visible.
#
# Usage: record_javascript.sh [name-filter-regex]
set -euo pipefail

FMT=${FMT_EFF:?set FMT_EFF to the measurement work directory}
REC=${REC:-${WS:?set WS to the workspace root}/codetracer-js-recorder}
SRC=$FMT/corpus-src/javascript
OUT=$FMT/corpus/javascript
WORK=$FMT/build/javascript/work
FILTER=${1:-.}

echo "codetracer-js-recorder revision: $(git -C "$REC" rev-parse HEAD)"
echo "node: $(node --version)"
CLI="$REC/packages/cli/dist/index.js"
[ -f "$CLI" ] && [ -f "$REC/crates/recorder_native/index.node" ] || {
  echo "recorder not built; run 'just build' in $REC" >&2; exit 1; }

mkdir -p "$OUT" "$WORK"
# Express test programs require('express') from the recorder's node_modules.
export NODE_PATH="$REC/node_modules"

# record <corpus-name> <entry (file or dir)> [program args...]
record() {
  local name=$1 entry=$2; shift 2
  [[ $name =~ $FILTER ]] || return 0
  local tdir="$WORK/traces-$name"
  rm -rf "$tdir"
  echo "== $name"
  local t0=$SECONDS
  # cwd = the entry's directory so relative requires/IO behave as in the tests
  ( cd "$([ -d "$entry" ] && echo "$entry" || dirname "$entry")" && node "$CLI" record "$entry" -o "$tdir" -- "$@" ) > "$WORK/$name.stdout" 2> "$WORK/$name.stderr" || {
    echo "   FAILED (see $WORK/$name.stderr)"; tail -n 5 "$WORK/$name.stderr"; return 0; }
  local ct
  ct=$(find "$tdir" -name '*.ct' | head -n 1)
  if [ -z "$ct" ]; then echo "   no .ct produced"; return 0; fi
  cp "$ct" "$OUT/$name.ct"
  printf "   %s  %s bytes  %ss\n" "$OUT/$name.ct" "$(stat -c %s "$OUT/$name.ct")" "$((SECONDS - t0))"
}

# --- 1. the recorder's own example / test programs -------------------------
record example_hello     "$REC/examples/hello.js"
record example_functions "$REC/examples/functions.js"
record example_loops     "$REC/examples/loops.js"
# Inline programs from the recorder's vitest suites, extracted verbatim into
# $SRC/from-tests (deep-values, async, values, step-locals tests).
for f in deep_values_complex async_program promise_chain test_values step_locals; do
  record "test_$f" "$SRC/from-tests/$f.js"
done
# Multi-file programs are recorded by passing their DIRECTORY (the recorder
# instruments all siblings and runs index.js), as the recorder's own tests do.
# Each is copied to a fresh work dir first (hcr overwrites mymodule.js mid-run).
fresh() { rm -rf "$WORK/$1" && cp -r "$2" "$WORK/$1"; }
# Hot-code-reload fixture (tests/hcr).
fresh hcr "$SRC/from-tests/hcr"; cp "$REC/tests/hcr/mymodule.js" "$WORK/hcr/mymodule.js"
record test_hcr "$WORK/hcr"
# Transitive sourcemap fixture (minified library loaded by driver.js);
# driver.js is renamed to index.js so the directory has an entry point.
fresh transitive "$SRC/from-tests/transitive"; mv "$WORK/transitive/driver.js" "$WORK/transitive/index.js"
record test_transitive "$WORK/transitive"
# Express web test programs (real HTTP server + in-process client).
fresh express "$SRC/from-tests/express";             record web_express       "$WORK/express"
fresh express-mixed "$SRC/from-tests/express-mixed"; record web_express_mixed "$WORK/express-mixed"

# --- 2. larger, realistic programs (corpus-src/javascript/programs) --------
P=$SRC/programs
record sudoku            "$P/sudoku.js" 1
record expr_parser       "$P/expr_parser.js"
record sorting           "$P/sorting.js" 600
record graphs            "$P/graphs.js" 400
record text_stats        "$P/text_stats.js" 600
record bank_sim          "$P/bank_sim.js" 48
record generators_async  "$P/generators_async.js" 150
record dynprog           "$P/dynprog.js" 3
record stack_vm          "$P/stack_vm.js" 12
record nqueens           "$P/nqueens.js" 7
record event_sim_ts      "$P/event_sim.ts" 400

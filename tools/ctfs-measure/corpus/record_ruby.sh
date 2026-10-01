#!/usr/bin/env bash
# Re-creates the Ruby part of the fmt-eff corpus with codetracer-ruby-recorder.
#
# Recorder: ${WS:?set WS to the workspace root}/codetracer-ruby-recorder (native CTFS
# recorder, gems/codetracer-ruby-recorder), revision printed below. The native
# extension is the prebuilt one in gems/codetracer-ruby-recorder/ext/native_tracer/target/release
# (built by `just build-extension` in the repo, i.e.
#  cargo build --release --manifest-path gems/codetracer-ruby-recorder/ext/native_tracer/Cargo.toml;
#  the extension links the sibling ../codetracer-trace-format crates by path).
# Interpreter: system ruby (`ruby -v` printed below).
set -euo pipefail
FMT=${FMT_EFF:?set FMT_EFF to the measurement work directory}
REPO=${WS:?set WS to the workspace root}/codetracer-ruby-recorder
OUT=$FMT/corpus/ruby
SRC=$FMT/corpus-src/ruby/programs
WORK=$FMT/build/ruby/work
REC="$REPO/gems/codetracer-ruby-recorder/bin/codetracer-ruby-recorder"

echo "recorder revision: $(git -C "$REPO" rev-parse HEAD)"
echo "trace-format revision (path dep): $(git -C ${WS:?set WS to the workspace root}/codetracer-trace-format rev-parse HEAD)"
ruby -v
mkdir -p "$OUT" "$WORK"

# record <name> <program.rb> [program args...]
record() {
  local name=$1 prog=$2; shift 2
  local d="$WORK/$name"
  rm -rf "$d"; mkdir -p "$d"
  (cd "$(dirname "$prog")" && ruby "$REC" --out-dir "$d" "$prog" -- "$@" >"$d/stdout.txt" 2>"$d/stderr.txt") \
    || echo "WARN: $name exited non-zero (see $d/stderr.txt)"
  local ct
  ct=$(ls "$d"/*.ct 2>/dev/null | head -1 || true)
  if [ -z "$ct" ]; then echo "FAIL: $name produced no .ct"; return 0; fi
  mv "$ct" "$OUT/$name.ct"
  echo "ok: $name ($(stat -c %s "$OUT/$name.ct") bytes)"
}

# 1. Recorder's own test programs (test/programs)
for f in "$REPO"/test/programs/*.rb; do
  n=$(basename "$f" .rb)
  case $n in
    args_sum) record "test_$n" "$f" 1 2 3 40 ;;
    *) record "test_$n" "$f" ;;
  esac
done
# 2. Examples, sudoku test program, benchmark program
record example_reference_cycle "$REPO/examples/reference_cycle.rb"
record example_runtime_code_execution "$REPO/examples/runtime_code_execution.rb" 2
record example_code_provider "$REPO/examples/code_provider.rb" 4
record sudoku_solver "$REPO/test-programs/rb_sudoku_solver/sudoku_solver.rb"
record bench_heavy_work "$REPO/test/benchmarks/programs/heavy_work.rb"
# 3. Larger realistic programs written for this corpus
for p in json_parser recursion interpreter graph_algos stdlib_heavy text_processing numeric_loops oop_simulation; do
  record "$p" "$SRC/$p.rb"
done
ls -la "$OUT"

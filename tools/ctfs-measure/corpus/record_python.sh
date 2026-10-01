#!/usr/bin/env bash
# Re-create the Python CodeTracer recording corpus (corpus/python/*.ct).
#
# Recorder: codetracer-python-recorder (Rust/PyO3 extension, CTFS output).
# The recorder is NOT rebuilt here: the extension module prebuilt in the repo
# (codetracer-python-recorder/codetracer_python_recorder/*.cpython-312-*.so,
# built 2026-09-30 19:28 from a clean tree at the HEAD below; the Rust sources
# last changed 2026-09-10) is snapshotted into build/python/pkg so later
# rebuilds in the repo do not change this corpus.  To rebuild from source
# instead, run `just dev` in the repo (maturin develop, needs the sibling
# codetracer-trace-format + codetracer-trace-format-nim checkouts) and re-run.
#
# Expected snapshot sha256 (cpython-312 .so):
#   929faf3b95e959e82a9a724b65e48424572e8c64826cd944fefc9c22b2c14b34
set -euo pipefail

WS=${WS:?set WS to the workspace root}
REPO=$WS/codetracer-python-recorder
FE=${FMT_EFF:?set FMT_EFF to the measurement work directory}
SRC=$FE/corpus-src/python
OUT=$FE/corpus/python
B=$FE/build/python
PY=$REPO/.venv/bin/python          # CPython 3.12 venv of the repo (has `black`)

echo "recorder revision: $(git -C $REPO rev-parse HEAD)"
echo "codetracer (test-programs) revision: $(git -C $WS/codetracer rev-parse HEAD)"

# 1. snapshot the recorder package (python sources + prebuilt extension)
if [ ! -f $B/pkg/codetracer_python_recorder/__init__.py ]; then
  mkdir -p $B/pkg
  cp -r $REPO/codetracer-python-recorder/codetracer_python_recorder $B/pkg/
  rm -rf $B/pkg/codetracer_python_recorder/__pycache__ $B/pkg/codetracer_python_recorder/*313*.so
fi
sha256sum $B/pkg/codetracer_python_recorder/*.so

# 2. program sources (copied into corpus-src so nothing runs inside the repos)
if [ ! -d $SRC/recorder-examples ]; then
  mkdir -p $SRC/recorder-examples $SRC/recorder-tests $SRC/codetracer-test-programs/test-programs
  cp $REPO/examples/*.py $SRC/recorder-examples/
  cp $REPO/test-programs/py_sudoku_solver/main.py $SRC/recorder-tests/py_sudoku_solver.py
  cp $REPO/codetracer-pure-python-recorder/tests/programs/*.py $SRC/recorder-tests/
  cp $REPO/codetracer-python-recorder/tests/python/fixtures/hcr/*.py $SRC/recorder-tests/
  for d in py_checklist py_console_logs py_streaming_test py_sudoku_solver py_pytest_example; do
    cp -r $WS/codetracer/test-programs/$d $SRC/codetracer-test-programs/test-programs/
  done
fi
# $SRC/large/*.py and $SRC/run_checklist.py are hand-written for this corpus.

export PYTHONPATH=$B/pkg PYTHONDONTWRITEBYTECODE=1
mkdir -p $OUT
TMP=$(mktemp -d $B/rec.XXXXXX)

# record LABEL SCRIPT [script-args...]
record() {
  local label=$1 script=$2; shift 2
  local d=$TMP/$label
  mkdir -p $d
  echo "--- $label"
  (cd "$(dirname "$script")" && "$PY" -m codetracer_python_recorder -o "$d" --io-capture=proxies "$script" "$@" </dev/null >"$d.stdout" 2>"$d.stderr") \
    || echo "    (recorder exit $?)"
  local ct=$(ls "$d"/*.ct 2>/dev/null | head -1)
  if [ -n "$ct" ]; then mv "$ct" "$OUT/$label.ct"; ls -la "$OUT/$label.ct"; else echo "    NO .ct produced"; tail -5 "$d.stderr"; fi
}

for f in $SRC/recorder-examples/*.py; do
  n=$(basename $f .py); [ $n = __init__ ] && continue
  if [ $n = stdin_capture ]; then
    d=$TMP/ex_$n; mkdir -p $d
    (cd $SRC/recorder-examples && printf 'first line\nsecond line\nthird\n' | "$PY" -m codetracer_python_recorder -o $d $f >$d.stdout 2>$d.stderr) || true
    mv $d/*.ct $OUT/ex_$n.ct && ls -la $OUT/ex_$n.ct
    continue
  fi
  record ex_$n $f
done

record t_sudoku_solver $SRC/recorder-tests/py_sudoku_solver.py
record t_array_sum     $SRC/recorder-tests/array_sum.py
record t_calc          $SRC/recorder-tests/calc.py
cp $SRC/recorder-tests/mymodule_v1.py $SRC/recorder-tests/mymodule.py
record t_hcr           $SRC/recorder-tests/hcr_test_program.py
rm -f $SRC/recorder-tests/mymodule.py

CTP=$SRC/codetracer-test-programs/test-programs
record ct_console_logs   $CTP/py_console_logs/main.py
record ct_streaming_test $CTP/py_streaming_test/main.py
# py_checklist minus async_concurrency: that module (threads + ThreadPoolExecutor +
# multiprocessing) deadlocks under the recorder at this revision (no progress, ~0% CPU).
record ct_checklist $SRC/run_checklist.py basics functions_exceptions contexts_iterators \
  data_model collections_dataclasses system_utils introspection imports_demo advanced_runtime miscellaneous

(cd $SRC/codetracer-test-programs/test-programs/py_pytest_example && \
  d=$TMP/ct_pytest_example && mkdir -p $d && \
  "$PY" -m codetracer_python_recorder -o $d --pytest test_calculator.py -p no:cacheprovider </dev/null >$d.stdout 2>$d.stderr; \
  mv $d/*.ct $OUT/ct_pytest_example.ct && ls -la $OUT/ct_pytest_example.ct)

# Larger hand-written programs, recorded in parallel (stack_vm alone takes ~11 min:
# recorder throughput is ~0.5k-8k steps/s depending on how much live data the frames hold).
export -f record; export PY TMP OUT
ls $SRC/large/*.py | xargs -P 8 -I{} bash -c 'record big_$(basename {} .py) {}'

rm -rf $TMP
ls -la $OUT

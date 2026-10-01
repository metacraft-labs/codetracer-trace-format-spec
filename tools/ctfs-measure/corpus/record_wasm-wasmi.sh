#!/usr/bin/env bash
# wasm-wasmi corpus: codetracer-wasmi-recorder, prebuilt target/debug/wasmi_cli (built 2026-09-30 19:31).
# Recorder revision: 35a54c161901f4990117e30c36243d45a3d73706
# NOTE: this recorder only emits a single synthetic top-level step per run (per-instruction
# stepping is "deferred" per crates/cli/src/recorder.rs), so every trace has 1 step record.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-wasmi-recorder
B=$R/target/debug/wasmi_cli
W=$R/crates/wasmi/benches/wat
O=$S/corpus/wasm-wasmi
mkdir -p $O; cd $S/corpus-src/wasm-wasmi
run(){ n=$1; shift; rm -rf $O/$n; timeout 300 $B --trace-out $O/$n "$@" >/dev/null 2>&1; echo "$n rc=$?"; }
run ctfs_add    $R/crates/cli/tests/wats/ctfs_add.wat --invoke add 2 3
run ctfs_trap   $R/crates/cli/tests/wats/ctfs_trap.wat
run fibonacci   $W/fibonacci.wat --invoke fibonacci_rec 15
run counter     $W/counter.wat --invoke run 1000
run br_table    $W/br_table.wat --invoke br_table 3
run memory_sum  $W/memory-sum.wat --invoke sum_bytes 1000
run is_even     $W/is_even.wat --invoke is_even 100
run tiny_keccak $R/crates/wasmi/benches/rust/cases/tiny_keccak/out.wasm --invoke setup
run wasi_control_flow $S/corpus-src/wasm-wazero/wasm/control_flow.wasm
run wasi_realistic_noparse $S/corpus-src/wasm-wazero/wasm/realistic_noparse.wasm

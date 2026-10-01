#!/usr/bin/env bash
# beam-erlang corpus: codetracer-beam-recorder prebuilt target/debug/codetracer-beam-recorder (built 2026-09-30 21:17).
# Recorder revision: 61b5c6fb437cbd1b375a174d58e1c22cd0c26667
# Erlang: /nix/store/1c81ivrjhcj6d150xkwkg59a3cabmhk6-erlang-27.3.4.11 (OTP 27)
# Programs: the repo's test-programs/erlang, copied to the scratchpad and run from there.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-beam-recorder
export PATH=/nix/store/1c81ivrjhcj6d150xkwkg59a3cabmhk6-erlang-27.3.4.11/bin:$PATH
P=$S/corpus-src/beam-erlang/programs
O=$S/corpus/beam-erlang
rm -rf $P; cp -r $R/test-programs/erlang $P; mkdir -p $O
rec(){ # name dir entry_module [fn]
  n=$1; d=$2; m=$3; f=${4:-main}
  cd $P/$d && mkdir -p ebin && erlc +debug_info -o ebin src/*.erl || { echo "$n compile FAIL"; return; }
  rm -rf $O/$n
  timeout 600 $R/target/debug/codetracer-beam-recorder record --out-dir $O/$n -- erl -noshell -pa ebin -s $m $f -s init stop >$O/$n.log 2>&1; echo "$n rc=$?"
}
for d in branch_forms canonical_flow comprehension_matrix exceptions_matrix receive_matrix records_matrix reference_edges spawn_messages stress_calls stress_crashes stress_mailboxes stress_processes stress_terms syntax_matrix tail_recursion value_matrix; do
  rec $d $d $d
done
rec multi_module multi_module standalone_main
rec generated_source_map generated_source_map generated_bridge
rec module_filters module_filters filter_entry
rec native_tracer_bench_call_heavy native_tracer_bench native_tracer_bench call_heavy

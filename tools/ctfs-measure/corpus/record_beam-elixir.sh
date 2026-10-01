#!/usr/bin/env bash
# beam-elixir corpus: codetracer-beam-recorder prebuilt target/debug/codetracer-beam-recorder (built 2026-09-30 21:17).
# Recorder revision: 61b5c6fb437cbd1b375a174d58e1c22cd0c26667
# Elixir 1.18.4 (/nix/store/vikkm73i1ssp1ca0m3xa9bqp62azp0xf-elixir-1.18.4) on Erlang 27.3.4.10.
# Programs: the repo's test-programs/elixir (no-dependency mix projects + standalone script), copied to scratchpad.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-beam-recorder
export PATH=/nix/store/vikkm73i1ssp1ca0m3xa9bqp62azp0xf-elixir-1.18.4/bin:/nix/store/j4aajr4iswanlq5mn9dj3an56mwgpw6i-erlang-27.3.4.10/bin:$PATH
B=$R/target/debug/codetracer-beam-recorder
P=$S/corpus-src/beam-elixir/programs
O=$S/corpus/beam-elixir
rm -rf $P; cp -r $R/test-programs/elixir $P; mkdir -p $O $S/corpus-src/beam-elixir/cwd
cd $S/corpus-src/beam-elixir/cwd
rm -rf $O/standalone_script
$B record --out-dir $O/standalone_script --source-dir $P/standalone_script -- elixir $P/standalone_script/standalone_script.ex >$O/standalone_script.log 2>&1; echo "standalone_script rc=$?"
rec(){ n=$1; mod=$2
  cd $P/$n || return
  export MIX_ENV=test MIX_BUILD_ROOT=$S/build/mix/$n
  mix compile >$O/$n.compile.log 2>&1 || { echo "$n compile FAIL"; return; }
  rm -rf $O/$n
  timeout 600 $B record --out-dir $O/$n -- mix run --no-compile -e "$mod.main()" >$O/$n.log 2>&1; echo "$n rc=$?"
}
rec basic_mix_app BasicMixApp
rec canonical_flow CanonicalFlow
rec constructs_core ConstructsCore
rec exception_flow ExceptionFlow
rec macro_locations MacroLocations
rec otp_agent OtpAgent
rec otp_application OtpApplication
rec otp_ets OtpEts
rec otp_genserver OtpGenServer
rec otp_supervisor OtpSupervisor
rec otp_task OtpTask
rec protocol_macro_behaviour ProtocolMacroBehaviour
rec reference_edges ReferenceEdges
rec task_messages TaskMessages
rec values_comprehensions ValuesComprehensions

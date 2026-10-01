#!/usr/bin/env bash
# Sway / FuelVM corpus: the fuel recorder's own test-programs plus a selection of
# script-type e2e tests from the workspace `sway` checkout (Forc.toml rewritten to
# use forc's implicit std instead of in-tree path deps). Projects are copied into
# corpus-src so forc never writes into a repo.
set -uo pipefail
F=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-fuel-recorder
SWAY=${WS:?set WS to the workspace root}/sway/test/src/e2e_vm_tests/test_programs/should_pass/language
BIN=$R/target/debug/codetracer-fuel-recorder    # prebuilt
export PATH=/nix/store/2jqxiciw19sqk0mlsldlm87babagp2b4-forc-0.70.3/bin:$PATH   # forc 0.70.3
SRC=$F/corpus-src/sway-fuel/projects; OUT=$F/corpus/sway-fuel
echo "recorder revision: $(git -C $R rev-parse HEAD)"
echo "sway e2e source revision: $(git -C ${WS:?set WS to the workspace root}/sway rev-parse HEAD)"
rm -rf $SRC; mkdir -p $SRC $OUT
for p in flow_test script_arith simple_trivial_chain trivial_chain_noinline; do
  cp -r $R/test-programs/$p $SRC/; rm -rf $SRC/$p/out
done
for p in nested_structs for_loops match_expressions_enums break_and_continue generic_traits \
         enum_instantiation ops supertraits b256_bitwise_ops many_stack_variables type_alias \
         totalord args_on_stack const_generics logging diverging_exprs struct_instantiation \
         match_expressions_all where_clause_methods generic_impl_self; do
  cp -r $SWAY/$p $SRC/sway_e2e_$p; d=$SRC/sway_e2e_$p
  rm -rf $d/out $d/Forc.lock $d/test.toml $d/json_abi_oracle*
  python3 - $d/Forc.toml <<'PY'
import sys,re
p=sys.argv[1]; t=open(p).read()
t=re.sub(r'(?ms)^\[dependencies\].*?(?=^\[|\Z)','[dependencies]\n',t)
open(p,'w').write(t)
PY
done
for d in $SRC/*/; do
  n=$(basename $d); rm -rf $OUT/$n
  if "$BIN" record -o $OUT/$n $d > $SRC/../log-$n.txt 2>&1; then echo "ok   $n"; else echo "FAIL $n"; rm -rf $OUT/$n; fi
done
# larger hand-written program (corpus-src/sway-fuel/extra/algos)
cp -r $F/corpus-src/sway-fuel/extra/algos $SRC/extra_algos; rm -rf $OUT/extra_algos
"$BIN" record -o $OUT/extra_algos $SRC/extra_algos > $SRC/../log-extra_algos.txt 2>&1 && echo "ok   extra_algos" || echo "FAIL extra_algos"

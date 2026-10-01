#!/usr/bin/env bash
# EVM (Solidity) corpus. Recorder: codetracer-evm-recorder @ 7aeed58d4cc98aa12a189955a1abfc695bf44b15
# Toolchain: solc 0.8.33 + foundry 1.8.1 (anvil) from nix store.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-evm-recorder
export PATH=/nix/store/9wll5q2pbhs7mi371myrwrqsnws06xzy-solc-0.8.33/bin:/nix/store/nn2pfn8n4278y3gc59igijv11sifznmy-foundry-1.8.1/bin:$PATH
cd $S/corpus/solidity-evm
for f in $R/test-programs/*/*.sol $R/contracts/*.sol; do
  name=$(basename $(dirname $f))_$(basename $f .sol)
  [ "$(basename $(dirname $f))" = contracts ] && name=contracts_$(basename $f .sol)
  echo "== $name"
  $R/target/debug/codetracer-evm-recorder record -o $S/corpus/solidity-evm/$name "$f" > $S/corpus/solidity-evm/$name.log 2>&1 || echo "FAIL $name"
done
# Larger helper program (corpus-src/solidity-evm/SortAndHash.sol)
$R/target/debug/codetracer-evm-recorder record -o $S/corpus/solidity-evm/large_SortAndHash $S/corpus-src/solidity-evm/SortAndHash.sol > $S/corpus/solidity-evm/large_SortAndHash.log 2>&1 || echo FAIL large

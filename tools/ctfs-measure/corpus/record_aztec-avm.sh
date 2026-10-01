#!/usr/bin/env bash
# Aztec AVM runtime corpus. These recordings were NOT regenerated here: regenerating needs the
# browser bundle, avm.wasm, chromium and the Noir-tracer probe built in-repo (and the repo has
# another agent's uncommitted work). They are COPIED from the outputs the repo's own Justfile
# recipes already wrote under ~/.cache (work dirs named after the milestone recipes):
#   m27-browser  : `just verify-m27`   browser demo, Token transfer_in_public via avm.wasm (public AVM trace)
#   m32-worker   : `just verify-m32`   same runtime hosted in a Web Worker (public AVM trace)
#   m34-wallet   : `just verify-m34`   transfer_in_public through the dev wallet (public AVM trace)
#   m38          : `just m38-arms`     private function executed by the Noir/ACVM tracer (replay arm; Token.transfer)
#   m39          : `just m39-trace-arms` nested private call, joined private half (Parent)
#   m40          : `just verify-m40*`  both halves of one tx: private half + AVM public half
# Synthetic writer drives (m24 roundtrip/backpressure/equivalence, m25 oq4/rung arms, m26 oq7,
# m41 writer containers) and every mutant/control/corrupt container were excluded.
set -eu
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/aztec-avm-runtime
C=$HOME/.cache
OUT=$S/corpus/aztec-avm; mkdir -p $OUT
echo "aztec-avm-runtime HEAD now: $(git -C $R rev-parse HEAD)"
cp(){ command cp -v "$C/$1" "$OUT/$2"; echo "  src mtime $(stat -c %y "$C/$1" | cut -c1-19); repo HEAD at that time: $(git -C $R log -1 --format=%H --before="$(stat -c %y "$C/$1" | cut -c1-19)")"; }
cp aztec-m27-browser/downloads/aztec-avm-01949fcc-7d92-7e9c-8000-000000002701.ct        public_m27_browser_token_transfer.ct
cp aztec-m32-worker/downloads/aztec-avm-worker-01949fcc-7d92-7e9c-8000-000000003201.ct  public_m32_worker.ct
cp aztec-m34-wallet/downloads/subject/aztec-avm-wallet-01949fcc-7d92-7e9c-8000-000000003401.ct public_m34_wallet_transfer.ct
cp aztec-m40-transaction/downloads/bothHalves/aztec-avm-public-half-01949fcc-7d92-7e9c-8000-000000004001.ct public_m40_tx_public_half.ct
cp aztec-m40-transaction/downloads/bothHalves/aztec-private-half-01949fcc-7d92-7e9c-8000-000000004002.ct private_m40_tx_private_half.ct
cp aztec-m40-trace/arms/privateHalf/Parent.ct          private_m40_privateHalf_Parent.ct
cp aztec-m39-trace/arms/transaction/Parent.ct          private_m39_nested_transaction_Parent.ct
cp aztec-m39-trace/arms/parentOnly/Parent.ct           private_m39_parentOnly_Parent.ct
cp aztec-m38-private-trace/arms/transfer/transfer.ct   private_m38_token_transfer.ct
cp aztec-m38-private-trace/arms/replay/private_function.ct private_m38_replay_private_function.ct

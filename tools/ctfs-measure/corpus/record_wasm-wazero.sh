#!/usr/bin/env bash
# wasm-wazero corpus: codetracer-wasm-recorder (wazero fork) built out-of-tree.
# Recorder revision: 78e7b49c61c60592c508b0ad5c55629f9bf27af6
# FFI writer: libcodetracer_trace_writer.a copied from codetracer-evm-recorder's cargo build output
#   (codetracer_trace_writer_nim-2759e4c6e8ff495e/out, built 2026-09-30 19:23 from the evm recorder's pinned trace-format rev).
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-wasm-recorder
RC=$HOME/.rustup/toolchains/1.89.0-x86_64-unknown-linux-gnu/bin/rustc
Z=/nix/store/13id30w3rvgj24nnz34f7qrncz48zd7l-zstd-1.5.7/lib
mkdir -p $S/corpus-src/wasm-wazero/ffi $S/corpus-src/wasm-wazero/wasm $S/corpus/wasm-wazero
cp ${WS:?set WS to the workspace root}/codetracer-evm-recorder/target/debug/build/codetracer_trace_writer_nim-2759e4c6e8ff495e/out/libcodetracer_trace_writer.a $S/corpus-src/wasm-wazero/ffi/ 2>/dev/null || true
if [ ! -x $S/build/wazero ]; then
  (cd $R && GOFLAGS=-buildvcs=false CGO_ENABLED=1 GOCACHE=$S/build/go/cache GOMODCACHE=$S/build/go/mod GOPATH=$S/build/go \
     CGO_LDFLAGS="-L$S/corpus-src/wasm-wazero/ffi -L$Z -Wl,-rpath,$Z" go build -o $S/build/wazero ./cmd/wazero)
fi
cd $S/corpus-src/wasm-wazero
for f in $R/test_code/*.rs $R/cmd/wazero/testdata/recorder-golden/*.rs $S/corpus-src/wasm-common/realistic.rs $S/corpus-src/wasm-common/realistic_noparse.rs $S/corpus-src/wasm-common/part_*.rs; do
  n=$(basename $f .rs)
  $RC -g -C debuginfo=2 -C opt-level=0 --edition 2021 --target wasm32-wasip1 -o wasm/$n.wasm $f 2>/dev/null || { echo "compile FAIL $n"; continue; }
  rm -rf $S/corpus/wasm-wazero/$n
  timeout 300 $S/build/wazero run -out-dir $S/corpus/wasm-wazero/$n wasm/$n.wasm >/dev/null 2>wasm/$n.stderr; echo "$n rc=$?"
done

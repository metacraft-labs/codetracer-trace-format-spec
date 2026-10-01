#!/usr/bin/env bash
# Reproduces measurements/2026-10-format-efficiency.md from a corpus of .ct
# recordings (see corpus/build_corpus.sh).
#
#   run_measurements.sh CORPUS_DIR OUT_DIR
#
# Needs: cargo (native), a Rust toolchain with the wasm32-wasip1 target
# (WASM_CARGO / WASM_RUSTC, default: rustup's 1.89.0), node >= 20.
set -euo pipefail
CORPUS=$(realpath "${1:?corpus dir}")
OUT=$(realpath -m "${2:?out dir}")
HERE=$(cd "$(dirname "$0")" && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$OUT/target}
TC=${WASM_TOOLCHAIN:-$HOME/.rustup/toolchains/1.89.0-x86_64-unknown-linux-gnu/bin}
REPEATS=${REPEATS:-3}
mkdir -p "$OUT"

(cd "$HERE" && cargo build --release --offline --bins --examples)
(cd "$HERE" && RUSTC=$TC/rustc "$TC/cargo" build --release --offline \
  --target wasm32-wasip1 --no-default-features --bin decode-bench)
BIN=$CARGO_TARGET_DIR/release
WASM=$CARGO_TARGET_DIR/wasm32-wasip1/release/decode-bench.wasm

find "$CORPUS" -name '*.ct' -size +0 | sort > "$OUT/corpus.list"
"$BIN/ctfs-measure" analyze --out "$OUT/analyze" --root "$CORPUS" --list "$OUT/corpus.list" \
  2> "$OUT/analyze.log"

# Decode speed: every rule of a trace is timed back to back, and the whole
# pass is repeated; report.py keeps the best of the repeats per (trace, rule).
for r in $(seq 1 "$REPEATS"); do
  MIN_RECORDS=2000 BENCH_MS=40 "$BIN/decode-bench" "$OUT/analyze/packs" > "$OUT/decode.native.$r.tsv"
  MIN_RECORDS=2000 BENCH_MS=40 node --no-warnings "$HERE/run_wasi.mjs" "$WASM" --dir "$OUT/analyze" -- \
    "$OUT/analyze/packs" > "$OUT/decode.wasm.$r.tsv"
done

# step-map.ns: full load (what the db-backend does at open) and one cold
# lookup, v1 against packed, natively and in WASM; then the chunk-target sweep
# and the per-candidate latency table on the largest maps.
BENCH_MS=60 "$BIN/decode-bench" stepmap "$OUT/analyze/stepmaps" > "$OUT/stepmap.native.tsv"
BENCH_MS=60 node --no-warnings "$HERE/run_wasi.mjs" "$WASM" --dir "$OUT/analyze" -- \
  stepmap "$OUT/analyze/stepmaps" > "$OUT/stepmap.wasm.tsv"
"$BIN/examples/stepmap_sweep" "$OUT/analyze/stepmaps" > "$OUT/stepmap_sweep.txt"
"$BIN/examples/event_kinds" "$CORPUS" "$OUT/corpus.list" > "$OUT/event_kinds.md"
# Durability: the cost of publishing every sealed chunk (ctfs-container.md
# §6). Needs a directory on a real file system (not tmpfs): DURABILITY_DIR.
"$BIN/examples/durability" "$CORPUS" "$OUT/corpus.list" "${DURABILITY_DIR:-$OUT}" > "$OUT/durability.md"
# (awk, not head: under pipefail, head closing the pipe early fails the sort.)
sort -t$'\t' -k3 -n -r "$OUT/analyze/stepmap.tsv" | awk -F'\t' '$2=="v1" && n++ < 6 {print $1}' \
  | sed "s|^|$CORPUS/|" > "$OUT/stepmap_latency.list"
"$BIN/ctfs-measure" stepmap-latency --out "$OUT/latency" --root "$CORPUS" --list "$OUT/stepmap_latency.list" \
  2> "$OUT/latency.log"

python3 "$HERE/report.py" "$OUT" > "$OUT/report.md"
echo "wrote $OUT/report.md"

#!/usr/bin/env bash
# Re-creates corpus/bash/*.ct with the codetracer-shell-recorders bash recorder.
#
# Recorder: ${WS:?set WS to the workspace root}/codetracer-shell-recorders
#   revision used: 8779f82776ac4b2f06057c63bc4f3b850a7e195b (checked below, warning only)
# Trace writer: ct-shell-trace-writer. The recorder repo's existing debug build
#   (codetracer-shell-recorders/target/debug/ct-shell-trace-writer, built
#   2026-09-30 from the revision above; sha256 7bda6e28...567f) was copied to
#   fmt-eff/build/shell/bin/ and is put first on PATH so the launcher picks it.
#   It was not rebuilt because the build compiles the sibling
#   codetracer-trace-format-nim repo in place. To rebuild instead:
#     CARGO_TARGET_DIR=$FMT/build/shell/cargo cargo build --release \
#       --manifest-path $REC/Cargo.toml && cp $FMT/build/shell/cargo/release/ct-shell-trace-writer $FMT/build/shell/bin/
# Interpreter: bash from PATH (GNU bash, version 5.3.9(1)-release (x86_64-pc-linux-gnu)).
# Programs: recorder fixtures (tests/fixtures/...) referenced in place, plus
#   hand-written workloads in corpus-src/bash/*.sh.
# NOTE: the shell recorders are slow (~20-50 recorded steps/s on a loaded
#   machine, they run `declare -p`/`typeset` on every step), so the larger
#   workloads are sized at 1k-17k steps. Recordings run in parallel (-P).
set -euo pipefail
FMT=${FMT_EFF:?set FMT_EFF to the measurement work directory}
REC=${WS:?set WS to the workspace root}/codetracer-shell-recorders
SRC=$FMT/corpus-src/bash
OUT=$FMT/corpus/bash
WORK=$FMT/build/shell/work-bash
JOBS=${JOBS:-8}
want=8779f82776ac4b2f06057c63bc4f3b850a7e195b
have=$(git -C "$REC" rev-parse HEAD)
[ "$have" = "$want" ] || echo "warning: recorder at $have, corpus was made at $want" >&2
export PATH=$FMT/build/shell/bin:$PATH
rm -rf "$OUT" "$WORK"; mkdir -p "$OUT" "$WORK"

# label<TAB>script
list() {
    for f in comprehensive errors functions multiline nested_functions output simple variables with_source; do
        printf 'fixture_%s\t%s\n' "$f" "$REC/tests/fixtures/bash/$f.sh"
    done
    printf 'fixture_cross_equivalent\t%s\n' "$REC/tests/fixtures/cross_shell/equivalent.sh"
    for f in "$SRC"/*.sh; do printf '%s\t%s\n' "$(basename "$f" .sh)" "$f"; done
}
LAUNCH=$REC/bash-recorder/codetracer-bash-recorder
record_one() {
    local label=$1 script=$2 dir=$WORK/$1
    mkdir -p "$dir"
    local t0=$SECONDS
    if (cd "$dir" && "$LAUNCH" --out-dir "$dir/out" -- "$script" >"$dir/stdout.txt" 2>"$dir/stderr.txt"); then :; else
        echo "note: $label exited non-zero (expected for error fixtures)" >&2
    fi
    local ct
    ct=$(ls "$dir"/out/*.ct 2>/dev/null | head -1 || true)
    if [ -n "$ct" ]; then cp "$ct" "$OUT/$label.ct"; echo "ok   $label ($((SECONDS - t0))s)"; else echo "FAIL $label (no .ct)"; fi
}
export -f record_one; export WORK OUT LAUNCH
list | xargs -P "$JOBS" -L1 bash -c 'record_one "$0" "$1"'
ls -la "$OUT"
"$FMT/target/release/ctfs-measure" analyze --out "$FMT/check-bash" "$OUT"/*.ct

#!/usr/bin/env bash
# Re-creates corpus/zsh/*.ct with the codetracer-shell-recorders zsh recorder.
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
# Interpreter: zsh from PATH (zsh 5.9.1 (x86_64-pc-linux-gnu)).
# Programs: recorder fixtures (tests/fixtures/...) referenced in place, plus
#   hand-written workloads in corpus-src/zsh/*.zsh.
# NOTE: the shell recorders are slow (~20-50 recorded steps/s on a loaded
#   machine, they run `declare -p`/`typeset` on every step), so the larger
#   workloads are sized at 1k-17k steps. Recordings run in parallel (-P).
# NOTE: do not use `setopt KSH_ARRAYS` in zsh programs: the recorder indexes
#   funcsourcetrace[1] and silently drops top-level steps under 0-based arrays.
set -euo pipefail
FMT=${FMT_EFF:?set FMT_EFF to the measurement work directory}
REC=${WS:?set WS to the workspace root}/codetracer-shell-recorders
SRC=$FMT/corpus-src/zsh
OUT=$FMT/corpus/zsh
WORK=$FMT/build/shell/work-zsh
JOBS=${JOBS:-8}
want=8779f82776ac4b2f06057c63bc4f3b850a7e195b
have=$(git -C "$REC" rev-parse HEAD)
[ "$have" = "$want" ] || echo "warning: recorder at $have, corpus was made at $want" >&2
export PATH=$FMT/build/shell/bin:$PATH
rm -rf "$OUT" "$WORK"; mkdir -p "$OUT" "$WORK"

# label<TAB>script
list() {
    for f in comprehensive errors functions output simple variables with_source; do
        printf 'fixture_%s\t%s\n' "$f" "$REC/tests/fixtures/zsh/$f.zsh"
    done
    printf 'fixture_cross_equivalent\t%s\n' "$REC/tests/fixtures/cross_shell/equivalent.zsh"
    for f in "$SRC"/*.zsh; do printf '%s\t%s\n' "$(basename "$f" .zsh)" "$f"; done
}
LAUNCH=$REC/zsh-recorder/codetracer-zsh-recorder
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
"$FMT/target/release/ctfs-measure" analyze --out "$FMT/check-zsh" "$OUT"/*.ct

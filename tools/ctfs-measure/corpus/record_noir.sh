#!/usr/bin/env bash
# Records every Noir program under noir/test_programs/execution_success with
# `nargo trace` (one .ct per program). Usage: record_noir.sh WORKSPACE OUT_DIR
set -u
WS=${1:?workspace}; OUT=${2:?out dir}
NARGO=${NARGO:-$WS/noir/target/release/nargo}
mkdir -p "$OUT"
echo "noir revision: $(git -C "$WS/noir" rev-parse HEAD)" > "$OUT/REVISION"
echo "nargo: $NARGO ($($NARGO --version | tr '\n' ' '))" >> "$OUT/REVISION"
cd "$WS/noir/test_programs/execution_success"
ls -d */ | tr -d / | xargs -P 8 -I{} sh -c '
  cd "{}" && timeout 300 "'"$NARGO"'" trace --silence-warnings --out-dir "'"$OUT"'/{}.d" >/dev/null 2>"'"$OUT"'/{}.err" \
   && mv "'"$OUT"'/{}.d/"*.ct "'"$OUT"'/{}.ct" && rm -rf "'"$OUT"'/{}.d" "'"$OUT"'/{}.err"'

#!/usr/bin/env bash
# Rebuilds the recording corpus behind measurements/2026-10-format-efficiency.md.
#
#   FMT_EFF=<work dir> WS=<workspace root> build_corpus.sh [language...]
#
# The recordings land in $FMT_EFF/corpus/<language>/*.ct. Each record_<language>.sh
# records one language with that recorder's own test programs and examples
# plus the programs under src/<language>/, using the recorder build already
# present in the workspace; each prints the recorder revision it used.
# MANIFEST.tsv lists the recordings the report was computed from, with their
# sizes and exec-record counts; a rebuild with other recorder revisions will
# differ in detail.
#
# The scripts were written against a Linux workstation with the workspace's
# recorders built; some need toolchains of their own (Godot, Elixir, Sui, ...)
# and say so at the top.
set -euo pipefail
: "${FMT_EFF:?set FMT_EFF to the measurement work directory}"
: "${WS:?set WS to the workspace root}"
export FMT_EFF WS
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$FMT_EFF/corpus-src" "$FMT_EFF/corpus"
cp -r "$HERE/src/." "$FMT_EFF/corpus-src/"

if [ $# -gt 0 ]; then
  langs=("$@")
else
  langs=()
  for s in "$HERE"/record_*.sh; do
    l=$(basename "$s" .sh)
    langs+=("${l#record_}")
  done
fi

for l in "${langs[@]}"; do
  echo "== $l"
  if [ "$l" = noir ]; then
    bash "$HERE/record_noir.sh" "$WS" "$FMT_EFF/corpus/noir"
  else
    bash "$HERE/record_$l.sh"
  fi
done

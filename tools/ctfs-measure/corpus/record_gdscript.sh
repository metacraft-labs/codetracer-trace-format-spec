#!/usr/bin/env bash
# GDScript corpus via the CodeTracer Godot fork (patched godot 4.6.2).
# Engine binary: prebuilt template_debug from a clone of codetracer-engine-godot at
#   e4bf7384d01eadd89ec78cf18bdc63814320cf81 (parent of workspace HEAD
#   352a996cdf617539e115b48ad757cf38ca1b9c69; the only diff is one script file).
# Programs: codetracer-engine-godot/test-programs/{gdscript,gdh0,mt14} at workspace HEAD.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
REPO=${WS:?set WS to the workspace root}/codetracer-engine-godot
BIN=${BIN:-/home/zahary/.cache/claude-rec2-tmp/n5/godot/bin/godot.linuxbsd.template_debug.x86_64}
WORK=$S/corpus-src/gdscript/work
OUT=$S/corpus/gdscript
echo "engine-repo HEAD: $(git -C $REPO rev-parse HEAD)"
echo "binary built from: $(git -C $(dirname $(dirname $BIN)) rev-parse HEAD)"
mkdir -p "$WORK" "$OUT"
rec() { # name entry [extra...]
  local name=$1 entry=$2; shift 2
  local proj=$WORK/$name; rm -rf "$proj"; mkdir -p "$proj/trace"
  cp "$REPO/test-programs/gdscript/$entry" "$proj/"
  for x in "$@"; do cp "$REPO/test-programs/gdscript/$x" "$proj/"; done
  printf 'config_version=5\n[application]\nconfig/name="ctcorpus-%s"\n' "$name" > "$proj/project.godot"
  CT_GDSCRIPT_TRACE="$proj/trace" timeout 120 "$BIN" --headless --path "$proj" --script "res://$entry" >"$proj/stdout.log" 2>&1 || true
  if [ -f "$proj/trace/gdscript_trace.ct" ]; then cp "$proj/trace/gdscript_trace.ct" "$OUT/$name.ct"; echo "ok $name"; else echo "FAIL $name"; fi
}
for p in g2probe gf_calls gf_values gf_typing gf_control_flow gf_collections gf_variant_types gf_functions gf_lambdas gf_props gf_signals gf_coroutine gf_threads gf_diag gf_diag_assert_fail n1_nested; do rec $p $p.gd; done
rec gf_zoo gf_zoo.gd gf_animal.gd gf_dog.gd
rec gf_node_main gf_node_main.gd gf_node.gd
# Larger realistic program authored for the corpus (corpus-src/gdscript/programs/big_sim.gd)
proj=$WORK/big_sim; rm -rf "$proj"; mkdir -p "$proj/trace"
cp $S/corpus-src/gdscript/programs/big_sim.gd "$proj/"
printf 'config_version=5\n[application]\nconfig/name="ctcorpus-big_sim"\n' > "$proj/project.godot"
CT_GDSCRIPT_TRACE="$proj/trace" timeout 300 "$BIN" --headless --path "$proj" --script "res://big_sim.gd" >"$proj/stdout.log" 2>&1 || true
[ -f "$proj/trace/gdscript_trace.ct" ] && cp "$proj/trace/gdscript_trace.ct" "$OUT/big_sim.ct" && echo "ok big_sim"

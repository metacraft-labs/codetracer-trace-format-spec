#!/usr/bin/env bash
# Solana corpus. Recorder: codetracer-solana-recorder @ e6e401e19abb5a68a60f0d2b773d81e8b704160e (prebuilt target/debug binary)
# Programs are the recorder's plain-Rust test programs (zero-arg entry fns), compiled with
# cargo-build-sbf 3.1.11 (nix store; platform tools cached in ~/.cache/solana) inside a scratch
# cdylib crate that include!s the file -- same scheme as tests/test_source_level_fidelity.rs.
set -u
S=${FMT_EFF:?set FMT_EFF to the measurement work directory}
R=${WS:?set WS to the workspace root}/codetracer-solana-recorder
SBF=/nix/store/56v7fiklx64fd92kjvq1kv9k3inb6ab8-cargo-build-sbf-3.1.11/bin/cargo-build-sbf
build_and_record() { # name src entry
  local name=$1 src=$2 entry=$3 W=$S/corpus-src/solana/build/$1
  mkdir -p $W/src
  printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n\n[lib]\ncrate-type = ["cdylib"]\npath = "src/lib.rs"\n\n[dependencies]\n\n[profile.release]\nopt-level = 0\ndebug = true\nstrip = "none"\nlto = "off"\noverflow-checks = true\n\n[workspace]\n' "$name" > $W/Cargo.toml
  # Copy the source verbatim (keeps line numbers and inner doc comments valid) and append an SBF entrypoint.
  { cat "$src"; printf '\n#[no_mangle]\npub extern "C" fn entrypoint(_input: *mut u8) -> u64 {\n    core::hint::black_box(%s());\n    0\n}\n' "$entry"; } > $W/src/lib.rs
  if ! CARGO_TARGET_DIR=$W/target $SBF --manifest-path $W/Cargo.toml > $W/build.log 2>&1; then echo "BUILD FAIL $name"; return; fi
  $R/target/debug/codetracer-solana-recorder record $W/target/sbpf-solana-solana/release/$name.so -o $S/corpus/solana/$name > $S/corpus/solana/$name.log 2>&1 || echo "RECORD FAIL $name"
}
mkdir -p $S/corpus/solana; cd $S/corpus/solana
build_and_record simple_trivial_chain $R/test-programs/source-fidelity/simple_trivial_chain/main.rs main
for f in $R/test-programs/solana/*.rs; do
  n=$(basename $f .rs)
  if grep -q '^pub fn process_instruction()\|^fn process_instruction()' $f; then e=process_instruction
  elif grep -q '^pub fn compute()\|^fn compute()' $f; then e=compute
  elif grep -q '^pub fn process_transfer()' $f; then e=process_transfer
  else echo "SKIP $n (no zero-arg entry fn)"; continue; fi
  build_and_record $n $f $e
done
build_and_record large_workload $S/corpus-src/solana/large_workload.rs compute

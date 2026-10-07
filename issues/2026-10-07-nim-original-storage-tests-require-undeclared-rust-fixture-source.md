# Original Nim storage tests require an undeclared Rust fixture source

## Measured source

At owning Nim `ea955fafa9c7a8a9014b6fba3b19d8e198c5e95a` plus six private reviewed Zstd/FFI harness postimages, original `just test` exited1 naturally after five unchanged `test_trace_storage_config.nim` cases could not locate `storage_config.*` and `manifest.*` fixtures. Source/index guards remained true. Selected compiler was actual Nix Nim2.2.8 (`ih7dxz...`) and GCC14.3 (`8v97...`), as bound in the raw tool receipt. This is a missing private source closure, not a measured CI failure or a reader defect.

`fixturePath` at lines7–15 searches a parent sibling `codetracer-trace-format/codetracer_ctfs/tests/fixtures/trace_storage`. The owning `repro.lock` contains only its self7e dependency, and `.github/sibling-repos` contains comments only. A read-only refreshed published `agents` source at `ff17c78104d6255df16ef78d23e544a83af796df` retains the same lookup, comment-only sibling declaration and self-only lock. Frozen owning source/pins were not advanced. Open and deleted issue history was searched; no exact prior record was found.

## Expected

The [owning named-hook seam](../nim-named-standard-hook-seam.md) requires original complete source-qualified native tests and preserves their fixture oracles. This source dependency must be available for those original cases to execute. The exact automatic acquisition/declaration mechanism is **not specified here. Proposed:** declare and qualify a committed shared Rust source closure instead of relying on an incidental parent checkout; preserve every original assertion and fixture byte.

## Evidence and scope

The raw RED run is `/tmp/promotion-campaign/traceea-guardian-private-fqcsd5sc/complete-original-native-proof.json` and its stdout/stderr. An independently reviewed private acquisition subsequently supplied complete canonical frozen Rust `f39e71016715b4ba67b626f4c971acfd6a766977`: all211 committed file/link payloads and modes, including eight exact fixture blobs, were verified against Git objects. This is **not** a Rust revision present in the owning Nim lock. The repaired-closure original full native suite and lint then exited0 with unchanged private/owning source guards.

No loose regenerated fixtures, expectation changes, production source edits, consumer pin transition, native Darwin qualification or whole CI claim follows from this private closure repair.

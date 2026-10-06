# Nim bulk position range arithmetic can prevent forward progress

|             |                                                                        |
| ----------- | ---------------------------------------------------------------------- |
| Status      | open; source-qualified risk, native fault reproduction not executed     |
| Recorded    | 2026-10-06                                                             |
| Observed in | codetracer-trace-format-nim @ 19cad035585cbc0845b84ddc11aa3b824825dfaa     |
| Area        | src/codetracer_trace_writer/new_trace_reader.nim; bulk position accessor |

## Observed

Read-only inspection of `stepAbsoluteGlobalLineIndices` at `19cad035` shows `endN = min(startN + count, totalEvents)`, then unsigned `want = endN - startN` and `stopN = startN + writable`. The `startN + count` sum is not admitted before clamping.

For the modular unsigned example `totalEvents = 2`, `startN = 1`, `count = high(uint64)` and an output array of length 2, the expression can produce `endN = 0`, `want = high(uint64)`, `writable = 2` and `stopN = 3`. This differs from the documented one remaining output entry. If the final short chunk and the next requested index still share a chunk index, the loop re-reads that chunk and repeatedly sets `n` to the same value below `stopN`. The existing zero-length defense does not admit this nonempty, nonprogressing case.

This is an inferred source arithmetic/control-flow risk, not a measured native hang. No potentially nonterminating fault was launched, no timeout/cancellation acceptance is claimed, and no source/test/pin was changed.

## Expected

Not specified. Proposed: give the bulk range API an explicit overflow-safe request contract, bounded by `totalEvents - startN` before addition, and require forward progress or a named malformed-boundary error. Preserve the currently documented count expression `min(count, total_events - startN, output.len)` and all existing valid output values.

The present public function comment already promises that count expression. No owning format specification at `d9c355f519fb0b950643b94d61bfff6f50832534` was found to decide overflow handling or a process-liveness bound for this accessor. This proposed completion must not be represented as an already normative format rule. The existing per-step/bulk performance comparison is a separate measured consumer issue and is not a causal control for this arithmetic boundary.

## Evidence

- Source: `src/codetracer_trace_writer/new_trace_reader.nim`, function starting at line 1417 at `19cad035`; SHA-256 `0352de947be1d1a49ca3c6918197a6095c4824edb2ccabf6b30a24f93415d963`.
- `git diff HEAD -- src/codetracer_trace_writer/new_trace_reader.nim` was empty at the source measurement; the approved constructor staging changes are unrelated.
- `stepAbsoluteGlobalLineIndex` at line 1271 uses the persistent `gliChunk`/`gliCache`; the bulk helper uses local events/positions. This is source mapping only, not performance attribution.
- Before filing, scoped explicit-766 mainline sync and actual upstream `latest` both named `d9c355f5`. Open specifications/issues were searched for the accessor, bulk range/overflow and global-line-index terms. Deleted issue history was searched with case-insensitive pickaxe/regex for the same terms; the existing root/header overflow records concern different boundaries.

A repair requires a reviewed owning contract and genuine bounded controls for zero counts, oversized counts, maximum unsigned counts, output truncation, final short chunks, cross-chunk reads and malformed nonprogressing chunks, alongside the full original suite. Preserve valid reader compatibility, original performance thresholds and consumer source pins.

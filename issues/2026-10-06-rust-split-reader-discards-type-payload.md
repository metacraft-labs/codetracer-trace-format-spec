# Rust split reader discards structured type payloads

| | |
|---|---|
| Status | in-progress; private source/controls pending |
| Recorded | 2026-10-06 |
| Observed in | codetracer-trace-format @ f39e71016715b4ba67b626f4c971acfd6a766977 |
| Area | shared finalized CTFS reader |

## Observed

codetracer_trace_reader::split_stream_reader decodes a complete type record but builds a DTO with specific_info=None; it also maps unknown type kinds to Raw and uses lossy UTF-8 conversions. These exact f39 source branches are observed; full-field private runtime fixtures are pending.

## Expected

[trace-events.md](../trace-events.md), TypeRecord, specifies Struct field names/type IDs and Pointer dereference type IDs. Preserve every representable declared field or expose a named unsupported result; do not silently replace it.

## Evidence

Exact source mapping and hashes: `/tmp/promotion-campaign/shared-rust-ctfs-reader-essential-seam-proposed/source-binding.json`. Owning specs synchronized at 1bad99e30ee57176515475985e7cbd08d89901ab; open issue/source and deleted-issue history searches found no competing record. Source inspection is distinct from runtime qualification. Frozen consumer pins remain unchanged.

## Suggested direction

Bridge complete structured types and strict target-DTO text; qualify forward/self/mutual references and all-field event/value fixtures.

## Related

[rust-checked-finalized-reader-seam.md](../rust-checked-finalized-reader-seam.md). Container version 6 remains a separate unsupported capability. Existing unknown self-delimiting value tags >=10 bounded-skip/warning policy is preserved; explicit completeness reporting is a new interface, not a defect in that policy.

# Rust format detection masks present marker failures

| | |
|---|---|
| Status | in-progress; private source/controls pending |
| Recorded | 2026-10-06 |
| Observed in | codetracer-trace-format @ f39e71016715b4ba67b626f4c971acfd6a766977 |
| Area | shared finalized CTFS reader |

## Observed

codetracer_trace_reader::ctfs_reader::detect_format converts read_file errors to legacy CBOR selection and maps unknown present events.fmt bytes to CBOR. This is an f39 source observation; present-member corruption controls are pending.

## Expected

Not specified explicitly for events.fmt in the current published format documents. Proposed: [rust-checked-finalized-reader-seam.md](../rust-checked-finalized-reader-seam.md) distinguishes genuine marker absence from present invalid marker data. Preserve legacy selection for actual absence and report malformed present markers; this proposal does not claim an existing normative marker rule.

## Evidence

Exact source mapping and hashes: `/tmp/promotion-campaign/shared-rust-ctfs-reader-essential-seam-proposed/source-binding.json`. Owning specs synchronized at 1bad99e30ee57176515475985e7cbd08d89901ab; open issue/source and deleted-issue history searches found no competing record. Source inspection is distinct from runtime qualification. Frozen consumer pins remain unchanged.

## Suggested direction

Separate FileNotFound from malformed member errors and reject unknown present markers with an explicit error.

## Related

[rust-checked-finalized-reader-seam.md](../rust-checked-finalized-reader-seam.md). Container version 6 remains a separate unsupported capability. Existing unknown self-delimiting value tags >=10 bounded-skip/warning policy is preserved; explicit completeness reporting is a new interface, not a defect in that policy.

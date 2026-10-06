# Rust reader API lacks an immutable metadata/events entry

| | |
|---|---|
| Status | in-progress; private source/controls pending |
| Recorded | 2026-10-06 |
| Observed in | codetracer-trace-format @ f39e71016715b4ba67b626f4c971acfd6a766977 |
| Area | shared finalized CTFS reader |

## Observed

The public trace entry opens a CTFS path internally while callers currently read metadata separately. A File handle alone remains mutable. The path-reopen gap is grounded at f39; no changed-image runtime result is claimed yet.

## Expected

Not specified. Proposed: [rust-checked-finalized-reader-seam.md](../rust-checked-finalized-reader-seam.md) requires an owned byte snapshot and reader-taking entry so a finalized caller binds metadata and events to one immutable image while preserving existing path APIs.

## Evidence

Exact source mapping and hashes: `/tmp/promotion-campaign/shared-rust-ctfs-reader-essential-seam-proposed/source-binding.json`. Owning specs synchronized at 1bad99e30ee57176515475985e7cbd08d89901ab; open issue/source and deleted-issue history searches found no competing record. Source inspection is distinct from runtime qualification. Frozen consumer pins remain unchanged.

## Suggested direction

Add nongeneric CtfsReader::from_bytes and read_trace_from_reader; retain live reader semantics separately and disclose checked allocation limits.

## Related

[rust-checked-finalized-reader-seam.md](../rust-checked-finalized-reader-seam.md). Container version 6 remains a separate unsupported capability. Existing unknown self-delimiting value tags >=10 bounded-skip/warning policy is preserved; explicit completeness reporting is a new interface, not a defect in that policy.

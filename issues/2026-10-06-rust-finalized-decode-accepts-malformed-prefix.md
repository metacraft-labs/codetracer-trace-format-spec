# Rust finalized decoding can accept a malformed stream prefix

| | |
|---|---|
| Status | in-progress; private source/controls pending |
| Recorded | 2026-10-06 |
| Observed in | codetracer-trace-format @ f39e71016715b4ba67b626f4c971acfd6a766977 |
| Area | shared finalized CTFS reader |

## Observed

codetracer_trace_writer::split_binary::decode_events stops at its first decode_event error and returns previously decoded events. codetracer_ctfs::chunked::decompress_all loops while a complete 16-byte header remains, leaving a shorter trailing member header unexamined. These are source observations at f39; concrete malformed fixtures and actual runtime refusal controls are pending.

## Expected

[trace-events.md](../trace-events.md) defines event framing; [seekable-zstd.md](../seekable-zstd.md) defines complete indexed chunks. A finalized complete-reader API must account for the complete member rather than report an invalid prefix as complete. Existing intentionally partial APIs must remain explicit.

## Evidence

Exact source mapping and hashes: `/tmp/promotion-campaign/shared-rust-ctfs-reader-essential-seam-proposed/source-binding.json`. Owning specs synchronized at 1bad99e30ee57176515475985e7cbd08d89901ab; open issue/source and deleted-issue history searches found no competing record. Source inspection is distinct from runtime qualification. Frozen consumer pins remain unchanged.

## Suggested direction

Add checked complete-buffer/frame decoding and use it in the finalized reader; preserve documented unchecked compatibility and distinguish container incomplete-block tolerance from member framing.

## Related

[rust-checked-finalized-reader-seam.md](../rust-checked-finalized-reader-seam.md). Container version 6 remains a separate unsupported capability. Existing unknown self-delimiting value tags >=10 bounded-skip/warning policy is preserved; explicit completeness reporting is a new interface, not a defect in that policy.

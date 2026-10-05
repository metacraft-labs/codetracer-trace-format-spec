# Nim CTFS Sharded Root Layout Repair

**Status:** ☐ Planned — reviewed design, implementation and qualification pending.

Owning issue: [sharded root entry packing](issues/2026-10-05-nim-sharded-root-entry-offset-omits-free-list-roots.md).

## Authority and measured defect

The owning `ctfs-container.md` Block 0 Layout and File Entries sections place version 2–5 entries after the 16-byte header and `R = 7 * maxShards * 6` reserved free-list bytes. At the owned clean source 68d32331d1b017ac5c2ab873d952c42b72558553, `rootBlockCount` includes R but `fileEntryOffset` does not. The published owning issue records genuine OLD7e/current7967 writer/importer roundtrip failure. This design repairs the owning writer boundary; it does not advance those consumer pins or promise version-6 writer support.

## Required behavior

Use one source-grounded full-profile root-entry start calculation for each already-supported declared header version. Current version-5 writer entries begin at `16 + 42 * maxShards`, with original 24-byte stride. Unsharded bytes remain identical. Writer creation, entry lookup and synchronization, header-based reader lookup, supported append/reopen capacity and analyzer directory traversal must agree on that start. Root region reservation and allocation must continue to place data outside the whole declared root region; reserved free-list bytes must remain untouched by directory updates. Reject malformed or unsupported layouts before out-of-bounds reads or writes. Do not relax readers to accept the old misplaced entries.

Preserve existing supported APIs, member forms, duplicate refusal, original count limits, streaming durability, compression and null/mapping semantics. Existing deliberate append restrictions must not silently expand: calculate their accepted capacity from the correct reserved prefix and retain their explicit refusal for unsupported overflow until separately specified. Analyzer results must count the same actual header/reserved/directory bytes rather than hide the shift.

## Genuine validation

Run all original owning tests unchanged. Add nonzero-shard in-memory and streaming writer/reader roundtrips with explicit byte oracles for the first entry at 58 for one shard, zero preserved reserved bytes, entries spanning reserved root blocks and original data-block exclusion. Include several shard counts and actual entry-boundary transitions; retain a byte-exact unsharded positive. Reopen/append a supported nonzero-shard file and verify existing plus newly appended member bytes and names through the canonical reader. Test real unsupported append overflow and malformed/truncated reserved-prefix refusal. Compare analyzer entry traversal and accounting to the serialized container. A deliberately misplaced entry or corrupted declared layout must fail the actual canonical oracle.

Bind source/compiler/tool identities and exact native command lines, naturally drain each owned process session and retain all failures. Cold typed execution must preserve automatic monitoring, real authoritative RunQuota, every original action and suite. Full hook/shipping/platform gates remain required. No mocks, emulation, timing widening, forced cache misses, producer pin advance, external-worker edits or automatic consumer dependency adoption.

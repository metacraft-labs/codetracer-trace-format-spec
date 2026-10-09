# Nim CTFS sharded root entries overwrite the reserved free-list roots

| | |
|---|---|
| Status | open |
| Recorded | 2026-10-05 |
| Observed in | codetracer-trace-format-nim @ 7e06d011 and 7967c179 |
| Area | codetracer_ctfs/types.nim: fileEntryOffset and rootBlockCount |

## Observed

A genuine native `createCtfs(blockSize=4096, maxRootEntries=200, maxShards=1)` container serializes the first `payload` entry at byte 16. Its header declares one shard and its reserved-root allocation correctly spans two blocks. The version-aware importer reads entries after the declared free-list roots, at byte 58, and reports `CTFS file not found: payload`. Both frozen OLD7e and current7967 diagnostic runs execute all nine test bodies: eight pass and this root-overflow case fails.

## Expected

`ctfs-container.md`, **Block 0 Layout** and **File Entries**, place entries after the free-list roots: `R = 7 * max_shards * 6`, so versions 2–5 begin entries at `16 + R`. Explicit root-entry counts may overflow block 0; the full reserved region is retained. One shard is not equivalent to an unsharded container.

## Evidence

`types.nim:fileEntryOffset` returns `HeaderSize + ExtHeaderSize + index * FileEntrySize`, omitting `R`. In the same source, `rootBlockCount` includes `R` in its reservation. Native diagnostic report: `/tmp/promotion-campaign/ui-ctfs-native-old-current-wv35f_er/report.json`; serialized fixture: `OLD7e-outputs/root-overflow.ct`. Compiler was the real Nim 2.2.8 diagnostic compiler, not a qualification of the owning UI compiler or original suite. Actual OLD7e and current7967 sources stayed unchanged; both child sessions drained naturally.

The first diagnostic attempt compiled successfully but ran no unittest bodies because input paths were interpreted as filters; it is not positive runtime evidence. The retained retry disables parameter filtering through the public unittest API and enforces the complete nine-body census.

## Suggested direction

Make writer root-entry packing agree with its header and reserved region, with genuine nonzero-shard and overflow round-trip/corruption tests. Do not relax readers to treat free-list roots as entries, restrict valid overflow directories to block 0, or advance frozen consumer dependencies implicitly.

## Related

Open and deleted issue history were searched after syncing this specification repo to `latest` at d67a578; there was no existing `issues/` record or matching historical entry.

## Decision (2026-10-08): the free list root area is removed

`ctfs-container.md` §1, "The free list root area is removed", records why: the area was designed on
2026-04-22 for a container-global, per-shard sub-block allocator; the allocator that was built keeps
its free lists inside each namespace, nothing reads or writes free-list state in block 0, and every
writer already places entries at the end of the header. `R` is `0` for every `max_shards`. Measured
against that text this defect inverts: `fileEntryOffset` (entries at 16) is correct, and
`rootBlockCount`, which still reserves `R`, over-reserves; so does `compact.nim`'s full-container
reader, which skips `R` (7 classes) before the entries. The repair is to drop `R` from both, not to
move the entries. The plan `nim-ctfs-sharded-root-layout-repair.md` needs restating on that basis;
its other requirements (one entry-start calculation for every path, refusal of malformed layouts,
"retain the explicit refusal for unsupported overflow") stand, and the last is now the rule
(`ctfs-container.md` §1, "The root directory is fixed at creation"). Scheduled in
`codetracer-specs` `milestones/CTFS-Keyed-Families.milestones.org` CKF-4N.

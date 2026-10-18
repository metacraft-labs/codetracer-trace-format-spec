# Nim CTFS Sharded Root Layout Repair

**Status:** ✓ Root layout repaired — `codetracer-trace-format-nim` b5b5fb4 (`agents`). The public
reader header admission section below is still planned.

## What changed, and why the first version of this plan was wrong

This plan was first written (2026-10-05) to move the Nim writer's entries after a free list root
area of `R = 7 * maxShards * 6` bytes, because `rootBlockCount` reserved `R` and `fileEntryOffset`
did not. On 2026-10-08 `ctfs-container.md` §1, "The free list root area is removed", made `R = 0`
for every `max_shards`: no implementation ever kept free-list state in block 0, and every writer
already put entry `i` at `16 + 24 * i`. Measured against that text the defect inverts, and the
repair is to drop `R` from the places that still counted it, not to move the entries.

## Required behavior (implemented in b5b5fb4)

- **One entry start for every path.** For a version-5 full container, entry `i` is at
  `16 + 24 * i` and `root_blocks = ceil((16 + MaxRootEntries * 24) / BlockSize)`, whatever
  `max_shards` says. `fileEntryOffset`, `rootBlockCount`, `growRootDirectory`'s grown entry count,
  the compact converter's full-container reader (`collectFullProfileMembers`, including auto-fill),
  the closed-container append path, `ct-space` and `tests/check_ctfs_container.nim` agree on it.
- **No reader accepts the old misplaced layout.** An entry written at `16 + R` is not found.
- **A sharded container is the unsharded container except byte 7**, in memory and streaming, below
  and above block 0's capacity, and after a growth.

Validation is in `tests/test_root_directory_overflow.nim`, `tests/test_root_directory_grows.nim` and
`tests/test_compact_container_layout.nim` (shard counts 1, 4, 16 and 255 at the 170/171 and 340/341
boundaries, a member in the last auto-filled slot, and a moved-entry negative control). Each new
test fails at e54d131 for the `R` reservation or skip, and the full `just test` passes at b5b5fb4.

Removing growth itself, and the readers in other repositories that still skip `R`, are not part of
this repair; they are `CTFS-Keyed-Families` CKF-8 and CKF-6.

## Public reader header admission before optional absence

The header-safe root lookup is retained exactly. `openNewTraceFromBytes` must first call the existing `ctfsVersionError` and return its named error for container versions outside the owning reader's supported set (version 5 only). It must then validate `rootDirectoryLayout` and return any root-header error before reading optional metadata or testing table presence. A valid version-5 container may retain the original optional-absence behavior. This separates malformed or unsupported input from a genuinely absent member.

This closes the measured regression in `issues/2026-10-05-nim-header-safe-lookup-masks-container-refusal.md`: unmodified `68d32331` passes all four original version/current controls with the same compiler and flags, while the reviewed seven postimages open stamped version 2. Metadata schema 6, shard roots, auto-filled and explicit counts, overflow/append boundaries, original APIs and all assertions remain unchanged. Version-6 support and frozen consumer pin advancement are outside this repair.

Acceptance requires the original version-refusal/current-positive groups, all existing sharded-root controls and the complete original native suite including both FFI tasks. The prior timing and source-component receipts remain historical evidence, not complete acceptance of the amended source.

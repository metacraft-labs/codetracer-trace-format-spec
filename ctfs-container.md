# CTFS Binary Container Format

CTFS (CodeTracer File System) is a block-based container format that stores multiple named files in a single `.ct` file. It provides a flat file system optimized for streaming writes, concurrent multi-producer access, and multiple simultaneous readers. All integers are **little-endian**.

> **Scope.** This specification defines the structure of the CTFS container format, and the records
> used by the open-source recorders that produce materialized traces. It does not describe how the
> Multi-Core Recorder (MCR) uses CTFS: which members an MCR recording writes, their layout, and its
> per-thread, checkpoint and page streams. Those are specified in `codetracer-specs`
> (`spec/Trace-Files/CTFS-Binary-Format.md`), and they are subject to change in each release. The
> boundary between the two repositories, and how a change that spans both is landed, is stated in
> `codetracer-specs/spec/Trace-Files/README.md`. Where this document names an MCR member, it uses it
> as an example of a container mechanism, not as that member's definition.

## Goals And Properties

These are the properties the format keeps. Every change to the container, and every member format
built on it, is checked against them; [ctfs-keyed-families.md](ctfs-keyed-families.md) §1 restates
them as obligations on the members that hold growing families. Until 2026-10-08 this table was a
list of ten properties; the goals added then (G4's torn-read rule, G5, G6, G10, G12, G13, G15) were
stated elsewhere in this document or followed from it, and are named here so that a design can be
measured against them.

| # | Goal | Description |
|---|------|-------------|
| G1 | Self-contained, no sidecars | All metadata in binary format within the container; no external files or JSON. Progress and every index live in members (§6, "Live progress"). The live coordination page (§6) is shared memory that exists only while the container is written and holds nothing the closed file lacks; it is not a sidecar. |
| G2 | Append-mostly, and nothing moves | Data is appended and never rewritten. The only in-place updates are the ones §6 "What is mutated in place" lists: an entry's words, a keyed member's record words (§7a), a member's partial last block, an unfilled mapping slot, and a namespace's root slots (§8). No block is ever relocated: a block number, once published, names the same block for the life of the container (§1, "The root directory is fixed at creation"). |
| G3 | Single writer per member, lock-free | Each member has one writer. The only shared mutable state is block allocation (`NextFreeBlock`) and, under a multi-process writer architecture, the root state (§6, "Writer architectures"). Only atomic fetch-and-add and atomic stores; no locks, mutexes or CAS retry loops. |
| G4 | Live out-of-process readers | Readers in other processes follow a container while it is written. A reader on the writer's machine attaches to the live coordination page and sees each published value whole; a reader that cannot attach follows seal-published state under a stated torn-read rule; a closed file is static (§6, "In-place record publication"). No block is relocated. |
| G5 | Crash durability at seal granularity | A writer whose process dies leaves a container that reads correctly through its last sealed chunk (§6, "Durability"). |
| G6 | Bounded per-operation work | No operation a writer performs inside the recorded program, or on a path that can stall it, takes time proportional to a member's or the container's size: no global rehash, rebuild, directory doubling, compaction or root growth ([ctfs-keyed-families.md](ctfs-keyed-families.md) §3, P9). |
| G7 | Random access with few block reads | O(log n) block mapping (at most 5 reads, none with a warm cache of mapping blocks); O(1) chunk seek via companion index; a keyed lookup at a stated read count ([ctfs-keyed-families.md](ctfs-keyed-families.md) §2). Block-aligned layout maps to HTTP range requests. Full profile only: the compact profile is resident before its first query and seeks in memory (§1d). |
| G8 | A fixed root one fetch reveals | One fetch of the root region (one 4 KB block unless the writer declared more, §1) reveals the full structure, and the region never grows. Full profile only, and deliberately so: §1d has no alignment because a one-shot load issues no ranged read. |
| G9 | Streaming-compatible | Companion index available during recording; no finalization needed. |
| G10 | Compact-profile compatibility | Every member format has a defined compact form (§1d-§1f) or is refused by name by a writer converting to the compact profile. |
| G11 | Encryption-aware | Container-level encryption flag; all content opaque without the key. |
| G12 | Deterministic where claimed | Where this document says two writers given the same input produce the same bytes (§1f, §5), layout is a function of the input, not of timing. |
| G13 | Implementable everywhere it is read | Writers in Nim and Rust; readers in Nim, Rust, Go, C#, TypeScript and Python. Every rule is stated so that each can follow it and refuse by name. |
| G14 | Compressed storage | Per-member Zstd via chunked compressed tables, transparent to the container layer; and, from v6, an optional whole-file scheme declared in the header (§1a, §1b) because a scheme that covers `meta.dat` cannot be declared inside it. The compact profile uses neither chunked tables nor seekable zstd (§1d). |
| G15 | Bounded dead space | Blocks no pointer reaches any more are bounded per member family and removed by repacking, never reused in place (§6, "Dead space"). |

### Non-Goals

No directories (flat namespace only), no file deletion or truncation, no file attributes, no built-in checksums, no redundancy.

**The root directory is bounded.** Its size is fixed when the container is created (§1), and no operation enlarges it. A container holds a closed set of named members in its root, typically 10 to 200. A family of members whose count grows with the recording (one per thread, one per checkpoint, one per bundled file) does not belong in the root. It belongs in a keyed member (§7a): one root member, or a small fixed set of them, that maps `u64` keys to records, values and streams, in one of the families of [ctfs-keyed-families.md](ctfs-keyed-families.md).

---

## 1. Container Header (16 bytes through v5; 24 bytes at v6)

Block 0 begins with the container header. Through version 5 it is 16 bytes. Version 6 extends it to 24 and is the only version that carries the `Profile` and `Compression` fields.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0--4 | 5 | Magic | `C0 DE 72 AC E2` ("CODE TRACE") |
| 5 | 1 | Version | `5`, or `6` for a container carrying the fields below |
| 6 | 1 | Encryption | `0` = none, `1` = AES-256-GCM |
| 7 | 1 | MaxShards | Maximum shard count (`0` = no sharding) |
| 8--11 | 4 | BlockSize | Block size in bytes (u32 LE, default 4096): 1024, 2048 or 4096 |
| 12--15 | 4 | MaxRootEntries | Maximum file entries (u32 LE, `0` = auto-fill block 0). Fixed when the container is created and never changed (§1) |
| 16 | 1 | Profile | **v6 only.** `0` = full, `1` = compact. Closed set |
| 17 | 1 | Compression | **v6 only.** Whole-file scheme: `0` = none, `1` = zstd. Closed set |
| 18--23 | 6 | Reserved | **v6 only.** MUST be zero; a non-zero byte is a refusal |

```c
struct ContainerHeader {        // versions 2 .. 5
    uint8_t  magic[5];          // C0 DE 72 AC E2
    uint8_t  version;           // 5
    uint8_t  encryption;        // 0=none, 1=AES-256-GCM
    uint8_t  max_shards;        // 0 = no sharding
    uint32_t block_size;        // default 4096
    uint32_t max_root_entries;  // 0 = auto-fill block 0
};

struct ContainerHeaderV6 {      // version 6
    uint8_t  magic[5];          // C0 DE 72 AC E2
    uint8_t  version;           // 6
    uint8_t  encryption;        // 0=none, 1=AES-256-GCM
    uint8_t  max_shards;        // 0 = no sharding
    uint32_t block_size;        // default 4096
    uint32_t max_root_entries;  // 0 = auto-fill block 0
    uint8_t  profile;           // 0=full, 1=compact
    uint8_t  compression;       // 0=none, 1=zstd (WHOLE-FILE)
    uint8_t  reserved[6];       // MUST be zero
};
```

**Why the header grew by 8 bytes rather than 2.** `Profile` and `Compression` are one byte each, so 18 would carry them. The six reserved bytes buy one property: the `FileEntry` array starts at the header's own size, and its three `u64` fields with their 24-byte stride are 8-byte aligned at 24 and misaligned at 18. When version 6 was written this was stated for UNSHARDED containers only, because a free list root area of `R = 7 * max_shards * 6` bytes then sat between the header and the entries and broke the alignment at odd shard counts. That area was removed on 2026-10-08 (*Block 0 Layout* below), so the property holds for every full-profile container.

The reserved bytes are not a growth area. A reader MUST refuse a non-zero value in them, because "ignored" and "unknown" are the same byte, and §1c says what ignoring an unknown byte has already cost this format. The only way to spend them is another version bump, which is the intended cost.

**A container that is not sharded writes `max_shards = 0`, and `1` is not a synonym for it.** The
parenthetical above says `0 = no sharding`, but it left "one shard" and "no sharding" describing the
same container without saying which byte to write, and two conforming writers duly picked
differently -- one wrote `0` and the other `1` for containers that are otherwise identical. A field
that admits two spellings of one state is not a specification of that state, so: a writer that does
not shard MUST write `0`. `1` means a sharded container whose maximum is one shard, which is a
different claim even where it is not yet a different layout.

**PER-MEMBER compression is not in the header.** Different internal files may use different compression settings, and which one a member uses is fixed by its format and so by its name -- see the correction below, which is the whole of what this paragraph is about. Nothing in it was ever about compressing the container as a whole, and from version 6 that case IS in the header: §1a, §1b.

> **Corrected 2026-09-25 (`MCR-Memory-Page-CAS.milestones.org` CAS-Z0).**  This
> paragraph used to end "the compression mode is specified per-stream in
> `meta.dat`".  `meta.dat` has no such field in any version (v4/v5 carry flags,
> identity, paths and the MCR fields — see `codetracer_trace_writer/meta_dat.nim`),
> and no reader looks for one.  A member's compression is a property of its
> **format, fixed by its name**: a Chunked Compressed Table (§7; `steps.dat` +
> `steps.idx`, and the chunked members other producers define, such as the MCR
> recorder's thread streams and snapshot payloads, which `codetracer-specs`
> specifies) keeps
> independent zstd frames in its data member and their offsets in its index
> member, and so does a seekable-zstd stream ([seekable-zstd.md](seekable-zstd.md); the
> materialized writers' `events.log`, which used it, is removed); a member whose format is not one of
> those is stored exactly as written.  No current writer compresses the
> container as a whole — the MCR recorder's buffered mode, which did, has been
> removed.

**Encryption IS in the header** because an encrypted container is opaque -- even `meta.dat` is unreadable without the key.

**WHOLE-FILE compression IS in the header, for exactly the reason encryption is.** A scheme applied to the container as a whole covers `meta.dat` and the entry array along with everything else, so a reader told to find the mode anywhere inside the container would have to decompress it in order to learn how to decompress it. That is the same circularity the encryption sentence above resolves, resolved the same way: the declaration sits in the one region the scheme does not cover. The two statements above are therefore not in tension with this one -- the first governs per-member compression, which is settled by a member's format inside the covered region and can be; the `Compression` field governs the whole-file scheme, which lives outside it and must.

**And this is the field the removed buffered mode did not have.** The correction above records that the MCR recorder once compressed the container as a whole and that the mode has been removed. Nothing in the header said it had done so, which is why nothing could refuse such a container by name. Version 6 does not reinstate that mode; it makes the declaration a precondition of ever having one again.

### Block 0 Layout

Block 0 contains the header and the file entries. The offsets below are the versions 2--5 layout; for version 6 substitute the 24-byte header given in §1a. The compact profile has no block 0 at all.

```
Block 0 (versions 2 .. 5):
  [0..15]                       ContainerHeader (16 bytes)
  [16 .. BlockSize-1]           FileEntry array (remaining space)
```

The entry array starts immediately after the header, whatever `MaxShards` says. Entry `i` is at
`16 + 24 * i` (version 6: `24 + 24 * i`), so every word of every entry is 8-byte aligned and lies
within one block.

**Auto-fill:** When `MaxRootEntries` is 0, file entries fill the remainder of block 0:

```
auto_entries = (BlockSize - 16) / 24
```

For BlockSize=4096: `auto_entries = (4096 - 16) / 24 = 170`. A reader MUST apply this rule to a
header carrying `0`. A reader that reads `0` as "no entries" reads an empty container and reports
success.

If `MaxRootEntries * 24 + 16 > BlockSize`, file entries overflow into contiguous blocks after block 0:

```
root_blocks = ceil((16 + MaxRootEntries * 24) / BlockSize)
```

Data block allocation begins at block number `root_blocks`. The blocks `[0, root_blocks)` are the
**root region**. They hold the header and the entry array, they are never data or mapping blocks,
and a writer pads the region to whole blocks.

#### The free list root area is removed (2026-10-08)

Until 2026-10-08 this section placed a **free list root area** of `R = 7 * max_shards * 6` bytes
between the header and the entry array. It is removed: `R` is `0` for every value of `MaxShards`.
The owner's rule was to remove it only if nothing needs it; the evidence below is that argument.
A writer MUST NOT reserve bytes between the header and the entries, and a reader MUST NOT skip any.

**Why it was invented.** The area was added on 2026-04-21/22, before any implementation existed
(`codetracer-specs` ae9bb8021, "namespaces use B-tree (not hash table), global free lists", and
bf8011e1e, "free list roots in block 0 before file entries, max_shards in header"). The design then
was a **container-global sub-block allocator**: the 32 B to 2048 B pools of §8 shared by *every*
namespace in the container, so that a slot released when one namespace promoted a value could be
reused by another, with one free list per pool class *per shard* (the home shard of a key, §9). Its
heads moved from a member (`pool-state.dat`, then `pools.dat`) into block 0 so that they were "always
available in a single block read". It was never meant for whole-block reuse of copy-on-write B-tree
pages; that design came later and put its free chain inside the namespace.

**Why nothing needs it (the certainty argument).** Read on 2026-10-08 at the tips named:

1. **No implementation of the design it served keeps state there.** The sub-block pools that were
   built (`codetracer-trace-format-nim` `ff17c78`, `sub_block_pool.nim`, milestone
   `CTFS-Format-Evolution` M8) are per namespace: a slot's `blockIdx` is "block index within the
   pool's buffer (not CTFS block num)", and the heads are serialized into the namespace's own image
   (`namespace.nim`, the `"NS"` v2 blob's `freeListHead` per pool). M8's deliverable "heads in
   block 0" was checked off for code that does not do it. The sharded variant (M10,
   `shard_writer.nim`) records "Per-shard free list management inside each ShardWriter (not in main
   file)". The copy-on-write B-tree (`cow_btree.nim`, milestone `CTFS-Lazy-Seekable-Coverage` M3)
   reclaims superseded pages through a chain rooted in its own header's `free_list_head`, "no
   separate B-tree free-list". Promotion, the only operation that ever freed a sub-block, is reached
   only from tests (`shard_writer.promoteSlot`).
2. **No writer writes the area.** Every writer places entry `i` at `16 + 24 * i`
   (`codetracer_ctfs` `fileEntryOffset`; the Rust `CtfsWriter`; the db-backend's writers; the
   native backend's `ctfs_meta_writer.rs`, which writes `max_shards = 0`); no producer writes
   `max_shards != 0` (the only container that does is the test `test_container.nim`'s
   `createCtfs(maxShards = 4)`, whose entries are at 16 as well).
3. **Every reader that mentions it only skips it, and none agree.** Readers that add `R` before
   the entries: `cas_dedup/ctfs.py` (8 classes), the wasm recorder's Go reader `container.go` (8),
   the recorder's test helper `cas_d1_support.nim` (8), the Nim compact converter `compact.nim` (7).
   `rootBlockCount` (Nim, 7) and `mcr_enrichment.nim` (7) reserve it in the block count only. None
   interprets the bytes. The db-backend overlay's `read_free_list_roots` / `write_free_list_roots`
   (`block_overlay.rs`, added with its M2 in 0232fd138, 2026-06-22, for "the M3 free-list/B-tree
   work") have no caller at `codetracer` `35b08e67e`; M3 then put the free chain in the namespace.
4. **The one byte-level witness disagreed with the old text.** This repository's
   `fixtures/minimal_trace.ct` declared `max_shards = 1` with its entries at 16 until it was
   regenerated by the split-stream writer on 2026-10-08 (94b7dcc); it now declares `max_shards = 0`
   (read at 4fc5486), so no container in the workspace declares a shard count at all.

So: no implementation keeps or reads free-list state in block 0, no writer reserves it as specified,
and the design it served was built differently. That is why it is removed rather than repaired. The
readers that skip it follow in the rollout (`codetracer-specs`
`milestones/CTFS-Keyed-Families.milestones.org` CKF-4 and CKF-6); none of their outputs changes for
any container a producer writes, since all have `max_shards = 0`. The removal also makes every entry
word 8-byte aligned at every shard count.

**What the area was for, and where that need is met now.** Space no longer used on disk is the
concern it served. Under this specification nothing reuses a block while a container may be read
(§1, rule 2; §6, "What is mutated in place"), so dead space is not reclaimed by a container-level
free list at all: it is bounded per member family and reclaimed by repacking
(§6, "Dead space"). Allocation state that a future sharded allocator needs lives with the shard
(§9) or in the live coordination page while the container is written (§6), never in block 0.

### The Root Directory Is Fixed At Creation (normative)

`MaxRootEntries`, and so `root_blocks`, is chosen by the writer when it creates the container. It is
written with the first publication of block 0 and never changes afterwards.

1. **The header is written once.** After its first publication, a writer never changes bytes 0..15
   of block 0 (0..23 at version 6). A writer that rewrites block 0 as a whole writes the same
   header bytes back.
2. **Growing the root directory is not an operation.** No writer, appender, exporter, slicer or
   other tool increases `MaxRootEntries` of an existing container, moves an allocated block to
   another block number, or rewrites pointers so that a block may be reused for anything else. A
   container that needs a larger directory is a different container: it is produced by copying
   members into a new one, as an export does.
3. **A full directory is a refusal.** Creating a member when no empty slot remains MUST fail,
   naming the member, the occupancy (`N of N entries used`) and `root_blocks`. The writer MUST NOT
   drop the member and continue. A recording that cannot store a member it needs fails the
   recording, naming the member, rather than closing a container that lacks it.
4. **The producer declares the count it needs.** A producer sizes the directory for the closed set
   of members it writes. A family whose count grows with the recording goes into a keyed member
   (§7a), never into the root. The RECOMMENDED declaration is `0`, one block (170 entries at the
   default block size), unless the producer's closed set needs more.

**What a reader may rely on.** Because the root region never grows and no block ever moves, a reader
MAY read `MaxRootEntries` and `root_blocks` once, when it opens the container, and keep them for the
container's whole life, including while the container is still being written. A block number it has
resolved keeps naming the same block. A block number in `[1, root_blocks)` is never a data block or
a mapping block, and a reader MUST refuse it wherever it resolves one, exactly as it refuses `0`
(§4, "Null block pointers on the read path").

**Why this is the rule, and the alternative that was tried.** In October 2026 the Nim library grew a
full directory in place (`codetracer-trace-format-nim` 7ccbb8b, b253090; the MCR recorder adb8608ef).
It doubled the root region, copied the data blocks it took over to the end of the container, and
rewrote the pointers it knew about. No specification described it, and it broke four things this
document relies on:

- §6's reader protocol re-reads `Size` and `MapBlock` but not the header. Eight live and following
  readers in the workspace froze `MaxRootEntries` at open, which was correct while it was immutable.
  After a growth they missed every new entry. A reader that held a `MapBlock` or a mapping block from
  before the growth resolved it into the new root region and read entry bytes as data.
- Any member that stores a container block number (a namespace as §8 specified it until 2026-10-07;
  a keyed member in address form (b), [ctfs-keyed-families.md](ctfs-keyed-families.md) §2.3) would
  hold numbers a growth did not rewrite.
- A closed-container append that grew rewrote every byte from the new root to the old end of file.
  It could no longer claim that an interrupted append damages no existing stream.
- The root region was published as one write, header first. A torn write or a concurrent reader
  could pair the new `MaxRootEntries` with the old bytes behind it.

The owner chose a bounded root on 2026-10-07: unbounded families move into keyed members, and
growth is retired. Recorded in
`codetracer-specs/issues/2026-10-07-ctfs-root-directory-growth-landed-without-a-spec-change.md`.
Which structure each family uses is chosen by benchmark ([ctfs-keyed-families.md](ctfs-keyed-families.md)
§7). The rollout is `codetracer-specs/milestones/CTFS-Keyed-Families.milestones.org`.

> **Implementation status (2026-10-08).**  Measured by the CTFS member census of 2026-10-06
> (codetracer-trace-format-nim `bb7376e`, codetracer-native-recorder `851dd4d73`).
>
> - **Conforming: a root declared at creation.** `codetracer_ctfs`
>   (`codetracer-trace-format-nim`, since c212173) reserves `root_blocks` blocks
>   at `createCtfs` and allocates data from block `root_blocks`; `writeToFile`
>   refuses a mapping or data block inside the region. Pinned by
>   `tests/test_root_directory_overflow.nim`.
> - **Not conforming, retired by `CTFS-Keyed-Families` CKF-8: growth.**
>   `codetracer_ctfs` 7ccbb8b and b253090 grow a full directory
>   (`container.nim` `growRootDirectory`; `container_append.nim`), and the MCR
>   recorder relies on it (adb8608ef; the recorder at `c5d7fc33f` pins
>   `ff17c78`, which contains it). Until CKF-8 removes it, a container
>   written through those revisions can carry a `MaxRootEntries` larger than the
>   one it was created with, and blocks that moved.
> - **Not conforming: readers and a library that still account for the removed
>   free list root area.** `codetracer_ctfs`'s `rootBlockCount` reserves `R`
>   (7 classes) and its compact converter skips it; `cas_dedup/ctfs.py`, the
>   wasm recorder's Go reader and a recorder test helper skip `R` computed with
>   8 classes. No producer shards, so no recording is read differently; they
>   follow in `CTFS-Keyed-Families` CKF-4 and CKF-6.
> - **Not conforming: rewriting or truncating a published member.**
>   `codetracer_ctfs` offers `truncateFileContent` and `rewriteFileContent`, and
>   the MCR recorder uses them to republish its live `corrmark.ns` on every span
>   marker (`codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md` §2.8): a
>   same-size image is rewritten in place (a multi-block overwrite a live reader
>   can see torn, not one of §6's in-place classes) and a size change truncates,
>   then appends (a moment of `Size = 0`, and an abandoned image per size change).
>   A completed recording loses nothing. `span_emit` uses `truncateFileContent`
>   to replace `spantype.ns` in a closed recording, which §5 forbids ("replacing
>   a member is not an operation"). Both are replaced in `CTFS-Keyed-Families` CKF-5.
> - **Block 0 only.** The Rust `codetracer_ctfs` writers (`writer.rs`,
>   `concurrent_writer.rs`) allocate from block 1, so they MUST NOT be given a
>   count past block 0 until CKF-4R makes them honour `root_blocks`. The Go reader in
>   `codetracer-wasm-recorder` refuses a root past block 0, and the db-backend's
>   `block_overlay.rs` reads block 0 only
>   (`codetracer-specs/issues/2026-09-29-db-backend-block-overlay-reads-root-directory-from-block-0-only.md`).
>
> How the MCR recorder sizes its root, and the note this one replaces (its
> 2026-09-29 MCR-specific parts), are in `codetracer-specs`
> `spec/Trace-Files/CTFS-Binary-Format.md`.

### 1a. Profile and Whole-File Compression (version 6)

Version 6 is version 5's body plus eight header bytes. Everything §2 says about `MapBlock`'s three forms, and everything §4 says about the block map, holds in a version-6 full-profile container unchanged.

`Profile` says which body layout follows the header:

| Value | Name | Body |
|-------|------|------|
| `0` | full | Block 0, `FileEntry` array, block map -- everything from *Block 0 Layout* above and §2 onward |
| `1` | compact | A directory of `(name, offset, length)` and the members concatenated raw, with no block map and no mapping blocks -- §1d, which is normative for every offset. A reader that does not implement it MUST refuse it rather than attempt the full body. Which profile a WRITER produces, and what a mid-recording switchover between them must preserve, is §1e |

The set is CLOSED: `0` and `1` are the only defined values and every other value is a refusal.

A compact container MUST write `max_shards = 0`. Block sharding (§9) partitions a block-number space, and the compact profile has no blocks, so "one shard" and "no sharding" would again be two spellings of one state -- the defect the `max_shards` note above was written for.

In a version-6 **full** container the `FileEntry` array starts 8 bytes later, because the header is 8 bytes longer. Every other offset in this document is relative to it and so is unchanged:

```
Block 0 (version 6, profile = full):
  [0..23]                       ContainerHeaderV6 (24 bytes)
  [24 .. BlockSize-1]           FileEntry array (remaining space)

auto_entries = (BlockSize - 24) / 24
root_blocks  = ceil((24 + MaxRootEntries * 24) / BlockSize)
```

`Compression` says whether the container body is stored under a whole-file scheme. The field covers **the container image from offset 24 to the end of the stored object**; the 24-byte header is always stored as plaintext, since it is what declares the scheme. A reader reconstructs the image as `header || decompress(rest)` and then every offset in this document holds unchanged, including block numbering -- block 0 is still the first `BlockSize` bytes of the reconstructed image and its first 24 bytes are still the header.

Whole-file compression and per-member compression are independent, over disjoint regions. A writer that sets a whole-file scheme would normally leave its members uncompressed; nothing here forbids both and nothing here recommends it. Where `Encryption` and `Compression` are both set, the body is compressed first and encrypted second, and a reader reverses that order -- compressing ciphertext accomplishes nothing, and an unstated order is a field pair with two spellings of one state.

A container at version 5 or below is a **full**-profile container with **no** whole-file compression. That is an inference from a KNOWN version with a fully specified body, not a default applied to an unrecognised value: versions 2 through 5 are each specified above and in the version history, and none of them admits a body a whole-file scheme could cover. The distinction matters because the opposite reading -- "a value I do not recognise means none" -- is the precise defect §1b and §1c close.

### 1b. The Closed Set Of Whole-File Compression Schemes

| Value | Name | Browser decompresses it transparently from `Content-Encoding`? | Offered by `DecompressionStream`? | Decoder already in the db-backend? |
|-------|------|---|---|---|
| `0` | `none` | n/a -- nothing to decompress | n/a | n/a |
| `1` | `zstd` | Yes: Chrome/Edge 123+, Firefox 126+, Safari 26+ (macOS/iOS). Not universal historically | Yes, as `"zstd"` -- defined by the Compression Streams specification, but NOT yet shipped everywhere `"gzip"` is | Yes: `zstd`/`zstd-safe` on native targets and the pure-Rust `ruzstd` on `wasm32-unknown-unknown` |

The set is CLOSED at those two members, and it is short on purpose. A member of this set is a promise that every reader of the format implements the scheme, so the set is bounded by the decoders the implementations already carry rather than by what compresses well. `gzip`, `deflate`, `deflate-raw` and `brotli` are each decompressed transparently by every browser and each offered by `DecompressionStream` (`"brotli"` in Chromium and Safari 18.4+, not yet in Firefox), and none of them is enumerated here, because adding one would oblige both the Nim and the Rust reader to take a new compression dependency -- including in the WebAssembly build. An enumerated member with no implementation is worse than an absent one: it is a capability a consumer can read in this document and cannot rely on.

The per-member compression method enum that `codetracer_ctfs` carries has a third value, `2 = LZ4`, marked "reserved, not yet implemented". That is the shape this set refuses to repeat, and it is named here so the refusal is visibly deliberate.

A BlockTracer archive declares `none`. Such an archive is stored pre-compressed and served with a `Content-Encoding`, so the browser has already decompressed it before any application code runs; what the loader holds is a compact container with raw members, and `none` is a true statement about those bytes rather than a convenient one.

### 1c. The Refusal Rule For The Version-6 Fields (normative)

§2's "Older versions are refused" already states this rule for the version byte, and states it as a MUST that names the value found. §1c extends the same rule to the two fields version 6 adds, and amends one word of §2's: the refusal is of a version the reader does not implement, which is every version other than the ones it reads.

A reader that encounters a `Version`, `Profile` or `Compression` value it does not implement, or a non-zero byte in `Reserved`, MUST fail and MUST NAME THE OFFENDING VALUE in its diagnostic. It MUST NOT fall back to another value, infer one from the container's contents, or treat an unknown scheme as `none`. A header too short to carry a field the version declares is likewise a refusal and NOT an absent field: "the byte says 0" and "there is no byte" are different facts, and a parser that returns the permissive value for both has no way to report the second.

**This is normative rather than advisory because the format has already paid for the advisory version.** Containers were once written with the corrected global line index packing while the version stamp still said 3, so a reader that trusted the stamp placed every step one line high and returned success while doing it. The repair is three artefacts that exist only because of it -- a named supported-version set in `meta.dat`'s schema (`[4, 5]` when the repair was written; one value, 6, since the 2026-10 revision, which is the same mechanism tightened), a named `LastShiftedGlobalIndexVersion = 3`, and an explicit opt-in whose own documentation says it exists so that behaviour does not depend on how a recording happened to be written. Nothing in the bytes could have caught that, because both packings addressed positions the trace's own space could address. A version whose accepted set is closed can catch it; a version a reader shrugs at cannot.

So the refusal is also what makes the version bump the right mechanism for introducing the profile. A reader predating version 6 holds a closed accepted set and the magic is unchanged, so it rejects a version-6 container at the version check, before it computes a single file-entry offset. It cannot ignore the bump: the version is already the field it consults to decide which header shape and which `MapBlock` forms it is holding, and a version-6 container's entry array really is 8 bytes further along, so a reader that shrugged would resolve every entry out of the reserved area. A capability flag would not have this property -- an unknown flag bit is exactly what an old reader ignores -- and a distinct magic would cost more than it buys, since it would also take the container out of the reach of every tool that identifies a `.ct` file by its first five bytes, including the ones whose whole job is to report what it is.

**What a writer writes.** Version 5 for a full-profile container with no whole-file scheme, which is every container any writer produces today and leaves them byte-identical. Version 6 only for a container that uses one of the two new fields. Reader support for 6 must ship everywhere before any writer emits it, which is the same rollout rule the version-5 note records.

### 1d. The Compact Profile Body (version 6, `Profile = 1`) -- normative

A compact container is the 24-byte header, a **directory** of `(name, offset, length)`, and the members concatenated. It has no block 0, no free list root area, no `FileEntry` array, no mapping block and no padding of any kind. Nothing in §2 and §4 onward applies to it.

```
Compact container (version 6, profile = 1):
  [0 .. 23]                     ContainerHeaderV6 (24 bytes, §1)
  [24 .. 27]                    MemberCount, N (u32 LE)
  [28 .. 28 + 24*N - 1]         Directory: N CompactDirectoryEntry records, 24 bytes each
  [28 + 24*N .. Size - 1]       The members' bytes, concatenated in directory order
```

```c
struct CompactDirectoryEntry {   // 24 bytes
    uint64_t name;               // base40-encoded name (§3), the packing FileEntry uses
    uint64_t offset;             // from the start of the container image
    uint64_t length;             // the member's bytes
};
```

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 8 | u64 LE | Name | Base40-encoded member name (§3) -- **the same packing, in the same byte order, as `FileEntry.Name`** |
| 8 | 8 | u64 LE | Offset | Byte offset of the member's first byte from the start of the container image |
| 16 | 8 | u64 LE | Length | The member's size in bytes. There is no separate stored/logical distinction |

So the first directory entry is at offset 28, entry `i` is at `28 + 24*i`, the `Name` of entry `i` is at `28 + 24*i`, its `Offset` at `36 + 24*i`, its `Length` at `44 + 24*i`, and the first member's byte is at `28 + 24*N`. A compact container's size is exactly

```
Size = 28 + 24*N + sum(entry[i].Length)
```

and a container whose length is not that value is refused. That identity is the layout's whole claim, so it is stated as an equation rather than as an absence of padding.

**Header fields in a compact container.** `Version = 6` and `Profile = 1`, by definition. `MaxShards = 0`, as §1a already requires. `BlockSize = 0` and `MaxRootEntries = 0`, and both are MUSTs with a reader-side refusal: there are no blocks, so a block size is not a smaller or larger version of anything, and there is no `FileEntry` array for a maximum to bound. Writing `4096` there because it is the default would be the `max_shards` defect again -- two spellings of one state, this time "there are no blocks" spelled as a block size. `Compression` is as §1b, and covers the container image from offset 24 to the end of the stored object exactly as it does for the full profile; a reader reconstructs `header || decompress(rest)` and then every offset above holds. Note what that means for the field, because it is easy to read backwards: the reconstructed image keeps the original 24-byte header and therefore still DECLARES its scheme. The `Compression` byte describes how the object was stored, not what the bytes in hand are, so a reader must not treat "declares a scheme" as "is still compressed" -- the two are the same byte in a stored object and different facts in a reconstructed one, and a decoder that refused every container declaring a scheme would refuse the reconstructed image along with the stored one. The checks above apply to the reconstructed image.

**Member names are carried unchanged from the full profile.** The `Name` field is §3's base40 packing in a `u64`, bit for bit what `FileEntry.Name` carries, so a compact container and a full container of one recording name the same members identically and a tool can compare the two name sets without transcoding either. §3's limits come with it: 12 characters, from `\0` (padding, index 0), `0`--`9` (1--10), `a`--`z` (11--36), `.` (37), `/` (38), `-` (39). **The alphabet has no space character and no capital letters**, and index 1 is `0` and not `\0`: a decoder whose table is off by one decodes `meta.dat` into something else entirely and reports success, which is why the packing is specified by reference here rather than restated.

**Members appear in directory order and the directory is not sorted.** Entry `i`'s member precedes entry `i+1`'s in the file. The order is the producing container's own member order, which is creation order for a full container's `FileEntry` array; a reader MUST NOT assume lexical or numeric order and MUST find a member by searching the directory, which is N comparisons of one `u64` each.

**A reader MUST refuse, naming the offending value, a compact container in which any of the following does not hold** (§1c's rule, applied to this body):

1. `28 + 24*N <= Size` -- the directory itself fits.
2. `entry[0].Offset == 28 + 24*N`, when `N > 0` -- the first member begins immediately after the directory.
3. `entry[i].Offset == entry[i-1].Offset + entry[i-1].Length` for every `i` in `1 .. N-1` -- the members are contiguous: no gap, no overlap, and ascending.
4. `entry[N-1].Offset + entry[N-1].Length == Size`, when `N > 0`; `Size == 28` when `N == 0` -- nothing follows the last member.
5. Every `Name` is non-zero and round-trips through §3's packing: re-encoding the decoded name reproduces the `u64`. This refuses a `u64` at or above `40^12`, which names nothing, and a packing with a padding character before a non-padding one, which §3's encoder cannot produce and whose decoded string is not the name the writer meant.
6. The `N` names are distinct.

**Why contiguity and the total, and not merely a bounds check.** A reader that checked only `Offset + Length <= Size` would accept a directory one of whose offsets had been perturbed and would then serve a member that is SHIFTED, or one of whose lengths had been perturbed and serve a member that is SHORT -- in both cases successfully, with no indication. Checks 2, 3 and 4 make a single perturbed `Offset` or `Length` field unrepresentable: either it breaks contiguity with its neighbour or it breaks the total, and there is no value it can take that does neither while leaving the member it describes wrong. This is the same argument as §1c's, one level down -- a structure that admits a wrong value a reader cannot distinguish from a right one is not a specification of that structure. It is not a checksum and does not claim to be: a flipped bit in a `Name` yields a different, well-formed name, and what the checks guarantee is that it cannot yield a wrong member's bytes under a right name.

**The directory is not a block map, and that is the design rather than a simplification.** A block map answers *which block holds byte N of this member*, which is what random access into a large member needs and what goals G7 and G8 are about ("Goals and properties"; property 2 and design goal 5 before 2026-10-08). A directory answers *where does this member start and how long is it*, which is what a one-shot load needs. The second costs one `(u64, u64)` per member against a 4 KB mapping block per member, and it is sufficient precisely because the whole file is resident before the first query is asked. The compact profile is therefore not a cheaper encoding of the full profile's structure; it answers a different question, and it is the right profile only for a container small enough that the answer to the first question is always "all of it".

**There is NO alignment requirement, and that is a statement about what alignment is for.** Nothing in a compact container is padded to a block, a page or a word: the directory begins at 28, a member begins wherever its predecessor ended, and a 12-byte member occupies 12 bytes. Goals G7 and G8 -- "block-aligned layout maps to HTTP range requests; one 4 KB fetch reveals full structure", formerly design goal 5 -- are what alignment serves, and it is correct for the full profile. A compact container is fetched whole and issues no ranged read, so there is nothing for alignment to serve and a reader MUST NOT round any offset or length to a boundary. A writer that aligned anyway would reintroduce exactly the cost the profile exists to remove, and because of check 4 it would also produce a container every conforming reader refuses.

**Measured effect.** See [`measurements/2026-10-compact-profile.md`](measurements/2026-10-compact-profile.md). On a 17-member, 20,000-step version-5 container the compact layout is 38,481 bytes against 110,592, a 65.2% reduction, and its structural overhead is 436 bytes (1.1%) against 72,547 (65.5%); on the five-member `fixtures/minimal_trace.ct` it is 474 against 24,576, 98.0%. Both predictions -- `28 + 24*N + sum(length)` for the compact size and an independent block model for the version-5 size -- reproduce the measured containers to the byte. The saving is one data block per member plus block 0, and NOT mapping blocks: fifteen of the seventeen members are direct at version 5 and the two mapped ones own 8,192 bytes between them. The same document records a finding that the compact profile's *compressed*-size advantage is compressor-dependent and does not reproduce under `gzip` on that container.

**Per-member compression is not used in a compact container.** The member formats that keep independent zstd frames inside themselves -- the chunked compressed tables of §7 and the seekable-zstd streams of [seekable-zstd.md](seekable-zstd.md) -- exist for one purpose: so that a reader can inflate the single frame a seek lands in rather than the whole member. A compact container is resident before its first query and seeks in memory, so that purpose does not arise, and the framing is pure cost -- a frame header per chunk, an index member describing offsets nothing consults, and a decompressor in a path whose whole claim is that it has one decompression step. A compact container's members are therefore stored as written, and `Compression` (§1b) or the transport's `Content-Encoding` is the only compression in the path. This is a constraint on WRITERS; a reader needs it only to know that a member's bytes are its content. How a member whose format consists of frames is stored instead -- the chunked compressed tables, `step-map.ns`, the seekable-zstd streams -- is §1f. §1e restates it as a checkable property of the OUTPUT -- no zstd frame in any member whose format is not itself compressed -- because a writer that CONVERTS a full container satisfies the constraint only by inflating, and copying a payload verbatim is what a careful conversion does first.

What is deliberately NOT claimed here is that raw members also make the stored object SMALLER. The argument for that is real -- independently compressed members deny a one-shot compressor the redundancy across members it would otherwise find, and it cannot un-compress them to look -- but it is a trade, not an identity: undoing per-member compression grows the input by the per-member ratio in exchange for a whole-file view of it, and it reduces the compressed size only when the one-shot ratio exceeds the per-member one. Measured both ways. On the published container of [`measurements/2026-10-compact-profile.md`](measurements/2026-10-compact-profile.md)'s reference the trade pays, 15.4% of `gzip -9`; on that document's container A it loses, making `gzip -9` 2.7x worse and `zstd -19` 7.2% worse, and only `xz -9e` improves -- by 44.5%, more than the claim. So the paragraph above stands on the framing being useless in this profile, which is a property of the profile, and not on a size figure that is a property of a corpus and a compressor.

### 1e. Which Profile A Writer Chooses, And What The Switchover Must Preserve

§1a says what each profile's body is and §1d says what a compact body looks like. Neither says which one a writer should produce, and this section does -- with the parts that affect CORRECTNESS stated as MUSTs and the part that is a judgement stated as a recommendation, because conflating the two is how a tuning constant acquires the authority of a format rule.

**The threshold is measured in RAW bytes, and that is normative.** A writer that chooses between the profiles on a size threshold MUST measure the threshold against the recording's RAW bytes -- the logical size of the members it would store, before any compression is applied to any of them. The reason is what the decision is about: the compact profile is for a container that is RESIDENT before its first query, and residency is a property of logical size. A threshold on compressed size would make the profile a function of compressibility, so two recordings of the same logical size could take different paths -- which is a difference no reader can act on, because the thing a reader must decide from the profile (do I load this whole, or do I walk a block map) does not depend on how well the bytes happened to compress. Measured, because the gap is not theoretical: two recordings of 79,995 identical raw bytes, one of every-step-identical events and one of pseudo-random ones, compress to 54 and 78,441 bytes through this format's own writer -- 1,452x apart. A compressed-size rule classifies them differently; a raw-byte rule cannot.

**The RECOMMENDED default is 1 MiB of raw bytes, and it is a judgement.** It is recommended rather than required, and no reader may depend on it: the profile is declared in the header and a reader reads the declaration, never a size. The reasoning is recorded so that a different figure can be argued against it rather than merely preferred -- the published container this profile was designed for has 85,118 bytes of logical content, an order of magnitude below it; blockchain traces are bounded by gas rather than by taste, so the workload clusters well under the figure; and 1 MiB is small enough that a whole-file load is unremarkable on any device that can run a client at all. What would move it is a peak-resident-memory measurement, not an opinion.

**The switchover MUST lose nothing, and this is the one rule here whose violation is SILENT.** A writer that buffers in memory under the threshold and switches to streaming the full profile when the threshold is crossed mid-recording MUST write every buffered event into the full container, in arrival order, before the first event that follows the switch. Every other defect this document guards against announces itself: a wrong layout fails §1d's checks, a wrong version is refused by name under §1c, a wrong offset breaks contiguity. A switchover that drops or reorders the buffered prefix produces a container that is VALID in every respect -- correct magic, correct version, intact block map, well-formed `meta.dat` -- and is missing part of the recording. Nothing in the bytes distinguishes it from a complete recording of a shorter program. So a writer MUST NOT discard the prefix and MUST NOT reorder it, and a writer's test suite is the only place this can be caught: the check is that the same recording written with the switchover and written full throughout answers every query identically, with the assertion on the CONTENT rather than on which profile was chosen.

**A writer may instead write the full profile throughout and convert it at close.** Its raw bytes are then exactly the sum of the lengths of the members the compact container would carry -- every frame inflated as §1f says, so `Size - 28 - 24*N` of that container -- and it emits the compact container when that sum is below the threshold and the full container when the sum reaches it. Such a writer has no switchover and so no prefix to lose: the full container is durable from its first seal (§6) until close, and a recording killed before close leaves it, where a buffering writer leaves nothing. Its cost is writing a small recording twice. The decision is taken on the finished recording, so two such writers with byte-identical full containers and one threshold choose the same profile.

**A writer that CONVERTS a full container into a compact one MUST undo per-member compression.** §1d's "members are stored as written" is a constraint on writers, and conversion is where it is easy to violate without noticing, because copying a member's payload verbatim is exactly what makes a conversion byte-exact and therefore exactly what a careful implementation does first. A full container's `steps.dat` carries one zstd frame per chunk; copied verbatim into a compact container those frames come with it, and the result declares a profile whose whole claim is that it has one decompression step while carrying hundreds of them. The obligation is therefore stated as a property of the OUTPUT, which is checkable without reading the writer: **a compact container MUST NOT contain a zstd frame in any member whose format is not itself a compressed format.** A writer that emits a member directly satisfies this by not compressing; a writer that converts satisfies it by inflating. (The reference encoder of a compact body deliberately does neither -- it copies payloads verbatim, which is what makes its round-trip a proof rather than an equivalence -- so the obligation belongs to the writer that chooses the members, not to the encoder that lays them out.)

**What buffering costs, stated here rather than discovered.** §6's "Durability" guarantee -- everything up to the last seal is on disk -- applies to a streaming writer. A writer that buffers under the threshold has written NOTHING when it is killed before it closes or switches, because the compact body cannot be written incrementally: its directory records every member's offset, and an offset is not knowable until every earlier member is complete. That is inherent to the layout rather than a defect in any implementation, and it is the reason the threshold exists at all: past it, the writer gives up the compact profile's one-shot load in exchange for the full profile's durability, and a recording large enough to be worth losing is on the durable side of the line.

**The size at rest is not the size on the wire, and the threshold bounds neither.** The threshold bounds the RAW bytes, which is what has to be resident. A compact container's size at rest is approximately that same figure, because its members are raw -- so for a compressible recording a compact container can be substantially LARGER at rest than a full container of the same recording, where per-member zstd has already run. Measured through this format's own writers on a recording 6 bytes under a 1 MiB threshold: the compact container is 1,048,670 bytes and the full container of the same recording is 16,384 -- 64x. Compressed for serving the order reverses (192 bytes against 285 under `brotli -q 11`), which is the point: a compact archive is a representation intended to be STORED AND SERVED pre-compressed, and comparing its raw size against a full container's compressed size compares two things neither of which is what a client fetches. Three figures -- at rest, on the wire, and as the loader sees it -- are what a publisher needs, and conflating any two of them is how a size argument goes wrong.

### 1f. A Framed Member In A Compact Container (normative)

§1d says a compact container's members carry no per-member compression, and §1e says it of the output. Some member formats ARE a sequence of independently compressed frames located by offsets: the chunked compressed tables of §7 (`steps.dat` with `steps.idx`, and likewise `values`, `calls`, `events`, `spans`, and producer-defined chunked members such as the MCR recorder's), `step-map.ns` ([internal-files.md](internal-files.md) §"`step-map.ns`"), and the seekable-zstd streams of [seekable-zstd.md](seekable-zstd.md). This section is how such a member is stored in a compact container, so that two writers store it alike and every reader reads it.

**Each frame is replaced by its decompressed content, and each offset that located a frame locates that content: in the same units, from the same origin.** Nothing else in the member changes. The records inside a chunk, the number of chunks, which records each chunk holds, the index member's header, its length and its other columns, and the step map's header and chunk-table keys are byte for byte what the full container of the same recording carries. Concretely:

- **A chunked compressed table.** `foo.dat` is the decompressed contents of its chunks, back to back in chunk order. `foo.idx` keeps its header (`chunk_size: u32` for the §7 layout, `[chunk_size: u32][index_version: u16][reserved: u16]` for `spans.idx`) and one entry per chunk, whose offset is now that chunk's content's first byte in `foo.dat`; a `spans.idx` entry keeps its `cumulative_records`. Chunk C's content spans `[offset_C, offset_{C+1})`, the last one ending at the end of `foo.dat`. Record N is found by the same arithmetic as in a full container; only "decompress the chunk" is gone.
- **`step-map.ns`.** Each chunk-table entry's `frame_offset` is the offset of the chunk's decompressed content, counted from the end of the table, and the contents follow the table back to back, the last ending at the end of the member. The 26-byte header and every key are unchanged.
- **A seekable-zstd stream** is stored as its decompressed content, without frames and without the seek-table frame: a compact container is read whole, so there is nothing for the seek table to locate.

A **reader** of a compact container takes a chunk's bytes as its content. It MUST NOT inflate them, and it MUST NOT decide whether to by looking for a zstd magic number in them: the profile is the declaration, and four bytes of content that happen to spell `28 B5 2F FD` are content. A refusal a full-profile reader makes of a frame that does not decode to its declared size has no counterpart here; every other check a reader makes of a chunk's records holds unchanged.

**Why this rule and not the two simpler ones.** Copying the frames verbatim is what §1e forbids, for the reason §1d gives. Dropping the index members as well -- one record stream per member, located by scanning -- would make a compact container's members a different format from the full container's: a reader would need a second seek path, and a record's location would no longer be the arithmetic both profiles share. Keeping the index with the offsets moved costs one `u64` per chunk and keeps the one difference between the profiles to whether a chunk is inflated.

**Two writers produce the same bytes.** A compact container is a function of the full container of the same recording: §1d's member order is the full container's member order, and this section fixes every byte of each member. A writer that converts its own full container at close therefore produces a compact container byte-identical to another conforming writer's whenever their full containers are byte-identical.

### Version History

| Version | Description |
|---------|-------------|
| 6 | 24-byte header with `Profile` and whole-file `Compression` (§1a, §1b) and six reserved bytes that MUST be zero. At `Profile = 0` the body is version 5's, so §2's `MapBlock` forms are unchanged; at `Profile = 1` the body is the compact layout of §1d -- a directory and the members concatenated, with no block 0, no mapping block and no alignment. NOT backward compatible, deliberately: the `FileEntry` array moves to `24`, and a reader predating this version refuses the container at the version check rather than reading entries out of the reserved area (§1c). A writer emits 6 only for a container that uses one of the new fields, and §1e says which profile it chooses and what a mid-recording switchover must preserve. |
| 5 | A member of at most one block is stored without a mapping block, its `MapBlock` carrying the direct-block tag (§2, "Members of at most one block"); an empty member has `MapBlock = 0`. Readers MUST accept 5 -- and 6, which is 5's body behind the extended header -- and MUST refuse every version they do not implement, naming it (§2, "Older versions are refused"; §1c). Writers MUST write 5 unless the container uses a version-6 field. |
| 4 | Query protocol, network reader, replication, RAM cache, cached trace reader. Backward compatible: v4 readers accept v3 and v2 containers. |
| 3 | 16-byte header with encryption; binary metadata; BlockSize 4096; MaxRootEntries 0 auto-fill; small file optimization; namespaces |
| 2 | Extended header with BlockSize and MaxRootEntries |
| 1 | Initial format |

**The keyed-families revision (2026-10-08) changes no version.** It makes `MaxRootEntries` immutable and growth a
non-operation (§1), states the goals the format keeps ("Goals and properties"), adds the in-place
record publication rule and the writer architectures (§6) and keyed members (§7a), and makes
namespaces member-relative with a live publication protocol (§8). It removes the free list root
area (§1), adds the live coordination page (§6) and bounds dead space (§6), and states how the
block-placement rule of the same date composes with them (§6). No container any producer has written
changes meaning: no producer has written a sharded container, a keyed member, or an
in-container namespace holding container block numbers, and the live coordination page changes no
byte on disk. Each keyed family's byte layout is added by the benchmark decision
([ctfs-keyed-families.md](ctfs-keyed-families.md) §8), and may bump a member's version, not the
container's. A
container that a growing writer enlarged (codetracer-trace-format-nim 7ccbb8b to its retirement) is
still read correctly by a reader of a closed container. It is not conforming, and is re-recorded
rather than kept: pre-1.0 there is no compatibility path (§2, "Older versions are refused").

---

## 2. File Entry (24 bytes)

An array of file entries follows the header in block 0, and continues through the root region (§1).

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 8 | u64 LE | Size | Logical file size in bytes |
| 8 | 8 | u64 LE | MapBlock | `0` for an empty member; the member's only data block with bit 63 set; otherwise its root mapping block (see below) |
| 16 | 8 | u64 LE | Name | Base40-encoded filename (12 chars max) |

```c
struct FileEntry {
    uint64_t size;       // logical file size in bytes
    uint64_t map_block;  // 0 = empty; DIRECT|b = only data block; else root mapping block
    uint64_t name;       // base40-encoded name
};
```

An entry where all 24 bytes are zero is an empty slot.

### `MapBlock` has three forms (version 5)

```c
#define CTFS_DIRECT (1ull << 63)
```

| `MapBlock` | The member is | `Size` |
|---|---|---|
| `0` | empty; it owns no block | `0` |
| `CTFS_DIRECT \| b` | one data block, `b`; it owns no mapping block | `1` .. `BlockSize` |
| any other value `m` | mapped: `m` is its level-1 mapping block (§4) | any |

**Writers.** A member's layout follows its size:

- **Created empty, and stays so until written.** Creating a member writes its name and nothing else
  (§5); no block is allocated for it. A member that is never written is `(Size, MapBlock) = (0, 0)`
  in the finished container.
- **First write: one data block, tagged.** The first append claims a data block `b` and stores
  `MapBlock = CTFS_DIRECT | b`. No mapping block exists while the member fits in one block.
- **Growing past one block: mapped from then on.** The append that takes `Size` past `BlockSize`
  claims a level-1 mapping block, puts `b` in its slot 0, claims the new data block(s), and then
  stores the untagged mapping block number in `MapBlock`, *before* it stores the new `Size` (§6).
- **In a container its writer has closed, a member with `0 < Size <= BlockSize` MUST be direct and
  a member with `Size > BlockSize` MUST be mapped.** A writer never allocates a mapping block for a
  member that never outgrows one block, so a small member costs one block instead of two, and an
  empty one costs none.

**Readers.** Decide the layout from `MapBlock`, never from `Size`:

- `MapBlock = 0`: the member is empty. A non-zero `Size` with it is a null pointer (§4, "Null block
  pointers on the read path") and MUST be refused.
- Tagged: the data block is `MapBlock & ~CTFS_DIRECT`. It is subject to every check a data-block
  pointer is -- not `0`, inside the container -- and a `Size` above `BlockSize` with it MUST be
  refused: one block cannot hold it.
- Untagged and non-zero: §4's mapping, whatever `Size` is. A mapped member with `Size <= BlockSize`
  is legitimate while its writer is between the two stores of a transition, and a live reader can
  observe exactly that (below); in a closed container it does not occur, but a reader cannot tell
  the two apart and does not need to.

**Why a tag, and not `Size <= BlockSize`.** The rule this replaced (container versions 3 and 4)
said "if `Size <= BlockSize`, `MapBlock` is the data block", and no writer ever implemented it.
Two things are wrong with it, and either would be disqualifying:

1. **A live reader cannot apply it.** A member crossing one block changes `MapBlock` (data block to
   mapping block) and `Size` (small to large) with two separate stores, and a concurrent reader
   (§6) loads them separately. Whichever order the writer stores them in, some interleaving hands
   the reader one old value and one new one: an old `Size` with the new `MapBlock` makes it read the
   mapping block as the member's bytes, and a new `Size` with the old `MapBlock` makes it read the
   data bytes as block pointers. Re-reading `Size` does not close the window -- the reader can see
   `Size` small on both reads with the new `MapBlock` between them. With the tag, the form travels
   in the same 8-byte word as the pointer, which is loaded atomically: the writer stores `MapBlock`
   before `Size`, a reader loads `Size` before `MapBlock`, and the one mixed pairing that remains
   possible -- an old `Size` with the new mapping -- reads the right bytes, because the mapping's
   slot 0 is the old data block.
2. **It cannot be told apart from the layout every writer produced.** Containers written before
   version 5 give every member, empty or not, a mapping block. A reader applying the size rule to
   them reads each small member's mapping block as its content; a reader that does not apply it
   reads a direct member's data as block pointers. Nothing in the bytes distinguishes them (the
   wasm recorder's Go reader says so in `container.go`, and the one reader that guessed,
   `tracing-formats-benchmarks/cas_dedup/ctfs.py`, guesses from slot contents and can be wrong on a
   short index member). The tag and the version bump make the layout explicit.

Bit 63 is free because no block number reaches it: block `2^63` would begin `2^63 * BlockSize`
bytes into the container, past any file a 64-bit offset can address. A
reader that predates version 5 refuses the container by its version byte; one that ignored the
version would meet a block number beyond its bound check and refuse it there, not misread it.

The small-member layout applies to `FileEntry.MapBlock` and to the `map_block` of every stream
record of a keyed member (§7a), which has the same three forms. Namespace descriptors (§8), which are
member-relative, and the chain and child pointers inside a mapping (§4) are unchanged.

**Older versions are refused.** A reader MUST refuse a container whose version byte is not one it
implements, naming the version it found and the ones it reads, before it resolves any member.
Pre-1.0 there is no compatibility path: older containers are re-recorded, and fixtures are
regenerated with their documented producers. Every writer of containers -- the trace-format
libraries, the MCR recorder's `ctfs_disk`, the native backend's `ctfs_meta_writer`, the
db-backend's overlay and test writers -- writes version 5, or 6 where it uses a version-6 field
(§1a). Refusing is not merely tidy: a version 4 container read under version 5's rules happens to
decode, but a reader that accepts it keeps every writer that still produces it alive, with its
mapping block per member.

This sentence read "whose version byte is not 5" until version 6 was added, and the change is the
word and not the rule. It was written against OLDER containers, where the hazard is a reader that
accepts one and thereby keeps an obsolete writer alive. A NEWER version is the opposite hazard --
a reader that accepts a header shape it does not know and resolves entries out of bytes that are
not entries -- and both are refusals, so the rule holds in both directions once it is stated as
"not one it implements". §1c carries the newer-version half, with the incident that makes it
normative.

**The container version is one gate of several, and it is not the gate that decides.** Stated here
because the opposite reading is natural and has been acted on: that a reader's accepted container
versions are *the* compatibility statement, so widening them admits the containers they name. They
are not. A container's members carry their own independently versioned schemas -- `meta.dat`'s
(`internal-files.md` §"Metadata (meta.dat)"), `step-map.ns`'s, and whatever a later member adds --
and a reader has a separate accepted set for each. §1c's rule is per FIELD: refuse a `Version`,
a `Profile`, a `Compression` or a member schema the reader does not implement, naming the one that
is wrong.

Two consequences, and the second is the one that costs time if it is not written down:

1. **A reader must name the field it is refusing on.** "Unsupported version" without saying *which*
   version -- the container's or a member's -- sends the reader of the diagnostic to the wrong
   field. A reader that admits a container version and then meets an unimplemented `meta.dat`
   schema must say `meta.dat`, and in particular must not report the member as *missing*: "absent"
   and "present at a schema I do not read" are the different facts §1c is about, one field down.
2. **Widening one accepted set does not widen another, and may change nothing at all.** The two
   versions move together in practice -- each revision that changes the body tends to change a
   member schema -- so a corpus at an older container version is usually also at an older member
   schema, and admitting its container version leaves it refused one layer in. MEASURED over the
   173 containers of the workspace this document is maintained in, at the 2026-10 revision: 136 at
   container version 3 or 4, every one of them carrying `meta.dat` schema 3, 4 or 5 or no
   `meta.dat` at all; 24 at container version 5, every one carrying schema 6; no counterexample in
   either direction. So a reader that widened its container set to admit the older corpus would
   have changed the refusal message on all 136 and the outcome on none of them.

   The corollary is the one worth acting on: **"the reader refuses our corpus" does not by itself
   identify the gate to move.** Measure which field refuses, per container, before changing any
   accepted set -- and, since a gate that admits a version commits the reader to its body, measure
   that the admitted bodies decode CORRECTLY and not merely that `open` succeeds. §1c's incident is
   exactly a body that parsed and was wrong.

**Measured effect.** See `measurements/2026-10-format-efficiency.md` §"Small and empty members":
across 1,042 recordings, the mapping blocks of members that never outgrow one block, and of empty
members, are 27.4% of all container bytes, and the median container is half that size without them.
Most recordings are small, and in a small recording almost every member fits one block.

---

## 3. Base40 Filename Encoding

File names are encoded as a single u64 using base40, packing exactly 12 characters into 8 bytes.

### Alphabet (40 characters)

| Index | Character |
|-------|-----------|
| 0 | `\0` (padding) |
| 1--10 | `0`--`9` |
| 11--36 | `a`--`z` |
| 37 | `.` |
| 38 | `/` |
| 39 | `-` |

### Encoding

Right-pad name with `\0` to 12 characters. Each character maps to index `c[i]` (0--39):

```
encoded = c[0]*40^0 + c[1]*40^1 + ... + c[11]*40^11
```

Since `40^12 < 2^64`, the result always fits in a u64.

### Decoding

```
while value > 0:
    remainder = value % 40
    value = value / 40
    if remainder > 0: name += alphabet[remainder]
    else: break  // trailing padding
```

### Properties

- **Numeric sort order:** Zero-padded numeric names sort numerically as u64 values.
- **Maximum length:** 12 characters. Accommodates the internal names this specification defines, for example `meta.dat` (8), `steps.dat` (9), `linehits.tc` (11), `step-map.ns` (11), `memwrites.tc` (12). A family of members that would need more distinguishing characters than that (one name per thread, per checkpoint) is a sign the family belongs in a keyed member (§7a), keyed by a `u64`.

---

## 4. Block Mapping Model

Each CTFS internal file tracks data blocks through a hierarchical bottom-up chain.

### Parameters

```
N = BlockSize / 8              entries per mapping block (u64 entries)
usable = N - 1                 data pointers per block (last entry is chain pointer)
```

Default (BlockSize=4096): `N = 512`, `usable = 511`.

### Structure

`FileEntry.MapBlock` points to a Level-1 mapping block. Each mapping block has N u64 entries:

- `[0]` through `[N-2]`: block pointers (data blocks at Level 1, lower-level mapping blocks at higher levels)
- `[N-1]`: chain pointer to next-level mapping block (0 if none)

### Capacity

| Level | Data blocks | With BlockSize=4096 |
|-------|-------------|---------------------|
| 1 | usable | 511 |
| 2 | usable^2 | 261,121 |
| 3 | usable^3 | 133,432,831 |
| 4 | usable^4 | 68,184,176,641 |
| 5 | usable^5 | 34,862,114,263,551 |

Maximum 5 levels. With BlockSize=4096, max file size is ~133 PB.

### Block Resolution

Given byte offset `pos`:

1. If `MapBlock` is `0`, the member is empty (§2). If it carries `CTFS_DIRECT`, the data block is `MapBlock & ~CTFS_DIRECT` and `pos` is below `BlockSize` (§2). Neither case reads a mapping block.
2. Otherwise, compute `block_index = pos / BlockSize`. Determine the mapping level, follow chain pointers to that level, navigate down through mapping entries (dividing by powers of `usable`) to reach the data block. At most 5 block reads.

**The levels are cumulative (normative).** Level 1 addresses data blocks `[0, usable)`, level 2
addresses `[usable, usable + usable^2)`, and so on. The level-2 block does not re-parent the level-1
root, so its slot `[0]` does not cover data blocks `0 .. usable-1` a second time. The index is rebased
by each level's capacity as the chain is walked up:

```
idx = block_index; level = 1; node = MapBlock
while idx >= usable^level:
    idx  -= usable^level
    level += 1
    node   = node[usable]        # chain pointer; allocate on write
# then descend: at level k, entry idx / usable^(k-1) selects the child, the remainder recurses
```

The cumulative capacity of an `L`-level member is therefore `usable + usable^2 + ... + usable^L`,
slightly more than the per-level figures in the table above. The two readings produce different
bytes for every member larger than `usable` data blocks, about 2 MB at the default block size, and
a reader using the wrong one returns the wrong data blocks without failing. Every writer in the
workspace (`codetracer_ctfs`'s `block_mapping.nim`, the Rust `CtfsWriter`) lays members out this way.
This rule was stated only in `codetracer-specs`' `CTFS-Binary-Format.md` §4 until 2026-10-07.

### Block Allocation

- **Claim block:** `atomic_fetch_add(NextFreeBlock, 1)` -- the only shared mutable state.
- **Extend mapping:** When a block index exceeds current level capacity, allocate and chain a new mapping block via `[N-1]`.
- **O(1) amortized** for sequential appends. Mapping blocks allocated only when a level fills up.

**Null pointers during allocation (normative).** "Allocate a new mapping block" applies only when the slot has genuinely never been used. A mapping is filled in strictly increasing block index order, so a writer **MUST** treat a null pointer as "not yet allocated" only when the block index being placed is the **first index that pointer covers** -- the chain pointer at `[N-1]` only when the rebased index is `0` at the level it leads to, a level-`k` child pointer only when the remainder `idx mod usable^(k-1)` is `0` -- and **MUST** refuse the write otherwise. A null anywhere else means an earlier index already resolved through that pointer, so the container is damaged, and allocating a replacement overwrites the only reference to the existing subtree: every data block beneath it becomes unreachable and unrecoverable while the append reports success.

A refusal must be all-or-nothing: a writer that claims its data block before walking the mapping has to roll that claim back, so a refused append leaves the container byte-identical and the damage it refused over is still visible to a repair tool.

This binds *writers*. A **reader** meeting the same null has no index question to ask -- it simply cannot resolve the block -- and its own rule follows.

**Null block pointers on the read path (normative).** A reader resolving a stream **MUST** refuse that stream, by name, when any block number it resolves is `0`, or lies in the rest of the root region, `[1, root_blocks)` (§1): the entry's mapping root, a chain pointer, a level-`k` child pointer, or a data-block pointer. The root region holds no data and no mapping, so a pointer into it is damage of the same kind as a null. Block 0 is the container's header and root directory, and `0` is the "unallocated" sentinel, so no stream may name it. This is **independent of, and additional to**, the whole-block bound a reader applies to a container whose length is not a block multiple: a null passes that bound trivially, since `0` is below every non-empty container's block count. A reader that omits it does not merely fail to detect damage -- it walks *into* block 0 and reads the container's own header and root directory as the stream's mapping table, so entry fields decode as block pointers and unrelated blocks are returned as the stream's content.

Three consequences bind with it:

- **A null is not an absence.** A reader **MUST NOT** report a stream whose entry exists but whose mapping is null as missing, nor as empty. "Not in this container", "in this container and empty", and "in this container but its mapping is not" are three different answers. An entry lookup that signals "no such name" by returning `(Size, MapBlock) = (0, 0)` **MUST** report presence separately, because `(0, 0)` is also a legitimately empty member.
- **A null is not a truncation.** The refusal **MUST NOT** blame a truncated or interrupted tail write. A container carrying a null pointer is typically a whole number of blocks and otherwise intact, and a message naming truncation sends an operator or a repair tool after damage that is not there.
- **A caller-visible failure, not a crash.** The block number comes out of the container, so on a damaged one it is corruption-controlled. It **MUST** be refused before it is multiplied by `BlockSize`; computing the offset first can overflow and abort the process instead of returning an error.

The measurements behind both halves, and the workspace-wide reader sweeps that found them, are recorded in `codetracer-specs` `spec/Trace-Files/CTFS-Container-Notes.md` (moved there from `CTFS-Binary-Format.md` §4 and §5d on 2026-10-07).

### Diagram

```
FileEntry
  MapBlock ---------> +-------------------------------+
                      | Level-1 Mapping Block         |
                      +-------------------------------+
                      | [0]:   data block 0           |
                      | [1]:   data block 1           |
                      | ...                           |
                      | [N-2]: data block N-2         |
                      | [N-1]: chain --------+        |
                      +--------------------- | -------+
                                             v
                                +---------------------------+
                                | Level-2 Mapping Block     |
                                +---------------------------+
                                | [0]:   L1 mapping block   |
                                | [1]:   L1 mapping block   |
                                | ...                       |
                                | [N-1]: chain ------> L3   |
                                +---------------------------+
```

---

## 5. Algorithms

### Creating a File

1. Find an empty slot in the file entry array (all 24 bytes zero). Any empty slot in the root region may be used, so a reader cannot assume members appear in slot order (§6).
2. Encode the filename using base40 and write to the `Name` field. Leave `Size` and `MapBlock` as zero. Claim no block: a member that is never written stays `(0, 0)` (§2).
3. Publish the entry for concurrent readers (§6, "Root publication").

If no empty slot remains, the creation fails, naming the member and the occupancy (§1, "The root
directory is fixed at creation", rule 3). There is no fallback.

### Appending Data

Four cases based on current file state:

1. **First write** (`MapBlock = 0`): claim a data block `b`, write the bytes, set `MapBlock = CTFS_DIRECT | b`. If the first write is longer than one block, it is case 3 applied to an empty member: claim the mapping block first, then the data blocks.
2. **Direct, fits** (`MapBlock` tagged, the new size `<= BlockSize`): write into the existing block.
3. **Direct-to-mapped transition** (`MapBlock` tagged, the new size `> BlockSize`): claim a level-1 mapping block, store `b` in its slot 0, claim and write the new data block(s) and record them in the mapping, then store the untagged mapping block number in `MapBlock`. Claims are made in that order -- mapping block, then data blocks in file order -- so two writers given the same appends allocate the same blocks.
4. **Mapped** (`MapBlock` untagged and non-zero): resolve/allocate through the mapping hierarchy.

After writing: atomically update `FileEntry.Size` (makes data visible to readers). Data must be fully written before Size is updated. Write barriers enforce ordering. In case 3 the store of `MapBlock` precedes the store of `Size`, with a barrier between them, so a reader that observes the new `Size` observes the mapped `MapBlock` (§6).

### Reading Data

- Return EOF if `offset >= file_entry.size`.
- Clamp read length to file size boundary.
- For each spanned block: resolve it as §4 says -- the tagged block directly, otherwise through the mapping hierarchy -- and read its bytes.

### Adding Members To A Closed Container (normative)

The algorithms above describe a writer that is still building its container. A second operation adds
members to a container that has already been closed: the file is complete, no writer holds it open,
and a producer of derived data (computed from a finished trace, and specified to live inside the same
`.ct`) adds its members. Without this operation such a producer would write a sidecar file or
re-implement the container layout, and both are wrong.

**Reopening.** The appender recovers the writer's state from the bytes:

- `BlockSize`, `MaxRootEntries` (a `0` means auto-fill, §1) and so `root_blocks` come from the
  header. The root may be any number of blocks the writer declared.
- `NextFreeBlock` is not stored. It is recovered as `file_length / BlockSize`. That is sound only
  because every allocated block is materialised on disk, so a closed container is a whole number of
  blocks. A file length that is not a block multiple means the container is truncated or still being
  written, and the appender MUST refuse it rather than round: a wrong `NextFreeBlock` overwrites live
  data. A **reader** MUST accept the same file, computing `floor(file_length / BlockSize)` and ignoring
  the partial tail, because a crash inside an appender's tail write leaves exactly that shape (below).

The append then runs "Creating a File" and "Appending Data" unchanged. It MUST additionally:

- **Refuse a name that already exists.** Replacing a member is not an operation; a half-replaced
  member is a silent wrong-bytes failure.
- **Refuse an encrypted container,** whose mapping is opaque without the key.
- **Refuse a batch the directory cannot hold whole.** If the root region has fewer empty slots than
  the batch has members, the append fails before it writes anything, naming the first member that
  does not fit and the occupancy. It does not grow the directory (§1, rule 2) and it does not attach
  part of the batch.
- **Validate every name against the base40 alphabet first.** §3's encoder maps an out-of-alphabet
  character to the padding index, so an unvalidated `"snap!pages"` is stored as `"snap"`.

**Write ordering.** Every new data and mapping block is written first, from the previous end of file.
The entries are written last, and they are the only bytes of the root region the append writes: the
header is not rewritten (§1, rule 1). A crash in between leaves unreferenced trailing blocks, which
waste space but leave the container readable, rather than an entry pointing at absent data. **Nothing
in `[root_blocks, previous end of file)` is ever rewritten,** so an interrupted append cannot damage an
existing member. (This held before October 2026, stopped holding when a growing append rewrote
everything from the new root to the old end of file, and holds again because growth is retired.)

**Batching.** A set of related members is appended in one call, and its entries are written by one
write call covering the changed entry bytes. Within one process that write either happens or does
not, so a crash never leaves part of the set attached. It does not make the set atomic to a reader
that re-reads the directory while the write is in progress; such a reader can observe a prefix of
the set. An implementation should not offer a one-member-at-a-time form of this operation.

**A crash inside the tail write.** The tail can be megabytes written in one extending write, so a
crash can land inside it and leave a file whose length is not a block multiple. The root region still
holds the previous entries, each pointing below the previous end of file, and the trailing fragment is
unreferenced. A reader accepts that file, as above. An appender refuses it, because it cannot tell a
partial tail from a container still being written. Flooring is a bound and not only arithmetic: a
reader applies `floor(file_length / BlockSize)` to every block number it resolves -- the mapping
root, every mapping block it walks, and every data block -- because the last data block's read is
clamped to `Size`, so a short read out of the partial region otherwise succeeds and returns wrong
bytes.

**The ordering is a durability claim, so test it as one.** Reversing the two phases leaves the final
bytes identical, so a test that compares the container before and after an append cannot see the
ordering. An implementation should be able to abandon an append between its phases and assert on the
file that leaves: the entries unchanged, and the file already grown.

The reference implementation is `codetracer-trace-format-nim`'s `codetracer_ctfs/container_append.nim`,
exposed to C consumers as `ct_container_append_files`.

### Extending A Member Of A Closed Container (normative)

An appender may also add bytes to the end of a member that already exists in a closed container (a
derived stream that grows by later passes). It reopens the container as above, refusing the same
encrypted and non-block-multiple files, and recovers the member's write state from its entry:

- The member's data block count is `ceil(Size / BlockSize)`. The last data block is **pending** if
  `Size` is not a block multiple: the new bytes are written into its unused tail first, in place, and
  then into new blocks. The bytes of that block past the old `Size` were never content, so no reader
  has read them.
- The walk that locates the last block and the mapping slots to extend applies §4's null-pointer
  rules: a null that is not the first index its pointer covers is damage, and the append refuses it.
- The new `MapBlock` (on a direct-to-mapped transition) is stored before the new `Size` (§5,
  "Appending Data").

A block cache over a container that may be extended this way MUST treat the extended member's last
data block and its mapping blocks as changed (§6, "What is mutated in place").

---

## 6. Concurrent Access (Threading Model)

### Multi-Producer Block Allocation

Multiple threads append to different internal files concurrently. Each file has a **single writer**. The only shared mutable state is `NextFreeBlock`:

```
Writer A (steps.dat):                    Writer B (events.dat):
  atomic_fetch_add(NextFreeBlock, 1)       atomic_fetch_add(NextFreeBlock, 1)
  → gets block 5                           → gets block 6
  write data to block 5                    write data to block 6
  update mapping for steps.dat             update mapping for events.dat
  atomic_store(steps.Size, new_size)       atomic_store(events.Size, new_size)
```

No locks, mutexes, or CAS loops -- only atomic fetch-and-add and atomic stores.

### Writer Protocol

1. Claim block(s) via `atomic_fetch_add(NextFreeBlock, count)`
2. Write data to claimed blocks
3. Update mapping block pointers (thread-local, no contention); on a direct-to-mapped transition, store the untagged `FileEntry.MapBlock` (one atomic 8-byte store)
4. Write barrier
5. Atomically store new `FileEntry.Size`
6. Publish the entry for concurrent readers (below)

**Publishing a partial block does not consume its logical block index (normative).** Step 6 lets a
reader see bytes that do not yet fill a block, and the writer normally keeps appending afterwards.
Those later bytes belong to the same logical block, because a reader resolves byte `p` to logical
block `p / BlockSize` and nothing in the entry records where a flush happened. So the partial block is
**pending**: allocated once, linked into the mapping at its index, rewritten in place at each further
flush, and counted only when the buffer fills it. A writer that instead allocates a fresh block for
the partial content, and advances its block count, places every later byte one block too early: the
reader serves the flushed block's padding as content and loses as many real bytes off the end. (The
Rust `ConcurrentCtfsWriter::flush` did exactly that until it was fixed; the measurement is in
`codetracer-specs` `spec/Trace-Files/CTFS-Container-Notes.md`.)

### Root Publication

An entry is published by writing its 24 bytes into the root region, or a root block that contains
them, in the order of the writer protocol: a new entry's `Name` together with zero `Size` and
`MapBlock`; after that, `MapBlock` (when it changes) before `Size`. Three rules follow from §1 and
from the atomicity argument of §2:

- **Every entry word is stored and loaded as one 8-byte unit, and lies within one block.** Entry
  `i` starts at `16 + 24 * i` (`24 + 24 * i` at version 6), so each of its three words is 8-byte
  aligned (§1). The pairing argument of §2 is stated per word. Whether a reader in another process
  sees each word whole is the subject of "In-place record publication" below.
- **A publication writes what changed, not the whole root region.** A writer SHOULD write only the
  entries, or the root blocks, that changed since its last publication, and MUST NOT rewrite the
  header (§1, rule 1). A writer that rewrote the whole region at every sealed chunk wrote 256 KiB per
  seal for a 64-block root, against the 69 µs per seal measured below for a one-block root.
- **One publisher per root block.** When several writers share a container, a write of a whole root
  block carries every entry in that block, so it comes from the one publisher that holds the current
  value of each of those entries. A writer that rewrote a root block from a stale copy would undo
  another writer's newer `Size`. Under a multi-process writer architecture the root region is not
  written from copies at all ("Writer architectures", below).

### In-Place Record Publication: The Live Coordination Page (normative)

Some words are stored in place while a reader may be reading them: a root entry's `Size` and
`MapBlock`, a keyed member's record words (§7a), and the index words a keyed family's realization
updates in place ([ctfs-keyed-families.md](ctfs-keyed-families.md) §4.3). This section is the rule
for all of them. A mapping slot and a pending block's bytes past `Size` are not covered: the first
changes once from `0` and is validated as a pointer (§4), the second is never content until a later
`Size` covers it.

**The gap.** The reader this format serves lives in another process and reads with
`pread`/`ReadFile`, or through a read-only mapping. Until 2026-10-08, §2 and §6 argued correctness
"per word", assuming that an aligned 8-byte store is observed whole by such a read. No operating
system the format runs on promises that for a file: POSIX's `read`-against-`write` atomicity (XSH
2.9.7) is stated for threads and Linux does not honour it for reads against writes; Windows makes no
such promise for `ReadFile`; network file systems promise less. A torn `Size` can exceed both its old
and its new value (`0x00FF` and `0x0100` tear to `0x01FF`) and send a reader past the data. It holds
in practice for aligned words on local file systems, and that is all.

**The rule: while a container is being written, its live state lives in shared memory.** Every
container that is being written has a **live coordination page** (below, "the page"): a
shared-memory region, one per container, that holds the container's live root state and the live
copies of every word updated in place, each under a sequence counter. Writers publish there first;
readers on the same machine attach to it and read consistent values with the ordinary memory
atomics shared memory provides. The file receives the same words at each seal, for durability and
for readers that cannot attach. When the container is closed the page is gone and the file is
static, so no read of a closed container can tear.

The file's footprint does not change: no sequence word is stored in the file, the root entry stays
24 bytes, and no bit of `MapBlock` is used for this.

> **Rejected: sequence counters on disk.** A draft of 2026-10-07/08 proposed a per-record sequence
> word stored in the file (a seqlock on disk), with, for root entries, either a 32-byte entry or a
> counter in `MapBlock` bits 48 to 62, and a checksum variant. The owner rejected all of them:
> "I see more and more value in the idea of having a shared memory page for coordination between
> multiple writers and multiple readers while the file is still being written. We can potentially
> implement these safety mechanism in the shared memory without increasing the footprint of the
> files." The protection is needed only while a file is being written, by readers on the writer's
> machine, and the page gives it there without growing every container for its whole life.

#### The page: naming and discovery

- **Identity.** The page belongs to one container file, identified by the file's **identity**: on
  Linux the device and inode numbers and, where the file system reports it, the birth time
  (`statx` `stx_btime`); on macOS the device, inode and birth time (`st_birthtimespec`); on Windows
  the volume serial number and the 128-bit file id (`GetFileInformationByHandleEx`, `FileIdInfo`).
- **Name.** `ctfs.` followed by the first 24 lowercase hexadecimal digits of SHA-256 over the
  identity's bytes (each field little-endian, in the order above). On Linux and macOS the object is
  POSIX shared memory, `shm_open("/ctfs.<24 hex>")` -- 30 characters, inside macOS's 31-character
  limit; on Windows a named, pagefile-backed section, `CreateFileMappingW(INVALID_HANDLE_VALUE, ...,
  L"Local\\ctfs.<24 hex>")`. `Local\` scopes it to the logon session; a reader in another session
  cannot attach (below).
- **Discovery.** A reader that opens a container computes the identity from its own open handle
  (`fstat`/`statx`, or the handle's file id), derives the name and tries to open the object
  read-only. It **attaches** only if the object exists, its magic and version are ones it reads, the
  identity stored in the page equals the identity of the file it opened, and the page's state is
  `live`. Anything else -- no object, a mismatch, `closed`, `abandoned` -- means "not attached", and
  the reader follows the file alone (below). The full identity in the page, not the hash in the
  name, is what binds the page to the file; a name collision or a reused inode is caught there.

#### The page: contents

The page is a header and a set of mirror slots, all fields little-endian and 8-byte aligned:

| Part | Holds |
|---|---|
| header | magic `CTLP`, version, the container identity, the page's capacity, `state` (`creating`, `live`, `closed`, `abandoned`), and `seal_epoch`, incremented after every seal's file writes complete |
| writers | one slot per writer process: process id, process start time (to tell a reused pid apart), a heartbeat updated at least once per second while it writes, and the `seal_epoch` of its last completed seal |
| allocation | `NextFreeBlock`; one counter per dense key family whose keys are assigned live (§7a); a bump counter for mirror slots |
| root mirror | one slot per root entry: `seq`, `Size`, `MapBlock`, `Name` |
| record mirror | slots for in-place-updated records of keyed members: a table of mirror extents `(member entry index, first record number, record count, first slot)`, and the slots themselves, each `seq` followed by the record's in-place words |

The capacity is fixed at creation and RECOMMENDED to be generous (16 MiB virtual by default): POSIX
shared memory and Windows pagefile-backed sections commit physical pages only when touched, and a
macOS shared-memory object cannot be resized after its first `ftruncate`. A record whose mirror slot
cannot be allocated because the page is full is followed by attached readers at seal granularity,
as a reader that cannot attach follows it; the page records that it overflowed.

#### Writing through the page

*Per in-place update* (a root entry's `Size`/`MapBlock`, a record's words), by the word's one writer
(§6, "Multi-producer block allocation"; §7a):

1. Every byte the new value makes reachable -- data, mapping blocks, a new record -- is written to the
   file first (`pwrite`/`WriteFile`, or through a mapping where that mapping is coherent with the
   readers' reads, see "Writer architectures").
2. In the page: store `seq + 1` (odd), with release ordering; store the payload words in the order
   the record kind requires (for a stream reference, `MapBlock` before `Size`); store `seq + 2` (even),
   with release ordering. These are ordinary aligned stores to shared memory, which every supported
   processor makes atomic per word and orders by the stated barriers, across processes as across
   threads.

*Per seal* (§6, "Durability"; at least as often as the durability rule requires):

3. Write the seal's changed root entries and records to the file, each a whole entry or record per
   write call, data before the entry that publishes it, as today.
4. Increment the writer's `seal_epoch` in its writer slot, then the page's `seal_epoch`.

So the page may run ahead of the file between seals (an attached reader can see a partial-chunk
flush the file does not yet publish), never behind it, and the file is exactly as durable as §6
"Durability" requires whether or not the page survives. A new record is written whole into the file
before it is reachable from the page or the file (§7a, rule 3).

#### Reading

**An attached reader** reads every word that can change in place from the page:

1. Load `seq` as `s1`. If `s1` is `0`, the word was never published. If `s1` is odd, retry.
2. Load the payload words.
3. Load `seq` as `s2`. Accept the payload if and only if `s2 == s1`; otherwise retry.

It retries a bounded number of times (8 is RECOMMENDED) and otherwise keeps the last value it
accepted for this refresh. It reads data, mapping and record bytes from the file, which step 1 of the
writer put there before the page published them. It re-checks the page's `state` at every refresh;
on `closed` it re-reads the root from the file once more and is then reading a static file.

**A reader that cannot attach** -- on another machine, through HTTP range requests, in a sandbox
without access to the writer's shared memory, in another Windows logon session, or because the page
overflowed for the record it follows -- follows only what the file publishes at seals, under this
torn-read rule:

- It reads each in-place word it depends on twice, the second read issued after the first completes,
  and accepts a value only when both reads agree, the value is not below the last one it accepted
  (`Size` never decreases), and every block number it resolves passes §4's checks.
- Where the member's format can validate content it does: a chunk index's offsets are monotone and
  below the data member's `Size`; a frame decodes to its declared size (§7, "Reading the last
  chunk"); an offset table's last entry does not exceed its data member's `Size`.
- Its results are **provisional until the container is closed**. A torn read that both reads agree on
  is not excluded, only made improbable; after close the file is static and a re-read is exact. It
  learns that the container is closed out of band (a server that attaches on the writer's machine
  and says so, or the end of the recording reported by the tool that made it) or by quiescence: the
  file's length and root region unchanged over an interval the reader states.
- A server that serves a container being written to remote readers (an HTTP range source of a live
  recording) SHOULD run on the writer's machine, attach to the page, and serve only state it read
  consistently there, so that its clients are not subject to this rule.

#### Lifetime, and writers that die

- **Creation.** The process that creates the container creates the page (exclusively: an existing
  object of the same name is opened and inspected first; if its writers are all dead it is removed and
  recreated, otherwise creation fails, naming the container), sets the identity, registers itself as
  a writer, sets `state = live`, and only then publishes the root region for the first time. A closed
  container reopened for appending (§5) gets a page for the duration of the append, created the same
  way.
- **Close.** The last writer to finish performs the final seal (every in-place word written to the
  file, every odd `seq` settled), sets `state = closed`, and removes the name (`shm_unlink`; on
  Windows the section disappears when its last handle closes). Readers still mapping the old page see
  `closed`.
- **A writer that dies.** A writer is dead when its pid no longer names a process with its recorded
  start time, or its heartbeat is older than 10 seconds. Its members stop growing; the file holds
  them through that writer's last seal, by the durability rule, with nothing else needed. Under W2 a
  surviving writer or the coordinator marks the slot dead, settles the dead writer's odd `seq` words
  in the page to the last value whose payload the crash rule accepts (below), and carries on.
- **Every writer dies.** The file is valid through each writer's last seal. On Windows the page
  disappears with the last handle. On Linux and macOS the object outlives the processes; the next
  process to open the container (a reader or a tool) finds every writer dead, sets
  `state = abandoned` and removes the name. A **salvage** tool MAY, before removing it, copy page
  values into the file's entries when the page shows a later `Size` than the file and every byte that
  `Size` covers is in the file (writer step 1 put it there before the page published it); salvage is
  an improvement, never a requirement of durability.
- **The crash rule.** A writer that dies between the odd and even stores of an update leaves `seq`
  odd. A record kind states whether every prefix of its payload stores is itself a valid record
  (**prefix-valid**); a stream reference is (§2's `MapBlock`-before-`Size` order), and so is a
  single-word value. For a prefix-valid record the settled value is the page's payload; otherwise it
  is the last value the file holds. Nothing in the file ever depends on a `seq`.

### Reader Protocol: Opening And Following A Container (normative)

This is the one protocol for every reader. A reader of a closed container performs the open steps
once. A reader that **follows** a container being written (a reader that offers following; none is
required to) refreshes it, and a refresh is the same steps again. Whether the reader is attached to
the live coordination page changes only where it reads the words that change in place (step 2): an
attached reader reads them from the page under its sequence counters; a reader that cannot attach --
the **file-only follower** -- reads them from the file under the torn-read rule (above). Everything
else, data, mapping and record bytes and every refusal, is read from the file and is identical for
both. (Following was first specified for the file-only follower, in 94b7dcc; this section is that
rule with the attached reader added, not a second protocol.)

**Opening.**

1. **Read the header once.** Version, block size, `MaxRootEntries` (applying auto-fill, §1) and
   `root_blocks` are fixed for the container's life (§1). A reader MAY keep them from the open. A
   reader that may follow then tries to attach to the live coordination page (above).
2. **Read every slot of the root directory.** A member can be created in any empty slot at any time,
   so a reader reads all `MaxRootEntries` slots, not only those after the last one it saw. For each
   slot it takes `Name`, then `Size`, then `MapBlock` (with acquire ordering) -- from the page's root
   mirror when attached, from the file under the torn-read rule otherwise. A slot whose `Name` is zero
   is empty.
3. **Resolve each member as §2 and §4 say,** refusing a block number that is `0`, in
   `[1, root_blocks)`, or past the container's whole blocks. Across a direct-to-mapped transition,
   loading `Size` before `MapBlock` rules out pairing a new `Size` with the old, tagged `MapBlock`,
   because the writer stored `MapBlock` first; the remaining mixed pairing, an old `Size` with the new
   mapping, reads correct bytes, because the mapping's slot 0 is the old data block.
4. **For a chunked stream, read its companion index** (§7). The readable records are those of the
   chunks the index publishes. Every published chunk but the last ends at the next entry's offset; the
   last ends at the end of its Zstandard frame (in a compact container, at the end of the member),
   never at the data member's `Size`, because a writer may already have written part of the next
   chunk (§7, "Writer Protocol").
5. **Follow keyed members by their family's rules** (§7a;
   [ctfs-keyed-families.md](ctfs-keyed-families.md) §5), reading their in-place words as step 2
   reads entries, and a namespace through its root slots (§8, "Live and incremental publication").

**A refresh** repeats step 2 and then:

- takes a member that has appeared since the last refresh as readable from now on -- a stream created
  lazily, `calls.dat`, and the close-time members (`step-map.ns`, `spantype.ns`, `linehits.tc`,
  `corrmark.ns`, `entry.dat`) once the writer has closed;
- extends each chunked stream it has opened by the index entries published since (step 4);
- picks up the growth of the interning tables, whose records the newly published chunks may refer
  to ("Durability" rule 2 publishes them first);
- re-reads the keyed members it follows (step 5).

After a refresh the reader answers exactly what a fresh open of the container at that moment would,
by a reader of the same kind: a fresh file-only open, or a fresh attached open (an attached reader can
see in-place words the page published ahead of the file's last seal; a file-only reader sees the
file as of its last seal). A refresh SHOULD NOT decode again a chunk it has already decoded, and
SHOULD NOT read again the members it has already read beyond their new bytes; what a refresh costs
should grow with what is new, not with the recording.

**A refresh MUST refuse, naming the member, and keep answering from its previous state**, when the
container has changed in a way no writer produces: a member's `Size` that decreased, or a member that
disappeared; a root entry whose name changed; a stream whose `chunk_size` changed; a published index
entry that changed or disappeared; offsets that decrease; a last offset past the data member's `Size`;
a keyed member's record that disappeared or whose key changed. It MUST NOT treat a shrunken `Size` as
a truncation to tolerate.

### What Is Mutated In Place, And What A Cache May Keep (normative)

Nothing is ever relocated (§1), so a block number names the same block for the container's life. A
block's **content** can still change in place, in exactly these cases:

| What | Changes how | Written by |
|---|---|---|
| The root region | entry words: `Name` once at creation; `MapBlock` and `Size` as the member grows, at seals | the container's publisher (§6, "Root publication"; "Writer architectures") |
| A member's last data block, while `Size` is not a block multiple | bytes past `Size` are filled in (the pending block above) | the member's writer; also a closed-container extension (§5) |
| A mapping block | a slot that was `0` is set, once, in increasing index order; a set slot never changes | the member's writer |
| A keyed member's record blocks | a record's in-place words, at seals (§7a) | the keyed member's writer |
| A keyed member's index blocks | only the words its realization names as updated in place: fill-once slots, node words, root slots ([ctfs-keyed-families.md](ctfs-keyed-families.md) §4.3) | the keyed member's writer |
| A namespace's page 0 | the root slots (§8) | the namespace's writer |

Everything else is written once. So a cache keyed by block number:

- MAY keep a data block of a member for good once that block lies wholly below an observed `Size` of
  the member, except a block of the kinds in rows 4 to 6 of the table;
- MAY keep a mapping block, but only its non-zero slots: a slot it holds as `0` must be re-read;
- MAY keep a filled fill-once slot of a keyed member's index for good, by the same argument;
- MUST re-read the root region, a member's last partial data block, the record and index blocks a
  keyed member's realization updates in place, and a namespace's page 0 at every refresh.

The same rules apply to a container that is closed but can still be appended to (§5). A remote or
partial cache (`.ctp`, an HTTP block cache, a RAM cache) of such a container re-reads the root region
when it revalidates, and drops the changeable blocks of every member whose `Size` changed.

### Dead Space (normative)

A block is **dead** when no published pointer reaches it: no entry, mapping block, record or index
node of the current state names it. Because nothing moves and nothing is reused while a container may
be read (§1, rule 2), dead blocks are never reclaimed in place. They are bounded instead, and removed
by copying.

**Where dead space comes from**, and the bound each source MUST state:

| Source | Bound |
|---|---|
| A copy-on-write index (a namespace commit, §8; a replaced trie or hash node, [ctfs-keyed-families.md](ctfs-keyed-families.md) §4.3) | the realization states its dead bytes per operation (for a B+tree commit, one root-to-leaf path) and its total at close as a fraction of live index bytes |
| A live index dropped or rebuilt at close (§5.6 of the families document) | the dropped structure's size, stated by the realization |
| A member rewritten whole (a query-time cache persisted again, for instance) | the previous image, every time; a producer that rewrites whole MUST state how many images it can abandon over a container's life, or repack (below) |
| Blocks allocated and never published (a writer that died, §6 "Writer architectures"; an append that failed) | the blocks in flight per writer at the moment of failure |
| A partial last block | not dead: it is the member's pending block |

**Reuse.** Block-level reuse inside a container that a reader may follow, or that a cache may hold,
is not an operation: a reused block number would name different bytes over time. A namespace's
in-member free chain (§8) is reused only where §8 allows (no other process can read the file). There
is no container-level free list, and block 0 holds none (§1).

**Reclamation is repacking.** A **repack** copies every live member of a closed container into a new
container -- the same copy operation as an export or a compact conversion -- which has no dead blocks
by construction. A producer whose dead fraction can exceed a stated threshold (RECOMMENDED: 25% of the
container) repacks at close, or offers the repack as a tool; a reader is never asked to tolerate a
dead block, because it never reaches one.

### Writer Architectures (normative)

A **container writer** is a process that allocates blocks of the container, writes member bytes, or
publishes root entries. Two architectures, and a hybrid, are specified. In every one the live
coordination page is the shared state; a producer uses W1 unless the benchmark decision of
[ctfs-keyed-families.md](ctfs-keyed-families.md) §8 admits W2 or the hybrid for it.

**W1: one container writer.** One process writes the container and hosts the page: it creates the
page, is its only registered writer, and keeps `NextFreeBlock` and every family counter there (its
threads update them with the same atomics they would use in private memory; readers use them to
measure lag). Producers in other
processes hand it their data by means of their own, which this format does not specify. Every rule
above applies as written.

**W2: several writer processes.** Each producing process writes its own members directly into the
container file, at offsets, with `pwrite` or `WriteFile`, into blocks it allocated. The processes
coordinate through the page. A container written under W2 obeys these additional rules:

1. **Cross-process atomics.** Every writer maps the page read-write. Block allocation is an atomic
   fetch-and-add on `NextFreeBlock` in the page (`__atomic_fetch_add`, `InterlockedExchangeAdd64`); an
   entry slot, a family key and a mirror slot are claimed the same way. Only lock-free, address-free
   atomics are used, so each is the same instruction it is between threads, and the processor's
   ordering rules apply across processes unchanged (x86-64 gives release and acquire for aligned plain
   stores and loads; arm64 needs `stlr`/`ldar` or explicit barriers). No lock and no CAS retry loop
   (G3).
2. **How root state reaches the file.** Each writer writes only its own members' entries and records
   to the file, each as one write call of the whole entry or record, at its seals (writer step 3
   above); no writer writes a whole root block. The header is written once, at creation (§1).
3. **Data reaches the file before the page publishes it.** A writer writes data with
   `pwrite`/`WriteFile`. If it writes through a file mapping instead, it does so only where the
   mapping is coherent with the readers' reads: on Linux and macOS a `MAP_SHARED` mapping and `pread`
   share the page cache on a local file system; Windows documents no coherence between a mapped view
   and `ReadFile`, so a Windows writer that uses a view publishes nothing through the page until it
   has written the same bytes with `WriteFile`.
4. **Durability (G5) when any writer dies.** Each writer's members are readable through its last
   seal, because data, mapping and record are written before the entry that publishes them, and the
   page is never needed to read the file.
5. **What a dead writer leaves, and what readers tolerate.** (a) **Holes:** blocks it allocated and
   never wrote. They are unreferenced and dead ("Dead space"); inside the file they read as zeros (a
   sparse region where the file system supports one), past the end of the file they do not exist. No
   published pointer names one, so a reader never reaches one; a reader that does is reading damage and
   refuses it by §4's rules. (b) **A partial chunk** in a pending block: bytes past the published
   `Size`, never content. (c) **A word left mid-update** in the page: settled by the crash rule. The
   survivors do not finish the dead writer's members. The last writer at close extends the file to a
   whole number of blocks, so that §5's appender sees a block-multiple length.
6. **Keys assigned by a shared counter depend on timing.** A reader or a replay takes such a key from
   the container and never allocates it again ([ctfs-keyed-families.md](ctfs-keyed-families.md) §6.3).
7. **One writer per member still holds (G3).** A member, and everything a keyed member owns, is
   written by one process. A family whose elements several processes create is partitioned into
   per-process members, or has records each owned by one process.
8. **No byte-reproducibility.** Blocks are claimed in the order the processes reach the counter, so a
   W2 container is not a function of the recording ("Block placement"): two runs place blocks
   differently, though both read identically. A producer that claims block-placement determinism
   cannot use W2 or the hybrid unless it serializes appends in recording order.

**The hybrid** keeps the root region and every family key allocator with one coordinator process, and
lets producers allocate blocks (through the page's counter) and write their own members' data and
mapping blocks; a producer hands the coordinator `(member, MapBlock, Size)` to publish, in the page
and at seals in the file. Rules 1 to 7 apply, with the coordinator as the only writer of the root
region.

### Guarantees

- **Writers:** Block allocation is atomic. Data fully written before mapping updated. Size updated only after data committed. Every in-place update is published through the live coordination page and reaches the file at the next seal. No block is ever moved or reused, and the root region never grows.
- **Readers:** An attached reader sees a previous or a new value of every published word, never a mixture. A reader that cannot attach sees seal-published state under the torn-read rule, provisional until close. After close the file is static. All data up to an accepted Size is valid. A block number a reader has resolved keeps naming the same block. No locks required.

### Live progress: per-stream following, no sidecar

A concurrent (out-of-process) reader answers "how much is readable right now" from
the **container itself**, per-stream, with no external artifact:

- **Readable byte extent** of each stream is `FileEntry.Size`, updated atomically
  after each commit (re-read periodically — Reader Protocol step 2).
- **Readable record count** is derived from the companion `.idx`, which is written
  incrementally as chunks seal (§7; a stream that seals partial chunks carries a
  cumulative record count in its index). No finalization step is required.

This is the **only** progress mechanism. The format is self-contained (G1:
"no external files or JSON"), so there is **no `.head.json` (or any) sidecar** and
**no in-process RPC** on the recorder to answer progress queries — a recorder must
not run an event loop to serve reads. When a live coordinator needs aggregate,
cross-stream progress state (e.g. a recording/replayable *frontier* expressed in
global event ids or checkpoints), that state is published through the recorder's
existing **shared-memory channel** consumable by an out-of-process observer — not
baked into the container and not served by an RPC on the writer.

Per YAGNI, no aggregate frontier field is defined in the container: per-stream
following covers every reader that exists today. A future consumer that genuinely
needs a single cross-stream frontier value from the `.ct` would motivate a
deliberate, versioned in-container addition — not a sidecar.

**Following a container** -- what a refresh re-reads, where the last published chunk ends, what it
must refuse, and that it answers as a fresh open would -- is §6, "Reader Protocol: opening and
following a container", for attached and file-only readers alike.

**Stream presence is structural, not flag-gated.** The same principle governs
*whether* a stream exists, not only how much of it is readable. Whether a trace
carries `steps.dat` / `spans.dat` / any optional stream is answered by
`findFile("<stream>.dat")` on the file-entry array — the authoritative,
streaming-correct source. The `meta.dat` stream-presence flags (bits 8..13 and bit 15, see
internal-files.md → "Stream-presence flags are a hint, not a gate") are an
optional, tautological hint and MUST NOT be used as a read gate: a writer may
only learn a stream is non-empty near the end and stamp its bit at close, so a
reader gating on the bit could not read a stream that structurally exists in a
still-recording trace. A reader resolves each optional stream by structural
presence + `FileEntry.Size`, exactly as it follows live progress.

### Durability: a writer publishes every sealed chunk

A writer that writes a container to a file (not one that builds it in memory) MUST keep the file
readable while it records, so that a recording whose process dies -- killed, crashed, out of
memory -- leaves a container a reader opens and reads up to the last chunk it completed.
Concretely:

1. **At open** -- at the latest, before the recording's first record -- the writer writes the whole
   root region, with its final header (§1), and `meta.dat`, complete: `meta.dat` is never rewritten,
   so every field and flag in it is fixed at open (`internal-files.md` §"Extended flags
   (`flags_ext`)"). A keyed member is created, with its header if its realization has one (§7a), no
   later than its first record.
2. **When a chunk of a Chunked Compressed Table seals** (§7: it reaches `chunk_size` records), the
   writer writes, before the append that sealed it returns: the chunk's bytes to their data blocks,
   every mapping slot or block that changed, the chunk's offset in the companion `.idx`, every
   interning record registered so far (appended when it was registered: "Block placement" below) (`paths`, `funcs`, `types`, `varnames`, `markers` and their
   offset tables -- so that whatever the chunk refers to is readable), and then the root entries (`Size`,
   `MapBlock`) of every member it grew -- in that order, data before the entry that publishes it
   (§6, "Writer Protocol"). A chunk of a stream that a keyed member's record references (§7a) is
   published the same way, one level down: the chunk, its mapping, the record's `MapBlock` and `Size`
   by "In-place record publication", and then the keyed member's own entry if the member grew. The
   entries written are those that changed
   (§6, "Root publication"); a seal does not rewrite the root region.
3. **Within a chunk, buffering is allowed.** The records of a chunk that has not sealed may live
   only in memory; a crash loses them, and nothing earlier.
4. **Close-time members** -- `step-map.ns` and any other index built from the whole recording --
   are written at close. A crashed container lacks them, and readers fall back as they do for any
   container without one. `calls.dat` is a chunked table in call-key order whose records complete
   when their calls return, so its first chunk seals only once its 256 lowest-key calls have all
   returned -- in practice, at close, since the root call is among them. A crashed container
   therefore usually carries no call records; its steps, values and events are still readable.
5. **"Written" means handed to the operating system** (`write`/`pwrite`): it survives the death of
   the process, not the loss of power. A writer need not `fsync`, at a seal or at close; a caller
   that wants power-loss durability syncs the file itself.

A writer MAY write more often than this -- the Nim writer historically wrote every append through
-- but not less: the Rust writer kept every split stream in memory until `finish`, so a recording
whose process died left nothing at all.

**Cost (measured, `measurements/2026-10-format-efficiency.md` §"Durability").** Replaying the 1,042
corpus recordings as a writer would emit them: writing each container once at close took 0.53 s in
total; publishing at every seal as above, with no attempt to coalesce writes, took 3.30 s -- 2.8 s
more for 202 MB and 5.6 million exec records, about 0.5 µs per exec record or 69 µs per seal, on a
btrfs NVMe file system. Adding an `fdatasync` at every seal took 110 s, which is why rule 5 does not
require one.

### Block placement: the container is a function of the recording

Two writers given the same recording write the same container file, byte for byte, block placement
included. A writer claims a block only by appending to a member (§5), and each append claims blocks
as §2 and §4 lay them out -- a member's first block direct, then on growth its level-1 mapping block
before the new data blocks, then chained levels -- so the file is determined by the sequence of
appends. A split-stream writer appends exactly as follows, and in no other order:

1. **At open**, before any record: the root region; the members `paths.dat`, `paths.off`,
   `funcs.dat`, `funcs.off`, `types.dat`, `types.off`, `varnames.dat`, `varnames.off`, `steps.dat`,
   `steps.idx`, `values.dat`, `values.idx`, `calls.dat`, `calls.idx`, `events.dat`, `events.idx`, in
   that order (creating a member claims nothing); record 0's offset, eight zero bytes, appended to
   `paths.off`, `funcs.off`, `types.off` and `varnames.off`, in that order; then the index header
   appended to `steps.idx`, `values.idx`, `calls.idx` and `events.idx`, in that order.
2. **When a value is interned** (a path, a type, a variable name, a correlation-marker label): its
   record is appended to the table's `.dat` (nothing for an empty record), then its end offset to the
   `.off`. `markers.dat` and `markers.off` are created at the first label, and `markers.off` gets its
   record-0 offset then.
3. **A function record** is appended as soon as it and every function with a lower id have a
   registered declaration path: at its registration when its path is registered, otherwise right
   after the record of the path that registers it, in id order.
4. **`meta.dat`** is created and written at the recording's first record (anything other than an
   interning registration).
5. **A chunk**: when a chunk seals, its bytes are appended to its `.dat`, then its index entry to its
   `.idx`. An exec record and its value record are appended when the step is complete, exec record
   first, so a step that seals both streams appends the `steps.dat` chunk and entry, then the
   `values.dat` chunk and entry. A call record is appended when it and every call with a lower key
   have returned; an I/O event record when it is registered; a span chunk seals at its record limit
   or at a flush (`internal-files.md` §"`spans.dat` / `spans.idx` / `spantype.ns`").
6. **At close**, in this order: `meta.dat`, if no record was made; the remaining function records,
   each registering its declaration path as in 3; the trailing partial chunks of `calls.dat`,
   `steps.dat`, `values.dat` and `events.dat`, in that order, each followed by its index entry; the
   span stream's last chunk, then `spantype.ns`; `srcviews.dat` / `srcviews.off`; `step-map.ns`;
   `linehits.tc`; `corrmark.ns`.

A writer in memory appends in the same order, and its container is the same bytes as the file the
same recording gives.

When a writer hands bytes to the operating system, and when it stores root entries, does not change
where the bytes go: rule 3 of "Durability" (buffering within a chunk) and "A writer MAY write more
often" stand. What is fixed is the order of appends, not the timing of I/O. The partial last block
of a member is zero beyond its `Size`.

**How block placement composes with the rest of this section.**

- **The fixed root** (§1) is what makes rule 1 possible: the root region is placed first and never
  moves, so every later block number is a function of the appends. Root growth would have broken
  the property; it is not an operation.
- **The live coordination page** (above) changes no byte of the file, so it does not affect placement.
  Neither does the attached reader.
- **Dead space** ("Dead space" above) is deterministic for a deterministic writer: a superseded index
  or a rewritten member is the same blocks in two runs of the same recording. A repack produces a new
  container, which is itself a function of the source container.
- **Keyed members** ([ctfs-keyed-families.md](ctfs-keyed-families.md)) placed by a materialized-trace
  writer join this list when a family's realization is decided (§8 of that document): the decision
  states where in rules 1-6 the family's appends go, and the realization MUST be a function of its
  entries (G12), as §8a's bulk load is.
- **Several appending threads or processes break the property.** Block placement determinism holds for
  a writer whose appends happen in one order fixed by the recording, which is what the split-stream
  writer does. A W1 writer whose threads append to different members concurrently, and every W2 or
  hybrid writer (several processes claiming blocks from one counter, "Writer architectures" below),
  claim blocks in an order set by timing, so two runs of the same recording place blocks differently,
  although each container is valid and reads identically. Such a writer does not claim this property,
  and MUST NOT be described as producing byte-identical containers. A producer that needs both
  several writers and the property serializes its appends in recording order (for example, writers
  hand finished chunks to one appender in a deterministic order), which costs the latency W2 exists to
  save. A writer that drains several producers (rings of several threads) appends in the order it
  happens to drain them, which also depends on timing even with one appending thread; the MCR
  recorder is such a writer and does not claim the property. Whether MCR containers should be
  byte-reproducible, and so whether W2 may ever be admitted for a producer that claims it, is an open
  question for the owner ([ctfs-keyed-families.md](ctfs-keyed-families.md) §9).

### Background Compression Writer

An alternative pattern: multiple producer threads write to per-thread buffers, a single background thread compresses and writes to CTFS. Since only one thread does block allocation, `NextFreeBlock` can be a plain local counter (no atomic overhead). That thread is then also the one publisher of the root region and of every keyed member (§6, "Root publication"; §7a). This is W1 ("Writer architectures") with one writing thread.

---

## 7. Companion Index Streams

For each chunked data stream `foo.dat`, a companion index `foo.idx` is stored as a separate CTFS internal file.

### Format

```
foo.idx:  [chunk_size: u32][offset_0: u64][offset_1: u64][offset_2: u64]...
```

- `chunk_size` (u32 LE): records per chunk (configurable per stream)
- Each subsequent u64: byte offset of that chunk in `foo.dat`

Chunks contain only compressed data -- no inline headers.

```
foo.dat:  [compressed_chunk_0][compressed_chunk_1][compressed_chunk_2]...
```

All chunks except the last contain exactly `chunk_size` records.

### Derived Information

- Record N is in chunk `N / chunk_size`
- Chunk C starts at `foo.idx[C]` (one u64 read, after the u32 header)
- Chunk C ends at `foo.idx[C+1]` (or `foo.dat` file size for last chunk)
- Compressed size of chunk C = `foo.idx[C+1] - foo.idx[C]`

### Seeking (O(1))

1. Compute `chunk = N / chunk_size`
2. Read `foo.idx[4 + chunk * 8]` -- byte offset
3. Read `foo.idx[4 + (chunk + 1) * 8]` -- next offset (or use `foo.dat` size)
4. Read and decompress `foo.dat[offset..next_offset]`
5. Read record `N % chunk_size` within decompressed data

Two u64 reads + one compressed chunk read. Index typically fits in 1-2 blocks.

### Writer Protocol

1. Accumulate records into buffer
2. When buffer reaches `chunk_size` records:
   a. Encode records
   b. Compress with Zstd
   c. Write compressed data to `foo.dat` and publish its file entry (`Size`, `MapBlock`)
   d. Append the chunk's u64 byte offset to `foo.idx` and publish its file entry

The data comes first: an index entry is what tells a reader a chunk exists, so it is published only
once the chunk it names is (§6, "Durability"). An earlier revision listed the index append before
the data write, which would let a following reader see an offset for bytes not yet written; no
writer did that.

The index is always up to date during active recording.

**Reading the last chunk of a stream that is still being written.** A reader MUST NOT take
`foo.dat`'s `Size` as the end of the last indexed chunk while the stream may still grow: the next
chunk's leading bytes can already be published without its index entry, so the size overshoots and
hands the decompressor bytes from two chunks. It derives the last frame's length from the frame
itself instead (for zstd, `ZSTD_findFrameCompressedSize`).

**Seeking assumes every chunk but the last is full.** A writer that seals a partial chunk mid-stream
(to publish records in flight) breaks `chunk = N / chunk_size`, and the index format above has no
per-chunk record count to recover from. Such a stream carries a cumulative record count in its index
(as `spans.idx` does), or its reader derives each chunk's occupancy from the chunk's contents.

---

## 7a. Keyed Members (Unbounded Families)

A **keyed member** is a root member, or one of a small fixed set of root members, that holds a family
whose size grows with the recording: it maps `u64` keys to fixed-size **records**, and through a
record to an inline value, to bytes in a value heap, or to a **keyed stream** -- a container stream
exactly like a root member's, with a `Size` and a `MapBlock` of §2's three forms, resolved by §4's walk
with §4's refusals, but referenced from a record instead of a root entry. The root's fixed size (§1)
then bounds the number of families, not the length or width of a recording.

Which structure maps keys to records is chosen per family, by benchmark, among the families and
candidate realizations of [ctfs-keyed-families.md](ctfs-keyed-families.md). That document also gives
the cost model (§2), the properties of the data (§3) and the decisions (§8). **Until a family's
realization is decided and its byte layout added there, no writer emits a keyed member for it**, and
the members a producer writes today stay as they are (the member catalogues say which:
[internal-files.md](internal-files.md) §"Member Catalogue", and for MCR `codetracer-specs`
`spec/Trace-Files/CTFS-Binary-Format.md` §2).

The rules below hold for every keyed member, whatever its realization (normative):

1. **Records are fixed-size, packed per block, and never straddle a block.** A member of records of
   size `S` (a multiple of 8) holds `floor(BlockSize / S)` records per block; the rest of each block is
   zero padding a reader ignores. Record `r`'s position is arithmetic on `r`.
2. **Records never move and are never reused.** A record stays at its position for the container's
   life; its key never changes; removing a key is an in-place state change (a tombstone), not the
   reuse of the record for another key. An index structure that changes shape moves references to
   records, never records ([ctfs-keyed-families.md](ctfs-keyed-families.md) §4.3).
3. **A record is written before anything makes it reachable**: before the member's `Size` covers it,
   before an index slot names it. A keyed stream's data and mapping are written before the record
   that references them, exactly as a root member's are before its entry (§6, "Writer protocol"). A
   record that refers into another member is published only after its target is durable, at a seal
   at or before the record's own ([ctfs-keyed-families.md](ctfs-keyed-families.md) §4.1, rule 4).
4. **In-place updates follow §6, "In-place record publication"**: published through the live
   coordination page, written to the file at seals. A record kind states its store order and whether
   it is prefix-valid. Records carry no sequence word on disk.
5. **One writer per keyed member** (§6), under either writer architecture (§6, "Writer
   architectures"). Every store into the member, a new record or a word of an existing one, is made by
   that writer.
6. **Stored container block numbers** (a keyed stream's `MapBlock` and mapping; an index reference in
   absolute form, [ctfs-keyed-families.md](ctfs-keyed-families.md) §2.3) are validated as §4 validates
   a mapping pointer, and are written only after the block they name. Every operation that copies the
   member into another container rewrites them or refuses, as the realization states.
7. **Durability.** When a chunk of a keyed stream seals, the chunk, its mapping and the record's words
   are written before the append returns, and the keyed member's own entry too if the member grew
   (§6, "Durability").
8. **Readers** follow a keyed member as the reader protocol follows the root (§6): they re-read the
   member's `Size` and the records and index words the realization updates in place at every refresh;
   between refreshes a record never disappears, its key never changes, and a keyed stream's `size`
   never decreases. A reader that observes any of these MUST refuse the member as damaged, naming the
   member and the key.
9. **Profiles.** A keyed member whose records hold container block numbers has no meaning in a
   compact container (§1d), which has none. A writer converting a container that holds one into the
   compact profile MUST either convert it by the compact form its realization defines or refuse,
   naming the member.

**What this replaced.** A draft circulated on 2026-10-07, and never landed, put a single layout here, the *stream directory*: an
unsorted table of 24-byte `(size, map_block, key)` records in creation order, found by scanning. It
is kept as one candidate realization of the dense family (F1, realization D4) and as the baseline
the benchmarks measure every other candidate against; it is not the design.

---

## 8. Namespaces (Small Files Collection)

A namespace maps 64-bit keys to values within a single CTFS member. It is designed for millions of
entries, most of them very small (a few time coordinates or step ids), built during or after a
recording and queried by key without reading the whole key set. Its values do not grow while a
reader follows them: a value that must grow live is referenced from a keyed member (§7a). A namespace
in this format (`NSB1`) is one realization of the sparse families of
[ctfs-keyed-families.md](ctfs-keyed-families.md): a copy-on-write B+tree (F4, realization B2) or,
bulk-loaded, a static index (F5, realization S2).

### Positions inside a namespace are member-relative (normative)

A namespace member is a self-describing image of 4096-byte **pages**: page `p` is the member's bytes
`[p * 4096, (p + 1) * 4096)`, whatever the container's `BlockSize`. Every position a namespace stores
is a page number or a byte offset within the namespace member: B-tree child pointers, descriptors,
free chains and pool slots. **A namespace never stores a container block number.** A reader resolves
a page through the namespace member's own mapping, like any other byte of a member.

This is what every implementation already does (`codetracer_ctfs`'s `cow_btree.nim` and
`sub_block_pool.nim`; the db-backend's `cow_namespace_reader.rs`; `step-map.ns` and `corrmark.ns`).
Earlier revisions of this section put sub-block free list heads in container block 0 and described
descriptors as holding "the root mapping block in main file"; no implementation did either, and the
block 0 area is removed (§1).

### Sub-Block Allocation Pools

Standard block allocation wastes space when millions of keys have 8--32 byte values. A namespace
subdivides its pages into sub-blocks:

| Pool size | Sub-blocks per 4096-byte page |
|-----------|-------------------------------|
| 32 B | 128 |
| 64 B | 64 |
| 128 B | 32 |
| 256 B | 16 |
| 512 B | 8 |
| 1024 B | 4 |
| 2048 B | 2 |

**Allocation lifecycle for a key:**

1. First append: allocate 32B from the namespace's 32B pool
2. When data outgrows current slot: allocate next larger size, copy data, release old slot
3. Continue doubling through 64B, 128B, 256B, 512B, 1024B, 2048B
4. Beyond 2048B: the value graduates to a run of whole pages (below)

Pools belong to the namespace, not to the container. A free sub-block stores its next pointer,
`(page: u32, slot_index: u16)`, in its own first 6 bytes. A namespace that persists freed sub-blocks
across commits keeps the pool heads in its own image, at a location its format defines. No generic
location is assigned, because no implementation persists them: the Nim pool manager rebuilds them in
memory.

### B-Tree Key Index

A namespace must support efficient lookup of a single key without downloading the entire key set. Each namespace uses a **B+-tree** on the 64-bit key. Each node occupies one page of the namespace member, so a lookup reads O(depth) pages, typically 3-4 for millions of entries, and over a network that is 3-4 round trips.

**Node layout.** Internal nodes contain sorted keys and child page pointers. Leaf nodes contain sorted keys and **entry descriptors**. Both kinds share one 8-byte node header and differ only in what follows the key array:

```
struct NodePage {              // exactly one 4096-byte page
    node_kind:  u8             // 0 = internal, 1 = leaf
    reserved:   u8             // 0
    count:      u16 LE         // number of KEYS in this node
    reserved:   [u8; 4]        // 0
    payload:    [u8; 4088]     // see below; trailing bytes are unspecified
};
```

Offsets (all integers little-endian): `node_kind [0]`, `reserved [1]`, `count [2..4)`, `reserved [4..8)`, `payload [8..4096)`.

The payload is two adjacent arrays with **no padding or alignment between them**, both indexed from the start of the payload at byte 8:

```
Leaf payload (node_kind == 1):
    keys:        count      x u64 LE      at  8
    descriptors: count      x D bytes     at  8 + count*8

Internal payload (node_kind == 0):
    keys:        count      x u64 LE      at  8
    children:    count + 1  x u64 LE      at  8 + count*8
```

`D` is the namespace's **descriptor size**, fixed for the whole namespace by the `leaf_type` bit of the namespace header: 8 bytes for Leaf Type A, 16 bytes for Leaf Type B. Internal nodes carry no descriptors, so their child array is always `u64` whatever `D` is.

A **child pointer is a page number** of the namespace member. Page 0 holds the `NamespaceHeader` and is never a node, so a valid child pointer is `>= 1` and `0` means "none". These are `u64` page numbers, **not** the `u32` breadth-first node indices of the older whole-tree serialization (the `"NS"`-magic blob with its embedded `BTR\0` tree, which this section does not specify and which is not wire-compatible with `NSB1`).

Keys within a node are **sorted ascending and unique**. Both node kinds are searched with the same `lower_bound(key)` over the key array, and use the result differently:

- **Leaf** (`node_kind == 1`): the key is present iff `i < count && keys[i] == key`, and its descriptor is `descriptors[i]`. Otherwise the key is absent.
- **Internal** (`node_kind == 0`): descend into child `i + 1` when `i < count && keys[i] == key`, and into child `i` otherwise.

That asymmetric rule follows from how separators are chosen: this is a **B+-tree with copy-up separators**. A split promotes the *first* key of the right-hand node, and that key stays in the right-hand node too. So for an internal node with keys `[s0 … s(count-1)]` and children `[c0 … c(count)]`, **`s_i` is the smallest key reachable through `c_(i+1)`**. A reader that treats a separator as an exclusive upper bound on its left subtree fails to find exactly the keys that are separators. Equivalently: every key `k` under `c_j` satisfies `s_(j-1) <= k < s_j`, with the bounds omitted at the ends.

**Fanout** follows from the page size and the descriptor size, and a writer must not exceed it:

```
order = (4096 - 8) / (8 + D)
```

which is **255** keys per node for Leaf Type A (`D = 8`) and **170** for Leaf Type B (`D = 16`). A leaf holds at most `order` keys; an internal node at most `order` keys and `order + 1` children. A reader must tolerate any `count <= order`, including short nodes at the right edge of a bulk-loaded tree; there is no minimum occupancy on the wire.

Bytes of the payload beyond the two arrays are **unspecified** and a reader must not depend on them.

**Free page chain.** A page on the namespace's free chain stores the next free page number as a `u64 LE` in its first 8 bytes and is otherwise zero. The head is `free_list_head` in the namespace header, and `0` ends the chain.

**Bulk load.** A writer that has all `(key, descriptor)` pairs sorted and duplicate-free may pack the tree bottom-up: consecutive runs of at most `order` keys, one leaf page each; then each internal level over the one below, grouping up to `order + 1` children per node with each child's **subtree minimum** as the separator before it; repeat until one node remains, and publish it as the root with `commit_id = 1` in root slot 0. The result reads identically to a tree built by insertion, though it is not byte-identical to one.

### Entry Descriptors

A descriptor is `D` bytes, and what they mean is decided by the namespace's format, which is named by
the member. The B-tree treats a descriptor as opaque. This section defines the **pooled** descriptor
scheme, which a namespace format uses unless it defines its own; `corrmark.ns`, for example, uses a
Leaf Type B descriptor of `[payload_offset: u64][payload_len: u64]` into a payload region of its own
member ([internal-files.md](internal-files.md) §"Correlation Index"). Every position in either scheme
is member-relative.

**Leaf Type A (8 bytes, pooled):**

```
  Sub-block (bit 63 = 0):
    bits 62-15:   page (48 bits)       the pool page, a page number of the namespace member
    bits 14-12:   pool_class (3 bits: 0=32B, 1=64B, ..., 6=2048B)
    bits 11-0:    slot_and_used (12 bits)

  Graduated (bit 63 = 1):
    bits 62-32:   first_page (31 bits) the first page of a run of whole pages
    bits 31-0:    data_size (32 bits)  the run is ceil(data_size / 4096) consecutive pages
```

**Leaf Type B (16 bytes, pooled):**

```
  Sub-block (first word == 0):
    first u64:    0 (discriminator)
    second u64:   bits 63-15 page (49 bits), bits 14-12 pool_class, bits 11-0 slot_and_used

  Graduated (first word != 0):
    first u64:    first_page            the first page of a run of whole pages
    second u64:   data_size             the run is ceil(data_size / 4096) consecutive pages
```

Page 0 is the namespace header and is never a pool page or part of a run, so a zero `first_page`
discriminates the sub-block form unambiguously. The 12-bit `slot_and_used` encodes slot position and
used bytes, split by pool class:

| pool_class | pool_size | slot_index bits | used_bytes bits |
|------------|-----------|-----------------|-----------------|
| 0 | 32B | 7 | 5 |
| 1 | 64B | 6 | 6 |
| 2 | 128B | 5 | 7 |
| 3 | 256B | 4 | 8 |
| 4 | 512B | 3 | 9 |
| 5 | 1024B | 2 | 10 |
| 6 | 2048B | 1 | 11 |

Sub-block support is optional: the `skip_sub_blocks` flag makes every value a page run from its first
byte.

### Namespace Header

The page store is **self-describing**: a reader reconstructs the tree (root selection, free chain,
bump cursor) from page 0 alone.

```
struct NamespaceHeader {       // 61 bytes, at the start of page 0
    magic:          [u8; 4]    // "NSB1" (namespace B-tree, format 1)
    root_block[2]:  u64        // double-buffered B-tree root slots: page numbers (0 = empty)
    commit_id[2]:   u64        // commit tag per root slot
    flags:          u8         // bit 0: leaf_type (0 = Type A, 1 = Type B)
                               // bit 1: skip_sub_blocks
                               // bits 2-7: MUST be zero
    free_list_head: u64        // head page of the whole-page free chain (0 = empty)
    next_free_page: u64        // bump-allocation cursor (first never-used page number)
    page_count:     u64        // total pages in the image
};
```

Offsets (little-endian): `magic [0..4)`, `root_block[0] [4..12)`, `root_block[1] [12..20)`, `commit_id[0] [20..28)`, `commit_id[1] [28..36)`, `flags [36]`, `free_list_head [37..45)`, `next_free_page [45..53)`, `page_count [53..61)`. A reader refuses a flag bit it does not implement, naming it (§1c's rule).

**Bytes `[61, 4096)` of page 0 are producer-private.** A reader must ignore them: it must not validate them and must not refuse an image for what it finds there. A producer may keep a self-describing record of its own there, with its own magic; the WASM snapshot page store keeps its `SnapshotFormatVersion` there, because `NSB1` carries no version field and version gating has to happen before any structural interpretation. The header will not grow into that space: a change that needs new header fields is a new magic (`NSB2`), not a longer `NSB1`.

**No entry count** -- the B-tree is the source of truth for which keys exist.

### Copy-On-Write And The Double-Buffered Root

A namespace that is persisted more than once (built incrementally, or extended by a later pass)
updates its tree **copy-on-write**, modelled on LMDB:

1. **Path copying.** A page reachable from a committed root is never modified in place. To change a
   node, the writer takes a fresh page (from the free chain when that is allowed, below, otherwise
   from `next_free_page`), copies the node into it, applies the change, and copies each ancestor up
   to a new root page the same way. The old spine remains a complete tree until the commit.
2. **The commit** writes the new root's page number into the root slot **not** in use, and then that
   slot's `commit_id`. The committed root is the valid slot with the higher `commit_id`; a slot with
   `commit_id = 0` is empty, and an empty namespace has both slots `0`. A commit's id is the other
   slot's id plus one, so the two valid ids of a namespace always differ by exactly 1.
3. **Crash recovery is implicit.** A crash before the commit leaves the previous root intact in the
   other slot. The pages of the abandoned spine are unreferenced and are recovered as free space.

### Live And Incremental Publication (normative)

A namespace in a container that a reader may follow, or that is extended after it was first
published, is published in this order. The root slots are the only words a reader can see change.

**Writer, per commit:**

1. Write every new page (the copied spine, new pool pages, page runs) as appended bytes of the
   namespace member, after its current end.
2. Publish the namespace member's entry, `MapBlock` before `Size` (§6), so every new page lies below
   the published `Size`.
3. Store `next_free_page` and `page_count` in page 0.
4. Barrier. Store the new root's page number in `root_block[s]`, where `s` is the slot with the lower
   `commit_id`; barrier; store `commit_id[s] = commit_id[1-s] + 1`.

**Reader:**

1. Locate page 0 through the namespace member's entry. Page 0's blocks never move (§1), so any
   `MapBlock` the reader has observed locates it.
2. Read `commit_id[0]`, `commit_id[1]`, `root_block[0]`, `root_block[1]`, then both `commit_id`s
   again. If either id changed, or the two non-zero ids do not differ by exactly 1, read again. The
   root slot words are not 8-byte aligned (`root_block[0]` is at byte 4), so a reader cannot rely on
   one load seeing a whole word; the double read and the "differ by exactly 1" rule are what reject a
   torn value.
3. Take the valid slot with the higher `commit_id`.
4. Then load the member's `Size` (and `MapBlock`). Because the writer published `Size` before the
   commit, every page reachable from that root lies below it. A page at or past `Size / 4096` is
   damage, and the reader MUST refuse the namespace, naming it.
5. Walk the tree. No page reachable from a committed root changes while the reader holds that root
   (below).

**Reclaiming superseded pages.** A page that a newer commit no longer reaches may be returned to the
free chain only when no reader can still hold a root that reaches it. LMDB establishes that with a
table of its readers' snapshots. A reader that follows a file from another process cannot register in
any such table, so **a writer of a container file that another process may read reuses no
superseded page**; it appends. An in-memory overlay that no other reader shares (`codetracer-specs`
`CTFS-Binary-Format.md` §11.4) may reclaim, and the pages it then persists are written as a commit
above.

**Descriptors are never updated in place.** A committed leaf is immutable, so a namespace has no
live-growing values. A family of values that grows while a reader follows it is a keyed member (§7a),
whose realization is chosen among [ctfs-keyed-families.md](ctfs-keyed-families.md) §5's candidates.

### Namespace Files

| Namespace | Leaf Type | Key | Purpose |
|-----------|-----------|-----|---------|
| `linehits.tc` | `NSB1` image (§8a), Type B | location address (a step's `global_position_index`) | Step ids at each source location |
| `corrmark.ns` | `NSB1` image (§8a), Type B | XXH64 of the correlation key | Correlation markers ([internal-files.md](internal-files.md) §"Correlation Index") |
| `memwrites.tc` | `NSB1` image (§8a), Type B | memory address | Write history; MCR only, its payload layout in `codetracer-specs` |
| `memreads.tc` | A | memory address | Memory read time coordinates (no writer yet) |

Every writer of the first three emits the §8a image: Leaf Type B with `skip_sub_blocks` set, and the
16 bytes of a descriptor as `[payload_offset: u64][payload_len: u64]` into the payload region that
follows the pages. This table listed `linehits.tc` and `memwrites.tc` as Type A until 2026-10-08; no
writer ever produced that. Which keyed family each member belongs to, and the realization it is
measured against, is [internal-files.md](internal-files.md) §"Member Catalogue" (and, for
`memwrites.tc`, the MCR catalogue).

`step-map.ns` is a single member and not a namespace, despite its suffix
([internal-files.md](internal-files.md) §"`step-map.ns`"). The namespaces and keyed members the MCR
recorder writes (`slc-mwr.ns`, `slc-mrd.ns`, its page store and others) are specified in
`codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md`. Until 2026-10-07 this table listed
`threads.ns`, `slc-mwr.ns` and `slc-mrd.ns`; they moved there with the scope note at the head of this
document.


### 8a. The `NSB1` namespace image (normative)

> **How §8a relates to §8.** §8 specifies the `NSB1` page format, its copy-on-write commit and its
> live publication for any producer. §8a fixes, byte for byte, the bulk-loaded image of the three
> members it names, so that two writers produce the same bytes ("Block placement", §6). Where §8a
> is stricter it governs those members: their page 0 is zero past the header and their nodes and
> payload padding are zero, where §8 leaves those bytes producer-private or unspecified for other
> images (the MCR page store keeps a record of its own in page 0). In the families of
> [ctfs-keyed-families.md](ctfs-keyed-families.md) an §8a image is realization S2, a static index
> (F5) built at close.

`linehits.tc`, `corrmark.ns` and `memwrites.tc` are not built from the sub-block pools above. Each is
one member holding an `NSB1` image: a B-tree of 64-bit keys in 4096-byte pages, followed by a payload
region that the B-tree's descriptors point into. Writers build it once, at close, over a known set of
keys ("bulk load"); the double root slot lets an incremental writer commit copy-on-write, but no
runtime member needs that, and a bulk-loaded image is fully determined by its entries, which is what
lets two writers produce the same bytes.

```
Page 0, the header (the rest of the page is zero):
  [0..4)    magic "NSB1"
  [4..12)   root_block[0]: u64 LE     -- page number of the B-tree root, 0 = none
  [12..20)  root_block[1]: u64 LE
  [20..28)  commit_id[0]: u64 LE      -- 0 = slot empty
  [28..36)  commit_id[1]: u64 LE
  [36]      flags: u8                 -- bit 0 leaf type (0 = A, 8-byte descriptors;
                                         1 = B, 16-byte), bit 1 skip_sub_blocks
  [37..45)  free_list_head: u64 LE    -- 0 in a bulk-loaded image
  [45..53)  next_free_page: u64 LE    -- first page number never allocated
  [53..61)  page_count: u64 LE        -- pages in the B-tree image, header included
Pages 1..page_count-1, B-tree nodes:
  [0]       node_kind: u8             -- 0 internal, 1 leaf
  [1]       0
  [2..4)    count: u16 LE             -- keys in the node
  [4..8)    0
  leaf:     keys[count] u64 LE, then descriptors[count]
  internal: keys[count] u64 LE, then children[count + 1] u64 LE page numbers
  the rest of the page is zero
Then the payload region, up to the end of the member, zero-padded to a multiple of 4096.
```

The committed root is the root of the slot with the larger nonzero `commit_id`; with both zero the
namespace is empty. A key `k` is found by descending from the root: in an internal node, child `i`
holds the keys in `[keys[i-1], keys[i])` (child 0 below `keys[0]`, the last child from the last key
up). The members here use **Type B** with `skip_sub_blocks` set (`flags = 3`), and every descriptor
is `[payload_offset: u64 LE][payload_len: u64 LE]`, an offset from the start of the member.

**Bulk load (normative, so that two writers produce the same bytes).** The entries are sorted by
key, keys distinct. The fan-out is `order = (4096 - 8) / (8 + descriptor size)` keys per node -- 170
for Type B. The leaves take the entries in runs of `order`, the last run holding the remainder, and
are allocated pages 1, 2, … in that order. Each level above groups the nodes below it in runs of
`order + 1`, allocating their pages after every page of the level below; an internal node's keys
are the smallest keys under its 2nd, 3rd, … children. The level with one node is the root, published
in slot 0 with `commit_id[0] = 1`; slot 1 stays zero. `next_free_page` and `page_count` are the page
count; `free_list_head` is 0. With no entries the image is the header page alone, with no root.

The payload region begins at the end of the B-tree pages, and each key's payload follows the
previous key's, in key order, with no padding between them; the member ends with zeros to the next
multiple of 4096 bytes.

A reader MUST refuse, naming the member: a member shorter than one page or not a multiple of 4096
bytes; another magic; a flag bit other than bits 0 and 1, or a leaf type other than the member's; a
`page_count` of 0 or beyond the member's pages; a committed slot whose root is 0; a page number
outside `[1, page_count)`, or a page reached twice; a node kind other than 0 or 1, a node with no
keys, a `count` whose keys and descriptors or children do not fit in the page, or nonzero reserved
node bytes; keys that do not ascend strictly within a node or fall outside the range their parent
gives them; leaves at different depths; and a descriptor whose payload does not lie inside the
member. Each of these, read on, either loops or answers a lookup with bytes no writer stored.

A lookup that finds a key equal to an internal node's `keys[i]` continues in child `i + 1`.

---

## 9. Multi-File Output Mode (Block Sharding)

Blocks are sharded across multiple files to exploit higher aggregate I/O throughput.

> **Status (2026-10-08).** No producer writes `MaxShards != 0`, and this section is a design, not a
> layout any reader has been tested against. Two of its premises changed: the free list root area
> in block 0 is removed (§1), and a namespace keeps its allocation state inside its own member
> (§8). Before a producer shards, this section is restated so that a
> namespace's pages and a keyed member's records and index stay in the main file, and so that a
> stored block number in absolute form carries its shard
> ([ctfs-keyed-families.md](ctfs-keyed-families.md) §2.3). Until then a writer MUST write
> `MaxShards = 0` (§1).

**Sharding scheme:** Block N is stored in file `N % num_shards`. Guarantees even distribution.

**Manifest file (`manifest.dat`):**

```
shard_count: u32
for each shard:
  path_length: varint
  path: bytes    (filesystem path or URL)
```

Reader resolves `block N` to `shard_file[N % shard_count]` at offset `(N / shard_count) * block_size`.

**Separation of structure and data:** the root region, mapping hierarchy, companion indices, namespace pages, keyed members' records and indexes, and `meta.dat` stay in the **main `.ct` file** -- never sharded. Only the data blocks of streams are distributed across shards.

**Shard affinity per namespace key:** Home shard = `hash(key) % num_shards`. Sub-block phase allocations use the home shard's pools, whose state lives with the shard. Multi-block growth uses round-robin across all shards.

**ShardWriter abstraction:** Each shard is managed by a ShardWriter. A single operation: `append(current_descriptor, data) -> new_descriptor`. For local multi-file, in-process calls. For network sharding, one RPC per operation.

---

## 10. Split-by-Time (Temporal File Splitting)

For long recordings, a producer may split the trace into multiple files at points where each split can stand alone. Each split file is a self-contained `.ct` container with its own block numbering and its own root directory, so splitting also bounds how many members of a growing family any one container holds. Where a producer splits, and what each split carries to stand alone, is that producer's format; the MCR recorder's is in `codetracer-specs`.

### Trace Directory Layout

```
my-recording/
  000000.ct     # Split 0 (or the only file)
  000001.ct     # Split 1
  000002.ct     # Split 2
  ...
```

Reader opens all `.ct` files in sorted order. Zero-padded indices ensure correct lexicographic sort.

**Split points:** More frequent splits produce smaller individual files and increase exploitable parallelism (each split can be analyzed independently). They also increase total trace size when each split must repeat state, such as a memory snapshot, to stand alone. The trade-off is between granularity (smaller files, more parallelism, finer-grained deletion) and space efficiency.

**Independent file deletion:** Any split file can be deleted; remaining files stay readable. Each split is self-contained with its own block numbering and namespace entries. The reader skips missing indices and continues with the next available file. This enables storage management policies such as retaining only segments around a known failure point.

**Namespace queries across splits:** Query all split files, concatenate results. Split ordering preserves chronological order.

**Processing parallelism:** Each split file can be processed independently -- the emulation layer runs one split per worker, building namespace entries for that time interval. No merge step is needed.

---

## 11. Network-Aware Reading

### Block-Aligned Access

All data at block-aligned boundaries. Predictable read counts:

| Operation | Reads | What |
|-----------|-------|------|
| Container discovery | `root_blocks` blocks (1 by default) | The root region (header + file entries) |
| Step lookup | 1-2 blocks | Companion index + one chunk |
| Namespace lookup | 3-5 blocks | B-tree walk + data read |
| Call lookup | 2-3 blocks | Companion index + one chunk |
| Interning table lookup | 2 blocks | Offset index + data record |

HTTP range requests: block `b` = bytes `[b * BlockSize, (b+1) * BlockSize)`.

### Partial Trace Cache (.ctp)

A `.ctp` file is a standard CTFS container holding only fetched blocks. Contains `presence.idx` (sorted array of remote block numbers) and `permanent.idx` (blocks not eligible for eviction). Blocks are fetched on demand and cached to disk, with an LRU RAM layer on top (default 256MB). Remote block numbers are stable, because no block ever moves (§1). Which cached blocks stay valid when the remote container is still being written, or is appended to after it closed, is §6, "What is mutated in place, and what a cache may keep".

### Smart Query Protocol (Optional)

Storage nodes resolve queries server-side (B-tree walks, chunk decompression) in one round-trip. Collapses 3-5 block reads into one request/response.

---

## Configuration Recommendations

| Setting | Default | Rationale |
|---------|---------|-----------|
| BlockSize | 4096 | Matches OS page size and HTTP range granularity |
| MaxRootEntries | 0 (auto) | Fills block 0 (170 entries); fixed at creation (§1). A producer whose closed set of members needs more declares more; a growing family goes into a keyed member (§7a), not into the root |
| LRU cache | 16--64 blocks | Balances memory vs mapping re-reads |
| Chunk threshold | 4096 events | Balance compression ratio vs seek granularity |

### Block Size Trade-offs

A full container's `BlockSize` is one of the three below (a compact container's is 0, §1d). A writer
MUST refuse to create a container with any other, and a reader MUST refuse one that declares any
other, naming the value: the mapping arithmetic of §4 assumes `BlockSize / 8` slots per mapping block,
and the sizes outside this table are untested by every implementation.

| BlockSize | N | usable | Max L5 file size |
|-----------|---|--------|------------------|
| 1024 | 128 | 127 | ~35 TB |
| 2048 | 256 | 255 | ~280 TB |
| 4096 | 512 | 511 | ~133 PB |

# CTFS Binary Container Format

CTFS (CodeTracer File System) is a block-based container format that stores multiple named files in a single `.ct` file. It provides a flat file system optimized for streaming writes, concurrent multi-producer access, and multiple simultaneous readers. All integers are **little-endian**.

## Properties

| # | Property | Description |
|---|----------|-------------|
| 1 | Compressed storage | Per-member Zstd via chunked compressed tables, transparent to the container layer; and, from v6, an optional whole-file scheme declared in the header (§1a, §1b) because a scheme that covers `meta.dat` cannot be declared inside it. The compact profile uses neither chunked tables nor seekable zstd (§1d). |
| 2 | Random-access seeking | O(log n) block mapping (at most 5 reads); O(1) chunk seek via companion index. Full profile only: the compact profile is resident before its first query and seeks in memory (§1d). |
| 3 | Low-contention concurrent writes | Single atomic `NextFreeBlock` counter; per-file single writer; no locks. |
| 4 | Multiple concurrent readers | Readers see consistent file sizes updated atomically by writers. |
| 5 | Network-efficient | Block-aligned layout maps to HTTP range requests; one 4 KB fetch reveals full structure. Full profile only, and deliberately so: §1d has no alignment because a one-shot load issues no ranged read. |
| 6 | Self-contained | All metadata in binary format within the container; no external files or JSON. |
| 7 | Streaming-compatible | Companion index available during recording; no finalization needed. |
| 8 | Encryption-aware | Container-level encryption flag; all content opaque without the key. |
| 9 | Append-only | All writes are appends; no in-place updates except atomic size counters. |
| 10 | Lock-free | Only atomic fetch-and-add and atomic stores required. |

### Non-Goals

No directories (flat namespace only), no file deletion or truncation, no file attributes, no built-in checksums, no redundancy. Designed for 10--200 files per container.

---

## 1. Container Header (16 bytes through v5; 24 bytes at v6)

Block 0 begins with the container header. Through version 5 it is 16 bytes. Version 6 extends it to 24 and is the only version that carries the `Profile` and `Compression` fields.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0--4 | 5 | Magic | `C0 DE 72 AC E2` ("CODE TRACE") |
| 5 | 1 | Version | `5`, or `6` for a container carrying the fields below |
| 6 | 1 | Encryption | `0` = none, `1` = AES-256-GCM |
| 7 | 1 | MaxShards | Maximum shard count (`0` = no sharding) |
| 8--11 | 4 | BlockSize | Block size in bytes (u32 LE, default 4096) |
| 12--15 | 4 | MaxRootEntries | Maximum file entries (u32 LE, `0` = auto-fill block 0) |
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

**Why the header grew by 8 bytes rather than 2.** `Profile` and `Compression` are one byte each, so 18 would carry them. The six reserved bytes buy one property, stated exactly because the general claim would be false: in an UNSHARDED container -- `max_shards = 0`, so the free list root area is empty and `R = 0` -- the `FileEntry` array starts at the header's own size, and its three `u64` fields with their 24-byte stride are 8-byte aligned at 24 and misaligned at 18. Unsharded is the default and it is the only thing the compact profile permits (§1a), so that is the case worth aligning. It is NOT a claim about sharded containers: there `R = 7 * max_shards * 6 = 42 * max_shards` already decides the alignment and already breaks it at odd shard counts, exactly as it does in the 16-byte header, and this version does not change that.

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
> `steps.idx`, the MCR thread streams `tNNN` + `iNNN`, the MCR snapshot payloads
> `<stem>.<x>zd` + `<stem>.<x>zi` of [internal-files.md](internal-files.md)) keeps
> independent zstd frames in its data member and their offsets in its index
> member, and so does a seekable-zstd stream such as an MCR per-file thread stream
> ([seekable-zstd.md](seekable-zstd.md)); a member whose format is not one of
> those is stored exactly as written.  No current writer compresses the
> container as a whole — the MCR recorder's buffered mode, which did, has been
> removed.

**Encryption IS in the header** because an encrypted container is opaque -- even `meta.dat` is unreadable without the key.

**WHOLE-FILE compression IS in the header, for exactly the reason encryption is.** A scheme applied to the container as a whole covers `meta.dat` and the entry array along with everything else, so a reader told to find the mode anywhere inside the container would have to decompress it in order to learn how to decompress it. That is the same circularity the encryption sentence above resolves, resolved the same way: the declaration sits in the one region the scheme does not cover. The two statements above are therefore not in tension with this one -- the first governs per-member compression, which is settled by a member's format inside the covered region and can be; the `Compression` field governs the whole-file scheme, which lives outside it and must.

**And this is the field the removed buffered mode did not have.** The correction above records that the MCR recorder once compressed the container as a whole and that the mode has been removed. Nothing in the header said it had done so, which is why nothing could refuse such a container by name. Version 6 does not reinstate that mode; it makes the declaration a precondition of ever having one again.

### Block 0 Layout

Block 0 contains the header, free list roots, and file entries. The offsets below are the versions 2--5 layout; for version 6 substitute the 24-byte header and the `24 + R` offsets given in §1a. The compact profile has no block 0 at all.

```
Block 0 (versions 2 .. 5):
  [0..15]                       ContainerHeader (16 bytes)
  [16..16+R-1]                  Free list roots (R bytes, fixed area)
  [16+R .. BlockSize-1]         FileEntry array (remaining space)
```

**Free list root area size:** `R = 7 * max_shards * 6` bytes (7 pool sizes, 6 bytes per root entry). Each root entry is `(block_num: u32, slot_index: u16)` = 6 bytes. Roots are laid out as `roots[shard_id][pool_class]` in row-major order. With `max_shards = 16`: `R = 672` bytes.

**Auto-fill:** When `MaxRootEntries` is 0, file entries fill the remainder of block 0:

```
auto_entries = (BlockSize - 16 - R) / 24
```

For BlockSize=4096, max_shards=16: `auto_entries = (4096 - 16 - 672) / 24 = 142` (this line said 141 until 2026-09-29; 3408 / 24 is exactly 142).

For BlockSize=4096, max_shards=0 (R=0): `auto_entries = (4096 - 16) / 24 = 170`.

If `MaxRootEntries * 24 + 16 + R > BlockSize`, file entries overflow into contiguous blocks after block 0:

```
root_blocks = ceil((16 + R + MaxRootEntries * 24) / BlockSize)
```

Data block allocation begins at block number `root_blocks`.

> **Implementation status (2026-09-29, `MCR-Memory-Page-CAS.milestones.org`
> CAS-Z0).**  The overflow is implemented by the Nim writer and by every reader
> of MCR recordings:
>
> - `codetracer_ctfs` (`codetracer-trace-format-nim`): `createCtfs` reserves
>   `root_blocks` blocks and starts data at block `root_blocks`
>   (`rootBlockCount`); the streaming publishes (`addFile`,
>   `truncateFileContent`, `syncRootBlock`, `syncAllEntries`) write the whole
>   root region; `writeToFile` refuses a mapping or data block inside it.
>   `readInternalFile` / `hasInternalFile` read the whole file and need no
>   change.  Pinned by `tests/test_root_directory_overflow.nim`.
> - The MCR recorder declares `root_blocks = 16` (2730 entries) for a
>   recording that takes periodic checkpoints (three members each) and block 0
>   alone (170) for every other recording.  Its disk root reader
>   (`ctfs_disk.readCtfsRootBlock`, under the replay-worker's and debugserver's
>   streaming loader) reads the whole region; `export --portable` and `slice`
>   size their output past block 0 when they must.
> - `ct upload`'s enrichment check (`codetracer`,
>   `mcr_enrichment.readCtfsRootDir`) reads the whole region instead of
>   clamping the count to block 0.
>
> Still block 0 only, and not handed an MCR recording today:
> `codetracer_ctfs`'s `container_append` (refuses a count past block 0, by
> name); the Rust `codetracer_ctfs` writers (`writer.rs`,
> `concurrent_writer.rs`: one root block, allocation from block 1, so they
> MUST NOT be given a count past block 0); the Go reader in
> `codetracer-wasm-recorder` (refuses).  Still block 0 only and ON an MCR
> recording's path, though dormant: `db-backend`'s `block_overlay.rs`, which
> the materialization cache opens over the session's `.ct` to persist into it
> (reached only once a replay worker answers `MaterializeInterval`, which
> `ct-native-replay` refuses today); on a recording past 170 entries it would
> report the directory full.  Filed as
> `codetracer-specs/issues/2026-09-29-db-backend-block-overlay-reads-root-directory-from-block-0-only.md`.  The other readers surveyed
> on 2026-09-29 -- the Rust `CtfsReader` / `concurrent_reader.rs`,
> `db-backend`'s `ctfs_container.rs`, `backend-manager`'s `meta_dat.rs`,
> `codetracer-native-backend`'s `mcr_ctfs_read_file_from_data`,
> `cas_dedup/ctfs.py` -- index the entry array from the start of the file by
> the header's count, so they read an overflowed directory unchanged.  That is
> established by reading their code, not by a test of each.

### 1a. Profile and Whole-File Compression (version 6)

Version 6 is version 5's body plus eight header bytes. Everything §2 says about `MapBlock`'s three forms, and everything §4 says about the block map, holds in a version-6 full-profile container unchanged.

`Profile` says which body layout follows the header:

| Value | Name | Body |
|-------|------|------|
| `0` | full | Block 0, free list roots, `FileEntry` array, block map -- everything from *Block 0 Layout* above and §2 onward |
| `1` | compact | A directory of `(name, offset, length)` and the members concatenated raw, with no block map and no mapping blocks -- §1d, which is normative for every offset. A reader that does not implement it MUST refuse it rather than attempt the full body. Which profile a WRITER produces, and what a mid-recording switchover between them must preserve, is §1e |

The set is CLOSED: `0` and `1` are the only defined values and every other value is a refusal.

A compact container MUST write `max_shards = 0`. Block sharding (§9) partitions a block-number space, and the compact profile has no blocks, so "one shard" and "no sharding" would again be two spellings of one state -- the defect the `max_shards` note above was written for.

In a version-6 **full** container the free list root area and the `FileEntry` array start 8 bytes later, because the header is 8 bytes longer. Every other offset in this document is relative to those and so is unchanged:

```
Block 0 (version 6, profile = full):
  [0..23]                       ContainerHeaderV6 (24 bytes)
  [24..24+R-1]                  Free list roots (R bytes, fixed area)
  [24+R .. BlockSize-1]         FileEntry array (remaining space)

auto_entries = (BlockSize - 24 - R) / 24
root_blocks  = ceil((24 + R + MaxRootEntries * 24) / BlockSize)
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

**The directory is not a block map, and that is the design rather than a simplification.** A block map answers *which block holds byte N of this member*, which is what random access into a large member needs and what property 2 and design goal 5 are about. A directory answers *where does this member start and how long is it*, which is what a one-shot load needs. The second costs one `(u64, u64)` per member against a 4 KB mapping block per member, and it is sufficient precisely because the whole file is resident before the first query is asked. The compact profile is therefore not a cheaper encoding of the full profile's structure; it answers a different question, and it is the right profile only for a container small enough that the answer to the first question is always "all of it".

**There is NO alignment requirement, and that is a statement about what alignment is for.** Nothing in a compact container is padded to a block, a page or a word: the directory begins at 28, a member begins wherever its predecessor ended, and a 12-byte member occupies 12 bytes. Design goal 5 -- "block-aligned layout maps to HTTP range requests; one 4 KB fetch reveals full structure" -- is what alignment serves, and it is correct for the full profile. A compact container is fetched whole and issues no ranged read, so there is nothing for alignment to serve and a reader MUST NOT round any offset or length to a boundary. A writer that aligned anyway would reintroduce exactly the cost the profile exists to remove, and because of check 4 it would also produce a container every conforming reader refuses.

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

§1d says a compact container's members carry no per-member compression, and §1e says it of the output. Some member formats ARE a sequence of independently compressed frames located by offsets: the chunked compressed tables of §7 (`steps.dat` with `steps.idx`, and likewise `values`, `calls`, `events`, `spans` and the MCR snapshot payloads), `step-map.ns` ([internal-files.md](internal-files.md) §"`step-map.ns`"), and the seekable-zstd streams of [seekable-zstd.md](seekable-zstd.md). This section is how such a member is stored in a compact container, so that two writers store it alike and every reader reads it.

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
| 6 | 24-byte header with `Profile` and whole-file `Compression` (§1a, §1b) and six reserved bytes that MUST be zero. At `Profile = 0` the body is version 5's, so §2's `MapBlock` forms are unchanged; at `Profile = 1` the body is the compact layout of §1d -- a directory and the members concatenated, with no block 0, no mapping block and no alignment. NOT backward compatible, deliberately: the `FileEntry` array moves to `24 + R`, and a reader predating this version refuses the container at the version check rather than reading entries out of the reserved area (§1c). A writer emits 6 only for a container that uses one of the new fields, and §1e says which profile it chooses and what a mid-recording switchover must preserve. |
| 5 | A member of at most one block is stored without a mapping block, its `MapBlock` carrying the direct-block tag (§2, "Members of at most one block"); an empty member has `MapBlock = 0`. Readers MUST accept 5 -- and 6, which is 5's body behind the extended header -- and MUST refuse every version they do not implement, naming it (§2, "Older versions are refused"; §1c). Writers MUST write 5 unless the container uses a version-6 field. |
| 4 | Query protocol, network reader, replication, RAM cache, cached trace reader. Backward compatible: v4 readers accept v3 and v2 containers. |
| 3 | 16-byte header with encryption; binary metadata; BlockSize 4096; MaxRootEntries 0 auto-fill; small file optimization; namespaces |
| 2 | Extended header with BlockSize and MaxRootEntries |
| 1 | Initial format |

---

## 2. File Entry (24 bytes)

An array of file entries follows the free list roots in block 0.

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

The small-member layout applies to `FileEntry.MapBlock` only. Namespace descriptors (§8) and the
chain and child pointers inside a mapping (§4) are unchanged.

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
- **Maximum length:** 12 characters. Accommodates all CTFS internal names: `meta.dat` (8), `steps.dat` (9), `threads.ns` (10), `syncord.log` (11), `linehits.tc` (11), `memwrites.tc` (12).

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

### Block Allocation

- **Claim block:** `atomic_fetch_add(NextFreeBlock, 1)` -- the only shared mutable state.
- **Extend mapping:** When a block index exceeds current level capacity, allocate and chain a new mapping block via `[N-1]`.
- **O(1) amortized** for sequential appends. Mapping blocks allocated only when a level fills up.

**Null pointers during allocation (normative).** "Allocate a new mapping block" applies only when the slot has genuinely never been used. A mapping is filled in strictly increasing block index order, so a writer **MUST** treat a null pointer as "not yet allocated" only when the block index being placed is the **first index that pointer covers** -- the chain pointer at `[N-1]` only when the rebased index is `0` at the level it leads to, a level-`k` child pointer only when the remainder `idx mod usable^(k-1)` is `0` -- and **MUST** refuse the write otherwise. A null anywhere else means an earlier index already resolved through that pointer, so the container is damaged, and allocating a replacement overwrites the only reference to the existing subtree: every data block beneath it becomes unreachable and unrecoverable while the append reports success.

A refusal must be all-or-nothing: a writer that claims its data block before walking the mapping has to roll that claim back, so a refused append leaves the container byte-identical and the damage it refused over is still visible to a repair tool.

This binds *writers*. A **reader** meeting the same null has no index question to ask -- it simply cannot resolve the block -- and its own rule follows.

**Null block pointers on the read path (normative).** A reader resolving a stream **MUST** refuse that stream, by name, when any block number it resolves is `0`: the entry's mapping root, a chain pointer, a level-`k` child pointer, or a data-block pointer. Block 0 is the container's header and root directory, and `0` is the "unallocated" sentinel, so no stream may name it. This is **independent of, and additional to**, the whole-block bound a reader applies to a container whose length is not a block multiple: a null passes that bound trivially, since `0` is below every non-empty container's block count. A reader that omits it does not merely fail to detect damage -- it walks *into* block 0 and reads the container's own header and root directory as the stream's mapping table, so entry fields decode as block pointers and unrelated blocks are returned as the stream's content.

Three consequences bind with it:

- **A null is not an absence.** A reader **MUST NOT** report a stream whose entry exists but whose mapping is null as missing, nor as empty. "Not in this container", "in this container and empty", and "in this container but its mapping is not" are three different answers. An entry lookup that signals "no such name" by returning `(Size, MapBlock) = (0, 0)` **MUST** report presence separately, because `(0, 0)` is also a legitimately empty member.
- **A null is not a truncation.** The refusal **MUST NOT** blame a truncated or interrupted tail write. A container carrying a null pointer is typically a whole number of blocks and otherwise intact, and a message naming truncation sends an operator or a repair tool after damage that is not there.
- **A caller-visible failure, not a crash.** The block number comes out of the container, so on a damaged one it is corruption-controlled. It **MUST** be refused before it is multiplied by `BlockSize`; computing the offset first can overflow and abort the process instead of returning an error.

See `CTFS-Binary-Format.md` section 4, "Null pointers during allocation", and section 5d, "Null block pointers on the read path", for the full statement and the measurements behind both halves.

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

1. Find an empty slot in the file entry array (all 24 bytes zero).
2. Encode the filename using base40 and write to the `Name` field. Leave `Size` and `MapBlock` as zero. Claim no block: a member that is never written stays `(0, 0)` (§2).
3. Sync block 0 for concurrent readers.

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
6. Flush file entry for concurrent readers

### Reader Protocol

1. Read block 0 for file entry array
2. Read `FileEntry.Size` (re-read periodically for streaming), then `FileEntry.MapBlock`, each with one atomic load
3. Read data up to `Size` via the form `MapBlock` has (§2). Across a direct-to-mapped transition, loading `Size` before `MapBlock` (acquire) rules out pairing a new `Size` with the old, tagged `MapBlock`, because the writer stored `MapBlock` first; the remaining mixed pairing, an old `Size` with the new mapping, reads correct bytes, because the mapping's slot 0 is the old data block
4. For compressed streams: use companion index for chunk-level seeking

### Guarantees

- **Writers:** Block allocation is atomic. Data fully written before mapping updated. Size updated only after data committed.
- **Readers:** See previous or new Size (never partial). All data up to observed Size is valid. No locks required.

### Live progress: per-stream following, no sidecar

A concurrent (out-of-process) reader answers "how much is readable right now" from
the **container itself**, per-stream, with no external artifact:

- **Readable byte extent** of each stream is `FileEntry.Size`, updated atomically
  after each commit (re-read periodically — Reader Protocol step 2).
- **Readable record count** is derived from the companion `.idx`, which is written
  incrementally as chunks seal (§7; a stream that seals partial chunks carries a
  cumulative record count in its index). No finalization step is required.

This is the **only** progress mechanism. The format is self-contained (Property 6:
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

1. **At open** -- at the latest, before the recording's first record -- the writer writes block 0
   and `meta.dat`, complete: `meta.dat` is never rewritten, so every field and flag in it is fixed at
   open (`internal-files.md` §"Extended flags (`flags_ext`)").
2. **When a chunk of a Chunked Compressed Table seals** (§7: it reaches `chunk_size` records), the
   writer writes, before the append that sealed it returns: the chunk's bytes to their data blocks,
   every mapping slot or block that changed, the chunk's offset in the companion `.idx`, every
   interning record registered so far (`paths`, `funcs`, `types`, `varnames`, `markers` and their
   offset tables -- so that whatever the chunk refers to is readable), and then the root entries (`Size`,
   `MapBlock`) of every member it grew -- in that order, data before the entry that publishes it
   (§6, "Writer Protocol").
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

### Background Compression Writer

An alternative pattern: multiple producer threads write to per-thread buffers, a single background thread compresses and writes to CTFS. Since only one thread does block allocation, `NextFreeBlock` can be a plain local counter (no atomic overhead).

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

---

## 8. Namespaces (Small Files Collection)

A namespace maps 64-bit keys to append-only data sequences within a single CTFS internal file. Designed for millions of entries, most very small.

### Operations

- **Create entry:** Implicitly on first append to a new key.
- **Append to entry:** Append bytes to a key's data sequence.

### Sub-Block Allocation Pools

Standard block allocation wastes space when millions of keys have 8--32 byte values. Namespaces subdivide blocks into sub-blocks with **global** free lists (shared across all namespaces).

| Pool size | Sub-blocks per 4096-byte block |
|-----------|-------------------------------|
| 32 B | 128 |
| 64 B | 64 |
| 128 B | 32 |
| 256 B | 16 |
| 512 B | 8 |
| 1024 B | 4 |
| 2048 B | 2 |

**Allocation lifecycle for a key:**

1. First append: allocate 32B from global free list
2. When data outgrows current slot: allocate next larger size, copy data, release old slot
3. Continue doubling through 64B, 128B, 256B, 512B, 1024B, 2048B
4. Beyond 2048B: promote to full CTFS block with normal mapping
5. Beyond one block: standard multi-level mapping hierarchy

**Free list roots** are stored in block 0, in the area between the header and file entries (see Section 1). Each root is `(block_num: u32, slot_index: u16)` = 6 bytes. A free sub-block stores its next pointer in its data bytes (6 bytes, fits in smallest pool).

### B-Tree Key Index

Each namespace uses a B-tree on the 64-bit key. Each node occupies one CTFS block. Lookup requires O(depth) block reads -- typically 3-4 for millions of entries.

Leaf nodes contain sorted keys and **entry descriptors**. Each namespace declares a **leaf type** in its header.

#### Namespace Header (9 bytes)

```c
struct NamespaceHeader {
    uint64_t root_block;  // B-tree root (0 = empty)
    uint8_t  flags;       // bit 0: leaf_type (0=Type A, 1=Type B)
                          // bit 1: skip_sub_blocks
};
```

No entry count -- the B-tree is the source of truth.

#### Leaf Type A -- Small Entries (8-byte descriptor)

Used by namespaces with many keys and small values (`memreads.tc`).

```
Entry descriptor (8 bytes = u64 LE, bit-packed):

  Sub-block (bit 63 = 0):
    bit 63:       0 (sub-block flag)
    bits 62-15:   block_num (48 bits)
    bits 14-12:   pool_class (3 bits: 0=32B, 1=64B, ..., 6=2048B)
    bits 11-0:    slot_and_used (12 bits)

  Graduated (bit 63 = 1):
    bit 63:       1 (graduated flag)
    bits 62-32:   map_block (31 bits)
    bits 31-0:    data_size (32 bits, up to 4GB)
```

The 12-bit `slot_and_used` encodes both slot position and data size. Split depends on pool_class: `slot_index` uses `log2(4096/pool_size)` bits, `used_bytes` uses `log2(pool_size)` bits. Sum is always 12.

Each leaf holds `(4096 - header) / 16` ~ 250 entries (8 bytes key + 8 bytes descriptor).

#### Leaf Type B -- Large Entries (16-byte descriptor)

Used by namespaces with fewer keys and large values (`threads.ns`, `slc-mwr.ns`, `slc-mrd.ns`).

```
Entry descriptor (16 bytes):

  Sub-block (map_block == 0):
    first u64:    map_block = 0 (discriminator)
    second u64 (bit-packed):
      bits 63-15: block_num (49 bits)
      bits 14-12: pool_class (3 bits)
      bits 11-0:  slot_and_used (12 bits)

  Graduated (map_block != 0):
    map_block:    u64 LE (root mapping block)
    data_size:    u64 LE (unlimited)
```

`slot_and_used` bit split by pool_class:

| pool_class | pool_size | slot_index bits | used_bytes bits |
|------------|-----------|-----------------|-----------------|
| 0 | 32B | 7 | 5 |
| 1 | 64B | 6 | 6 |
| 2 | 128B | 5 | 7 |
| 3 | 256B | 4 | 8 |
| 4 | 512B | 3 | 9 |
| 5 | 1024B | 2 | 10 |
| 6 | 2048B | 1 | 11 |

Each leaf holds `(4096 - header) / 24` ~ 170 entries (8 bytes key + 16 bytes descriptor).

Sub-block support is optional -- `skip_sub_blocks` flag causes full-block allocation from the start.

### Namespace Files

| Namespace | Leaf Type | Key | Purpose |
|-----------|-----------|-----|---------|
| `linehits.tc` | `NSB1` (§8a) | location address | Step ids at each source location |
| `memwrites.tc` | `NSB1` (§8a) | memory address | Memory write time coordinates (MCR only) |
| `corrmark.ns` | `NSB1` (§8a) | XXH64 of the correlation key | Correlation markers |
| `memreads.tc` | A | memory address | Memory read time coordinates |
| `threads.ns` | B | thread_id | Per-thread event streams |
| `slc-mwr.ns` | B | slice_id | Per-thread-slice write address sets |
| `slc-mrd.ns` | B | slice_id | Per-thread-slice read address sets |


### 8a. The `NSB1` namespace image (normative)

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

A reader MUST refuse, naming the member: a member shorter than the header or not a multiple of 4096
bytes; another magic; a page number at or beyond the member's page count or 0 below the root; a node
kind other than 0 or 1; a `count` whose keys and descriptors or children do not fit in the page;
and a descriptor whose payload does not lie inside the member.

---

## 9. Multi-File Output Mode (Block Sharding)

Blocks are sharded across multiple files to exploit higher aggregate I/O throughput.

**Sharding scheme:** Block N is stored in file `N % num_shards`. Guarantees even distribution.

**Manifest file (`manifest.dat`):**

```
shard_count: u32
for each shard:
  path_length: varint
  path: bytes    (filesystem path or URL)
```

Reader resolves `block N` to `shard_file[N % shard_count]` at offset `(N / shard_count) * block_size`.

**Separation of structure and data:** B-tree blocks, mapping hierarchy, companion indices, free list metadata, and `meta.dat` stay in the **main `.ct` file** -- never sharded. Only data blocks are distributed across shards.

**Shard affinity per namespace key:** Home shard = `hash(key) % num_shards`. Sub-block phase allocations use the home shard's free lists. Multi-block growth uses round-robin across all shards.

**ShardWriter abstraction:** Each shard is managed by a ShardWriter. A single operation: `append(current_descriptor, data) -> new_descriptor`. For local multi-file, in-process calls. For network sharding, one RPC per operation.

---

## 10. Split-by-Time (Temporal File Splitting)

For long recordings, CTFS splits the trace into multiple files at full memory snapshot boundaries. Each split file is a self-contained `.ct` container with its own block numbering.

### Trace Directory Layout

```
my-recording/
  000000.ct     # Split 0 (or the only file)
  000001.ct     # Split 1
  000002.ct     # Split 2
  ...
```

Reader opens all `.ct` files in sorted order. Zero-padded indices ensure correct lexicographic sort.

**Split points:** A new file begins each time MCR writes a full memory snapshot (as opposed to a delta snapshot). The frequency of full snapshots is configurable -- more frequent splits produce smaller individual files and increase exploitable parallelism (each split can be analyzed independently). However, more frequent splits increase total trace size because each split begins with a full memory snapshot that duplicates the entire address space at that point. The trade-off is between granularity (smaller files, more parallelism, finer-grained deletion) and space efficiency (fewer snapshots, less duplication).

**Independent file deletion:** Any split file can be deleted; remaining files stay playable. Each split is self-contained with its own snapshot, block numbering, and namespace entries. The reader skips missing indices and continues with the next available file. This enables storage management policies such as retaining only segments around a known failure point.

**Namespace queries across splits:** Query all split files, concatenate results. Split ordering preserves chronological order.

**Processing parallelism:** Each split file can be processed independently -- the emulation layer runs one split per worker, building namespace entries for that time interval. No merge step is needed.

---

## 11. Network-Aware Reading

### Block-Aligned Access

All data at block-aligned boundaries. Predictable read counts:

| Operation | Reads | What |
|-----------|-------|------|
| Container discovery | 1 block | Block 0 (header + file entries) |
| Step lookup | 1-2 blocks | Companion index + one chunk |
| Namespace lookup | 3-5 blocks | B-tree walk + data read |
| Call lookup | 2-3 blocks | Companion index + one chunk |
| Interning table lookup | 2 blocks | Offset index + data record |

HTTP range requests: block `b` = bytes `[b * BlockSize, (b+1) * BlockSize)`.

### Partial Trace Cache (.ctp)

A `.ctp` file is a standard CTFS container holding only fetched blocks. Contains `presence.idx` (sorted array of remote block numbers) and `permanent.idx` (blocks not eligible for eviction). Blocks are fetched on demand and cached to disk, with an LRU RAM layer on top (default 256MB).

### Smart Query Protocol (Optional)

Storage nodes resolve queries server-side (B-tree walks, chunk decompression) in one round-trip. Collapses 3-5 block reads into one request/response.

---

## Configuration Recommendations

| Setting | Default | Rationale |
|---------|---------|-----------|
| BlockSize | 4096 | Matches OS page size and HTTP range granularity |
| MaxRootEntries | 0 (auto) | Fills block 0; sufficient for most traces |
| LRU cache | 16--64 blocks | Balances memory vs mapping re-reads |
| Chunk threshold | 4096 events | Balance compression ratio vs seek granularity |

### Block Size Trade-offs

| BlockSize | N | usable | Max L5 file size |
|-----------|---|--------|------------------|
| 1024 | 128 | 127 | ~35 TB |
| 2048 | 256 | 255 | ~280 TB |
| 4096 | 512 | 511 | ~133 PB |

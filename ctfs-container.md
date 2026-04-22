# CTFS Binary Container Format

CTFS (CodeTracer File System) is a block-based container format that stores multiple named files in a single `.ct` file. It provides a flat file system optimized for streaming writes, concurrent multi-producer access, and multiple simultaneous readers. All integers are **little-endian**.

## Properties

| # | Property | Description |
|---|----------|-------------|
| 1 | Compressed storage | Per-stream Zstd via chunked compressed tables. Transparent to the container layer. |
| 2 | Random-access seeking | O(log n) block mapping (at most 5 reads); O(1) chunk seek via companion index. |
| 3 | Low-contention concurrent writes | Single atomic `NextFreeBlock` counter; per-file single writer; no locks. |
| 4 | Multiple concurrent readers | Readers see consistent file sizes updated atomically by writers. |
| 5 | Network-efficient | Block-aligned layout maps to HTTP range requests; one 4 KB fetch reveals full structure. |
| 6 | Self-contained | All metadata in binary format within the container; no external files or JSON. |
| 7 | Streaming-compatible | Companion index available during recording; no finalization needed. |
| 8 | Encryption-aware | Container-level encryption flag; all content opaque without the key. |
| 9 | Append-only | All writes are appends; no in-place updates except atomic size counters. |
| 10 | Lock-free | Only atomic fetch-and-add and atomic stores required. |

### Non-Goals

No directories (flat namespace only), no file deletion or truncation, no file attributes, no built-in checksums, no redundancy. Designed for 10--200 files per container.

---

## 1. Container Header (16 bytes)

Block 0 begins with a 16-byte header.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0--4 | 5 | Magic | `C0 DE 72 AC E2` ("CODE TRACE") |
| 5 | 1 | Version | `4` |
| 6 | 1 | Encryption | `0` = none, `1` = AES-256-GCM |
| 7 | 1 | MaxShards | Maximum shard count (`0` = no sharding) |
| 8--11 | 4 | BlockSize | Block size in bytes (u32 LE, default 4096) |
| 12--15 | 4 | MaxRootEntries | Maximum file entries (u32 LE, `0` = auto-fill block 0) |

```c
struct ContainerHeader {
    uint8_t  magic[5];          // C0 DE 72 AC E2
    uint8_t  version;           // 4
    uint8_t  encryption;        // 0=none, 1=AES-256-GCM
    uint8_t  max_shards;        // 0 = no sharding
    uint32_t block_size;        // default 4096
    uint32_t max_root_entries;  // 0 = auto-fill block 0
};
```

**Compression is not in the header.** Different internal files may use different compression settings; the compression mode is specified per-stream in `meta.dat`.

**Encryption IS in the header** because an encrypted container is opaque -- even `meta.dat` is unreadable without the key.

### Block 0 Layout

Block 0 contains the header, free list roots, and file entries:

```
Block 0:
  [0..15]                       ContainerHeader (16 bytes)
  [16..16+R-1]                  Free list roots (R bytes, fixed area)
  [16+R .. BlockSize-1]         FileEntry array (remaining space)
```

**Free list root area size:** `R = 7 * max_shards * 6` bytes (7 pool sizes, 6 bytes per root entry). Each root entry is `(block_num: u32, slot_index: u16)` = 6 bytes. Roots are laid out as `roots[shard_id][pool_class]` in row-major order. With `max_shards = 16`: `R = 672` bytes.

**Auto-fill:** When `MaxRootEntries` is 0, file entries fill the remainder of block 0:

```
auto_entries = (BlockSize - 16 - R) / 24
```

For BlockSize=4096, max_shards=16: `auto_entries = (4096 - 16 - 672) / 24 = 141`.

For BlockSize=4096, max_shards=0 (R=0): `auto_entries = (4096 - 16) / 24 = 170`.

If `MaxRootEntries * 24 + 16 + R > BlockSize`, file entries overflow into contiguous blocks after block 0:

```
root_blocks = ceil((16 + R + MaxRootEntries * 24) / BlockSize)
```

Data block allocation begins at block number `root_blocks`.

### Version History

| Version | Description |
|---------|-------------|
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
| 8 | 8 | u64 LE | MapBlock | Root mapping block number (0 = no data) |
| 16 | 8 | u64 LE | Name | Base40-encoded filename (12 chars max) |

```c
struct FileEntry {
    uint64_t size;       // logical file size in bytes
    uint64_t map_block;  // root mapping block (0 = no data)
    uint64_t name;       // base40-encoded name
};
```

An entry where all 24 bytes are zero is an empty slot.

### Small File Optimization

If `Size <= BlockSize`, the `MapBlock` field points directly to the single data block (no mapping block allocated). Once a file grows beyond one block, a mapping block is allocated and the data block becomes the first entry in the mapping hierarchy.

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

1. If `file_entry.size <= BlockSize`: `MapBlock` is the data block directly (small file optimization).
2. Otherwise, compute `block_index = pos / BlockSize`. Determine the mapping level, follow chain pointers to that level, navigate down through mapping entries (dividing by powers of `usable`) to reach the data block. At most 5 block reads.

### Block Allocation

- **Claim block:** `atomic_fetch_add(NextFreeBlock, 1)` -- the only shared mutable state.
- **Extend mapping:** When a block index exceeds current level capacity, allocate and chain a new mapping block via `[N-1]`.
- **O(1) amortized** for sequential appends. Mapping blocks allocated only when a level fills up.

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
2. Encode the filename using base40 and write to the `Name` field. Leave `Size` and `MapBlock` as zero.
3. Sync block 0 for concurrent readers.

### Appending Data

Four cases based on current file state:

1. **First write** (size=0, map_block=0): Allocate a data block, set `MapBlock`.
2. **Small file, fits** (size <= BlockSize, no boundary crossing): Write to existing block.
3. **Small-to-mapped transition** (size <= BlockSize, crosses boundary): Allocate mapping block, move data block ref to slot 0, update `MapBlock`, allocate new data block.
4. **Normal mapped file** (size > BlockSize): Resolve/allocate through mapping hierarchy.

After writing: atomically update `FileEntry.Size` (makes data visible to readers). Data must be fully written before Size is updated. Write barriers enforce ordering.

### Reading Data

- Return EOF if `offset >= file_entry.size`.
- Clamp read length to file size boundary.
- For each spanned block: resolve via mapping hierarchy (or directly for small files), read bytes.

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
3. Update mapping block pointers (thread-local, no contention)
4. Write barrier
5. Atomically store new `FileEntry.Size`
6. Flush file entry for concurrent readers

### Reader Protocol

1. Read block 0 for file entry array
2. Read `FileEntry.Size` (re-read periodically for streaming)
3. Read data up to `Size` via block mapping
4. For compressed streams: use companion index for chunk-level seeking

### Guarantees

- **Writers:** Block allocation is atomic. Data fully written before mapping updated. Size updated only after data committed.
- **Readers:** See previous or new Size (never partial). All data up to observed Size is valid. No locks required.

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
   c. Append u64 byte offset to `foo.idx`
   d. Write compressed data to `foo.dat`
   e. Sync both file entry sizes for concurrent readers

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

Used by namespaces with many keys and small values (`memwrites.tc`, `linehits.tc`, `memreads.tc`).

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
| `linehits.tc` | A | global line index | Source line hit time coordinates |
| `memwrites.tc` | A | memory address | Memory write time coordinates |
| `memreads.tc` | A | memory address | Memory read time coordinates |
| `threads.ns` | B | thread_id | Per-thread event streams |
| `slc-mwr.ns` | B | slice_id | Per-thread-slice write address sets |
| `slc-mrd.ns` | B | slice_id | Per-thread-slice read address sets |

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

# Internal Files

A CTFS container (`.ct` file) stores several named internal files. This document describes the standard files and their data abstractions.

## Reusable Data Abstractions

These are higher-level structures built on top of CTFS internal files. They are not part of the container format but are standard patterns used by all CodeTracer recorders and readers.

### Fixed-Size Record Table

A CTFS file storing N records of constant size S. Record `i` occupies bytes `[i*S, (i+1)*S)`. Seeking is O(1): compute byte offset, resolve the CTFS block.

**Example uses:** mmap entries (33 bytes each), fixed-size index entries.

### Variable-Size Record Table (dat + off)

Two CTFS files working together:

- **Data file** (e.g., `paths.dat`): records appended sequentially, variable length
- **Offset file** (e.g., `paths.off`): fixed-size table of u64 values, entry `i` = byte offset of record `i` in the data file

To read record `i`:

1. Read `offset[i]` from offset file (8 bytes at position `i * 8`)
2. Read `offset[i+1]` to determine length (or use data file size for last record)
3. Read `offset[i+1] - offset[i]` bytes from data file at `offset[i]`

### Chunked Compressed Table (dat + idx)

Extends fixed-size or variable-size tables with per-chunk compression. Records are grouped into chunks of `chunk_size` records, each independently compressed with Zstd.

- **Data file** (`foo.dat`): concatenated compressed chunks, no inline headers
- **Index file** (`foo.idx`): starts with `chunk_size: u32`, then one `u64` byte offset per chunk

Record N is in chunk `N / chunk_size`. The companion index provides O(1) access to any chunk's byte offset. See [ctfs-container.md](ctfs-container.md) Section 7 for full details.

### Interning Tables

Deduplicated records using the variable-size record table pattern. A `.dat` file holds serialized records, a `.off` file holds the offset index. Event streams store numeric IDs that reference interned records.

| Table | Data File | Offset File | Record Format |
|-------|-----------|-------------|---------------|
| Source paths | `paths.dat` | `paths.off` | raw bytes (file path) |
| Variable names | `varnames.dat` | `varnames.off` | raw bytes (name) |
| Types | `types.dat` | `types.off` | kind: u8, lang_type_len: varint, lang_type: bytes, specific_info: binary |
| Functions | `funcs.dat` | `funcs.off` | global_line_index: varint, name_len: varint, name: bytes |

Records are referenced by 0-based index. Interning tables are loaded at reader startup (typically 1-5 MB total).

---

## Runtime Tracing (DB Traces)

A materialized trace `.ct` from runtime recorders (Python, Ruby, JavaScript, Bash, Noir/WASM):

| File | Abstraction | Purpose |
|------|-------------|---------|
| `meta.dat` | Binary metadata | Program, paths, recorder info (see Metadata section) |
| `steps.dat` | Chunked compressed | Combined execution + values stream (steps with embedded variable values) |
| `steps.idx` | Companion index | Chunk index for `steps.dat` |
| `calls.dat` | Var-size record | Call stream (complete call records with args/return) |
| `events.dat` | Chunked compressed | IO event stream (stdout, stderr, file ops, errors) |
| `events.idx` | Companion index | Chunk index for `events.dat` |
| `paths.dat` | Var-size record | Interned source paths |
| `paths.off` | Offset index | Path offset index |
| `funcs.dat` | Var-size record | Interned function records |
| `funcs.off` | Offset index | Function offset index |
| `types.dat` | Var-size record | Interned type records |
| `types.off` | Offset index | Type offset index |
| `varnames.dat` | Var-size record | Interned variable names |
| `varnames.off` | Offset index | Variable name offset index |
| `linehits.tc` | Namespace (Type A) | Source line to step ID mapping |
| `memwrites.tc` | Namespace (Type A) | Variable/place to change history |

### Stream Descriptions

| Stream | CTFS File | Abstraction | Access Pattern |
|--------|-----------|-------------|----------------|
| Execution | `steps.dat` | Chunked compressed | Sequential scan, point lookup |
| Values | `steps.dat` | Chunked compressed | Point lookup by step index |
| Calls | `calls.dat` | Var-size record | Random access by call_key |
| IO Events | `events.dat` | Chunked compressed | Paginated scan |

`steps.dat` records are tiny (2-4 bytes each), so chunks hold thousands of steps. The values stream is parallel-indexed with the execution stream -- record N in `steps.dat` corresponds to step N.

`calls.dat` is indexed by `call_key`. To find a step's enclosing call, use proportional (interpolation) search on `calls.dat` -- each call record stores `[first_step_id, last_step_id]` ranges.

Event type wire formats are specified in [trace-events.md](trace-events.md).

---

## Multi-Core Recorder (MCR) Traces

| File | Abstraction | Purpose |
|------|-------------|---------|
| `meta.dat` | Binary metadata | Platform, tick source, timestamps |
| `threads.ns` | Namespace (Type B) | Per-thread event streams (keyed by thread_id) |
| `syncord.log` | Append-only | Global synchronization ordering |
| `geid.idx` | Fixed-size record | GEID-to-checkpoint index |
| `cp.dat` | Var-size record | Checkpoint data (base snapshots + delta chains) |
| `cp.off` | Offset index | Checkpoint ID to offset in `cp.dat` |
| `memwrites.tc` | Namespace (Type A) | Address to write history (omniscient queries) |
| `linehits.tc` | Namespace (Type A) | Source line to GEID lists (line hit queries) |

All files are append-only during recording.

### Thread Streams via Namespaces

Thread event streams are stored in `threads.ns`, a namespace keyed by `thread_id` (u64). This replaces the previous model of one CTFS file per thread (`t00000000001`, etc.), which was limited by MaxRootEntries. With namespaces, the thread count is unlimited -- the B-tree scales to millions of keys.

### Checkpoint Packing (cp.dat + cp.off)

MCR checkpoints are packed as a variable-size record table. Each checkpoint record contains register state, thread ticks, and page data (full pages or byte-level deltas against the parent checkpoint).

Checkpoints form incremental chains: a base checkpoint stores a full memory snapshot, followed by delta checkpoints storing only changed pages.

**Restoring memory state at a target GEID:**

1. Look up GEID in `geid.idx` to find checkpoint ID
2. Read `cp.off[checkpoint_id]` for byte offset in `cp.dat`
3. Follow parent chain backward to nearest base checkpoint
4. Read base + all deltas sequentially from `cp.dat`
5. Apply page deltas in order to reconstruct full memory state
6. Hand register state to last-mile controller for emulation to exact target tick

The variable-size record table makes this a single contiguous scan through `cp.dat`.

---

## Metadata (meta.dat)

A single binary metadata file using split-binary encoding.

### Layout

```
Header (8 bytes):
  magic: "CTMD" (4 bytes: 0x43, 0x54, 0x4D, 0x44)
  version: u16 LE (currently 1)
  flags: u16 LE

Fields (varint-prefixed):
  program: varint length + UTF-8 bytes
  args_count: varint
    args[0..args_count-1]: varint length + UTF-8 bytes each
  workdir: varint length + UTF-8 bytes
  recorder_id: varint length + UTF-8 bytes
  path_count: varint
    paths[0..path_count-1]: varint length + UTF-8 bytes each
```

### Extended Fields (flags bitmask)

**Flag bit 0 -- MCR fields:**

```
  tick_source: varint (0=rdtsc, 1=monotonic, 2=perf_counter)
  total_threads: varint
  atomic_mode: varint (0=relaxed, 1=seq_cst)
  total_checkpoints: u32 LE
  start_time_unix_us: u64 LE
```

The `compression` field specifies the default per-stream compression. This is in metadata (not container header) because different internal files may use different settings.

---

## Global Line Index

Source files are concatenated into a virtual address space where each line has a unique global index.

```
global_index(file_id, line) = prefix_sums[file_id] + line

prefix_sums[0] = 0
prefix_sums[k] = prefix_sums[k-1] + line_count[k-1]
```

The prefix-sum array is computed once at startup from the interning table.

### Uses

1. **Compact Step events**: A step stores one global line index instead of separate (path_id, line).
2. **Namespace key for `linehits.tc`**: Maps global line index to hit time coordinates.

### Namespace Key Summary

| Namespace | Key | Meaning |
|-----------|-----|---------|
| `linehits.tc` | global line index | Source line hit time coordinates |
| `memwrites.tc` | memory address | Memory write time coordinates |
| `memreads.tc` | memory address | Memory read time coordinates |
| `slc-mwr.ns` | slice_id | Per-thread-slice write address sets |
| `slc-mrd.ns` | slice_id | Per-thread-slice read address sets |
| `threads.ns` | thread_id | Per-thread event streams |

---

## Native Recorder Files

These files are used by the native recorder for binary/debug information. They may be present in `.ct` files produced by the native recorder.

### `filemap.bin`

Maps CTFS-internal short names to real filesystem paths for binaries, debug symbols, and source files.

**Header** (8 bytes):

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic: `46 4D 41 50` ("FMAP") |
| 4 | 2 | Version (u16 LE). Current: 1. |
| 6 | 2 | Entry count (u16 LE) |

**Each entry**:

| Field | Size | Encoding |
|-------|------|----------|
| `ctfs_name` | 8 | u64 LE (Base40-encoded) |
| `entry_type` | 1 | 0 = Binary, 1 = DebugSymbol, 2 = SourceFile |
| `flags` | 1 | bit 0: is_main_executable, bit 1: is_dynamic_linker |
| `build_id_len` | 1 | u8 |
| `build_id` | build_id_len | raw bytes |
| `path_len` | 1-10 | LEB128 varint |
| `path` | path_len | UTF-8 string |

Type-specific trailing fields:

- **DebugSymbol**: `binary_ref` (u64 LE) -- Base40-encoded CTFS name of parent binary
- **SourceFile**: `compilation_dir_len` (LEB128) + `compilation_dir` (UTF-8)
- **Binary**: no additional fields

### `platform.bin`

Platform description for the recording machine.

**Header**: `50 4C 41 54` ("PLAT", 4 bytes)

**Fixed fields** (20 bytes at offset 4):

| Offset | Size | Field |
|--------|------|-------|
| 4 | 1 | `os`: 0=Linux, 1=macOS, 2=Windows, 3=FreeBSD |
| 5 | 1 | `arch`: 0=x86_64, 1=aarch64, 2=riscv64 |
| 6 | 1 | `pointer_size`: typically 8 |
| 7 | 1 | `endianness`: 0=little-endian |
| 8 | 4 | `page_size` (u32 LE) |
| 12 | 2 | `kernel_major` (u16 LE) |
| 14 | 2 | `kernel_minor` (u16 LE) |
| 16 | 2 | `kernel_patch` (u16 LE) |
| 18 | 6 | Reserved (zero) |

**Variable fields** (after offset 24): `libc_name` and `kernel_version` as LEB128-prefixed UTF-8 strings.

### `mmap.bin`

Memory mapping table.

**Header** (8 bytes): Magic `4D 4D 41 50` ("MMAP") + entry count (u32 LE).

**Each entry** (33 bytes, fixed-size):

| Offset | Size | Field |
|--------|------|-------|
| +0 | 8 | `address` (u64 LE) |
| +8 | 8 | `size` (u64 LE) |
| +16 | 8 | `binary_ref` (u64 LE, Base40) |
| +24 | 8 | `file_offset` (u64 LE) |
| +32 | 1 | `permissions` (u8: bit 0=read, 1=write, 2=execute, 3=private) |

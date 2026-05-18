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
| `meta.dat` | Binary metadata | Platform, tick source, timestamps, hook profile (see Metadata section) |
| `threads.ns` | Namespace (Type B) | Per-thread event streams (keyed by thread_id) |
| `syncord.log` | Append-only | Global synchronization ordering |
| `geid.idx` | Fixed-size record | GEID-to-checkpoint index |
| `cp.dat` | Var-size record | Checkpoint data (base snapshots + delta chains) |
| `cp.off` | Offset index | Checkpoint ID to offset in `cp.dat` |
| `cp0.regs` | Raw binary | Initial register snapshot at record-start (152 bytes typical) |
| `cp0.mem` | Raw binary | Initial memory snapshot at record-start (sequence of `(address, size, bytes)` tuples) |
| `cp0.fsbase` | Raw binary | Initial `fsbase`/`gsbase` (16 bytes; x86-64 Linux only) |
| `cp0.maps` | Raw text | Verbatim `/proc/self/maps` text at cp0 capture time |
| `debug.dat` | Raw binary | Full ELF of the recorded binary, including `.debug_*` sections |
| `memwrites.tc` | Namespace (Type A) | Address to write history (omniscient queries) |
| `linehits.tc` | Namespace (Type A) | Source line to GEID lists (line hit queries) |

All files are append-only during recording.

**Two flavours of MCR checkpoint state coexist in the container:**

- **`cp0.*` — initial-state seed (this section).** Captured once at
  record start by the LD_PRELOAD interposer (or any future
  ptrace-based equivalent). Seeds the emulator before replay begins:
  `cp0.regs` flows into `mcrSetRegisters`, `cp0.mem` into a sequence
  of `mcrLoadMemoryRegion` calls, `cp0.fsbase`/`cp0.maps` provide
  diagnostic / rebase context, and `debug.dat` is parsed for DWARF
  line resolution. All are MCR-only and all are optional in the sense
  that the replay backend falls back to degraded behaviour when they
  are absent.
- **`cp.dat` + `cp.off` — delta-chain checkpoints (next sub-section).**
  Periodic snapshots written during recording so the replay engine
  can seek to an arbitrary tick without re-emulating from cp0. Also
  MCR-only.

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

### Initial Register Snapshot (cp0.regs)

A flat, raw-bytes CTFS file carrying the GPR state of the first
recorded thread at the moment cp0 was captured. Written by the
LD_PRELOAD `__libc_start_main` wrapper after libc startup completes
and just before control transfers to the user `main` (writer:
`codetracer-native-recorder/ct_interpose/src/ct_interpose/full_snapshot.c`,
`_ct_full_snapshot_write_regs`). Total typical size is 152 bytes (one
thread, compact layout).

**Outer wrapper** (per thread, repeated end-to-end if multiple threads
are present; readers stop after the first thread):

| Offset | Size | Field |
|--------|------|-------|
| +0 | 4 | `tid` (u32 LE) -- recording-thread id; 0 for the main thread |
| +4 | 4 | `reg_data_len` (u32 LE) -- length of the inner register body |
| +8 | `reg_data_len` | `reg_data[reg_data_len]` -- one of the two layouts below |

**Inner layout A -- compact, 144 bytes (`reg_data_len = 144`).**
Written by the LD_PRELOAD wrapper. Eighteen `u64 LE` values in the
exact argument order of `mcrSetRegisters`:

| Index | Offset | Register |
|-------|--------|----------|
| 0 | 0 | `rax` |
| 1 | 8 | `rbx` |
| 2 | 16 | `rcx` |
| 3 | 24 | `rdx` |
| 4 | 32 | `rsi` |
| 5 | 40 | `rdi` |
| 6 | 48 | `rbp` |
| 7 | 56 | `rsp` |
| 8 | 64 | `r8` |
| 9 | 72 | `r9` |
| 10 | 80 | `r10` |
| 11 | 88 | `r11` |
| 12 | 96 | `r12` |
| 13 | 104 | `r13` |
| 14 | 112 | `r14` |
| 15 | 120 | `r15` |
| 16 | 128 | `rip` (resume address = the user's real `main`) |
| 17 | 136 | `rflags` |

**Inner layout B -- ptrace `user_regs_struct`, 216 bytes
(`reg_data_len = 216`).** Written by recorders that capture state via
`PTRACE_GETREGS` (no producer ships this today; readers accept it for
forward compatibility). Twenty-seven `u64 LE` values in Linux's
`<sys/user.h>` order:

| Index | Offset | Register |
|-------|--------|----------|
| 0 | 0 | `r15` |
| 1 | 8 | `r14` |
| 2 | 16 | `r13` |
| 3 | 24 | `r12` |
| 4 | 32 | `rbp` |
| 5 | 40 | `rbx` |
| 6 | 48 | `r11` |
| 7 | 56 | `r10` |
| 8 | 64 | `r9` |
| 9 | 72 | `r8` |
| 10 | 80 | `rax` |
| 11 | 88 | `rcx` |
| 12 | 96 | `rdx` |
| 13 | 104 | `rsi` |
| 14 | 112 | `rdi` |
| 15 | 120 | `orig_rax` |
| 16 | 128 | `rip` |
| 17 | 136 | `cs` |
| 18 | 144 | `eflags` |
| 19 | 152 | `rsp` |
| 20 | 160 | `ss` |
| 21 | 168 | `fs_base` |
| 22 | 176 | `gs_base` |
| 23 | 184 | `ds` |
| 24 | 192 | `es` |
| 25 | 200 | `fs` |
| 26 | 208 | `gs` |

Readers select the layout by inspecting `reg_data_len`. Any other
length is rejected. Reader contract: the emulator's
`mcrSetRegisters` consumes the decoded registers verbatim; see
`ct_emulator/src/ct_emulator/ctfs_bridge.nim::loadInitialStateFromTrace`
(Nim) and `codetracer/src/db-backend/src/emulator_session.rs`
(`decode_first_thread_registers`, Rust) for the canonical decoders.

### Initial Memory Snapshot (cp0.mem)

A flat, raw-bytes CTFS file holding the live program memory as
captured at cp0 time. Written by the same LD_PRELOAD interposer
(`_ct_full_snapshot_walk` in `full_snapshot.c`). Typical size scales
with the program's resident set: a few megabytes for trivial
programs, ~90 MB for `inventory_service`. The recorder bounds total
size with the soft cap `CT_FULL_SNAPSHOT_LIMIT_MB` (default 256 MB)
which emits a warning but does not truncate.

**Wire format.** A sequence of `(address, size, bytes)` tuples
concatenated end-to-end, one tuple per readable, non-skipped
`/proc/self/maps` entry. There is no count prefix, no per-region
header beyond `(address, size)`, no terminator, and no padding --
parsing stops when the file ends.

Per tuple:

| Offset | Size | Field |
|--------|------|-------|
| +0 | 8 | `address` (u64 LE) -- region start in the recorded process's VAS |
| +8 | 8 | `size` (u64 LE) -- region length in bytes |
| +16 | `size` | `bytes[size]` -- raw region contents read via `pread(/proc/self/mem)` |

The writer drops any region for which a full read fails (e.g. EIO on
PROT_NONE guards) and excludes regions whose pathname is in the
recorder's skip-set (e.g. `[vvar]`, `[vsyscall]`). `[stack]` is
included in the `__libc_start_main` wrapper's re-capture but excluded
from the earlier library-constructor capture.

Reader contract: each tuple is installed into the emulator via
`mcrLoadMemoryRegion(address, bytes_ptr, size)`. See
`ct_replayer/src/ct_replayer/trace_loader.nim::readMemorySnapshot`
(Nim) and `codetracer/src/db-backend/src/emulator_session.rs`
(Rust) for the canonical parsers.

### Initial Segment Bases (cp0.fsbase)

A 16-byte raw binary CTFS file holding the recording thread's
`fsbase` and `gsbase` at cp0 time. The emulator needs `fsbase` to
step past libc's stack-canary fetch (`mov rdi, fs:[0x28]`) inside
`__libc_start_main`; without it the emulator faults a few hundred
instructions into libc startup.

Layout (little-endian, no header):

| Offset | Size | Field |
|--------|------|-------|
| +0 | 8 | `fsbase` (u64 LE) -- value from `arch_prctl(ARCH_GET_FS, ...)` |
| +8 | 8 | `gsbase` (u64 LE) -- value from `arch_prctl(ARCH_GET_GS, ...)` |

Writer: `ct_full_snapshot_write_fsbase_linux` in `full_snapshot.c`.
A read error during recording leaves the corresponding slot zero; an
entirely absent sidecar means the emulator defaults both bases to
zero (pre-M-EM3 behaviour), which is correct for programs that never
touch TLS but breaks libc startup.

x86-64 Linux only. Other platforms do not currently ship this file.

### Address-Space Map (cp0.maps)

A raw-text CTFS file containing a verbatim, byte-for-byte copy of the
recording process's `/proc/self/maps` at cp0 capture time. No
filtering, no normalisation, no trailing terminator beyond whatever
the kernel emitted.

**Encoding.** UTF-8-compatible 7-bit ASCII (kernel never emits
non-ASCII bytes in this file). One mapping per line; each line follows
the standard Linux kernel format:

```
<start>-<end> <perms> <offset> <dev>:<inode>    <pathname>
```

where `<start>` and `<end>` are lowercase hexadecimal addresses
without a `0x` prefix, `<perms>` is the 4-character `rwxp`/`rwxs`
string, `<offset>` is a hex file offset, `<dev>` is the
`<major>:<minor>` device pair (also hex), `<inode>` is a decimal
inode number, and `<pathname>` is the resolved mapping path or a
bracketed pseudo-name such as `[heap]`, `[stack]`, `[vvar]`, or
`[vdso]`. Anonymous mappings have an empty pathname.

The recorder buffers the file through a 128 KiB stack buffer
(`maps_buf` in `_ct_full_snapshot_walk`) and writes the truncated
length on overflow; in practice processes with <~1500 mappings fit
without truncation.

Reader contract: the replay backend parses this text to recover the
ASLR load base of the main executable so it can rebase runtime PCs
into the static address space DWARF encodes. Without `cp0.maps`,
line resolution falls back to line 1 for relocated binaries. See
`codetracer/src/db-backend/src/emulator_session.rs` (`parse_cp0_maps`)
for the parser.

### Bundled Debug Binary (debug.dat)

A raw-binary CTFS file containing the **full ELF of the recorded
binary**, captured at record time exactly as it exists on disk -- no
stripping, no filtering, no repackaging. Includes the regular code /
rodata / .eh_frame sections as well as every `.debug_*` section
present in the recorded ELF. Typical size: a few MB for ordinary
release builds; the recorder enforces a 64 MiB soft cap
(`MaxDwarfBundleBytes`) and skips the bundle with a warning if the
binary is larger.

Writer: `readBinaryForDwarfBundle` in
`codetracer-native-recorder/ct_cli/src/ct_cli/dwarf_paths_extractor.nim`,
which `readFile`s the binary path verbatim. The bundle is then
written to the container via `tw.writeRawFile("debug.dat", bytes)`
from `record_cmd.nim`.

Reader contract: the replay backend parses the blob with
`DwarfIndex::from_elf_bytes` to resolve emulator PCs to
`(file, line, function)` triples. The ELF wrapper is required (the
parser handles both the wrapper and the embedded DWARF), and future
milestones plan to consume `.eh_frame` from the same blob for stack
unwinding. When `debug.dat` is absent or unreadable, the backend
falls back to producing `(paths[0], 1)` line locations.

Why bundle the whole ELF instead of just `.debug_*` sections: the
DWARF parser already understands the ELF container and would have to
synthesise one if handed loose sections; carrying the original file
also keeps a single, audit-friendly artifact in the trace.

---

## Metadata (meta.dat)

A single binary metadata file using split-binary encoding.

### Layout

```
Header (8 bytes):
  magic: "CTMD" (4 bytes: 0x43, 0x54, 0x4D, 0x44)
  version: u16 LE (currently 3)
  flags: u16 LE
    bit 0       -- FLAG_HAS_MCR_FIELDS (extended block present)
    bit 1       -- FLAG_HAS_REPLAY_LAUNCH_FIELDS (M-RLP-1, see below)
    bit 2       -- FLAG_HAS_LAYOUT_SNAPSHOT (M-RLP-2, see below)
    bit 3       -- FLAG_HAS_TRACE_FILTER_PROVENANCE (filter chain block present, TF-M7)
    bits 4..15  -- reserved; readers reject when set

Fields (varint-prefixed):
  recording_id: varint length + UTF-8 bytes (required, M-REC-1)
  program: varint length + UTF-8 bytes
  args_count: varint
    args[0..args_count-1]: varint length + UTF-8 bytes each
  workdir: varint length + UTF-8 bytes
  recorder_id: varint length + UTF-8 bytes
  path_count: varint
    paths[0..path_count-1]: varint length + UTF-8 bytes each
```

Varints are unsigned LEB128 (max 10 bytes). Strings are UTF-8 without
a NUL terminator.

**`recording_id` (M-REC-1).** The canonical identifier for this
recording: a UUIDv7 (RFC 9562) minted by the recorder at record start
and stored in its lowercase hyphenated 36-char text form (e.g.
`01949fcc-7d92-7e9c-aaaa-bbbbbbbbbbbb`). UUIDv7's first 48 bits are
the Unix-epoch-ms timestamp big-endian, so two recordings made on the
same host one millisecond apart sort by id lex-ascending — the
load-bearing property that lets `ls <traces>/` and the SQLite
recording index serve recordings in creation order without a separate
timestamp column. Required as of v3; parsers reject metadata with a
missing or malformed value. Rationale and migration roadmap:
`codetracer-specs/Refactoring-Plans/Recording-Identifier-Migration.md`.

### Version History

- **v1** -- initial release. Removed before any external consumer
  shipped; `meta.json` carried the `hookProfile` / `hookStrategies`
  fields out-of-band during the v1 window.
- **v2** -- appended `hookProfile` + `hookStrategies` to the end of
  the MCR extended-fields block.
- **v3** (current, M-REC-1, 2026-05-18) -- prepended a required
  `recording_id` UUIDv7 string before the existing `program` field.
  Pre-1.0 there is no backcompat shim: v2 fixtures must be
  regenerated. Spec:
  `codetracer-specs/Refactoring-Plans/Recording-Identifier-Migration.md`
  § 3.

### Extended Fields (flags bitmask)

**Flag bit 0 -- MCR fields.** When set, the block below follows the
paths list. Every field is varint-encoded (no fixed-width integers):

```
  tick_source: varint (TickSource enum ordinal)
  total_threads: varint
  atomic_mode: varint (AtomicMode enum ordinal)
  total_events: varint
  total_checkpoints: varint
  start_time_unix_us: varint
  platform: varint length + UTF-8 bytes
  tick_granularity: varint length + UTF-8 bytes
  tick_source_str: varint length + UTF-8 bytes
  atomic_mode_str: varint length + UTF-8 bytes
  start_time_str: varint length + UTF-8 bytes
  hookProfile: varint length + UTF-8 bytes                  (v2+)
  hookStrategies_count: varint                              (v2+)
    hookStrategies[0..count-1]: varint length + UTF-8 bytes each
```

Notes:

- `tick_source` / `atomic_mode` are stored as raw enum ordinals; the
  paired `tick_source_str` / `atomic_mode_str` strings carry the
  human-readable form for diagnostic surfaces.
- `hookProfile` names the active MCR hook profile (e.g. `"default"`,
  `"dotnet"`, `"pal_probe"`).
- Each `hookStrategies[i]` names one active hook strategy (e.g.
  `"ldpreload"`, `"seccomp_unotify"`, `"callsite_patch"`). The
  combined `(hookProfile, hookStrategies)` pair is the canonical
  record of how a trace was recorded -- consumers must round-trip it
  on re-record / re-export.
- v2 writers always emit the `hookProfile` + `hookStrategies` block
  when `FLAG_HAS_MCR_FIELDS` is set (even with empty values); v1
  fixtures lack the tail entirely.

**Flag bit 1 -- Replay-launch fields (M-RLP-1, §6A.5).** When set,
the block below follows the MCR extended-fields block (or, if
`FLAG_HAS_MCR_FIELDS` is clear, follows the `paths` list directly).
Records replay-launch address-space hardening state captured at
record time so the replay backend can decide between hard-pin and
soft-pin modes:

```
  aslr_disabled: u8 (0 = false, 1 = true)
```

**Flag bit 2 -- Layout snapshot (M-RLP-2, §6B.7).** When set, the
block below follows the replay-launch block (or, if
`FLAG_HAS_REPLAY_LAUNCH_FIELDS` is clear, follows the MCR / `paths`
block per the same composition rules).  Carries a fingerprint of the
recording process's address-space layout at `__libc_start_main`
wrapper entry; the replay side computes the same fingerprint at the
same instrumentation point and compares against `layout_hash`:

```
  layout_hash: u64 LE (XXH64 of fingerprint bytes, seed 0)
  fingerprint_len: varint
  fingerprint[fingerprint_len]: bytes
```

Per-entry fingerprint layout (one tuple per `/proc/self/maps`
entry, in stream order): `u64 start`, `u64 end`, `u32 prot_flags`
(bit 0=R, 1=W, 2=X, 3=private), `varint name_len`, `bytes name`,
`u8 build_id_len`, `bytes build_id`.

**Flag bit 3 -- Trace filter provenance (TF-M7).** When set, the
block below follows the layout-snapshot block (or, if upstream
flag bits are clear, follows the most recent populated block per
the same composition rules). The provenance captures which filter
files were active for the recording session, in their composition
order, per [Trace-Filters.md](Trace-Filters.md) § 7:

```
  trace_filter_count: varint
    trace_filter_entries[0..count-1]:
      path: varint length + UTF-8 bytes
      sha256: 32 raw bytes (no length prefix)
```

Notes:

- Entries appear in composition order: builtin default first, then
  project auto-discovered filter, then env-var filters, then CLI
  `--trace-filter:` arguments. See
  [Trace-Filters.md](Trace-Filters.md) § 5 for the composition rules.
- The `path` field MAY use sentinel values for filters that aren't
  loaded from a real file path — `<inline:builtin-default>` is the
  recommended sentinel for the recorder-embedded default. Sentinel
  paths begin with `<` and end with `>`.
- `sha256` is the raw 32-byte SHA-256 digest of the filter file's
  bytes (or, for inline filters, the embedded TOML string's bytes).
  Computed once at load time. Readers SHOULD render this as a
  lowercase hex string for diagnostic surfaces.
- A `trace_filter_count` of `0` is legal and means "no filters were
  active" (distinct from the flag being clear, which means "the
  recorder did not record provenance"). Recorders that implement
  trace filters MUST set the flag and emit at least the builtin
  default entry.
- Schema-versioning for the filter chain itself lives in each filter
  file's `[meta] version = N` field (Trace-Filters.md § 11); the
  `meta.dat` block only records provenance, not the rules.

The canonical writer is `writeMetaDatToBuffer` in
`codetracer-trace-format-nim/src/codetracer_trace_writer/meta_dat.nim`.
The canonical readers are the same Nim file's `readMetaDat` and the
Rust `parse_meta_dat` in
`codetracer/src/db-backend/src/ctfs_trace_reader/meta_dat.rs`.

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

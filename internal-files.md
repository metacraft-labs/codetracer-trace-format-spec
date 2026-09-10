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
| Source paths | `paths.dat` | `paths.off` | raw bytes (file path); column-aware traces append a per-line byte-length table — see "paths.dat Layout A" below |
| Variable names | `varnames.dat` | `varnames.off` | raw bytes (name) |
| Types | `types.dat` | `types.off` | kind: u8, lang_type_len: varint, lang_type: bytes, specific_info: binary |
| Functions | `funcs.dat` | `funcs.off` | global_line_index: varint, name_len: varint, name: bytes |
| Correlation-marker labels | `markers.dat` | `markers.off` | raw bytes (boundary label) |

Records are referenced by 0-based index. Interning tables are loaded at reader startup (typically 1-5 MB total).

#### `markers.dat` — correlation-marker labels

The boundary label of a correlation marker (`corrmark.ns`, and the
`boundary_id` field of a `MarkerPayload`) is interned like any other repeated
string, so the hot path can pass an integer.

- **Record format:** raw label bytes, exactly as `varnames.dat`.
- **Id assignment:** 0-based, in first-declaration order, assigned by the
  writer's `ensure_marker_id`-style operation. Ids are per-container.
- **Written lazily.** Unlike the four tables above — which every trace
  creates — `markers.dat` / `markers.off` appear only once a marker label is
  actually interned. A recording that declares no marker is byte-identical to
  one written before marker labels existed.
- **`meta.dat` flag: bit 14**, `FLAG_HAS_CORRELATION_INDEX` — the same bit
  that covers `corrmark.ns`, **not** bit 12's `FLAG_HAS_INTERNING_TABLES` set.

##### Why bit 14 rather than joining bit 12

Three reasons, and the first is decisive:

1. **Bit 12 is a cross-implementation agreement.** Its own definition requires
   it to match in three places — the Nim writer's
   `codetracer_trace_writer/meta_dat.nim`, Rust's
   `codetracer_trace_writer::meta_dat::FLAG_HAS_INTERNING_TABLES`, and the
   db-backend's `FLAG_HAS_INTERNING_TABLES`. Redefining what it covers means
   changing a settled three-way contract to describe a table two of those
   readers have no use for.
2. **The marker table is meaningless without `corrmark.ns`.** They are written
   together and read together, so one bit describing "this recording carries a
   correlation index, and the label table it refers to" is the honest unit.
   Bit 12's four tables are, by contrast, always created together and always
   present.
3. **Bit 15 is the last bit.** It is spoken for by
   `FLAG_HAS_LINE_COUNT_TABLE`, and spending a bit on a table already implied
   by bit 14 would be spending the format's last one for nothing.

Both bits keep the semantics bits 8–14 already have: **additive hints** — the
file-entry array, not the flag, is the authority on what a container holds, and
a reader that has no use for the index loses nothing by ignoring it.

**"Additive" is about the FILES, not about the bit.** An earlier revision of
this section said a reader that does not know bit 14 "ignores `corrmark.ns` and
`markers.dat` entirely". That is not what any of the three implementations do,
and the difference is the whole rollout: every reader refuses a container whose
flag word carries a bit outside its own known mask, so an unrecognised bit
rejects the *container*, not just the files it announces. This is the same
property bit 13 records, and it makes the ordering **readers before writers** —
a writer that sets the bit before the readers know it makes every recording
with a correlation marker fail to open, for a reason that has nothing to do
with the index.

The order that was actually followed: the constant landed in
`codetracer-trace-format-nim`'s `meta_dat.nim`, `codetracer_trace_writer::
meta_dat` (Rust), the db-backend's `ctfs_trace_reader::meta_dat` and
backend-manager's `meta_dat` — including in each one's `KNOWN_FLAGS_MASK` —
before any writer stamped it. The two rejection tests that had been aimed at
bit 14 moved to bit 15, since a rejection test aimed at a bit the reader now
knows is a test of nothing.

Bit 15 is a valid target for those tests even though this document allocates
it to `FLAG_HAS_LINE_COUNT_TABLE`, because what a reader rejects is a bit
outside **its own** `KNOWN_FLAGS_MASK`, not a bit this document has left
unassigned. No implementation reads or writes the line-count table yet, so
bit 15 is unknown to every one of them, and the tests assert exactly the
behaviour the next allocated bit will meet. Whichever reader implements the
line-count table first must move these tests again — and at that point the
flag word is full, so it will have to move them onto a `version` the reader
does not know instead.

#### `paths.dat` Layout A — per-line byte-length table (column-aware traces)

When `meta.dat` bit 4 (`FLAG_HAS_COLUMN_AWARE_STEPS`) is set, each `paths.dat`
record carries a per-line byte-length table after the path bytes so the
reader can resolve `global_position_index → (file, line, column)`:

```
paths.dat record (column-aware traces):
  path_len:    varint
  path_bytes:  [u8] × path_len
  line_count:  varint
  line_lengths: [varint] × line_count    (zigzag-delta encoded from previous line)
```

`line_lengths[0]` is an absolute zigzag varint; subsequent entries are
deltas from `line_lengths[i-1]`. Each line length SHOULD be the source
line's byte count plus 1 so that the trailing "one past EOL" position
(used for end-of-line breakpoints and statement-end markers) gets its
own address. See `trace-events.md` §"paths.dat per-line offset table"
for the rationale and the on-wire decoding algorithm.

Pre-column traces have no `line_count` field; the record ends at
`path_bytes`. Readers detect the extension via `meta.dat` bit 4 and parse
the extra fields only when the flag is set. `paths.off` continues to
point at record starts regardless of layout.

#### `paths.dat` line-count table (line-only traces)

When `meta.dat` bit 15 (`FLAG_HAS_LINE_COUNT_TABLE`) is set, each
`paths.dat` record carries the file's line count after the path bytes:

```
paths.dat record (line-count table):
  path_len:   varint
  path_bytes: [u8] × path_len
  line_count: varint
```

This is Layout A's framing without its trailing per-line table. The two
layouts state the same field, `line_count`, under the two addressing
modes: a column-aware file's positions are addressable columns, so its
size is `sum(line_lengths)` and `line_count` is the length of that table;
a line-only file's positions are lines, so its size *is* `line_count` and
there is no per-line table to follow. That single sizing rule —
`file_size` is the number of positions the file has — is stated in
`trace-events.md` §"Per-File Contiguous Integer Ranges", and this record
is what lets a reader apply it to a line-only trace instead of assuming a
size.

Requirements:

* **Bits 4 and 15 are mutually exclusive.** A record cannot be in both
  layouts, and a Layout A record already carries `line_count`. Writers
  MUST NOT set both; readers MUST reject a header that does.
* **`line_count` is mandatory under bit 15, for every record.** A writer
  that cannot determine a file's real line count MUST record the ceiling
  it lays the file out with (the conventional `100000`) rather than omit
  the field or record a sentinel. An omitted count would return that one
  file to being sized by assumption, which is the defect this table
  removes.
* **`line_count` MUST NOT be zero.** A file sized zero shares its base
  with the next file and the two become indistinguishable at decode.
  Readers MUST reject such a record rather than substitute a default.
* **Writers MUST refuse a step whose line exceeds the file's recorded
  `line_count`.** Such a step's address falls inside the *next* file's
  range, so it is a well-formed address of a location that was never
  recorded, and no reader can detect it — see `trace-events.md`
  §"Per-File Contiguous Integer Ranges".

Traces without bit 15 have no `line_count` field; the record is the bare
path bytes and every file is sized by the writer's convention, which the
container does not record. `paths.off` continues to point at record
starts regardless of layout.

**A reader MUST decide the layout from the `meta.dat` bits, never by
inspecting the record bytes.** The three record spaces overlap: a bare
record whose first byte happens to equal its own remaining length decodes
cleanly under either extended layout, yielding a truncated path and a
fabricated count with no error.

---

## Runtime Tracing (DB Traces)

A materialized trace `.ct` from runtime recorders (Python, Ruby, JavaScript, Bash, Noir/WASM):

| File | Abstraction | Purpose |
|------|-------------|---------|
| `meta.dat` | Binary metadata | Program, paths, recorder info (see Metadata section) |
| `steps.dat` | Chunked compressed | Execution stream: one compact record per debugger step |
| `steps.idx` | Companion index | Chunk index for `steps.dat` |
| `values.dat` | Chunked compressed | Value stream: one record per step with visible variable values |
| `values.idx` | Companion index | Chunk index for `values.dat` |
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
| Values | `values.dat` | Chunked compressed | Point lookup by step index |
| Calls | `calls.dat` | Var-size record | Random access by call_key |
| IO Events | `events.dat` | Chunked compressed | Paginated scan |

`steps.dat` records are tiny (2-4 bytes each), so chunks hold thousands of steps. The values stream is parallel-indexed with the execution stream -- record N in `values.dat` corresponds to step N in `steps.dat`.

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

#### Cross-OS portability

The `cp0.*` sidecars are *platform-specific captured state*: the
recorder writes the SysV x86-64 ABI register file (`cp0.regs`), the
contents of process-readable memory regions (`cp0.mem`), the
filesystem path map (`cp0.maps` — currently the verbatim Linux
`/proc/self/maps` text), and the `fs`/`gs` segment bases
(`cp0.fsbase`). The format is one-way: the recorder writes the host's
state at capture time; the replay backend interprets that state
against its own (host-agnostic) emulator.

Crucially, the *replay path* is host-agnostic. The
`EmulatorReplaySession` in `db-backend` interprets the captured
register values, installs `cp0.mem` regions into the emulator's
memory map via `mcrLoadMemoryRegion`, and parses `cp0.maps` *as text*
to compute the static-PC rebase delta. Nowhere on the replay path is
there a `std::fs::read("/proc/self/maps")`, `CreateProcess()`, or
`mach_vm_region()` call: every memory access goes through the
emulator's internal region table, and DWARF line resolution uses the
bundled `debug.dat` rather than touching the host filesystem. The
emulator itself is the same Nim code compiled to either native x86-64
or wasm32, depending on the build.

Consequence: a `.ct` recorded on Linux x86-64 replays identically on
macOS, Windows, or inside a wasm32 browser sandbox. The only host
dependency is the architectural support in the emulator (currently
x86-64); the host *operating system* is irrelevant. Cross-OS replay
is therefore a property of the file format and the replay path, not a
separate code path that needs feature-flagging.

The
`codetracer/src/db-backend/tests/xos_replay.rs` integration test
(M-XOS-Fixture) pins this contract by replaying a Linux-recorded
fixture (`tests/fixtures/xos/xos_hello.ct`) via
`EmulatorReplaySession::new_from_ctfs_bytes` and asserting that the
DAP-relevant surfaces (callstack, locals, breakpoints) come back
populated. A true macOS-host run requires CI infra and remains
deferred; the structural argument above explains why the Linux
fixture is sufficient evidence that the host-decoupled replay path
works.

### Thread Streams via Namespaces

Thread event streams are stored in `threads.ns`, a namespace keyed by `thread_id` (u64). This replaces the previous model of one CTFS file per thread (`t00000000001`, etc.), which was limited by MaxRootEntries. With namespaces, the thread count is unlimited -- the B-tree scales to millions of keys.

#### Per-file thread streams (MCR recorder) are seekable-zstd

The MCR recorder currently writes the per-file model — one `tNNN` file per thread
(`t` + 11 zero-padded digits). These streams are **chunked, per-chunk Zstd-compressed,
and seekable**, exactly like `steps.dat` / `calls.dat`:

- `tNNN` (`.dat`): `[zstd(chunk_0)][zstd(chunk_1)]...` — each chunk is the bare
  concatenation of raw event records (each record is self-describing: it begins with
  an `EventHeader` whose `size` gives the full record length, so the reader walks a
  decompressed chunk by `header.size` with no per-record length prefix).
- Companion index **`iNNN`** (`i` + 11 digits), NOT `tNNN.idx`: CTFS keys every file
  by the base40 encoding of its first 12 characters, and `tNNN` is already 12
  characters, so `tNNN.idx` would collide with the data file. `iNNN` is a distinct
  12-char key. Layout is the standard seekable-zstd companion index —
  `[chunk_size: u32 LE][offset_0: u64 LE][offset_1: u64 LE]...` where `chunk_size` is
  events per chunk and `offset_i` is the byte offset of chunk `i` in `tNNN`.

To read event `N` of a thread: `chunk = N div chunk_size`, decompress
`tNNN[offset[chunk] .. offset[chunk+1])`, walk `N mod chunk_size` records — O(chunk),
never the whole stream. The `threads.ns` namespace variant (above) carries the same
per-chunk-compressed, seekable payload keyed by `thread_id` instead of one file per
thread; both are the seekable-zstd model, differing only in how the per-thread
streams are addressed within the container.

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
  version: u16 LE (currently 4)
  flags: u16 LE
    The flag word holds two DIFFERENT classes of bit (see "Two classes of
    flag bit" below). Section-presence bits gate the parse of a
    variable-length block INSIDE meta.dat and MUST be honoured. Capability
    and stream-presence bits describe the trace and MUST NOT be treated as
    a reject-on-unknown gate.

    -- Section-presence (a block is embedded in meta.dat; MUST read to parse):
    bit 0       -- FLAG_HAS_MCR_FIELDS (MCR extended block present)
    bit 1       -- FLAG_HAS_REPLAY_LAUNCH_FIELDS (M-RLP-1, see below)
    bit 2       -- FLAG_HAS_LAYOUT_SNAPSHOT (M-RLP-2, see below)
    bit 3       -- FLAG_HAS_TRACE_FILTER_PROVENANCE (filter chain block present, TF-M7)
    -- Capability (format-variant declared at open; not a stream gate):
    bit 4       -- FLAG_HAS_COLUMN_AWARE_STEPS (column-aware step encoding, see trace-events.md §"Reader Behaviour and Back-Compat")
    bit 5       -- FLAG_HAS_ALTERNATE_SOURCE_VIEWS (srcviews.dat present, see §"Alternate Source Views" below)
    bit 6       -- FLAG_SUPPORTS_COLUMN_BREAKPOINTS (capability bit; see §"Column-Aware Capability Flags" below)
    bit 7       -- FLAG_SUPPORTS_COLUMN_MOTIONS (capability bit; see §"Column-Aware Capability Flags" below)
    -- Stream-presence (ADDITIVE HINT; tautological with a named stream file;
    -- see "Stream-presence flags are a hint, not a gate" below):
    bit 8       -- FLAG_HAS_CALL_STREAM       (calls.dat present)         M17a
    bit 9       -- FLAG_HAS_STEP_STREAM       (steps.dat present)         M23a
    bit 10      -- FLAG_HAS_VALUE_STREAM      (values.dat present)        M23b
    bit 11      -- FLAG_HAS_IO_EVENT_STREAM   (events.dat present)        M23c
    bit 12      -- FLAG_HAS_INTERNING_TABLES  (paths/funcs/types/varnames.dat present) M23d
    bit 13      -- FLAG_HAS_SPAN_STREAM       (spans.dat / spans.idx present) RS-M1
    bit 14      -- FLAG_HAS_CORRELATION_INDEX (corrmark.ns + markers.dat/.off) WTCI
    -- Capability (record-layout variant declared at open):
    bit 15      -- FLAG_HAS_LINE_COUNT_TABLE (every paths.dat record carries the
                   file's line_count; see §"`paths.dat` line-count table" below)

    No bit is reserved: version 4 assigns all sixteen. A further flag needs a
    meta.dat version bump, not a spare bit.

### Two classes of flag bit

The `flags` word conflates two things a reader must NOT treat alike:

1. **Section-presence bits (0..3)** gate the parse of a *variable-length
   block inside meta.dat itself*. There is no separate file to inspect, so a
   reader MUST read these to parse meta.dat correctly. They are decided when
   meta.dat is written (at open) and never change — so they carry no
   streaming hazard.

2. **Capability bits (4..7)** declare a *format variant* of the trace (e.g.
   column-aware steps) chosen at open. They too are set once and describe how
   to interpret data that is present; they are not a reject-on-unknown gate.

3. **Stream-presence bits (8..14)** claim that a *separately named stream
   file* exists in the container (`steps.dat`, `spans.dat`, `corrmark.ns`, …).
   This claim is **tautological with the container's own structure**: the
   stream exists iff the file entry exists. See the next subsection.

### Stream-presence flags are a hint, not a gate

A stream-presence bit (8..14) is an **optional hint**, redundant with a
`findFile("<stream>.dat")` on the container's file-entry array. The
authoritative answer to "does this trace carry stream X?" is the
**structural presence of the named file**, and the authoritative answer to
"how much of it is readable right now?" is that file's `FileEntry.Size`
(ctfs-container.md §6, "Live progress: per-stream following"). Therefore:

- A reader **MUST NOT gate** reading a stream on its presence bit, and **MUST
  NOT reject** a container merely because a stream file is present while its
  bit is clear. It resolves each optional stream by `findFile` + `Size`.
- **Streaming correctness (normative).** A stream file and its `FileEntry.Size`
  become visible the instant the writer creates the stream and commits data —
  *mid-run*, not at close. A presence bit, by contrast, may be stamped only
  when the writer learns the stream is non-empty, which can be deferred to
  close. A reader that gates on the bit would therefore be unable to read a
  stream that structurally exists in a *still-recording* trace — a violation
  of the requirement that every consumer can load a trace while the target is
  still running. Gating on structure (file presence + `Size`) is the only
  streaming-correct rule; the bit MUST NOT be a precondition.
- These bits are **additive**: a reader that does not understand a stream
  ignores its file (and its bit) and reads the rest correctly. Bit 14
  (`corrmark.ns`) is additive on the same terms. No bit is reserved for
  reject-on-unknown in version 4.
- Writers MAY still set bits 8..14 as a fast-path hint. When they do, the bit
  MUST be set as soon as the stream is created (so it is visible mid-run),
  never deferred to close; a writer that cannot guarantee mid-run stamping
  SHOULD leave the bit clear and rely on structural presence rather than emit
  a bit that lies to a live reader.

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

- **v3.2** (WTCI, 2026-09-09) -- flag bit 14 additionally covers the
  correlation-marker label interning table (`markers.dat` + `markers.off`),
  written lazily beside `corrmark.ns`. No layout change and no new bit: the
  table is meaningless without the index the same bit already announces, and
  joining bit 12's `FLAG_HAS_INTERNING_TABLES` set would have redefined a flag
  whose meaning is agreed across the Nim writer, `codetracer_trace_writer::
  meta_dat` (Rust) and the db-backend. See § "Interning Tables".
- **v3.1** (WTCI, 2026-09-09) -- allocated flag bit 14
  `FLAG_HAS_CORRELATION_INDEX`, extending the stream-presence run to
  8..14, and moved `FLAG_HAS_LINE_COUNT_TABLE` to bit 15, which the
  line-only sizing work had provisionally taken. The correlation index is
  a named stream file, so it belongs in the stream-presence run; the
  line-count table is a record-layout capability and does not. Neither
  bit had shipped. The flag word is now fully assigned. No layout change: the bit is a stream-presence hint for
  `corrmark.ns` and, like bits 8..13, is redundant with the container's
  file-entry array. A reader that ignores it loses nothing; a reader that
  trusts it over the file entry is wrong for the same reason it would be
  for `spans.dat`. Contract:
  `codetracer-specs/Testing/CTFS-Correlation-Marker-Contract.md`.

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

### Column-Aware Capability Flags

`FLAG_HAS_COLUMN_AWARE_STEPS` (bit 4) signals only that *column data is
present on the wire*. It says nothing about whether the columns are
sharp enough for the GUI to place a breakpoint at, or motion-step
through, a specific column. Two additional capability bits encode the
recorder's contract with the GUI:

| Bit | Name | Set when | GUI affordance gated on it |
|-----|------|----------|-----------------------------|
| 6 | `FLAG_SUPPORTS_COLUMN_BREAKPOINTS` | The recorder emits column positions sharp enough for breakpoint placement at a specific `(line, column)` pair | Per-column breakpoints; column gutter marks in the source view (M6 Alt+click affordance) |
| 7 | `FLAG_SUPPORTS_COLUMN_MOTIONS` | The recorder supports per-column step-over / step-in / step-out (the step predicate fires per statement-start, not per line) | Column-aware step buttons; "step to next sub-expression" affordances |

**Contract.** When either capability flag is clear, the GUI MUST disable
the corresponding affordance — no per-column breakpoint UI, no
per-column motion buttons — and fall back to line-only behaviour. A
trace MAY set `FLAG_HAS_COLUMN_AWARE_STEPS` (so column data is
available for *display*) while leaving both capability bits clear: the
columns are good enough to highlight which sub-expression is current,
but not sharp enough to place a breakpoint at or to step to.

Setting either capability bit while `FLAG_HAS_COLUMN_AWARE_STEPS` is
clear is undefined behaviour: capability flags presuppose column data
on the wire. Writers SHOULD enforce this by gating the capability bits
behind `columnAwareSteps` in their writer state.

Recorders SHOULD set both capability bits when their step predicate is
genuinely per-statement (e.g. JavaScript, Cairo, Solidity); recorders
whose step predicate is per-line (e.g. Ruby's TracePoint, Aiken's
line-oriented parser) MUST leave both bits clear even if they happen to
emit a column for display purposes.

Readers expose these bits through the existing `metadata.flags` JSON
surface as `supports_column_breakpoints` / `supports_column_motions`,
parallel to the established `has_column_aware_steps` field.

---

## Global Line Index

Source files are concatenated into a virtual address space where each line has a unique global index.

```
global_index(file_id, line) = prefix_sums[file_id] + (line - 1)

prefix_sums[0] = 0
prefix_sums[k] = prefix_sums[k-1] + line_count[k-1]
```

The prefix-sum array is computed once at startup from the interning table.

`line` is 1-based, so the `- 1` puts a file's first line at its own
`prefix_sums[file_id]` and its last line at `prefix_sums[file_id] +
line_count - 1`. This is what makes `file_size = line_count` in
[trace-events.md](trace-events.md) §"Source Location Addressing" correct: a
file's range is exactly the addresses its lines occupy, with none left over
and none spilling into the next file.

It is also the same in-file offset the column-aware mode uses. There `q =
p - file_base[f]` is a 0-based index over every (line, column) pair, so
`q = 0` is line 1, column 1. Line-only mode is that scheme with one address
per line instead of one per column position, and `line = q + 1` inverts it.

> **Encoding `prefix_sums[file_id] + line` is wrong and was specified here
> until 2026-09.** It leaves `prefix_sums[file_id]` unused and pushes a
> file's last line one address past the end of its own range. With the
> `file_size = line_count` sizing that error is invisible while every file is
> allocated a fixed oversized stride, and becomes a wrong answer at every
> file boundary the moment real line counts are used: with counts `[10, 10]`,
> `(file 0, line 10)` encodes to `10`, which decodes to `(file 1, line 0)`.

`line_count[k]` is the count `paths.dat` records for file `k` when
`meta.dat` bit 15 is set (§"`paths.dat` line-count table"). A trace
without that bit states no counts, and the recurrence above is evaluated
against the writer's convention of `100000` per file — a number the
reader must assume, and which is wrong for any file that has more lines
than that. Sizing files at their real counts also shortens every address:
the space is the program's total line count rather than
`file_count × 100000`, which is what the varint budget in §"Varint IDs"
assumes.

### Uses

1. **Compact Step events**: A step stores one global line index instead of separate (path_id, line).
2. **Namespace key for `linehits.tc`**: Maps global line index to hit time coordinates.

### Correlation Index (`corrmark.ns`)

A namespace mapping a correlation key to the markers a recording carries for
it, so a consumer can answer "does this recording cover this span?" with a
B-tree lookup instead of a scan of the event stream. Built **during recording**
by every writer that observes a marker.

**Key.** `XXH64(seed = 0, key_bytes)`. For distributed-trace correlation
(`kind = 0`) `key_bytes` is the 24-byte buffer `trace_id_be || span_id_be` —
the 16 big-endian bytes of the trace id followed by the 8 big-endian bytes of
the span id, i.e. wire order, **not** a hex rendering.

**Leaf type B**, `[payload_offset: u64][payload_len: u64]` descriptors into an
appended payload region, as `memwrites.tc` is actually built (see
`memwrites_builder.nim`; note the Leaf-Type-A attribution in
ctfs-container.md § 8 predates that implementation). Type B is required here
regardless: a collision bucket is variable-length.

**Key, per kind.** `kind = 0` (distributed-trace span) hashes
`trace_id_be || span_id_be`. `kind = 1` (boundary crossing) hashes
`marker_id` (8 bytes big-endian, the interned label id — see § "Interning
Tables") followed by the raw `key_value` bytes. **No separator is needed**:
`marker_id` is fixed width, so the split point is always byte 8 and two
distinct `(marker_id, key_value)` pairs cannot produce the same buffer.

**Value — a bucket, because a 64-bit key over a large corpus collides:**

```
bucket:
  entry_count : u32 LE
  entries[entry_count]:
    identity          : 24 bytes  interpreted by `kind`:
                          kind 0: trace_id[16] || span_id[8], wire order
                          kind 1: marker_id u64 BE || key_fingerprint u64 BE
                                  || reserved[8] (zero)
    wall_time_unix_ns : u64 LE
    monotonic_time_ns : u64 LE
    geid              : u64 LE    coordinate into the event stream
    thread_id         : u64 LE
    kind              : u16 LE    0 = distributed-trace span
                                  1 = MarkerPayload boundary crossing
    flags             : u16 LE    bit 0: direction (enter = 0, exit = 1)
                                  bits 1..15 reserved
```

60 bytes per entry. Entries are sorted by `(trace_id, span_id, geid)`.

**A B-tree hit is a hash hit, not a match, and a reader MUST confirm it.**

- **kind 0** carries its full 24-byte `(trace_id, span_id)`, so confirmation is
  exact.
- **kind 1** confirms in two parts: `marker_id` is compared **exactly** — it is
  a fixed-width interned id — while `key_value` rests on a 64-bit fingerprint
  under a different seed from the index key. `key_value` is the per-event match
  value and is therefore different on essentially every marker, so interning it
  would grow one record per marker and save nothing; it stays variable-length,
  and a fixed-width entry can only carry a digest of it. A false positive
  consequently needs an exact hit on the interned boundary *and* a 64-bit
  collision on the key.

A bucket whose entries all fail confirmation is a *miss*, indistinguishable in
its answer from an empty result.

**Absence is a distinct answer from a miss.** The presence of the `corrmark.ns`
file entry — which a reader already parses out of block 0, so this costs no
extra read — says whether the recording was indexed at all:

| `corrmark.ns` | key found | meaning |
|---|---|---|
| present | yes, full key confirms | the recording covers this span |
| present | no (or bucket mismatch) | the recording definitively does not |
| absent | — | **not indexed** — says nothing about the span |

A consumer MUST NOT collapse the third row into the second. Reporting an
unindexed recording as "no match" is what made the original failure mode hard
to diagnose.

**Retention.** B-tree blocks stay in the main `.ct` and are never sharded
(ctfs-container.md § "Separation of structure and data"), so an index entry can
outlive the data blocks it points at. A lookup that resolves an entry MUST
consult the recording's retention state before reporting a hit, and report
*expired* rather than a hit when the payload is gone. The index accelerates the
question; it is never the authority on whether the data still exists.

**Inspecting an index.** `ct print --correlation-index <file.ct>` reports the
entries and, separately, whether the namespace is present at all. That view
exists because a `kind = 0` entry is otherwise unobservable: unlike a boundary
crossing it writes no `MarkerPayload` and no I/O event, so a recorder that
declared coverage and one that silently dropped the call produce byte-identical
event streams.

### Implementation

| Piece | Where |
|---|---|
| Encoder, reader, bulk load | `codetracer-trace-format-nim/src/codetracer_trace_writer/corrmark_builder.nim` |
| Writer API (`registerSpanCoverage`, `registerCorrelationMarker*`, `ensureMarkerId`) | `.../codetracer_trace_writer/multi_stream_writer.nim` |
| C ABI (`trace_writer_mark_span_coverage[_hex]`, `trace_writer_mark_correlation[_by_id]`, `trace_writer_ensure_marker_id`) | `.../codetracer_trace_writer_ffi.nim`, declared in `include/codetracer_trace_writer.h` |
| Rust binding | `codetracer-trace-format/codetracer_trace_writer_nim` (`TraceWriter` trait + `NimTraceWriter`) |
| MCR writer | `codetracer-native-recorder/ct_recorder/src/ct_recorder/trace_writer.nim` |
| Consumer | `codetracer-ci/apps/Monolith/Monolith.TraceStorage/CtfsCorrelationIndex.cs` |

The C ABI takes the ids as **wire bytes**, with a `_hex` wrapper that converts.
The conversion lives in the shared library rather than in each recorder because
the index keys on the wire bytes: a recorder that hashed the hex rendering
instead would produce an index that is present, correct-looking, permanently
unqueryable, and silent.

Full contract, including why this shape was chosen over a scan:
`codetracer-specs/Testing/CTFS-Correlation-Marker-Contract.md`.

### Namespace Key Summary

| Namespace | Key | Meaning |
|-----------|-----|---------|
| `linehits.tc` | global line index | Source line hit time coordinates |
| `memwrites.tc` | memory address | Memory write time coordinates |
| `memreads.tc` | memory address | Memory read time coordinates |
| `slc-mwr.ns` | slice_id | Per-thread-slice write address sets |
| `slc-mrd.ns` | slice_id | Per-thread-slice read address sets |
| `threads.ns` | thread_id | Per-thread event streams |
| `corrmark.ns` | XXH64 of the correlation key | Correlation markers — which distributed-trace spans (and cross-process boundaries) this recording touches |

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

---

## Alternate Source Views (Deminification Support)

When a recorder encounters a **minified source** (heuristic: average line
length exceeds a configurable threshold, default 500 characters) AND no
companion sourcemap V3 exists upstream, it MAY pre-format the source at
record time using a language-appropriate formatter (`prettier` for
JS/TS, `black` for Python, etc.) and bake the formatted view +
position map into the trace.

The replay-server's existing sourcemap V3 translation path then
discovers the formatted view through these CTFS internal files —
**no replay-time subprocess invocation**.

### `srcviews.dat` / `srcviews.off`

Variable-size record table interning **alternate views** of source
paths registered in `paths.dat`. Each record carries one formatted
view of one source.

| Field | Encoding | Notes |
|-------|----------|-------|
| `path_id` | varint | Index into `paths.dat` — the original source this view applies to |
| `view_kind` | u8 | 0 = `raw` (no transformation, rarely emitted), 1 = `prettier_format`, 2 = `black_format`, 3-127 reserved, 128+ = vendor-specific |
| `view_name_len` | varint | Length of `view_name` |
| `view_name` | bytes | Human-readable name shown in the UI (e.g., `"lodash.fmt.js"`); typically the original name with a `.fmt.<ext>` suffix |
| `content_len` | varint | Length of `content` |
| `content` | bytes | The formatted source as UTF-8 bytes |
| `map_len` | varint | Length of `map` (0 = no sourcemap) |
| `map` | bytes | A sourcemap V3 (JSON, UTF-8) mapping positions in `content` BACK to positions in the original source at `path_id`. The inverse map direction matters: replay-server's existing P3 translation expects `(generated, line, col) → (original_source, line, col, name?)` segments, where "generated" is the formatted view and "original" is the recorded minified source. |

Records are referenced by 0-based index. The reader loads
`srcviews.dat` lazily — most traces won't carry any alternate
views.

### Discovery rules

A replay-server consuming a CTFS trace SHOULD:

1. Load `srcviews.dat` / `srcviews.off` if present.
2. For each recorded step whose `(path_id, line, column)` lookup
   targets `paths.dat[path_id]`, check whether any
   `srcviews.dat` entry has matching `path_id`. If so, prefer
   the alternate view for UI display:
   - Surface `view_name` as the file path in DAP `stackTrace`
     responses.
   - Surface `content` via the DAP `source` request (or the UI's
     filesystem-based reader through a materialized sidecar — both
     resolutions are acceptable).
   - Translate the recorded `(line, column)` through `map` to
     positions in `content` before reporting.
3. When multiple alternate views exist for one path (e.g., a
   `prettier_format` AND a `black_format` — unusual but legal),
   pick the one whose `view_kind` matches the source's language.
   Fall back to the lowest-numbered `view_kind` if no clean match.

### Recorder responsibility

Recorders that emit alternate views MUST:

1. Set the `meta.dat` flag bit 5 = `FLAG_HAS_ALTERNATE_SOURCE_VIEWS`
   (bits 0-4 were allocated by prior milestones — see
   trace-events.md §"Reader Behaviour and Back-Compat" for the
   strict-rejection contract on unknown bits).
2. Run the formatter as a one-shot at record start, NOT per-step.
3. Skip the format pass when:
   - The source has a sibling `<source>.map` upstream (the
     upstream sourcemap is authoritative; recorder-side
     re-formatting would override the user's choice of map).
   - The source's average line length is below the threshold
     (heuristic for "this is minified code").
   - The formatter's output line count does not exceed the
     input's (no point materializing an identical view).
   - The configured kill switch is active (the
     `CT_AUTOFORMAT={0|off|false|no}` environment variable is the
     spec-canonical mechanism).
4. Treat formatter failures as soft: log a one-shot warning, omit
   the alternate view, continue recording. The trace remains
   usable without the view.

### Back-compat

Pre-extension traces (no `srcviews.dat`) are byte-for-byte
compatible with column-aware readers (P6.5 contract): the
`paths.dat` per-line offset table (Layout A) is independent of the
alternate-views machinery. Readers that detect the bit-5 flag but
don't understand alternate views MUST reject the trace cleanly per
the existing "unknown flag bits cause rejection"
contract.

### Implementation status

- **codetracer-js-recorder** (commit `d493ab9`) ships an
  out-of-CTFS variant: formatted views land under
  `<trace_dir>/files/<name>.fmt.js` + `<name>.fmt.js.map` rather
  than `srcviews.dat`. This is a transitional convention
  predating this spec section; future js-recorder releases will
  migrate to `srcviews.dat`.
- **codetracer-python-recorder** (commit `06129daf`) ships the
  module + CLI flag + tests but defers the recording-flow
  integration pending the writer-side wire-format change this
  section describes.
- **codetracer-trace-format-nim** writer support for
  `srcviews.dat` is the prerequisite for closing the Python
  recorder integration.

The campaign that drove this section is documented in
`codetracer-specs/Planned-Features/Column-Aware-Tracing-And-Deminification.milestones.org`
§P6.2.

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
- **Offset file** (e.g., `paths.off`): fixed-size table of u64 values. For a table of `N` records it
  holds **`N + 1`** entries: entry `i` is the byte offset of record `i` in the data file, and entry
  `N` is the data file's length. An empty table's offset file is the single entry `0`.

To read record `i`:

1. Read `offset[i]` and `offset[i+1]` from the offset file (16 bytes at position `i * 8`)
2. Read `offset[i+1] - offset[i]` bytes from the data file at `offset[i]`

The record count is `offset_file_size / 8 - 1`. A reader MUST refuse an offset file that is not a
non-empty multiple of 8 bytes, whose entries decrease, whose first entry is not `0`, or whose last
entry is not the data file's length. (Both writers have always written the trailing entry; this
section described `N` entries with a fallback to the data file's size until 2026-10, and one
downstream writer followed that reading and produced tables the libraries' readers refused.)

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

**A function's `global_line_index` is a LINE address, in every trace, including a column-aware one.**
The column extension re-interprets the address carried by *step* records -- `trace-events.md`
§"Source Location Addressing" scopes it to step events throughout, and §"Pre-extension traces" talks
only about "their step records". It says nothing about `funcs.dat`, and two conforming writers duly
read that silence differently: one addressed a declaration site in the line space and the other in
the column-aware byte-offset space that its step records use, so the same function in the same trace
got `100000` from one writer and `8` from the other. A declaration site is a line, not a cursor
position -- there is no column at which a function is declared -- so it is the line space, and a
writer must not substitute the position space merely because the trace is column-aware.
| Correlation-marker labels | `markers.dat` | `markers.off` | raw bytes (boundary label) |

Records are referenced by 0-based index. Interning tables are loaded at reader startup (typically 1-5 MB total).

**An empty name is a name.** No interned name has a minimum length: a zero-length
`varnames.dat` record, a `types.dat` record with `lang_type_len = 0`, and a `funcs.dat`
record with `name_len = 0` are well-formed, and recorders write them (a type a language
leaves unnamed). A writer records an empty name like any other, and interns it once. A
reader returns it as the empty string. It must not report an empty name as a failed
lookup: one implementation's C ABI returned the same null for "empty" as for "failed",
its caller read every empty name as an error with no message, and a recording that held
one did not open. An API that signals failure through a null or absent result must
therefore return an empty name as a present, zero-length result.

**When an entry is interned.** A writer interns an entry at the first of: the recorder registering
it (a path, a variable name, a function, a type), or a record that refers to it. Ids are assigned
from 0 in that order, and an entry is interned once: registering an entry that is already in the
table returns the id it has and writes nothing. A registered entry is interned even if no record
ever refers to it. The two writers used to differ here -- one deferred a registered path or variable
name until a step or a value first used it -- and the same registrations then produced different
ids, and so a different `paths.dat`, `funcs.dat` (whose `global_line_index` is derived from the path
id), `varnames.dat`, and a different address in every step record. A writer whose API takes ids
from the recorder instead of names (a low-level event stream, where a `Path` event *is* the
registration and its id is its position among the `Path` events) relies on the recorder never
registering an entry twice; the tables are deduplicated, so a repeated registration there is a
recorder error, not a second entry.

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
- **`meta.dat` flag: bit 15**, `FLAG_HAS_CORRELATION_INDEX` — the same bit
  that covers `corrmark.ns`, **not** bit 12's `FLAG_HAS_INTERNING_TABLES` set.

##### Why bit 15 rather than joining bit 12

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
3. **Bit 15 is the last bit, and one bit is all this needs.** Giving the index
   and its label table a bit each would spend the format's last two on a pair
   that is always written and read together.

Both files keep the semantics bits 8–13 already have: **additive hints** — the
file-entry array, not the flag, is the authority on what a container holds, and
a reader that has no use for the index loses nothing by ignoring it.

##### Why the stream-presence bits are no longer contiguous

Bit 14 went to `FLAG_HAS_LINE_COUNT_TABLE` while this index was being drafted
against the same bit. Both describe the container, so they could not share one:
a container setting either would have announced the other to every reader. The
line-count table shipped first and kept 14; the index took 15. Nothing on disk
carried either bit at the time, so the choice cost no compatibility.

That leaves stream-presence as bits 8..13 and 15, with a record-layout
capability at 14 between them. The grouping is worth stating precisely because
the bit number is the only thing that identifies a flag, and a reader that
infers a bit's class from its neighbours will be wrong about 14.

**"Additive" is about the FILES, not about the bit.** An earlier revision of
this section said a reader that does not know bit 15 "ignores `corrmark.ns` and
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
before any writer stamped it.

The two rejection tests that had been aimed at bit 15 moved to bit 14, since a
rejection test aimed at a bit the reader now knows is a test of nothing. Bit 14
is a valid target even though this document allocates it, because **what a
reader rejects is a bit outside its own `KNOWN_FLAGS_MASK`, not a bit this
document has left unassigned.** The db-backend does not implement the
line-count table, so bit 14 is unknown to it, and the test asserts exactly the
behaviour the next flag it does not know will meet.

With the flag word fully assigned, a reader that later implements every flag
has no bit left to probe, and the test has to move onto a `version` it does not
recognise instead. `rejects_unsupported_version` already covers that shape.

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

Requirements:

* **Bit 4 describes every record, so it is chosen before the first one.**
  A writer that has already written a bare record cannot then declare
  bit 4: nothing re-frames the earlier record, and the container would
  state a layout its own first record is not in. A writer MUST refuse
  that — at the opt-in, or at close if the opt-in has no way to fail —
  rather than finalize the container.
* **Every Layout A record carries a non-empty table, and a file's table is
  fixed by its first registration.** A writer MUST refuse, naming the path, a
  column-aware registration whose table is empty or sums to `0` (either way
  the file would have `file_size` 0, `trace-events.md` §"Per-File Contiguous
  Integer Ranges"), and
  a registration that names an already-interned path with a table different
  from the one recorded -- including a table offered for a path first interned
  without one. Returning the existing id silently, as both writers did, hid a
  recorder that interned a file through a step or a call before registering
  its table: the file was recorded with no table, and every later position in
  it resolved into the next file. (The Python recorder did exactly this from
  2026-09-30 until `e573455`.)
* **A file whose lines hold nothing** -- an empty `__init__.py`, whose one
  line has no bytes, or a file of blank lines only -- gives its first line one
  position: `[0]` is registered as `[1]` and `[0, 0]` as `[1, 0]`, keeping the
  line count. Its column 1 on line 1 is then addressable and its size is not
  `0`.
  (Lines of `0` positions elsewhere in a table are allowed -- a blank line
  in a recorder that does not add the one-past-EOL position -- since only the
  file's total size must be non-zero.)
* **A file whose source the recorder cannot read** -- a frozen module, code
  compiled from a string -- is registered with the conventional table:
  `100000` lines of `1024` positions each, the column-aware counterpart of the
  line-count table's `100000` ceiling (§"`paths.dat` line-count table"). A step
  on such a file whose column exceeds `1024` is recorded at column `1024` of
  its line, as a line `0` is recorded as line `1` (§"Global Line Index"); a step
  whose line exceeds `100000` is refused, as under bit 14. The cost is address
  space: such a file occupies about 10^8 positions, so files registered after it
  get longer absolute positions. A recorder SHOULD therefore register a file it
  can read whenever it can, and use the fallback only where no source exists.
* **Readers MUST decode a Layout A record whole.** The record's length
  is known from `paths.off`, so `path_len`, the path, `line_count` and
  exactly `line_count` line lengths MUST consume it with nothing left
  over. Reading only the `path_len` prefix accepts records in the other
  layouts too: a bare absolute path begins with `/` (47), so a bare
  record longer than 48 bytes "decodes" into the wrong path with no
  error, and a shorter one fails — which of the two a caller sees
  depends on how long the path happens to be.

#### `paths.dat` path versions (line-count-table traces)

A trace that declares bit 14 MAY register the same path more than once, as
successive **versions** of the file — what a recorder does when the program's
source changes under it and it keeps running. Each version is its own
`paths.dat` record, with its own path id, its own `line_count` and its own
slot in the position space; the path bytes of all versions are identical, and
only the id tells them apart. A step addresses the version whose id it was
written against.

* **Versions require bit 14.** Without the line-count table a second record
  would carry no size, and both versions would be laid out at the conventional
  stride where no step could be bounded against its own version's lines.
  Writers MUST refuse a version on a trace without bit 14, and on a
  column-aware trace (bit 4), whose files are sized in columns.
* **Readers MUST NOT deduplicate `paths.dat` by path bytes.** Two records with
  equal bytes are two ids; merging them would move every step of the later
  version into the earlier one's range.
* **A writer resolves a bare path to its newest version.** After a version is
  registered, a step (or registration) that names the path by string alone is
  attributed to the newest version's id, so a recorder's hot path need not
  track versions. Which version was live when is recorded in the execution
  stream by `SourceReload` markers, not inferred from ids.

#### `paths.dat` line-count table (line-only traces)

When `meta.dat` bit 14 (`FLAG_HAS_LINE_COUNT_TABLE`) is set, each
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

* **Bits 4 and 14 are mutually exclusive.** A record cannot be in both
  layouts, and a Layout A record already carries `line_count`. Writers
  MUST NOT set both; readers MUST reject a header that does.
* **`line_count` is mandatory under bit 14, for every record.** A writer
  that cannot determine a file's real line count MUST record the ceiling
  it lays the file out with (the conventional `100000`) rather than omit
  the field or record a sentinel. An omitted count would return that one
  file to being sized by assumption, which is the defect this table
  removes.
* **`line_count` MUST NOT be zero.** A file sized zero shares its base
  with the next file and the two become indistinguishable at decode.
  Readers MUST reject such a record rather than substitute a default.
* **Writers MUST refuse a step whose line exceeds the file's recorded
  `line_count`**, and the refusal fails the recording's close
  (`trace-events.md` §"Recorder Integration — A Failed Call Fails the
  Recording"). Such a step's address falls inside the *next* file's
  range, so it is a well-formed address of a location that was never
  recorded, and no reader can detect it — see `trace-events.md`
  §"Per-File Contiguous Integer Ranges".

Traces without bit 14 have no `line_count` field; the record is the bare
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
| `meta.dat` | Binary metadata | Program, arguments, recorder identity (see Metadata section); source paths are in `paths.dat` |
| `steps.dat` | Chunked compressed | Execution stream: one compact record per debugger step |
| `steps.idx` | Companion index | Chunk index for `steps.dat` |
| `values.dat` | Chunked compressed | Value stream: one record per step with visible variable values |
| `values.idx` | Companion index | Chunk index for `values.dat` |
| `calls.dat` | Chunked compressed | Call stream (complete call records with args/return) |
| `calls.idx` | Companion index | Chunk index for `calls.dat` |
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
| `step-map.ns` | Step-map blob | `(path_id, line)` to step ids; line-only traces (see below) |
| `linehits.tc` | Namespace (Type A) | Source line to step ID mapping |
| `memwrites.tc` | Namespace (Type A) | Variable/place to change history |

### Stream Descriptions

| Stream | CTFS File | Abstraction | Access Pattern |
|--------|-----------|-------------|----------------|
| Execution | `steps.dat` | Chunked compressed | Sequential scan, point lookup |
| Values | `values.dat` | Chunked compressed | Point lookup by step index |
| Calls | `calls.dat` | Chunked compressed | Random access by call_key |
| IO Events | `events.dat` | Chunked compressed | Paginated scan |

`steps.dat` records are tiny (2-4 bytes each), so chunks hold thousands of steps. The values stream is parallel-indexed with the execution stream -- record N in `values.dat` corresponds to step N in `steps.dat`.

`calls.dat` is indexed by `call_key`. To find a step's enclosing call, use proportional (interpolation) search on `calls.dat` -- each call record stores `[first_step_id, last_step_id]` ranges.

Event type wire formats are specified in [trace-events.md](trace-events.md).

### Chunking and compression of the runtime streams

These are normative: two writers that differ here write containers of different size and
seek granularity for the same recording.

| Stream | Records per chunk (`chunk_size`) |
|--------|----------------------------------|
| `steps.dat` | 4096 |
| `values.dat` | 256 |
| `calls.dat` | 256 |
| `events.dat` | 64 |

Every chunk is exactly one Zstandard frame, compressed at level 3 in one shot, so that the frame
declares its content size (`Frame_Content_Size` present); it carries no checksum and uses no
dictionary. Readers size the decompression buffer from the declared content size. Every chunk but
the last holds exactly `chunk_size` records. The smaller value and call chunks keep a point lookup
-- the dominant read -- to decompressing at most 256 records.

The interning tables and `meta.dat` are stored uncompressed. `step-map.ns` compresses its own
lists (§"`step-map.ns`").

### `step-map.ns`

The `(path_id, line)` to step-id index a reader answers a line breakpoint, a "run to line" or a
line-hit query from without scanning `steps.dat`. **A line-only trace (no `meta.dat` bit 4) MUST
carry it; a column-aware trace MUST NOT** -- its step addresses are byte-offset positions, and a
map keyed by the registered line would disagree with them. A reader that finds no `step-map.ns`
falls back to scanning the execution stream.

It is a single member (despite the `.ns` suffix it is not a CTFS namespace). Version 2, current
since the 2026-10 revision, run-length-codes the gaps between step ids, delta-codes the keys, and
stores the result as zstd chunks; integers outside the frames are little-endian:

```
Header (26 bytes):
  magic: u32 = 0x53544D50 ("STMP")
  version: u16 = 2
  chunk_count: u32
  path_count: u32            -- distinct path_ids with at least one step
  line_count: u32            -- distinct (path_id, line) keys
  step_count: u64            -- step ids in all lists together
Chunk table, chunk_count x 20 bytes, in key order:
  frame_offset: u64          -- offset of the chunk's frame, counted from the end of the table
  first_path_id: u64         -- key of the chunk's first line record
  first_line: u32
Frames: one Zstandard frame per chunk, back to back. Frame i spans
  [frame_offset[i], frame_offset[i+1]); the last one ends at the end of the member.
Chunk content (a frame, decompressed): line records in ascending (path_id, line) order:
  path_delta: varint         -- path_id minus the previous record's; 0 for a chunk's first record,
                                whose path_id is the table's first_path_id
  line: varint               -- the line itself when path_delta > 0 or the record is the chunk's
                                first; otherwise the line minus the previous record's line (>= 1)
  count: varint              -- step ids on this line (>= 1)
  runs, until their repeats add up to count:
    gap: varint              -- the difference between consecutive step ids (>= 1)
    repeat: varint           -- how many consecutive step ids have that gap (>= 1)
                                The id before a list's first is -1, so the first run's gap is the
                                first step id plus 1. Runs are maximal: adjacent runs of a list
                                never have the same gap.
```

**Chunking (normative, so that two writers produce the same bytes).** Line records are appended to
the current chunk in key order; when an append brings the chunk's decompressed size to 65,536 bytes
or more, the chunk is closed after that record, and the next record opens a new one. A record is
never split, so one hot line's list may make a chunk larger than the target. Every frame follows the
rule of the runtime streams above: level 3, compressed in one shot so that it declares its content
size, no checksum, no dictionary. A trace with no steps carries the 26-byte header with every count
`0` and no chunk, so the file's presence still says the index was built.

**Reading.** A reader that wants the whole map -- the db-backend builds `(path, line) -> ids` at
open -- inflates the chunks in order and decodes the records. A reader that wants one line
binary-searches the chunk table for the last chunk whose first key is not above the target,
inflates that chunk alone and scans it. A reader MUST refuse, by name, a step map whose decoded
counts disagree with the header, a chunk whose first record's key is not its table key, keys that
do not ascend strictly, a `count`, `gap` or `repeat` of `0`, runs whose repeats overshoot `count`,
or a frame that does not decode to its declared size: each is a map that would answer some
breakpoint with the wrong steps.

**Why this layout (measured; `measurements/2026-10-format-efficiency.md` §"`step-map.ns`").**
Version 1 stored every step id as an uncompressed `i64` behind a 32-byte entry per line: 8.1 bytes
per step over the corpus's maps of 1,000 steps or more, and 806 KB of a 938 KB container on the
writer benchmark's 100,000 steps. The ids of one line ascend, usually by the length of a loop body,
so their gaps are small and repeat. Over those maps:

| Layout | Bytes per step | Full load in WASM, ns per step | Reading the member too |
|---|---:|---:|---:|
| v1 | 8.07 | 0.70 | 1.63 |
| v1, zstd'd as it stands | 1.43-1.58 | -- | -- |
| gap varints, no zstd | 1.03 | 1.84 | 2.40 |
| gap varints, zstd | 0.087 | 6.98 | 7.53 |
| **run-length gaps, zstd (version 2)** | **0.100** | **4.71** | **5.24** |

Compressing version 1 as it stood ("chunked zstd like the other streams") recovers far less,
because eight-byte integers that grow by small, irregular steps are poor zstd input. Gap varints
alone leave the map the largest member of most containers. Gap varints under zstd are the smallest,
but every gap then has to come out of the zstd decoder, and ruzstd -- the decoder the WASM reader
uses -- is slow on the long repeats a loop produces: one 98,301-step map of 81 bytes took 2.1 ms to
load. Run-length coding hands the decoder a run instead of its expansion: 14% larger than gap
varints under zstd, a third faster to load in WASM, and 81 times smaller than version 1.

The cost is decoding at open. Loading every list into memory, which is what the db-backend does,
costs about 3.6 ns per step more than version 1 in WASM, counting the read of the member: 0.4 ms
for a 100,000-step map, 36 ms for 10 million. That is with the member in the page cache, the case
most favourable to the larger layout; from a disk or over the network, the 81-fold smaller member
pays back more than its decoding. A lookup after the load is unchanged, and a reader that wants one
line inflates one chunk of at most about 64 KiB.

A step id is the step's **exec-record index**: its index in `steps.dat`, and so the index of its
value record in `values.dat`. Thread, raise/catch and source-reload records count, so the ids of a
trace with thread switches are not consecutive. `path_id` and `line` are the coordinates the step
was registered at (the `paths.dat` id and the line as registered, truncated to 32 bits), not its
global line index -- except that a step registered at line `0` is keyed under line `1`, as it is
recorded everywhere else (§"Global Line Index", "Line 0 is line 1, everywhere").

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

#### Snapshot payloads (MCR recorder) are Chunked Compressed Tables of bytes

*(Added 2026-09-25, `MCR-Memory-Page-CAS.milestones.org` CAS-Z0.)*

The MCR recorder's memory-snapshot payloads are stored through the same Chunked
Compressed Table as `steps.dat` and the thread streams above — **the container's
one compression layer; nothing snapshot-specific is layered on top** (owner
decision, 2026-09-24: snapshots "can likely piggyback on the existing compression
of the CTFS format (there won't be any win from double compression)").

The payload classes, by LOGICAL name:

| Logical name | What it holds |
|---|---|
| `cp.<kind>.mem` | the page bytes of a stage-0 boundary snapshot (`kind` = `prein` / `entry` / `postl`) |
| `cp.<kind>.cas` | that boundary's page-CAS hash stream (`MCR-Memory-Page-CAS.md` §3.3) |
| `cppages.ns` | the trace's page-CAS page store (§5.1) |
| `cp0.mem`, `cpN.mem` | a full memory snapshot, `(addr: u64, size: u64, bytes[size])*` — the initial one, or periodic checkpoint `N` |
| `t_start.mem` | the macOS recording-start snapshot (same framing as `cp0.mem`) |

Each is stored as a record-size-**1** table: every record is one byte, so
`chunk_size` is the number of payload BYTES per chunk (the recorder uses
1 048 576), every chunk but the last inflates to exactly `chunk_size` bytes, and
the payload's length is `(chunks - 1) * chunk_size` plus the last frame's
declared content size.  Random access holds: byte `o` is in chunk
`o div chunk_size`.

**Member names.**  `foo.dat` / `foo.idx` cannot be used: the logical names are
already up to 12 characters, the most CTFS keys (§ base40 in
[ctfs-container.md](ctfs-container.md)).  So the data and index members are named
by replacing the extension `<ext>` with `<ext[0]>zd` and `<ext[0]>zi`:

```
cp.entry.mem -> cp.entry.mzd (data)  +  cp.entry.mzi (index)
cp.entry.cas -> cp.entry.czd         +  cp.entry.czi
cppages.ns   -> cppages.nzd          +  cppages.nzi
cp3.mem      -> cp3.mzd              +  cp3.mzi
```

A name whose derived member would exceed 12 characters is refused by the
writer, never truncated.

**The legacy form, and how a reader tells the two apart.**  Traces written
before 2026-09-25 carry the same payload RAW under the logical name.  A reader
MUST decide the form by **which members exist**, never by inspecting bytes (a raw
memory page can begin with the zstd frame magic):

| `<logical>` | `<data>` + `<index>` | Meaning |
|---|---|---|
| absent | both present | compressed form — inflate |
| present | both absent | legacy raw form — read as is |
| present | any present | **malformed** — refuse |
| any | exactly one present | **malformed** (half a pair) — refuse |
| absent | both absent | the payload is absent |

A writer emits exactly one form, and adds the data and index members together
or not at all (it checks for two free root entries first; CTFS cannot remove a
member once added).  Every frame's declared content size MUST match what the
index implies, so a truncated or re-ordered payload fails rather than decoding
to plausible bytes.

Layouts (`cp.<kind>.lay`), register files (`cp.<kind>.reg`, `cpN.regs`) and the
checkpoint index members stay raw: each is well under one block, and a
compressed pair costs two members and at least four blocks where the raw member
costs one member and two.

**A payload that fits in one block is stored raw (normative since 2026-09-30,
`MCR-Memory-Page-CAS.milestones.org` CAS-D1).**  The same arithmetic applies to
a snapshot payload that happens to be small.  A writer MUST store a payload of
**at most `block_size` bytes** (the container header's block size, 4096 for every
MCR trace) in the **raw form**, under its logical name, and MUST store a payload
of more than `block_size` bytes in the compressed form.  The threshold is on the
PAYLOAD length, before compression, so the decision needs no trial compression
and two writers given the same payload choose the same form.  Why exactly that
threshold: a compressed form occupies at least one data block and one index
block, each with its own block-map block, and a second root entry — four blocks
where the raw member of a one-block payload occupies two (its data block and its
block-map block) — so no payload of one block can be made smaller by
compressing it, and the rule is exactly "compress only what compression can
shrink by a block".  A payload of zero bytes is covered by the rule (a raw
member of length 0); a producer that omits an empty payload altogether (the MCR
recorder omits an empty `cp.<kind>.mem`, `MCR-Memory-Page-CAS.md` §5.1) writes
no member at all.  In practice the payload the rule catches is a small stage-0
boundary's `cp.<kind>.cas` (`cp.prein.cas`, ~1.5 KB on Windows).
(The block counts above are container version 4's, where every member has a
block-map block. Under version 5 a member of at most one block has none
(`ctfs-container.md` §2), so the compressed form costs at least two blocks and
the raw one-block member costs one; the threshold and its reason are unchanged.)

This adds no reader obligation.  The raw form is the legacy form in the table
above, which every reader already resolves; the form is still decided by which
members exist, never by the bytes; and the rule is a WRITER rule only — a reader
MUST accept either form for a payload of any length (a pre-2026-09-25 trace
carries large payloads raw, and a writer that predates this rule carries small
ones compressed).  Consistent with the owner decision above: there is still one
compression layer, the container's own; this only says when it is not worth
applying.

Implementations: `codetracer-native-recorder/ct_recorder/src/ct_recorder/snapshot_payload.nim`
(writer and reader); `tracing-formats-benchmarks/cas_dedup/ctfs.py`
(`read_payload`, independent Python reader).

### Checkpoint Packing (cp.dat + cp.off)

> **Not what the MCR recorder writes (corrected 2026-09-25, CAS-Z0).**  No
> producer in the workspace writes `cp.dat` / `cp.off`; the design below is
> unimplemented.  The MCR recorder's periodic checkpoints are FULL snapshots in
> five member kinds: `cpidx.idx` (`count: u32`, then `id: u32` per checkpoint),
> `cpidx_full.idx` (`count: u32`, then `(id: u32, geid: u64)` per checkpoint),
> `cpdata.bin` (concatenated `(id: u32, geid: u64, n: u32, (tid: u32, tick: u64)[n])`
> records, raw, no page data), `cpN.mem` (the memory, as a compressed snapshot
> payload — above) and `cpN.regs` (`(tid: u32, len: u32, bytes[len])*`).  There
> is no incremental chain and no delta encoding.  `Multi-Core-Recorder.md`
> §12.3-§12.4 states the same.  (`cpidx_full.idx` is 14 characters and `_` is not
> in the base40 alphabet, so its 12-character key decodes as `cpidx<NUL>full.i`;
> readers that look members up by encoded key find it, readers that compare
> decoded names do not.)

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
Header (12 bytes):
  magic: "CTMD" (4 bytes: 0x43, 0x54, 0x4D, 0x44)
  version: u16 LE (6; see "Version History")
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
    bit 5       -- FLAG_HAS_ALTERNATE_SOURCE_VIEWS (allocated; not set from version 6 on -- srcviews.dat is found by presence, see §"Alternate Source Views")
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
    -- Capability (record-layout variant declared at open):
    bit 14      -- FLAG_HAS_LINE_COUNT_TABLE (every paths.dat record carries the
                   file's line_count; see §"`paths.dat` line-count table" below)
    -- Stream-presence, continued (see the note below on why it is not adjacent):
    bit 15      -- FLAG_HAS_CORRELATION_INDEX (corrmark.ns + markers.dat/.off) WTCI

    No bit is reserved: version 4 assigned all sixteen. A further flag goes
    in `flags_ext`.
  flags_ext: u32 LE -- always present at version 6; see "Extended flags
    (`flags_ext`)" below.

### Extended flags (`flags_ext`)

`flags_ext` is the u32 at bytes 8..12, immediately after the u16 `flags`; the
body starts at byte 12. Version 6 always carries it, `0` included.

```
  flags_ext bit 0     -- FLAG_EXT_HAS_SOURCE_RELOAD: steps.dat may contain
                         SourceReload records (tag 0x08); see trace-events.md
                         §"Source Reload Marker (Tag 0x08)"
  flags_ext bits 1-31 -- reserved
```

Requirements:

* **One layout.** Version 5 carried the word only when an extended flag was
  set and version 4 never did, so a reader had two header lengths to tell
  apart by version; version 6 always carries it and has one. A `flags_ext` of
  `0` is the ordinary value for a recording that uses no extended feature.
* **Unknown extended bits MUST be refused.** Unlike the u16 capability and
  stream-presence bits, every allocated extended bit changes what a stream may
  contain (bit 0 admits a step-stream tag), so a reader that ignored one it
  does not know would misdecode the stream. A reader refuses a container
  whose `flags_ext` carries a bit it does not implement, naming the bits.
* **A header shorter than 12 bytes MUST be refused.**
* **Bit 0 is a capability declared at open** (version 6). It says that
  `steps.dat` MAY contain `SourceReload` records, not that it does. A recorder
  that can observe a reload -- one attached to a reload agent -- declares it
  before the first record, as it chooses bits 4 and 14, and a writer MUST
  refuse a `SourceReload` in a trace that did not, failing the call
  (`trace-events.md` §"Recorder Integration — A Failed Call Fails the
  Recording"). A trace that declared it and recorded no reload is
  well-formed. Version 5 set the bit exactly when a reload had occurred, which
  is only known at close; under the durability rule (`ctfs-container.md` §6)
  `meta.dat` is written at open and never rewritten, so a crashed recording
  would otherwise hold a sealed chunk with tag 8 under a `meta.dat` that
  makes its readers refuse it.
* **`meta.dat` is written once, at open, and is complete then.** Every field
  and every flag is fixed before the first record. A writer MUST refuse a
  call that would change one after that -- setting the working directory or
  the arguments, enabling a capability -- rather than rewrite the member.
  (Stream-presence bits 8-13 and 15 are hints decided at open from what the
  writer will create; see §"Stream-presence flags are a hint, not a gate".)

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

3. **Stream-presence bits (8..13 and 15)** claim that a *separately named
   stream file* exists in the container (`steps.dat`, `spans.dat`,
   `corrmark.ns`, …). This claim is **tautological with the container's own
   structure**: the stream exists iff the file entry exists. Bit 14 sits
   inside that numeric range but is a capability bit, not a stream-presence
   one — see § "Why the stream-presence bits are no longer contiguous". See
   the next subsection.

### Stream-presence flags are a hint, not a gate

A stream-presence bit (8..13, 15) is an **optional hint**, redundant with a
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
- **Version 6: a stream-presence bit is set exactly for the streams a writer
  creates at open.** `meta.dat` is written once, complete, before the first
  record (ctfs-container.md §6), so a bit can only state what is known then.
  The runtime writers create `calls`, `steps`, `values`, `events` and the
  interning tables at open and set bits 8-12. They create `spans.dat`,
  `corrmark.ns` / `markers.dat` and `srcviews.dat` lazily, when the first
  record of that kind arrives, so they never set bits 13, 15 or 5; a reader
  finds those members by presence. Two writers given the same recording
  therefore write the same flag word.

Fields (varint-prefixed):
  recording_id: varint length + UTF-8 bytes (required, M-REC-1)
  program: varint length + UTF-8 bytes
  args_count: varint
    args[0..args_count-1]: varint length + UTF-8 bytes each
  workdir: varint length + UTF-8 bytes
  recorder_id: varint length + UTF-8 bytes
```

The flag-gated blocks of §"Extended Fields (flags bitmask)" follow `recorder_id`.

**`meta.dat` carries no path list (version 6).** A trace's source paths are the
records of `paths.dat` (+ `paths.off`, §"Interning Tables"), in id order, and
nothing else. Versions 3 to 5 also wrote every path into `meta.dat`, after
`recorder_id`: a second copy of `paths.dat` that every reader parsed on open.
On the WASM writer's benchmark recording it was 46,904 of `meta.dat`'s 47,023
bytes and 14% of the container (`measurements/2026-10-format-efficiency.md`
§"`meta.dat`'s path list"), and two copies of one list are two answers that
can disagree -- the reason this section once had to say that the list "is
`paths.dat`'s path strings in id order", after one writer assembled it from
only one of its registration routes. Therefore:

- **A writer that knows a source path MUST intern it in `paths.dat`**, however
  the recorder supplied it -- a registration, a step, a `Path` event, or a
  `--source` list given to a recorder that writes no steps (the MCR recorder,
  the native backend's RR/TTD exporter). A container whose recording names no
  source path has an empty `paths.dat` or none.
- **A reader takes source paths from `paths.dat` only.** There is no fallback
  to `meta.dat`, and a reader MUST NOT look for one: in a version 6 header the
  bytes after `recorder_id` are the next flag-gated block, or nothing.

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
- **v3** (M-REC-1, 2026-05-18) -- prepended a required
  `recording_id` UUIDv7 string before the existing `program` field.
  Pre-1.0 there is no backcompat shim: v2 fixtures must be
  regenerated. Spec:
  `codetracer-specs/Refactoring-Plans/Recording-Identifier-Migration.md`
  § 3.

- **v3.2** (WTCI, 2026-09-09) -- flag bit 15 additionally covers the
  correlation-marker label interning table (`markers.dat` + `markers.off`),
  written lazily beside `corrmark.ns`. No layout change and no new bit: the
  table is meaningless without the index the same bit already announces, and
  joining bit 12's `FLAG_HAS_INTERNING_TABLES` set would have redefined a flag
  whose meaning is agreed across the Nim writer, `codetracer_trace_writer::
  meta_dat` (Rust) and the db-backend. See § "Interning Tables".
- **v3.1** (WTCI, 2026-09-09) -- allocated flag bit 15
  `FLAG_HAS_CORRELATION_INDEX`, spending the last bit of the flag word.
  The index was drafted against bit 14, which `FLAG_HAS_LINE_COUNT_TABLE`
  took first; both describe the container, so they could not share one.
  Neither bit had shipped, so the choice cost no compatibility, and it
  leaves stream-presence as bits 8..13 and 15 with a record-layout
  capability at 14 between them. No layout change: the bit is a
  stream-presence hint for `corrmark.ns` and, like bits 8..13, is
  redundant with the container's file-entry array. A reader that ignores
  it loses nothing; a reader that trusts it over the file entry is wrong
  for the same reason it would be for `spans.dat`. A further flag now
  needs a version bump rather than a spare bit. Contract:
  `codetracer-specs/Testing/CTFS-Correlation-Marker-Contract.md`.
- **v4** (2026-09-08) -- the line-only `global_position_index`
  encode became `prefix_sums[k] + (line - 1)`; see § "Global Line
  Index". No header field changed. The version moved because it is the
  only thing in a container that tells the two encodes apart: both land
  inside the trace's own address space, so a v3 container read under the
  v4 decode reports every step one line high without failing. Readers
  refuse v3 and below rather than read them. (Bits 14 and 15 and the
  `markers.dat` table listed above as v3.1 / v3.2 were allocated after
  this bump, under version 4.)
- **v5** (GDH-M2, 2026-09-10) -- a `flags_ext: u32 LE` word follows the
  u16 `flags`, written only when an extended flag is set; see § "Extended
  flags (`flags_ext`, version 5)". Bit 0 admits the `SourceReload`
  execution-stream record (`trace-events.md` § "Source Reload Marker (Tag
  0x08)"), which marks a switch to the path versions of § "`paths.dat`
  path versions".
- **v6** (current, 2026-10-01) -- the path list after `recorder_id` is
  gone; `paths.dat` is the only list of source paths (§ "`meta.dat` carries
  no path list"). `flags_ext` is always present, so there is one header
  length. Readers refuse every other version, naming it: the bytes after
  `recorder_id` mean something different in v5 and below, and a reader
  that guessed would read a path count as an MCR field. Pre-1.0, there is
  no compatibility shim; fixtures are regenerated. The same revision moved
  the container to version 5 (`ctfs-container.md` §2), compressed
  `step-map.ns` (§ "`step-map.ns`"), made the step-encoding rule
  normative (`trace-events.md` § "Encoding Rules"), and made an
  `events.dat` record's kind the recorder's exact `EventLogKind`
  (`trace-events.md` § "EventLogKind (u8 enum)").

### Extended Fields (flags bitmask)

**Flag bit 0 -- MCR fields.** When set, the block below follows
`recorder_id`. Every field is varint-encoded (no fixed-width integers):

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
`FLAG_HAS_MCR_FIELDS` is clear, follows `recorder_id` directly).
Records replay-launch address-space hardening state captured at
record time so the replay backend can decide between hard-pin and
soft-pin modes:

```
  aslr_disabled: u8 (0 = false, 1 = true)
```

**Flag bit 2 -- Layout snapshot (M-RLP-2, §6B.7).** When set, the
block below follows the replay-launch block (or, if
`FLAG_HAS_REPLAY_LAUNCH_FIELDS` is clear, follows the MCR block or
`recorder_id` per the same composition rules).  Carries a fingerprint of the
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

**Line 0 is line 1, everywhere.** A recorder may hand a writer line `0` -- some language runtimes
report it for code that has no source line of its own. Line 0 is not a source line, and the
encode above would put it one address below the file's base, in the previous file (or, for file
0, at `2^64 - 1`). A writer therefore records a step, a function or any other location registered at
line `0` as line `1` of the same file, in **every** member that carries the location: the
`global_position_index` of `steps.dat` (line-only and column-aware alike), `funcs.dat`'s
`global_line_index`, and the key of `step-map.ns`, whose `line` is the line *as recorded*, so `1`.
A reader that resolves the address and a reader that looks the line up in `step-map.ns` then
agree. Until 2026-10 the step stream recorded `1` while `step-map.ns` keyed the raw `0`, so a
breakpoint on line 1 missed those steps and one on line 0 found steps the step stream placed on
line 1. No line-only recording in the measurement corpus (288 step maps from 14 recorders) has a
step at line 0, so the rule changes no measured trace; it settles what the writers already did in
the address and did not do in the index. A writer MUST NOT refuse line 0.

`line_count[k]` is the count `paths.dat` records for file `k` when
`meta.dat` bit 14 is set (§"`paths.dat` line-count table"). A trace
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

1. Write `srcviews.dat` / `srcviews.off`. Readers find them by presence.
   (Bit 5, `FLAG_HAS_ALTERNATE_SOURCE_VIEWS`, stays allocated but is not set
   from version 6 on: the members are created when the first view is, after
   `meta.dat` has been written; see §"Stream-presence flags are a hint, not a
   gate".)
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

## Document history

This file carried no history table before 2026-09-30; earlier changes are in
the git history of `codetracer-trace-format-spec`.

| Date | Change |
|---|---|
| 2026-09-30 | **Snapshot payloads: a payload that fits in one block is stored raw** (`MCR-Memory-Page-CAS.milestones.org` CAS-D1).  "Snapshot payloads (MCR recorder)" gains a normative writer rule: a snapshot payload of at most `block_size` bytes (4096) is stored in the raw form under its logical name, one of more than `block_size` in the compressed form; the threshold is on the uncompressed length, so the choice needs no trial compression.  A compressed form costs at least four blocks (data and index members, each with a block-map block) and two root entries where a one-block raw member costs two blocks and one entry, so compression cannot shrink such a payload.  No reader change: the raw form is the legacy form every reader already resolves, told apart by which members exist.  Measured cause: on a Windows `fx_small` page-CAS trace the boundary-A `cp.prein.cas` (1 228 bytes compressed) occupied four blocks for a ~1.5 KB payload, the two blocks that tied a page-CAS trace with the compressed legacy trace it replaces. |
| 2026-10-01 | **Format-efficiency revision** (`measurements/2026-10-format-efficiency.md`). `meta.dat` version 6: no path list, `paths.dat` is the only list of source paths, and `flags_ext` is always present. `step-map.ns` version 2: keys delta-coded, step-id gaps run-length-coded, zstd chunks of about 64 KiB behind an uncompressed chunk table; 81 times smaller than version 1 on the corpus. Together with container version 5 (`ctfs-container.md` §2), the normative step-encoding rule and the exact `EventLogKind` in `events.dat` (`trace-events.md`). |

# Internal Files

A CTFS container (`.ct` file) stores several named internal files. This document describes the standard files and their data abstractions.

> **Scope.** This specification defines the structure of the CTFS container format, and the records
> used by the open-source recorders that produce materialized traces. It does not describe how the
> Multi-Core Recorder (MCR) uses CTFS: which members an MCR recording writes, their layout, and its
> per-thread, checkpoint and page streams. Those are specified in `codetracer-specs`
> (`spec/Trace-Files/CTFS-Binary-Format.md`), and they are subject to change in each release. The
> boundary between the two repositories is stated in `codetracer-specs/spec/Trace-Files/README.md`.
> The MCR material this document carried until 2026-10-07 moved there; the sections below that name
> it say where.

## Member Catalogue

Every member a materialized-trace writer produces, with the property that decides how it is stored
and, for a member that holds a family keyed by a `u64`, the abstract family it belongs to
([ctfs-keyed-families.md](ctfs-keyed-families.md) §5: F1 dense, F2 clustered, F3 sparse exact live,
F4 sparse ordered live, F5 static index). "Today" is the realization every writer emits now; it stays
until the benchmark decision of [ctfs-keyed-families.md](ctfs-keyed-families.md) §8 says otherwise.
"Measured against" names the candidates the benchmark plan compares for that member.

Members of an MCR recording, and members CodeTracer's own tools persist into a recording (the
db-backend's `coverage.tc`, its persisted `memwrites.tc` and `linehits.tc`, the race detector's
`slc-*`), are catalogued in `codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md` §2. A runtime
trace has no `memwrites.tc` (§"Memory writes are not part of a runtime trace").

**Block placement.** A split-stream writer's members are placed in the order `ctfs-container.md` §6,
"Block placement", fixes, so the container is a function of the recording. A realization chosen for
any member below joins that order, and MUST itself be a function of its entries (G12).

### Singletons (one root member each; no family)

| Member | What it is | Written | Live readers | Notes |
|---|---|---|---|---|
| `meta.dat` | binary metadata | once, at open | yes | never rewritten (§"Extended flags") |
| `steps.dat`, `values.dat`, `calls.dat`, `events.dat`, `spans.dat` | chunked compressed streams | live, append-only (P1); `calls.dat` in call-key order, so in practice at close | yes | monotonic streams; only `Size` and `MapBlock` change in place |
| `entry.dat` | the recorded program entry ([recorded-entry-identity.md](recorded-entry-identity.md)) | once, at close, last | no | optional |

`events.log` and `events.fmt`, a seekable-zstd stream, were removed (`trace-events.md`); a container
carrying them is refused.

### Keyed families

| Member | Key, and its shape | Lookups | Value class | Written | Family | Today | Measured against |
|---|---|---|---|---|---|---|---|
| `steps.idx`, `values.idx`, `calls.idx`, `events.idx` | chunk number: dense, writer-assigned | exact (chunk `N / chunk_size`) | V0 (an offset) | live, append-only | F1 | D1 (an offset array after a `u32` header) | unchanged: no candidate beats arithmetic |
| `spans.idx` | chunk number: dense; records located by cumulative count | exact by chunk; record `N` by binary search over `cumulative` | V0 (offset, cumulative count) | live, append-only | F1 (with a monotone cumulative column, M1-style) | D1 with 16-byte entries (§"`spans.dat` / `spans.idx` / `spantype.ns`") | unchanged |
| `paths`, `funcs`, `types`, `varnames`, `markers`, `srcviews` (`.dat` + `.off`) | interned id: dense, writer-assigned | exact, enumeration | V1 | live, append-only | F1 | D2 (Variable-Size Record Table) | unchanged |
| `step-map.ns` | `(path_id, line)`: sparse over a dense product, ordered | exact, ordered | V2 (step-id list), sealed | close-time | F5 | S1 (sorted, chunk fences, compressed frames) | unchanged unless a benchmark shows S2/S3 better for line-breakpoint lookups |
| `linehits.tc` | a step's location address (`global_position_index`): the global line index in a line-only trace, a byte-offset position in a column-aware one; the keys are the addresses that were hit | exact | V2 (step-id list) | close-time, after `step-map.ns`, when enabled | F5, or F1 over the address space in a line-only trace | S2 (the `ctfs-container.md` §8a image) | F1 D2 over the global line index (line-only traces; one record per line, hit or not) against S1, S2, S3; decided by the measured fraction of addresses hit. A column-aware trace's address space is too sparse for F1 |
| `corrmark.ns` | `XXH64` of a correlation key: sparse, uniform | exact, confirmed against the bucket | V2 (a bucket of 60-byte entries) | close-time, after `linehits.tc`, only when a marker or span coverage was declared (§"Correlation Index") | F5 | S2 (the §8a image) | S3 (a static hash) and S1; a replacement keeps the property that the image is a function of its entries |
| `spantype.ns` | span type id: dense, writer-assigned | exact | V1 | close-time, after the span stream's last chunk | F1 (static) | a flat table (SPTY v1) | unchanged |
| `memreads.tc` | memory address | exact | V2 | no writer | F3 or F5 | specified only | — |

### What a writer does until the decision

Every member above keeps the format its own section specifies. A writer MUST NOT emit a keyed layout
from [ctfs-keyed-families.md](ctfs-keyed-families.md) §5 for any of them before §8 there records the
decision and adds the layout. When it does, this catalogue gains the member's new format, every
producer and consumer is updated in one rollout (`codetracer-specs`
`milestones/CTFS-Keyed-Families.milestones.org`), and the recorders that write through the
`codetracer_trace_writer` library advance their pins together.

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

**Appending while a reader follows.** A writer appends record `N` by writing its bytes to the data
file and publishing the data file's entry, and only then appending the new terminal offset to the
offset file and publishing that entry (`ctfs-container.md` §6). A reader takes the record count from
the offset file, so it never sees a record whose bytes are not yet published. It loads the offset
file's `Size` and entries first and the data file's `Size` after them, so the data file is at least
as long as the last offset. While the table may still grow, the data file can be longer, by a record
whose offset is not yet appended; a reader of a live table therefore checks
`last offset <= data Size`, and a reader of a closed one checks equality, as above.

### Chunked Compressed Table (dat + idx)

Extends fixed-size or variable-size tables with per-chunk compression. Records are grouped into chunks of `chunk_size` records, each independently compressed with Zstd.

- **Data file** (`foo.dat`): concatenated compressed chunks, no inline headers
- **Index file** (`foo.idx`): starts with `chunk_size: u32`, then one `u64` byte offset per chunk

Record N is in chunk `N / chunk_size`. The companion index provides O(1) access to any chunk's byte offset. See [ctfs-container.md](ctfs-container.md) Section 7 for full details.

### Keyed Member (families that grow with the recording)

One root member, or a small fixed set of them, that maps `u64` keys to fixed-size records, and through
them to values or to container streams. A producer uses it for a family whose count grows with the
recording, so that the fixed root directory bounds the number of families and not the size of the
recording. The container rules are [ctfs-container.md](ctfs-container.md) §7a; the families, their
candidate structures and how one is chosen are [ctfs-keyed-families.md](ctfs-keyed-families.md). The
Fixed-Size Record Table and the Variable-Size Record Table above are themselves realizations of the
dense family (F1, D1 and D2) for records that are only appended.

### Interning Tables

Deduplicated records using the variable-size record table pattern. A `.dat` file holds serialized records, a `.off` file holds the offset index. Event streams store numeric IDs that reference interned records.

| Table | Data File | Offset File | Record Format |
|-------|-----------|-------------|---------------|
| Source paths | `paths.dat` | `paths.off` | raw bytes (file path); column-aware traces append a per-line byte-length table — see "paths.dat Layout A" below |
| Variable names | `varnames.dat` | `varnames.off` | raw bytes (name) |
| Types | `types.dat` | `types.off` | kind: u8, lang_type_len: varint, lang_type: bytes, specific_info: binary |
| Functions | `funcs.dat` | `funcs.off` | global_line_index: varint, name_len: varint, name: bytes |

**A function record is written as soon as it can be.** It needs its declaration path's id, so it is
appended once it and every function with a lower id have a registered declaration path -- at
registration, or right after the path record that registers it -- and the rest at close
(`ctfs-container.md` §6, "Block placement", rules 3 and 6). Writing every function at close, as one
writer did, left a recording killed mid-run with call records that named functions absent from
`funcs.dat`.

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
  line_count:  varint                     (0 = the conventional table; see below)
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
* **Every Layout A record carries a table of non-zero size, and a file's table
  is fixed when the file is first interned.** In a column-aware trace a path
  is interned by its first mention -- an explicit registration, or a step, a
  function, a call or an id request that names it -- and its table is decided
  then, by the **writer**:
  - a table the caller gives is recorded as given, except that a table whose
    lines hold nothing (all zero, including `[0]` for an empty file) gives its
    first line one position (`[0]` becomes `[1]`, `[0, 0]` becomes `[1, 0]`),
    so that the file's size is not `0` (`trace-events.md` §"Per-File
    Contiguous Integer Ranges");
  - an empty table, or no table at all (the path was first mentioned by a
    step, a function, a call or an id request), records the **conventional
    table**: `100000` lines of `1024` positions each, the column-aware
    counterpart of the line-count table's `100000` ceiling (§"`paths.dat`
    line-count table"). On a file with the conventional table the writer
    records a column above `1024` at column `1024` of its line, as it records
    line `0` as line `1` (§"Global Line Index"), and refuses a line above
    `100000`.
  The two writers apply these rules identically, so recorders do not each
  re-implement them.

  **The conventional table is written as `line_count = 0`**, with no
  `line_lengths` after it: a one-byte record body instead of the 100,000
  varints of the table spelled out (about 100 KB in an uncompressed member,
  and 0.4-0.8 MB of memory in each writer, per file). `0` was never a valid
  count -- a file of no positions is refused above -- so the value is free,
  and it is the **only** encoding of the conventional table: a writer that
  records it, whether by the fallback or because a recorder passed the same
  100,000 × 1024 table itself, MUST write `0`, so two writers agree byte for
  byte. A reader MUST decode `line_count = 0` as 100,000 lines of 1024
  positions, and SHOULD hold it as that rule rather than as an array.
  Recorders hit the fallback routinely -- the PolkaVM recorder registers every
  program binary this way, and the JS and EVM recorders an `<unknown>` path --
  so on a recording of a few hundred kilobytes the spelled-out table would have
  been the largest member.

  The cost of the conventional table that remains is address space: a
  file with it occupies about 10^8 positions, so files interned after it get
  longer absolute positions. A recorder SHOULD therefore give a file's real
  table whenever it can read the source, and give it **before** the file's
  first mention.
* **A table offered after the file was interned is refused unless it is the
  table already recorded.** The file's size fixed the base of every file
  interned after it, and positions already written depend on those bases, so a
  later, different table cannot be honoured. A writer MUST refuse it, naming
  the path, and fail the call. Returning the existing id silently, as both
  writers did, hid a recorder that mentioned a file in a step or a call before
  registering its table: the file kept a table that did not describe it, and
  positions in it resolved to the wrong lines. (The Python recorder did exactly
  this from 2026-09-30 until `e573455`; a 2026-10 survey found the same
  ordering in the JS, Solana and PolkaVM recorders.)
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
| `spans.dat` | Chunked compressed | Optional: span records (§"Optional runtime members") |
| `spans.idx` | Span index | Optional: chunk offsets and cumulative record counts for `spans.dat` |
| `spantype.ns` | Span-type index | Optional: span ids by span type |
| `markers.dat` | Var-size record | Optional: interned correlation-marker labels |
| `markers.off` | Offset index | Optional: marker label offset index |
| `corrmark.ns` | `NSB1` namespace | Optional: correlation index (§"Correlation Index (`corrmark.ns`)") |
| `linehits.tc` | `NSB1` namespace | Optional: location address to step ids (§"Optional runtime members") |

A runtime trace has no `memwrites.tc` (§"Memory writes are not part of a runtime trace").

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

In a compact container (`ctfs-container.md` §1d) these chunks, and `step-map.ns`'s, are stored as
their decompressed content, the index offsets locating the content (`ctfs-container.md` §1f).

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
inflates that chunk alone and scans it. **A lookup of line `0` is a lookup of line `1`.** A step
registered at line `0` is keyed under line `1` (below), so the steps a caller asking for line `0`
means are filed there; a reader answers `(path_id, 0)` with line 1's ids, as the address resolver
answers line 1 for those steps, and never with an empty answer that a key no writer stores would
give. A reader MUST refuse, by name, a step map whose decoded
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


### Optional runtime members

A runtime writer creates these members only when the recording asks for them, on the first record of
their kind (§"Stream-presence flags are a hint, not a gate"): a recording that uses none of them is
byte-identical to one written by a writer that does not implement them. Both reference writers
implement all of them, and both readers read all of them.

#### `spans.dat` / `spans.idx` / `spantype.ns` — the span stream

A span is a bounded, labeled interval of execution named by the coordinate *(process_ord,
thread_id, step range)*: an HTTP request, a process, a test, or a native↔VM crossing (below). The
product contract (what a span is for, the request panel, live sessions) is
`codetracer-specs/Trace-Files/CTFS-Request-Span-Streams.md`; this section is the byte layout, which
is normative.

**`spans.dat`** is a Chunked Compressed Table of span records. A chunk's content is the
concatenation of length-framed records, `(record_len: varint, record)*`, as in `calls.dat`. Each
chunk is one Zstandard frame at level 3, compressed in one shot (content size declared, no checksum,
no dictionary). In a compact container (`ctfs-container.md` §1d) a chunk is stored as its content.

**A span chunk may be short anywhere in the stream.** The writer seals the buffered records as a
chunk when there are 64 of them, and also whenever the recorder flushes the stream (a live
recorder flushes so that an in-flight span is visible at once). A chunk is never empty: a flush
with nothing buffered writes nothing. `chunk_size` is therefore an upper bound, and "record `N` is in
chunk `N / chunk_size`" (`ctfs-container.md` §7) does **not** hold for this stream; the index says
where every record is.

**`spans.idx`** is not the §7 index. It is:

```
Header (8 bytes):
  chunk_size: u32 LE       -- the seal-at threshold, 64; an upper bound, not a locator
  index_version: u16 LE    -- 2
  reserved: u16 LE         -- 0
Entries, 16 bytes each, entry i at 8 + 16*i:
  offset: u64 LE           -- chunk i's first byte in spans.dat
  cumulative: u64 LE       -- span records in chunks 0..i together
```

The record count is the last entry's `cumulative` (0 with no entry); chunk `i` holds
`cumulative[i] - cumulative[i-1]` records; the chunk holding record `N` is the first whose
`cumulative` exceeds `N`, found by binary search. Chunk `i` spans `[offset[i], offset[i+1])`; the
last chunk is the single Zstandard frame at its offset (in a compact container: to the end of the
member). Opening the stream decompresses nothing; reading a record decompresses its chunk.

The writer appends a chunk to `spans.dat` **before** the index entry that publishes it, so an
entry always names a complete chunk.

A reader MUST refuse, naming `spans.idx`: a member shorter than its header; `chunk_size` 0; an
`index_version` other than 2 (version 1, offsets only, never shipped); a nonzero `reserved`; an entry
region that is not a whole number of entries; an offset past the end of `spans.dat`; offsets or
cumulative counts that decrease from one entry to the next. A chunk whose frame does not decode, or
whose content does not split into whole framed records, is refused naming `spans.dat`.

**Record** (all integers varints unless noted; strings are `varint length + UTF-8 bytes`):

```
span_id:            varint   1-based; 0 is refused
parent_span_id:     varint   0 = none
flags:              u8       bit 0 open, bit 1 external; other bits refused
status:             u8       0 unknown | 1 ok | 2 error; other values refused
start_wall_ns:      varint   UNIX epoch nanoseconds
end_wall_ns:        varint   0 when open
process_ord:        varint   0 = primary process
thread_id:          varint
start_step:         varint   first step id in the span
end_step:           varint   last step id; 0 when open
external_recording: string   only when flags.external: the other container's recording_id
external_path:      string   only when flags.external: its path relative to this container
span_type:          string   "web-request" | "process" | "test" | "native-vm" | ...
label:              string
structural:         u8       bit 0 contiguous on one thread, bit 1 shares timeline,
                             bit 2 concurrent with siblings; other bits refused
metadata_count:     varint
metadata:           (key: string, value: string) x metadata_count, in emission order
```

A reader MUST refuse a record that is truncated, has bytes left over after its last field, sets an
unknown `flags` or `structural` bit, has a `status` above 2, has `span_id` 0, is open with a
nonzero `end_wall_ns` or `end_step`, or carries a string that is not valid UTF-8 (the same holds for
the names in `spantype.ns`). A writer MUST refuse to write those, and also a record that is
not external but carries external strings, since they would be lost.

**When the members are created.** `spans.dat` and then `spans.idx` are added to the container when
the first span is registered, after `meta.dat` (which is written before the first record), so their
place among the members is fixed by when that first span arrives. `spantype.ns` is added at close,
after the last chunk is sealed and before `step-map.ns`. Neither writer stamps `meta.dat` bit 13: the
members are found by presence.

**Append-only, last record wins.** A writer appends an open record (flags.open) when a span starts
and a record with the same `span_id` when it ends; nothing is rewritten. The settled view of the
stream is, per `span_id`, the last record carrying it, in ascending `span_id`; a raw read returns
the records in append order. Metadata order is part of the record.

**Following.** A reader that follows the container (`ctfs-container.md` §6) sees new spans as new
index entries; a consumer that remembers how many entries it has read decodes only the chunks
published since.

**`spantype.ns`** is written once, at close, when the stream exists. It indexes span ids by type:

```
Header (18 bytes):
  magic: u32 LE = 0x53505459   -- the bytes "YTPS" on disk
  version: u16 LE = 1
  type_count: u32 LE
  type_table_offset: u64 LE   -- 18
Type table, type_count x 28 bytes, in type-id order:
  span_type_id: u32 LE        -- 0-based, first-appearance order of span_type in the stream
  name_len: u32 LE
  name_offset: u64 LE
  span_count: u32 LE
  spans_offset: u64 LE
Then every type's name bytes, in type-id order; then every type's span ids, u64 LE, ascending and
distinct (an open record and its completion contribute their span_id once).
```

A reader MUST refuse a bad magic, another version, or any name or list that lies outside the
member.

#### Native↔VM crossings

A host that runs an embedded VM records each entry into the VM as a span of type the host names
(e.g. `native-vm`), so that `[start_step, end_step]` bound the steps executed inside the VM frame
(`nested-trace-correlation.md` §1). Writers mint the crossing's `span_id` themselves — 1, 2, 3, …
per container, from a counter separate from the ids recorders give `register_span`. The two would
collide under last-record-wins, so a recording that opens crossings MUST NOT also register spans of
its own; a writer is not required to detect the mix.

`begin_crossing(span_type)` appends, and immediately flushes, an open record: `span_id` the next
minted id, `parent_span_id` 0, flags open, status unknown, every wall time, `process_ord` and
`thread_id` 0, `start_step` the number of exec records written so far, after the writer has written any step it
was holding back (so the id of the first step inside the crossing), `end_step` 0, `label` empty, structural bits 0 and 1 set, no metadata.
`end_crossing(span_id)` appends, and flushes, the settled record: the same fields with flags 0,
status ok, and `end_step` the last exec record written (their count minus one, or 0 when there is
none). Crossings nest and close innermost first: `end_crossing` of anything but the innermost open
crossing is an error, and writes nothing. A crossing still open at close stays open in the stream.

#### `linehits.tc` — line hits

An optional line-to-steps index, written at close when the recorder enabled it before its first
step. It is an `NSB1` namespace image (`ctfs-container.md` §8a), Type B, keyed by the step's
location address -- the `global_position_index` the step record carries -- with one entry per
distinct address. The entry's payload is the ids of the steps recorded at that address, ascending,
each a varint, back to back. Every exec record that is a step records a hit, with its exec-record
index as its id (§"`step-map.ns`"). A reader resolves a key to its descriptor and decodes the
varints in `[payload_offset, payload_offset + payload_len)`; a payload outside the member, a varint
longer than 10 bytes, or a payload that does not decode to whole varints, is refused. Every step
record -- absolute, delta and column delta -- records a hit at the address it decodes to. The member
is written at close, after `step-map.ns`, and covers the steps recorded after it was enabled. `step-map.ns` answers the same question for
line-only traces and is the member a reader should prefer; `linehits.tc` also serves column-aware
traces.

#### Memory writes are not part of a runtime trace

A runtime recorder observes variables and their values, never machine addresses; a change to a
variable is the value the next step records in `values.dat`. A runtime writer therefore records no
memory writes and MUST NOT write `memwrites.tc` (or `memreads.tc`), and a runtime trace reader has no
memory-write stream to read. `memwrites.tc` is a member of MCR traces, written by the native
recorder's omniscient preparation and read by its replay backend; its payload layout is given in
`codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md` §3.10, and its image is
`ctfs-container.md` §8a.

---

## Multi-Core Recorder (MCR) Traces (moved)

How the Multi-Core Recorder uses CTFS is outside this specification's scope (see the scope note at the
head of this document). On 2026-10-07 the MCR member formats that were specified here moved,
unchanged in substance, to `codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md`. The member
layout they describe is changing: per-thread streams, checkpoints and bundled files move out of the
root directory into keyed members (`ctfs-container.md` §7a), with each family's structure chosen by
benchmark ([ctfs-keyed-families.md](ctfs-keyed-families.md)). What moved, and where:

| Was here | Now in `codetracer-specs` `CTFS-Binary-Format.md` |
|---|---|
| "Multi-Core Recorder (MCR) Traces" (the member table, the two flavours of checkpoint state) | §2, the MCR member layout; the old table is kept in Appendix A |
| "Cross-OS portability" | §3.1 |
| "Thread Streams via Namespaces", "Per-file thread streams (MCR recorder) are seekable-zstd" | §2.3 (the per-thread stream family); the per-file `tNNN` / `iNNN` text is kept in Appendix A |
| "Snapshot payloads (MCR recorder) are Chunked Compressed Tables of bytes" | §2.5; the member-name derivation it gave is kept in Appendix A |
| "Checkpoint Packing (cp.dat + cp.off)" | §2.4 (the snapshot family); the original delta-chain design is kept in Appendix A |
| "Initial Register Snapshot (cp0.regs)", "Initial Memory Snapshot (cp0.mem)", "Initial Segment Bases (cp0.fsbase)", "Address-Space Map (cp0.maps)", "Bundled Debug Binary (debug.dat)" | §3.2 to §3.6, verbatim |
| "`memwrites.tc` (MCR)", the payload layout added on 2026-10-08 (abb73d4) | §3.10, verbatim |

---

## Metadata (meta.dat)

A single binary metadata file.

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
  ignores its file (and its bit) and reads the rest correctly. Bit 15
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

**Every text field of `meta.dat` is UTF-8**, here and in the flag-gated blocks. A writer MUST refuse
text that is not -- a program, argument or working directory that the host's operating system
handed over as arbitrary bytes included -- rather than store it, and a reader MUST refuse a
`meta.dat` that carries such text. The interning tables (`paths.dat`, `funcs.dat`, `types.dat`,
`varnames.dat`, `markers.dat`) are different: they hold the bytes they were given.

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

  **`flags_ext = 0` is valid at v6, and a reader MUST NOT refuse it.**
  Spelled out because it is v5's rule reversed, and a reader that carried
  v5's forward would refuse the common case. At v5 the word exists ONLY
  because a bit is set, so a zero word there is a writer that emitted a
  version it did not need, and refusing it is what keeps "the version says
  the word is there" from decaying into "the word is always there". At v6
  the word is unconditional -- that is the whole of what "one header
  length" means -- so a recording with no extended capability carries
  `flags_ext = 0`, and most recordings do. The two rules are not in tension:
  each says the word's presence is decided by the version and never guessed
  from its value.

  **A v6 reader also needs the container's version 5 body**, in practice if
  not in principle: the two moved in the same revision, and every v6
  `meta.dat` in the corpus sits inside a version-5 container, whose members
  of at most one block carry `ctfs-container.md` §2's direct-block tag. A
  reader that admitted this schema without that form would refuse at the
  container layer instead, which is a correct refusal on a different field
  and reads like an unrelated failure. `ctfs-container.md` §2, "The container
  version is one gate of several", is the general statement.

### Extended Fields (flags bitmask)

**Flag bit 0 -- MCR fields.** When set, the block below follows
`recorder_id`. Every field is varint-encoded (no fixed-width integers).
The block's byte layout stays in this document, although its subject is the MCR recorder, because
every reader of `meta.dat` has to parse it to reach the blocks that follow it (bits 1 to 3). What the
fields mean for an MCR recording is specified in `codetracer-specs`.

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
agree, and a `step-map.ns` lookup of line `0` is answered as line `1` (§"`step-map.ns`",
"Reading"). Until 2026-10 the step stream recorded `1` while `step-map.ns` keyed the raw `0`, so a
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
B-tree lookup instead of a scan of the event stream. Its entries are collected
**during recording** by every writer that observes a marker; the image is written
at close (§"When it is written", below), so in the families of
[ctfs-keyed-families.md](ctfs-keyed-families.md) it is a static index (F5).

**Key.** `XXH64(seed = 0, key_bytes)`. For distributed-trace correlation
(`kind = 0`) `key_bytes` is the 24-byte buffer `trace_id_be || span_id_be` —
the 16 big-endian bytes of the trace id followed by the 8 big-endian bytes of
the span id, i.e. wire order, **not** a hex rendering.

An `NSB1` image (ctfs-container.md §8a), Type B, `[payload_offset: u64][payload_len: u64]`
descriptors into the payload region. Type B is required: a collision bucket is variable-length.

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

60 bytes per entry. Within a bucket, entries are sorted by their 24 identity bytes, then `geid`,
and entries equal in both keep the order they were declared in; buckets follow their keys' order,
and the descriptor spans its bucket exactly. A `kind = 1` entry's two times and `thread_id` are 0,
and its `flags` bit 0 is set when the declared direction is `recv` or `receive`.

**Fingerprint.** `key_fingerprint` is `XXH64(seed = 2654435761, key_value)`.

**When it is written.** At close, after `linehits.tc`, and only when the recording declared at
least one marker or span coverage: a recording that declared none has no `corrmark.ns`, which reads
as "not indexed". `markers.dat` / `markers.off` are added when the first label is interned: after
the stream members, and before `meta.dat` when no record has been written yet. Neither reference
writer sets `meta.dat` bit 15.

**The event a boundary marker writes.** A `kind = 1` marker is also an I/O event in `events.dat`
(the debugger's marker list reads it there): `kind` 0, empty content, `step_id` the marker's step,
and as metadata the JSON object

```
{"marker_id":N,"boundary_id":"<label>","direction":"<send|recv>","key_text":"<key_text>","key_value":"<key_value>"
 then, when show_text or show_value is non-empty:  ,"show_text":"<show_text>","show_value":"<show_value>"
 then, when description is non-empty:              ,"description":"<description>"
}
```

on one line, with no spaces, `key_text` defaulting to `key` and `show_text` to `show`. Strings escape
`"` and `\` with a backslash, newline, carriage return and tab as `\n`, `\r`, `\t`, any other byte
below `0x20` as `\u00` and two lowercase hex digits, and copy every other byte as it is. A
`kind = 0` span-coverage marker writes no event.

**The step a marker belongs to.** One value is both the event's `step_id` and the entry's `geid`:
the step the caller names, or by default the last exec record written (0 before the first). No step
is written for a marker.

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
| Encoder, reader, bulk load | Nim `codetracer_trace_writer/corrmark_builder.nim`; Rust `codetracer_trace_writer::corrmark` |
| Writer API (`registerSpanCoverage`, `registerCorrelationMarker*`, `ensureMarkerId`) | `.../codetracer_trace_writer/multi_stream_writer.nim` |
| C ABI (`trace_writer_mark_span_coverage[_hex]`, `trace_writer_mark_correlation[_by_id]`, `trace_writer_ensure_marker_id`) | `.../codetracer_trace_writer_ffi.nim`, declared in `include/codetracer_trace_writer.h` |
| Rust binding | `codetracer-trace-format/codetracer_trace_writer_nim` (`TraceWriter` trait + `NimTraceWriter`) |
| Rust writer and reader | `codetracer_trace_writer::corrmark`, `CtfsTraceWriter` (`ensure_marker_id`, `register_correlation_marker[_by_id]`, `register_span_coverage[_hex]`), `codetracer_trace_reader::correlation_reader` |
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
| `linehits.tc` | location address (a step's `global_position_index`) | Step ids at each source location |
| `memwrites.tc` | memory address | Memory write history; MCR traces only (§"Memory writes are not part of a runtime trace") |
| `memreads.tc` | memory address | Memory read time coordinates |
| `corrmark.ns` | XXH64 of the correlation key | Correlation markers — which distributed-trace spans (and cross-process boundaries) this recording touches |

The rows `slc-mwr.ns`, `slc-mrd.ns` and `threads.ns` were here until 2026-10-07. They are MCR members
and moved to `codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md` §2 with the scope note above;
the per-thread streams are a dense keyed family there (§2.3), not a namespace.

---

## Native Recorder Files (moved)

`filemap.bin`, `platform.bin` and `mmap.bin` are members the native (MCR) recorder writes. On
2026-10-07 they moved, with the scope note at the head of this document, to `codetracer-specs`
`spec/Trace-Files/CTFS-Binary-Format.md` §3.7 to §3.9. `filemap.bin` changed version there
(version 2: a bundled file is found by its entry's index, not by a member name).

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
| 2026-10-04 | **Framed members in a compact container** (`ctfs-container.md` §1f). A chunked compressed table, `step-map.ns` and a seekable-zstd stream keep their format in a compact container with every frame replaced by its decompressed content and every offset that located a frame locating that content; nothing else in the member changes, so a compact container is a function of the full container of the same recording. A writer may write the full profile throughout and convert at close, its threshold measured on the compact members' lengths (§1e). |
| 2026-10-08 | **Member Catalogue, and keyed families instead of stream directories.** New §"Member Catalogue": every member a materialized-trace writer produces, its key shape, lookups, value class and lifecycle, and the abstract family it belongs to ([ctfs-keyed-families.md](ctfs-keyed-families.md)), with the realization written today and the candidates the benchmarks compare. The "Keyed Member" abstraction is added. No member's format changes. |
| 2026-10-07 | **Scope, and the MCR material moved out** (owner decision 2026-10-07; `codetracer-specs/issues/2026-10-07-ctfs-root-directory-growth-landed-without-a-spec-change.md`). A scope note at the head says this document covers the container and the records of the open-source recorders, not how MCR uses CTFS. "Multi-Core Recorder (MCR) Traces" (the member table, cross-OS portability, thread streams, snapshot payloads, checkpoint packing, `cp0.regs`, `cp0.mem`, `cp0.fsbase`, `cp0.maps`, `debug.dat`), "Native Recorder Files" (`filemap.bin`, `platform.bin`, `mmap.bin`) and the `threads.ns` / `slc-mwr.ns` / `slc-mrd.ns` rows of the namespace summary moved to `codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md`; each section left a pointer saying where. The `meta.dat` MCR field block stays, because every `meta.dat` reader parses it. New: the live append order for a variable-size record table. |

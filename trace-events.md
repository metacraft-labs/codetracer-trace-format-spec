# Trace Events

CodeTracer records program execution as a stream of `TraceLowLevelEvent` values. Each event captures one atomic action: a step to a source line, a function call, a variable binding, etc.

## Event Stream Redesign

Interning events (Path, Function, Type, VariableName) are not part of the event stream. They are stored as CTFS interning tables — append-only variable-size record tables with offset indices:

| Old Event | CTFS Table | Record Content |
|-----------|------------|----------------|
| Path (tag 1) | `paths.dat` + `paths.off` | File path (bytes) |
| VariableName (tag 2) | `varnames.dat` + `varnames.off` | Variable name (bytes) |
| Variable (tag 3) | Removed (legacy) | — |
| Type (tag 4) | `types.dat` + `types.off` | TypeKind (u8) + lang_type (bytes) + specific_info (binary) |
| Function (tag 6) | `funcs.dat` + `funcs.off` | global_line_index (varint) + name (bytes) |

The `ensure_*_id()` API in the trace writer appends a record to the corresponding table and returns its index (0-based). The index is used in subsequent events (Step references path via global line index, Call references function by index, Value references variable name and type by index).

### Benefits

1. **Smaller event stream**: Interning events were ~15-20% of stream volume. Removing them reduces stream size and improves compression ratio.
2. **Random-access interning**: The seek-based reader loads interning tables at startup (they're small: typically 1-5MB total). No need to scan the event stream to discover paths/functions/types.
3. **No ordering dependency**: Events no longer require "Path must appear before Step referencing it." The interning tables are self-contained.
4. **Simpler encoder**: The `ensure_*_id()` call either finds an existing entry or appends a new one. No event emission needed.

### Multi-Stream Architecture

Different UI panels need different data, so the event stream is split into separate streams. Each stream is stored in its own CTFS internal file and can be loaded, compressed, and cached independently.

#### 1. Execution Stream (`steps.dat`) — the main timeline, one record per step

This is what the debugger steps through. Each record is compact and fixed-size where possible.

| Tag | Event | Fields | Size |
|-----|-------|--------|------|
| 0 | AbsoluteStep | global_position_index: varint | ~3 bytes |
| 1 | DeltaStep | delta: signed varint | 2 bytes |
| 2 | Raise | exception_type_id: varint, message_len: varint, message: bytes | varies |
| 3 | Catch | exception_type_id: varint | ~2 bytes |
| 4 | ThreadSwitch | thread_id: varint | 2 bytes |
| 5 | ThreadStart | thread_id: varint | 2 bytes (legacy — present in the current canonical Nim writer; the spec intent is to infer this from the first ThreadSwitch to a new thread_id) |
| 6 | ThreadExit | thread_id: varint | 2 bytes (legacy — present in the current canonical Nim writer; the spec intent is to infer this from the last step in a thread) |
| 7 | DeltaColumn | delta: signed varint | 2 bytes (column-aware traces only) |

Step records do not carry `call_key`. To find a step's enclosing call, use proportional (interpolation) search on `calls.dat` — each call record stores `[first_step_id, last_step_id]` ranges. This is O(log log C), typically 2-3 iterations, and avoids doubling the step record size.

Raise is emitted when an exception is raised (before unwinding). Catch is emitted when a `try/except` handler catches the exception.

#### 2. Value Stream (`values.dat`) — variable values, parallel-indexed by step

One record per step, containing all variable values visible at that step. This is the largest stream (values are variable-length and numerous).

| Tag | Event | Fields |
|-----|-------|--------|
| 0 | StepValues | count: varint, then count × (name_id: varint, value: streaming CBOR) |
| 1 | BindVariable | variable_id: varint, place: varint |
| 2 | DropVariable | variable_id: varint |
| 3 | DropVariables | count: varint, ids: [varint] |
| 4 | CellValue | place: varint, value: streaming CBOR |
| 5 | CompoundValue | place: varint, value: streaming CBOR |
| 6 | AssignCell | place: varint, new_value: streaming CBOR |
| 7 | AssignCompoundItem | place: varint, index: varint, item_place: varint |
| 8 | VariableCell | variable_id: varint, place: varint |
| 9 | Assignment | to: varint, pass_by: u8, from: varint |

The value stream is indexed in parallel with the execution stream — record N in `values.dat` corresponds to step N in `steps.dat`. For steps with no variables (possible), the record is empty (just a zero count).

The materialized container stores the value stream as its own seekable CTFS file pair, `values.dat` + `values.idx`, because execution records and value records have fundamentally different sizes and chunking needs. It is gated additively behind the `meta.dat` capability flag `has_value_stream` (bit 10); readers that do not know the bit ignore `values.dat`/`values.idx`.

##### Forward Compatibility and Self-Delimiting Events

Historically, value-stream events in `values.dat` (tags 0–9) were not self-delimiting. Each event kind had an ad-hoc field layout with no universal length header. Consequently, when a reader encountered a tag it did not know (such as tag 9 `Assignment` encountered by a pre-tag-9 binary), it could not determine the byte boundary of that event. Because a guess would mis-frame all subsequent events in the record, the reader had to refuse the entire record. In practice, this caused readers to drop all visible variable values (`StepValues`) for every step containing the new event (measured as 338 value lines silently lost on the 486-step HCR trace, Known-Test-Failures.md:984-993).

To resolve this forward-compatibility hazard (HX-OQ-8):
1. **Wire layout for forward-compatible events (tags ≥ 10):**
   Every value-stream event with tag ≥ 10 is self-delimiting and carries a varint length prefix immediately following the tag byte:
   ```
   [tag: u8 (≥ 10)][payload_len: varint][payload: payload_len bytes]
   ```
2. **Reader specification:**
   - When a reader encounters a known tag ≥ 10, it reads `payload_len` and decodes the event payload.
   - When a reader encounters an **unknown tag ≥ 10**:
     1. It reads `payload_len: varint`.
     2. If `pos + payload_len > record_end`, it reports a truncation error.
     3. Otherwise, it skips `payload_len` bytes (`pos += payload_len`).
     4. It MUST emit a diagnostic warning naming the skipped tag and context (e.g. one-shot warning to stderr naming the tag and count skipped so as to remain loud without flooding logs).
     5. It MUST continue decoding the remaining events in that step's record, thereby **preserving** the visible variable values (`StepValues` tag 0) and any other known events for that step.
   - When a reader encounters an **unknown tag < 10**, it continues to refuse the record with an error, as tags < 10 do not carry the self-delimiting length prefix and cannot be skipped safely.
3. **Rationale and cost trade-off:**
   - **Skip vs. Refuse:** A container capability flag would force an old reader to REFUSE the entire trace, losing all inspection utility. A per-event length prefix allows old readers to SKIP unknown events while preserving known variable values at that step.
   - **Cost:** Exactly 1 varint per event (typically 1 byte for payloads < 128 bytes) on tags ≥ 10.
   - **Backward compatibility:** Existing tags 0–9 in existing containers remain completely valid and unchanged. No existing container needs to be rewritten.


#### 3. Call Stream (`calls.dat`) — call tree records, one per function call

Each record represents a complete function call with entry/exit information.

| Field | Type |
|-------|------|
| function_id | varint |
| parent_key | signed varint (-1 for root) |
| first_step_id | varint |
| last_step_id | varint |
| depth | varint |
| args_count | varint |
| args | args_count × (`varname_id: varint`, `value_len: varint`, `value: streaming CBOR`) |
| return_value_len | varint |
| return_value | streaming CBOR (or the VoidReturn marker) |
| raised_exception_len | varint |
| raised_exception | streaming CBOR, zero-length when the call returned normally |
| children_count | varint |
| children_keys | [varint] × children_count |

**`call_key` is not stored in the record.** It is the record's position in
`calls.dat`, which is what makes the stream addressable by it.

**Each argument is its own entry, and carries its own `varname_id`.** The
framing is the one `values.dat` tag 0 `StepValues` uses for the same job — a
count, then that many `(name, value)` pairs — and for the same reason: an
argument's NAME is part of what the record is for, and a reader assembling a
`(variable, value)` pair has nowhere else to get it.

A writer MUST NOT collapse the arguments into one entry holding a serialized
vector. That form fits the framing and loses the names, so a reader cannot
tell it from a single-argument call and cannot report the names of the
arguments it did receive. It is spelled out because one reference writer did
exactly that, under a synthetic `varname_id` of 0, and the resulting container
was unreadable to the other implementation's reader — which had never been
pointed at one, so nothing said so.

Each call record's *contents* are finalized when the function returns (not at call entry), so they contain complete information. The `call_key` is a sequential index assigned at call entry, and records are stored in `call_key` order — i.e. **call-entry order**, not completion order. A caller therefore precedes its callees in `calls.dat` (a parent's `call_key` is smaller than every child's), even though the parent finalizes last.

##### Assembling an event stream: storage order is not event order

`calls.dat` is stored in call-entry order because that is what makes it
addressable by `call_key`. A tool that **assembles an event stream** from a
container — `ct-print --events` / `--full`, a DAP trace processor, any UI that
interleaves calls with steps — does not present it in that order. It walks the
steps and emits a `call_entry` at each record's `first_step_id` and a
`call_exit` at its `last_step_id`. Those are two different orders, and the
storage sentence above decides neither of them.

Records routinely share a `last_step_id`. Every frame still open when a
recording ends is finalized with `last_step_id = step_count - 1`, so all of them
report the same one; so does a callee whose caller emits no step between the
callee's return and its own. At that point the completion order is simply not in
the data, and the assembler is choosing it.

**An assembler MUST NOT emit a call's `call_exit` before the `call_exit` of any
call in that call's subtree.** A child's `[first_step_id, last_step_id]` range
lies inside its parent's, so a parent's exit is the last event of its subtree.
An ordering that breaks this renders a frame as outliving its own callee — not a
state the program was ever in, and not one a call tree can represent.

**Where the container does not otherwise distinguish them, an assembler MUST
order `call_exit` events by `last_step_id` ascending, then `call_key`
descending.** A parent's `call_key` is always smaller than every descendant's,
so descending `call_key` places the innermost frame first among any set of
records sharing an exit step, which satisfies the requirement above and is
total. It is pinned as a single ordering, rather than left to each assembler,
so that two implementations reading one container produce one event stream.

Two frames that share an exit step and are *not* related by nesting are
indistinguishable in the records; the rule orders them by descending `call_key`
because it must order them somehow, and that order carries no claim about which
finished first.

`call_entry` is the mirror and needs no tie-break beyond the natural one: emit
at `first_step_id`, ascending `call_key`, which is outermost first.

This is spelled out because it is invisible to the checks a conformance suite
reaches for first. Under either order the count of `call_exit` events is right,
every record decodes, and the return value attached to each one is right. Only a
question about *which* call closed first can see it — and a per-fixture list of
expected function names asks that question only of the fixtures someone wrote it
out for. Derive it from `children_keys` instead and it holds for every fixture:
walk the assembled events and refuse any `call_exit` whose call still has an
unclosed child. See `conformance-testing.md` §"Assert names, not counts".

#### 4. IO Event Stream (`events.dat`) — I/O events for the event log pane

| Field | Type |
|-------|------|
| kind | u8 (EventLogKind) |
| step_id | varint (cross-reference to execution stream) |
| metadata | length-prefixed bytes |
| content | length-prefixed bytes |

`step_id` serves as the time coordinate — it references the step in `steps.dat` when this I/O event occurred. The event log pane loads pages from `events.dat` directly without touching the execution or value streams.

#### Stream Summary

| Stream | CTFS File | Purpose | Access Pattern | Typical Record Size |
|--------|-----------|---------|----------------|-------------------|
| Execution | `steps.dat` | Step-by-step timeline | Sequential scan, point lookup | 2-4 bytes |
| Values | `values.dat` | Variable values per step | Point lookup (parallel to exec) | 50-500 bytes |
| Calls | `calls.dat` | Call tree | Random access by call_key | 20-200 bytes |
| IO Events | `events.dat` | I/O event log | Paginated scan | 20-1000 bytes |
| Interning | `*.dat` + `*.off` | Paths, functions, types, names | Loaded at startup | Total 1-5MB |

#### Benefits

1. **Event log loads instantly**: `events.dat` is independent, small, directly paginated
2. **Call tree loads independently**: `calls.dat` is indexed by call_key, no step scanning needed
3. **Step navigation is fast**: `steps.dat` records are tiny (2-4 bytes), so chunks hold thousands of steps
4. **Value loading is on-demand**: `values.dat` is loaded only for the current step's variables
5. **Streams compress independently**: DeltaStep-heavy `steps.dat` compresses extremely well; value-heavy `values.dat` gets different Zstd settings

### Varint IDs

Note that ALL IDs in the redesigned events use **varints** instead of fixed u64 LE:
- variable_id, function_id, type_id, name_id: typically 1-2 bytes (values < 16384)
- global_line_index: typically 2-3 bytes
- place: varint (signed zigzag for negative places)

This dramatically reduces per-event size. Combined with DeltaStep, the average event size drops from ~15 bytes to ~4 bytes.

## Event Variants by Stream

Events are no longer in a single stream. Each event type belongs to exactly one of the four streams described above.

### Execution Stream Events (`steps.dat`)

| Tag | Variant | Fields | Description |
|-----|---------|--------|-------------|
| 0 | `AbsoluteStep` | `global_position_index: varint` | Execution stepped to a source position (full state at chunk/function boundaries) |
| 1 | `DeltaStep` | `delta: signed varint` | Compact step encoding — signed delta from previous step's `global_position_index` |
| 2 | `Raise` | `exception_type_id: varint`, `message_len: varint`, `message: bytes` | Exception raised (before unwinding) |
| 3 | `Catch` | `exception_type_id: varint` | Exception caught by a try/except handler |
| 4 | `ThreadSwitch` | `thread_id: varint` | Execution switched to a different thread |
| 5 | `ThreadStart` | `thread_id: varint` | Legacy — current canonical Nim writer emits this; spec intent is to infer from first ThreadSwitch |
| 6 | `ThreadExit` | `thread_id: varint` | Legacy — current canonical Nim writer emits this; spec intent is to infer from last step |
| 7 | `DeltaColumn` | `delta: signed varint` | Column-only step within the current line; emitted only when the trace's `meta.dat` `FLAG_HAS_COLUMN_AWARE_STEPS` bit is set (see §"Source Location Addressing") |

### Value Stream Events (`values.dat`)

| Tag | Variant | Fields | Description |
|-----|---------|--------|-------------|
| 0 | `StepValues` | `count: varint`, then count × (`name_id: varint`, `value: streaming CBOR`) | All variable values visible at this step |
| 1 | `BindVariable` | `variable_id: varint`, `place: varint` | Bind a variable to a memory place |
| 2 | `DropVariable` | `variable_id: varint` | Drop a single variable |
| 3 | `DropVariables` | `count: varint`, `ids: [varint]` | Drop multiple variables (end of scope) |
| 4 | `CellValue` | `place: varint`, `value: streaming CBOR` | Cell value at a place |
| 5 | `CompoundValue` | `place: varint`, `value: streaming CBOR` | Compound value at a place |
| 6 | `AssignCell` | `place: varint`, `new_value: streaming CBOR` | Assign to cell |
| 7 | `AssignCompoundItem` | `place: varint`, `index: varint`, `item_place: varint` | Assign to compound item |
| 8 | `VariableCell` | `variable_id: varint`, `place: varint` | Associate variable with cell |
| 9 | `Assignment` | `to: varint`, `pass_by: u8`, `from: varint` | Variable assignment or parameter passing |

### Call Stream Records (`calls.dat`)

Call records are not tagged events — each record is a complete function call written when the function returns.

| Field | Type | Description |
|-------|------|-------------|
| `function_id` | varint | Reference to the `funcs.dat` interning table |
| `parent_key` | signed varint (-1 for root) | Parent call's `call_key` |
| `first_step_id` | varint | First step in this call |
| `last_step_id` | varint | Last step in this call |
| `depth` | varint | Call stack depth |
| `args_count` | varint | Number of arguments |
| `args` | args_count × (`varname_id: varint`, `value_len: varint`, `value: streaming CBOR`) | One entry per argument — see §"Call Stream (`calls.dat`)" |
| `return_value_len` | varint | Byte length of `return_value` |
| `return_value` | streaming CBOR (or VoidReturn marker) | Return value |
| `raised_exception_len` | varint | Byte length of `raised_exception`; 0 when none |
| `raised_exception` | streaming CBOR | Present if call ended with unhandled raise |
| `children_count` | varint | Number of child calls |
| `children_keys` | [varint] × children_count | Child call keys |

`call_key` is the record's position in `calls.dat`, not a stored field.

### IO Event Stream Records (`events.dat`)

IO event records are not tagged — each record has a fixed structure.

| Field | Type | Description |
|-------|------|-------------|
| `kind` | u8 (EventLogKind) | Event category |
| `step_id` | varint | Cross-reference to execution stream step |
| `metadata` | length-prefixed bytes | Event metadata |
| `content` | length-prefixed bytes | Event content |

### Interning Tables (unchanged)

| Old Event | CTFS Table | Record Content |
|-----------|------------|----------------|
| Path (tag 1) | `paths.dat` + `paths.off` | File path (bytes) |
| VariableName (tag 2) | `varnames.dat` + `varnames.off` | Variable name (bytes) |
| Variable (tag 3) | Removed (legacy) | — |
| Type (tag 4) | `types.dat` + `types.off` | TypeKind (u8) + lang_type (bytes) + specific_info (binary) |
| Function (tag 6) | `funcs.dat` + `funcs.off` | global_line_index (varint) + name (bytes) |

### Removed Events

The following events from the legacy single-stream format have been removed or subsumed:

| Old Tag | Old Variant | Disposition |
|---------|-------------|-------------|
| 7 | `Call` | Subsumed by call records in `calls.dat` |
| 8 | `Return` | Subsumed by call records in `calls.dat` (return_value field) |
| 25 | `VoidReturn` | Subsumed by VoidReturn marker in call records |
| 9 | `Event` | Moved to `events.dat` as IO event records |
| 10 | `Asm` | Removed (unused by current recorders) |
| 20 | `ThreadStart` | Removed (can be inferred from first ThreadSwitch to a new thread_id) |
| 21 | `ThreadExit` | Removed (can be inferred from last step in a thread) |
| 23 | `DropLastStep` | Removed (no longer needed — multi-stream writer can correct in place) |

## Key Sub-types

### ValueRecord (tagged enum, serialized as CBOR)

| Variant | Fields |
|---------|--------|
| `Int` | `i: i64`, `type_id: usize` |
| `Float` | `f: f64`, `type_id: usize` |
| `Bool` | `b: bool`, `type_id: usize` |
| `String` | `text: String`, `type_id: usize` |
| `Sequence` | `elements: Vec<ValueRecord>`, `is_slice: bool`, `type_id: usize` |
| `Tuple` | `elements: Vec<ValueRecord>`, `type_id: usize` |
| `Struct` | `field_values: Vec<ValueRecord>`, `type_id: usize` |
| `Variant` | `discriminator: String`, `contents: ValueRecord`, `type_id: usize` |
| `Reference` | `dereferenced: ValueRecord`, `address: u64`, `mutable: bool`, `type_id: usize` |
| `Raw` | `r: String`, `type_id: usize` |
| `Error` | `msg: String`, `type_id: usize` |
| `None` | `type_id: usize` |
| `Cell` | `place: i64` |
| `BigInt` | `b: Vec<u8>` (base64 in JSON), `negative: bool`, `type_id: usize` |
| `Char` | `c: char`, `type_id: usize` |
| `ValueRef` | `ref_id: u32` |

All compound variants (`Sequence`, `Tuple`, `Struct`, `Variant`, `Reference`) carry an optional `ref_id: Option<u32>` field used for cyclic value encoding (see below). The field is omitted for leaf values.

## Cyclic Value Encoding

Languages like Python, Ruby, and JavaScript allow cyclic object references:

```python
a = []
a.append(a)  # a[0] is a itself
```

The `ValueRecord` type supports cycles through two mechanisms:

### Value Reference (ValueRecord variant)

A new `ValueRecord` variant:

```
ValueRef { ref_id: u32 }
```

Tag in CBOR: map with `{"kind": "ValueRef", "ref_id": N}`

### Encoding Protocol

During value encoding, the encoder maintains a mapping from object identity (memory address or language-level `id()`) to a monotonically increasing reference ID:

```
seen: HashMap<ObjectId, u32>
next_ref_id: u32 = 0

function encode(obj):
    if obj.id in seen:
        return ValueRef { ref_id: seen[obj.id] }
    
    ref_id = next_ref_id
    next_ref_id += 1
    seen[obj.id] = ref_id
    
    // Encode normally (recurse into children)
    record = encode_fields(obj)
    record.ref_id = ref_id  // Tag this node with its ref_id for the decoder
    return record
```

### Decoding Protocol

The decoder builds the inverse mapping:

```
refs: HashMap<u32, ValueRecord>

function decode(record):
    if record is ValueRef:
        return refs[record.ref_id]  // Return previously decoded node
    
    refs[record.ref_id] = record  // Register before recursing (handles cycles)
    decode children...
    return record
```

### Wire Format

In the CBOR payload of value events, each `ValueRecord` that might be part of a cycle carries an optional `ref_id` field:

```json
{
  "kind": "Struct",
  "ref_id": 0,
  "field_values": [
    {"kind": "ValueRef", "ref_id": 0}
  ],
  "type_id": 5
}
```

The `ref_id` field is omitted for leaf values (Int, Float, Bool, String, None) that cannot be part of cycles.

### Impact on Existing Traces

- Old traces without `ref_id` fields remain readable (the field is optional)
- Old decoders encountering `ValueRef` treat it as an unknown variant (graceful degradation)
- The `ref_id` adds 0 bytes for leaf values and ~5-10 bytes for compound values (CBOR map entry)

## Streaming Value Encoder API

### The Problem

Current flow: `Python object -> ValueRecord tree (heap allocated) -> CBOR bytes -> output buffer`

Each value encoding allocates `Vec<ValueRecord>` for sequences, `Box<ValueRecord>` for variants/references, `String` for text values -- all thrown away after CBOR serialization. For a moderately complex object (e.g. a 50-field struct containing nested lists), a single value event may allocate dozens of heap objects that exist only to be serialized and immediately freed.

### The Solution

Direct flow: `Python object -> CBOR bytes in output buffer` (single pass, zero intermediate allocation)

The recorder walks the object graph in the target language (Python, Ruby, JS, etc.) and calls streaming encoder methods that append CBOR bytes directly to the output buffer. A hash table (keyed by object identity) tracks visited objects for cycle detection. No intermediate `ValueRecord` tree is constructed.

### C FFI API

```c
// Start encoding a value event (tag 5: variable_id + CBOR payload)
void trace_writer_begin_value(trace_writer_t w, uint64_t variable_id);

// Compound value openers -- append CBOR map/array header, register ref_id
void trace_value_begin_struct(trace_writer_t w, uint32_t type_id, 
                               uint32_t field_count, uint32_t ref_id);
void trace_value_begin_sequence(trace_writer_t w, uint32_t type_id,
                                 uint32_t element_count, int is_slice,
                                 uint32_t ref_id);
void trace_value_begin_tuple(trace_writer_t w, uint32_t type_id,
                              uint32_t element_count, uint32_t ref_id);
void trace_value_begin_variant(trace_writer_t w, uint32_t type_id,
                                const char* discriminator, uint32_t ref_id);
void trace_value_begin_reference(trace_writer_t w, uint32_t type_id,
                                  uint64_t address, int is_mutable,
                                  uint32_t ref_id);

// Leaf value writers -- append complete CBOR value
void trace_value_write_int(trace_writer_t w, int64_t value, uint32_t type_id);
void trace_value_write_float(trace_writer_t w, double value, uint32_t type_id);
void trace_value_write_bool(trace_writer_t w, int value, uint32_t type_id);
void trace_value_write_string(trace_writer_t w, const char* text, uint32_t type_id);
void trace_value_write_none(trace_writer_t w, uint32_t type_id);
void trace_value_write_raw(trace_writer_t w, const char* text, uint32_t type_id);
void trace_value_write_error(trace_writer_t w, const char* msg, uint32_t type_id);

// Cycle reference -- append ValueRef record
void trace_value_write_ref(trace_writer_t w, uint32_t ref_id);

// Close compound value
void trace_value_end(trace_writer_t w);

// Finish the value event (closes the CBOR payload, writes to output stream)
void trace_writer_end_value(trace_writer_t w);
```

### Scalars are written in CBOR's own types

A leaf value goes into `values.dat` as the CBOR type that denotes it:
`trace_value_write_int` writes a CBOR integer, `trace_value_write_bool` a CBOR
boolean, and **`trace_value_write_float` a CBOR float** — half, single or
double, whichever round-trips the value, since a reader decodes all three to
the same number.

**A float is NOT written as text.** It is stated because it was: the JSON
projection of a `Float` carries its value as a decimal string — JSON cannot
express an infinity or a NaN, and `"inf"`, `"+inf"`, `"-inf"` and `"nan"` are
the spellings it uses — and one reference implementation shared a single
serializer definition between the two formats, so the text form reached
`values.dat` as well. CBOR encodes the non-finites natively and needs no such
workaround, and a binary format should not inherit a text format's limits.

The symptom was that the other implementation's reader refused every container
carrying a float, with *"invalid type: floating point `2.5`, expected a
string"* — but only because the two forms are different CBOR types. Had both
been text, the divergence would have been invisible.

A reader SHOULD accept any CBOR numeric type where a float is specified.
Writers choose the narrowest float that round-trips, and an implementation may
encode a whole-numbered float as an integer; refusing those would make a value
unreadable on the strength of how its producer chose to pack it.

### Usage Example (Python recorder pseudocode)

```python
def encode_value(writer, obj, seen):
    obj_id = id(obj)
    if obj_id in seen:
        trace_value_write_ref(writer, seen[obj_id])
        return
    
    ref_id = len(seen)
    seen[obj_id] = ref_id
    
    if isinstance(obj, int):
        trace_value_write_int(writer, obj, ensure_type_id(writer, "Int"))
    elif isinstance(obj, list):
        type_id = ensure_type_id(writer, "List")
        trace_value_begin_sequence(writer, type_id, len(obj), False, ref_id)
        for elem in obj:
            encode_value(writer, elem, seen)
        trace_value_end(writer)
    elif isinstance(obj, dict):
        # ... similar
```

### Internal Implementation

The streaming encoder maintains a stack of compound values being built:

```nim
type
  CompoundFrame = object
    kind: CompoundKind  # struct, sequence, tuple, variant, reference
    expectedChildren: int
    writtenChildren: int

  StreamingValueEncoder = object
    buffer: ptr SafeBuffer     # Points to the TraceWriter's event buffer
    stack: array[32, CompoundFrame]  # Fixed-size stack (max nesting depth 32)
    stackDepth: int
    payloadStart: int          # Position where CBOR payload began (for length patching)
```

When `begin_struct` is called:
1. Write CBOR map header (field_count + 2 for "kind" and "type_id" and optionally "ref_id")
2. Write "kind" key + variant name
3. Write "type_id" key + value
4. If ref_id != 0: write "ref_id" key + value
5. Push frame onto stack

When leaf value writers are called within a compound:
- Write the CBOR field name (for structs) or just the value (for sequences)
- Increment writtenChildren

When `end` is called:
- Pop frame, verify writtenChildren == expectedChildren

When `end_value` is called:
- Verify stack is empty
- The CBOR payload is complete in the buffer

### Key Properties

1. **Zero intermediate allocation**: No ValueRecord tree, no Vec, no Box, no String copying
2. **Single pass**: Bytes are appended during the object walk, not after
3. **Bounded stack**: Fixed 32-entry stack (max nesting depth) -- no heap allocation
4. **Cycle safe**: The `seen` hash table is maintained by the caller (the recorder), not by the encoder
5. **Compatible**: The CBOR output is byte-identical to what the current two-pass approach produces

### TypeRecord

- `kind: TypeKind` (u8 enum -- Seq, Set, HashSet, OrderedSet, Array, Varargs, Struct, Int, Float, String, CString, Char, Bool, Literal, Ref, Recursion, Raw, Enum, Enum16, Enum32, C, TableKind, Union, Pointer, Error, FunctionKind, TypeValue, Tuple, Variant, Html, None, NonExpanded, Any, Slice)
- `lang_type: String` -- the language-level type name
- `specific_info: TypeSpecificInfo` -- one of:
  - `None`
  - `Struct { fields: Vec<FieldTypeRecord> }` where each field has `name: String`, `type_id: usize`
  - `Pointer { dereference_type_id: usize }`

### EventLogKind (u8 enum)

| Value | Kind | Description |
|-------|------|-------------|
| 0 | Stdout | Standard output write |
| 1 | Stderr | Standard error write |
| 2 | Stdin | Standard input read |
| 3 | FileWrite | File write |
| 4 | FileRead | File read |
| 5 | NetworkSend | Network send |
| 6 | NetworkRecv | Network receive |
| 7 | Error | Uncaught exception / error |
| 8 | Log | Application log message |

Removed unused kinds (ReadDir, OpenDir, CloseDir, Socket, Open — these can be re-added when recorders actually emit them).

## Raw Byte Fidelity

Trace recorders must preserve the exact bytes present in memory for all captured values. The trace format must not transform, validate, or sanitize value data during recording.

### Requirements

1. **No UTF-8 validation**: String values may contain invalid UTF-8, surrogate pairs, or arbitrary byte sequences. The recorder captures the raw bytes without validation or replacement.

2. **No null termination**: Byte buffers may contain embedded null bytes (`\0`). The recorder uses length-prefixed encoding, not null-terminated C strings.

3. **No encoding conversion**: The recorder does not convert between encodings (e.g., Latin-1 to UTF-8). The original bytes are stored as-is.

4. **Byte string encoding**: All captured values use CBOR **byte strings** (major type 2) in the wire format, not text strings (major type 3). Text strings (major type 3) are reserved for trace metadata (field names, type names, function names) where UTF-8 validity is guaranteed by the recorder itself.

### Wire Format

In the CBOR payload of value events:

| Field | CBOR type | Reason |
|-------|-----------|--------|
| ValueRecord string values (`text` in String, `r` in Raw) | Byte string (type 2) | May contain invalid UTF-8 |
| Variable names | Text string (type 3) | Controlled by recorder, always valid UTF-8 |
| Function names | Text string (type 3) | Controlled by language runtime |
| Type names | Text string (type 3) | Controlled by language runtime |
| CBOR map keys ("kind", "type_id", etc.) | Text string (type 3) | Fixed vocabulary |

### C FFI Implications

The streaming value encoder C API uses `const uint8_t* data, size_t len` pairs instead of `const char*` for value data:

```c
// Old (wrong — truncates at null, assumes UTF-8):
void trace_value_write_string(trace_writer_t w, const char* text, uint32_t type_id);

// New (correct — preserves arbitrary bytes):
void trace_value_write_string(trace_writer_t w, const uint8_t* data, size_t len, uint32_t type_id);
void trace_value_write_raw(trace_writer_t w, const uint8_t* data, size_t len, uint32_t type_id);
```

### Display Layer

The debugger UI (not the recorder) is responsible for:
- Attempting UTF-8 decoding for display
- Showing hex escape sequences for non-UTF-8 bytes
- Indicating encoding issues (e.g., "contains invalid UTF-8")

This separation ensures the trace is a faithful record of program state, regardless of how the UI chooses to present it.

### Known Issues in Current Recorders

These must be fixed as part of the byte fidelity audit:

| Recorder | Issue | Fix |
|----------|-------|-----|
| Python (PyO3) | `value.extract::<String>()` does UTF-8 validation, may reject or transform surrogates | Use `value.extract::<Vec<u8>>()` or `value.as_bytes()` |
| Python (PyO3) | `value.str()` calls Python `str()` which may transform repr | Capture `bytes` representation where possible |
| C FFI | `const char*` parameters truncate at `\0` | Use `(const uint8_t*, size_t)` pairs |
| Nim FFI | `cstring` parameters truncate at `\0` | Use `(ptr byte, cint)` pairs |
| Ruby | `to_s` may transcode strings | Use `bytes` method to get raw encoding |
| All | CBOR text string (type 3) for values | Switch to byte string (type 2) |

## Split-Binary Encoding (default)

The split-binary format uses compact binary encoding for event envelopes (fixed-size fields) and falls back to CBOR only for dynamic payloads (ValueRecord, TypeRecord, AssignmentRecord). This gives better compression ratios and faster decoding than pure CBOR.

### Wire format per event

Each event is encoded as a concatenation of:

1. **Tag byte** (1 byte): variant index 0-23
2. **Fixed fields**: little-endian integers at their natural width
3. **Strings**: 4-byte LE length prefix + UTF-8 bytes
4. **Dynamic payloads**: 4-byte LE CBOR length prefix + CBOR bytes

### Per-variant encoding

| Tag | Variant | Encoding | Total size |
|-----|---------|----------|------------|
| 0 | Step | `tag(1) + path_id(u64 LE) + line(i64 LE)` | 17 bytes |
| 1 | Path | `tag(1) + str` | 5 + len |
| 2 | VariableName | `tag(1) + str` | 5 + len |
| 3 | Variable | `tag(1) + str` | 5 + len |
| 4 | Type | `tag(1) + cbor(TypeRecord)` | 5 + cbor_len |
| 5 | Value | `tag(1) + variable_id(u64 LE) + cbor(ValueRecord)` | 13 + cbor_len |
| 6 | Function | `tag(1) + path_id(u64 LE) + line(i64 LE) + str(name)` | 21 + name_len |
| 7 | Call | `tag(1) + function_id(u64 LE) + cbor(args)` | 13 + cbor_len |
| 8 | Return | `tag(1) + cbor(ValueRecord)` | 5 + cbor_len |
| 9 | Event | `tag(1) + kind(u8) + str(metadata) + str(content)` | 10 + meta_len + content_len |
| 10 | Asm | `tag(1) + count(u32 LE) + [str]*count` | 5 + sum(4 + line_len) |
| 11 | BindVariable | `tag(1) + variable_id(u64 LE) + place(i64 LE)` | 17 bytes |
| 12 | Assignment | `tag(1) + cbor(AssignmentRecord)` | 5 + cbor_len |
| 13 | DropVariables | `tag(1) + count(u32 LE) + [variable_id(u64 LE)]*count` | 5 + 8*count |
| 14 | CompoundValue | `tag(1) + place(i64 LE) + cbor(ValueRecord)` | 13 + cbor_len |
| 15 | CellValue | `tag(1) + place(i64 LE) + cbor(ValueRecord)` | 13 + cbor_len |
| 16 | AssignCompoundItem | `tag(1) + place(i64 LE) + index(u64 LE) + item_place(i64 LE)` | 25 bytes |
| 17 | AssignCell | `tag(1) + place(i64 LE) + cbor(ValueRecord)` | 13 + cbor_len |
| 18 | VariableCell | `tag(1) + variable_id(u64 LE) + place(i64 LE)` | 17 bytes |
| 19 | DropVariable | `tag(1) + variable_id(u64 LE)` | 9 bytes |
| 20 | ThreadStart | `tag(1) + thread_id(u64 LE)` | 9 bytes |
| 21 | ThreadExit | `tag(1) + thread_id(u64 LE)` | 9 bytes |
| 22 | ThreadSwitch | `tag(1) + thread_id(u64 LE)` | 9 bytes |
| 23 | DropLastStep | `tag(1)` | 1 byte |

Where `str` means: `length(u32 LE) + utf8_bytes`, and `cbor(T)` means: `cbor_length(u32 LE) + cbor_bytes`.

## Source Location Addressing

Step events address source locations through a single varint, `global_position_index`. The index is a linear address into a *per-trace global position space* that subsumes the older line-only `global_line_index` scheme.

### The address is a prefix sum, and nothing else

There is exactly one scheme. An address is a position in a per-trace space
built by concatenating files in file-id order; it is **not** a packing of
`(file_id, line)` into disjoint bit-fields.

A bit-field packing — `(path_id << 32) | line`, or any other fixed stride —
is not this format. It is excluded on three grounds, and readers must not
accept it:

* It bounds the trace. A shift of 32 caps both the file count and the lines
  per file, and neither limit belongs in a container whose whole point is to
  hold whatever a program did.
* It defeats the varint. §"Varint IDs" budgets `global_line_index` at 2-3
  bytes and the average event at ~4. A shifted address costs 5 bytes for
  every file after the first, because the file id sits above bit 32 — a
  ~50% inflation of the step stream on any multi-file trace. A prefix-sum
  address stays proportional to the program's total line count, which is
  small enough to varint well and to delta-encode between neighbouring
  steps.
* It cannot be told apart from a real address. Both schemes produce
  well-formed integers from the same field, a container records no
  discriminator, and both inverses always return a plausible `(file, line)`.
  A reader handed the wrong one does not fail — it answers a location that
  was never in the trace.

### Per-File Contiguous Integer Ranges

Each source file registered in the trace's path interning table (`paths.dat`) is assigned a contiguous half-open integer range `[file_base, file_base + file_size)` in the global position space. The ranges are laid out in file-id order with no gaps:

```
file 0:  [0,                              file_size_0)
file 1:  [file_size_0,                    file_size_0 + file_size_1)
file 2:  [file_size_0 + file_size_1,      ...)
...
```

`file_size` depends on the addressing mode:

| Addressing mode | `file_size` | Address semantics |
|-----------------|-------------|-------------------|
| Line-only (legacy / column extension off) | `line_count` | Each integer addresses one line. The decoder maps `global_position_index → (file, line)`. |
| Line + column (column extension on) | `sum(line_lengths)` | Each integer addresses one (line, column) pair with 1-based columns. The decoder maps `global_position_index → (file, line, column)`. |

`line_lengths[i]` is the number of addressable column positions on line `i`. Implementations are free to clamp `line_lengths[i]` to `actual_column_count_of_line_i + 1` so the trailing "one past EOL" position (used for end-of-line breakpoints, statement end markers, and the implicit newline) gets its own address.

There is one sizing rule, not two: **`file_size` is the number of
positions the file has.** The rows above differ only in what a position
*is* — an addressable column in one mode, a line in the other — so the
counts are in different units (bytes on one axis, lines on the other)
while the rule that consumes them is the same. Implementations MUST
compute both through the same function. A file sized by one rule in the
writer and another in a reader shifts the base of every file after it,
and a position then resolves into the wrong file at a line number that is
in range and therefore unrefusable.

`file_size` MUST NOT be zero: a file with no positions shares its base
with the next file, and the two are indistinguishable at decode.

**Where the sizes come from.** In line + column mode the per-line table
in each `paths.dat` record carries them. In line-only mode the file's
`line_count` is carried by the same record when `meta.dat` bit 14
(`FLAG_HAS_LINE_COUNT_TABLE`) is set — see
[internal-files.md](internal-files.md) §"`paths.dat` line-count table".
A line-only trace *without* that bit records no sizes at all, and a
reader can only apply a convention its writer also applied; the
conventional value is `100000` addresses per file. Writers SHOULD set bit
14. The convention is not merely unrecorded but unsound above its own
ceiling: a file with more than `100000` lines addresses positions inside
the *next* file's range, which is a well-formed address of a location
that was never recorded.

Because such an address is in-space, **no reader can detect it.** A
writer that records `line_count` therefore MUST refuse a step whose line
exceeds it, at the point of registration, where the file's size is known.

### Decoding `global_position_index`

Given a varint `p` and the per-file cumulative-size table, the decoder resolves `(file, line, column)` as follows:

1. **Binary-search the file table** on cumulative `file_base` to find the file `f` such that `file_base[f] ≤ p < file_base[f] + file_size[f]`. Cost: `O(log F)` where `F` is the number of registered files.
2. **Compute the in-file offset** `q = p - file_base[f]`.
3. **Line-only mode:** `line = q + 1` (lines are 1-based). Done. The inverse is `global_index(file_id, line) = prefix_sums[file_id] + (line - 1)` — see [internal-files.md](internal-files.md) §"Global Line Index", which carries the encode side and a note on the off-by-one this pair had until 2026-09.
4. **Line + column mode:** Binary-search the file's per-line cumulative-length table to find the line `l` such that `line_base[l] ≤ q < line_base[l] + line_lengths[l]`. Then `column = q - line_base[l] + 1`. Cost per resolution: `O(log L)` where `L` is the number of lines in file `f`.

Total decode cost is `O(log F + log L)` per step. Cumulative-sum tables are computed once at trace open (typically a few hundred kilobytes for a large multi-file trace) and cached in memory; per-step lookups are then two binary searches and add no I/O.

### Encoding Rules for Step Records

Step records reference source locations through `global_position_index` (absolute) or signed deltas of `global_position_index` (delta). The compact variants are documented in §"Compact Step Encoding".

A delta within the same line moves through column positions only (small magnitude, typically ±1 to ±N where N is the line length). A delta that crosses a line boundary jumps by at least `current_column + 1`. A delta that crosses a file boundary jumps by potentially millions and is normally promoted to an AbsoluteStep.

### Back-Compatibility

The position-index scheme is a strict superset of the legacy line-index scheme:

* Pre-extension traces have no per-line offset table. Their step records' `global_position_index` values are interpreted as `global_line_index` (each integer addresses one line). Readers surface the column slot as `None` (Rust `Option<u32>`, Nim `Option[uint32]`).
* The presence of the per-line offset table — and therefore the column-aware decoding — is signalled by a `meta.dat` flag bit. See §"Reader Behaviour and Back-Compat".
* A line-only trace's per-file `line_count` table is signalled by its own bit (14, `FLAG_HAS_LINE_COUNT_TABLE`) and is independent of the column extension. It changes no step record and no address arithmetic — only where the `file_size` values in §"Per-File Contiguous Integer Ranges" come from. A trace without it is sized by the `100000`-per-file convention, as every line-only trace was before the bit existed.

The on-wire encoding of `global_position_index` (varint) is identical to the legacy `global_line_index` (varint). The interpretation changes; the bytes do not.

## Compact Step Encoding

Step events use two variants for efficient encoding. Both variants address source locations through `global_position_index` (see §"Source Location Addressing"). When the column extension is disabled, `global_position_index ≡ global_line_index` and the legacy line-only decoding applies.

### AbsoluteStep (Tag 0)

Used at function entry, after large jumps, or when the delta would exceed DeltaStep's range.

```
[Tag: 0x00] [global_position_index: varint]
Total: 3-4 bytes typical (1 tag + 2-3 varint bytes)
```

### DeltaStep (Tag 1)

Used for consecutive steps within the same function or nearby code. Stores the signed delta from the previous step's `global_position_index`.

```
[Tag: 0x01] [delta: signed varint]
Total: 2 bytes typical (1 tag + 1 varint for delta ±63)
```

The signed varint uses zigzag encoding: `(delta << 1) ^ (delta >> 63)`, then unsigned LEB128.

| Delta range | Varint size | Total event size |
|-------------|------------|-----------------|
| ±63 | 1 byte | 2 bytes |
| ±8191 | 2 bytes | 3 bytes |
| ±1048575 | 3 bytes | 4 bytes |
| Larger | Use AbsoluteStep | 3-4 bytes |

### Encoding Rules

1. The first step in a trace is always AbsoluteStep
2. After a Call event, the next step is AbsoluteStep (new function context)
3. After a Return event, the next step is AbsoluteStep (returning to caller)
4. All other steps use DeltaStep if the delta fits in 3 varint bytes (±1048575), otherwise AbsoluteStep

### Column Encoding — `DeltaColumn` (chosen)

The column extension introduces a second axis (column) into step records. Two on-wire encodings were considered; the empirical benchmark in `tracing-formats-benchmarks` (`results/ctfs_column_extension/REPORT.md`, P6.2, 2026-06-10) **selected the separate `DeltaColumn` variant (Candidate A)** as the on-wire encoding. The losing candidate is documented below for archival reasons.

#### Chosen — separate `DeltaColumn` variant (Tag 0x07)

Adds a third step-event tag dedicated to column-only motion.

```
[Tag: 0x07] [delta: signed varint]
Total: 2 bytes typical (1 tag + 1 varint for column delta ±63)
```

Semantics:

* `DeltaColumn` advances the cursor's column within the current line. Line is unchanged.
* When a `DeltaStep` (Tag 0x01) is decoded and it crosses a line boundary, the cursor's column is reset to column 1 of the new line. A subsequent `DeltaColumn` then advances within that new line.
* A `DeltaStep` that does **not** cross a line boundary leaves the column at its previous value. (Encoders are free to emit either a `DeltaColumn` or a `DeltaStep` with a small column-only delta in that case; the result is identical because `global_position_index` is one-dimensional.)
* An `AbsoluteStep` carries the full `(line, column)` through its varint and resets both axes.

Wire-format properties:

* **Tag allocation.** Tags 0x00-0x06 are already taken (AbsoluteStep, DeltaStep, Raise, Catch, ThreadSwitch, ThreadStart, ThreadExit — the last two are present in the current canonical Nim writer even though they are marked for future removal in §"Sketched Removals"). `DeltaColumn` is allocated to tag **0x07**. This avoids any conflict with existing event types and keeps the column extension entirely additive on the wire.
* **Column-only step cost:** 2 bytes (1 tag + 1 zigzag varint).
* **No size change on existing events.** `DeltaStep` and `AbsoluteStep` byte layouts are unchanged. Existing column-unaware readers see the new tag, fail the unknown-flag-bit check in `meta.dat`, and refuse to open the trace cleanly rather than misdecoding.

#### Rejected — extended `DeltaStep` with column-delta flag bit (after P6.2 benchmark)

**Status:** rejected after benchmark in `tracing-formats-benchmarks` P6.2. Empirically ~2× more expensive than the chosen `DeltaColumn` variant on every measured corpus (raw bytes/step +84% to +102% vs the line-only baseline, vs +1.9% to +20.3% for `DeltaColumn`). Retained here for archival / spec-history reasons only — writers MUST NOT emit this format; readers MUST NOT recognise it.

Folds the column delta into the existing `DeltaStep` event by prefixing the body with a single-byte flag header.

```
[Tag: 0x01] [flag_byte: u8] [delta_line: signed varint] [delta_column: signed varint?]
```

Semantics:

* `flag_byte` low bit (`0x01`) — when set, `delta_column` follows after `delta_line`.
* `flag_byte` bits 1-7 — reserved; must be zero in writers, must be tolerated as zero by readers.
* `delta_line` is the signed line delta (same zigzag varint as the line-only baseline).
* `delta_column`, when present, is the signed column delta from the previous step's column.
* `AbsoluteStep` (Tag 0x00) carries a full `global_position_index` varint; the (line, column) decomposition happens at decode time as in §"Source Location Addressing".

Wire-format properties:

* **Single event per step regardless of axis.** No separate column-only event; a same-line step emits `delta_line = 0` and `delta_column ≠ 0`.
* **Cost of a column-aware step:** `1 (tag) + 1 (flag) + varint(delta_line) + varint(delta_column)`. For a typical `delta_line = 0, delta_column = ±1` step this is **4 bytes** (vs 2 bytes for the line-only `DeltaStep` baseline) — a 2-byte regression for same-line steps.
* **Cost of a line-only step (column extension on, no column change):** `1 (tag) + 1 (flag = 0) + varint(delta_line)` = **3 bytes** (vs 2 bytes baseline) — a 1-byte regression.
* **Breaking change.** Pre-extension readers expect `tag(0x01) + signed_varint` directly; this format inserts a `flag_byte` before the varint. An old reader will misdecode the flag byte as the start of a varint (zigzag value 0 for `flag_byte = 0`, or some other small value) and then misalign permanently. The `meta.dat` column-extension flag (see §"Reader Behaviour and Back-Compat") MUST be checked before parsing the execution stream; readers that don't know the flag MUST refuse to open the trace.

#### Comparison (archival)

| Property | `DeltaColumn` (chosen) | Extended `DeltaStep` (rejected) |
|----------|------------------------|--------------------------------|
| Wire compatibility with line-only readers | Additive — new tag 0x07; old readers reject cleanly via `meta.dat` bit 4 | Breaking — flag-byte prefix shifts every `DeltaStep` body |
| Column-only step cost | 2 bytes (separate event) | 4 bytes (flag + line delta = 0 + column delta) |
| Line-only step cost | 2 bytes (unchanged) | 3 bytes (flag overhead) |
| Mixed line+column step cost | 4 bytes (two events) | 4 bytes (one event) |
| Encoder complexity | Low — pick one of two tags per step | Medium — flag-byte assembly per event |
| Decoder complexity | Low — tag dispatch | Medium — flag-byte parsing per event |

**Empirical result (P6.2, synthetic 100k-step corpora, raw bytes/step vs line-only baseline):**

| Corpus | `DeltaColumn` | Extended `DeltaStep` |
|--------|--------------:|---------------------:|
| `python_co_positions`   | +4.6%  | +98.3%  |
| `js_sourcemap_minified` | +1.9%  | +101.9% |
| `cpp_dwarf`             | +18.3% | +86.0%  |
| `rust_dwarf`            | +20.3% | +84.4%  |
| `cairo`                 | +12.2% | +92.0%  |

`DeltaColumn` wins on every corpus shape, including the within-line-heavy `js_sourcemap_minified` end and the line-transition-heavy `rust_dwarf` / `cpp_dwarf` end. See `tracing-formats-benchmarks/results/ctfs_column_extension/REPORT.md` for the full table and methodology.

### paths.dat per-line offset table

The column extension adds a per-line length table so the decoder can resolve `(line, column)` from an in-file offset. Two layouts are under consideration.

#### Layout A — inline per-file table in `paths.dat`

Each path record in `paths.dat` is extended to carry its per-line length table after the existing path bytes:

```
paths.dat record (column-extension on):
  path_len: varint
  path_bytes: [u8] × path_len
  line_count: varint
  line_lengths: [varint] × line_count       (zigzag-encoded delta from previous line length)
```

Notes:

* `line_lengths[0]` is encoded as an absolute zigzag varint; subsequent entries are deltas from `line_lengths[i-1]`. The empirical distribution of line lengths in source code is heavily centred around 20-80 chars with low variance line-to-line, so delta-encoding is expected to halve the bytes per line vs raw absolute lengths.
* Pre-extension `paths.dat` records have no `line_count` field. Readers detect the extension via the `meta.dat` flag (see §"Reader Behaviour and Back-Compat") and parse the extra fields only when the flag is set.
* `paths.off` (offset companion) keeps pointing to record starts; no schema change.

#### Layout B — companion stream `paths.lineoffsets.dat`

The line-length tables are pulled out of `paths.dat` into a parallel CTFS internal file:

```
paths.lineoffsets.dat:
  For file_id 0..F-1, in file-id order:
    line_count: varint
    line_lengths: [varint] × line_count    (zigzag-delta encoded)

paths.lineoffsets.off (optional):
  offset of file_id i's record in paths.lineoffsets.dat
```

Notes:

* `paths.dat` itself is unchanged; readers that ignore the column extension don't see any extra bytes in their I/O path.
* Reading source-file paths (a frequent operation in the UI, e.g. populating a file picker) doesn't pull line-length data into cache.
* Costs one extra CTFS internal file per trace.

#### Choice — Layout A

**Layout A is the chosen layout.** Reasons:

1. `paths.dat` is already loaded at trace open as part of the interning-table warm-up; appending per-line data adds I/O exactly when the reader is already paying it.
2. The total per-line data is small (a few hundred KB for a 10K-line trace), estimated <1% of total trace size — cache pressure is negligible.
3. Fewer CTFS internal files = simpler container layout, simpler sharded-trace logic (see `ctfs-container.md`).

Layout B remains documented as a fallback. If a real column-aware recorder (P6.4-P6.6 in the campaign that landed this extension) surfaces measurable cache-thrash on path-only lookups (e.g. UI file picker), revisit.

### Reader Behaviour and Back-Compat

The column extension is signalled by a new flag bit in `meta.dat`. See `internal-files.md` §"Metadata (meta.dat)" for the current flag-byte layout and for which bits are allocated; every unallocated bit is reserved with strict rejection.

**Allocation:** bit 4 — `FLAG_HAS_COLUMN_AWARE_STEPS`. When set:

* Per-line offset tables are present in `paths.dat` (Layout A — see §"paths.dat per-line offset table" and `internal-files.md` §"paths.dat Layout A").
* Step records' `global_position_index` addresses `(line, column)` pairs, not lines.
* The execution stream MAY contain `DeltaColumn` (tag 0x07) records that advance the cursor's column within the current line.

When `FLAG_HAS_COLUMN_AWARE_STEPS` is clear (legacy default):

* `global_position_index ≡ global_line_index` — single-axis line addressing.
* No per-line offset tables.
* Step records use the existing line-only `AbsoluteStep` / `DeltaStep` layout; `DeltaColumn` (tag 0x07) MUST NOT appear.

Reader rules:

1. A reader that **understands** the column-extension flag MUST honour both modes (load per-line tables when the flag is set; surface `column` as `None` when clear).
2. A reader that **does not understand** the flag MUST detect the unknown bit (per the existing "reserved; readers reject when set" rule for unallocated bits in `internal-files.md`) and refuse to open the trace rather than silently misdecoding the step stream.
3. Writers MUST NOT mix column-aware and line-only step records within a single trace. The flag is trace-global.

`FLAG_HAS_COLUMN_AWARE_STEPS` is a *wire-format* flag — it says columns
are present, not that they are sharp enough for breakpoint placement or
per-column motion. Two separate **capability** flags tell the GUI
whether per-column affordances should be enabled:

* bit 6 — `FLAG_SUPPORTS_COLUMN_BREAKPOINTS`: recorder's columns are
  sharp enough to place a breakpoint at a specific `(line, column)`.
* bit 7 — `FLAG_SUPPORTS_COLUMN_MOTIONS`: recorder supports per-column
  step over / in / out (step predicate fires per statement-start, not
  per line).

When either capability bit is clear, the GUI MUST disable the
corresponding affordance. See `internal-files.md` §"Column-Aware
Capability Flags" for the full contract.

### Compression Impact

In a typical trace, ~80-90% of steps are sequential lines within a function (delta +1 or small positive). With DeltaStep:

- Most steps: 2 bytes (down from 17 bytes) — 8.5x reduction
- Function entry/return: 3-4 bytes (AbsoluteStep with varint)
- Weighted average: ~2-3 bytes per step

Combined with Zstd compression on the already-compact delta stream, effective per-step storage drops below 1 byte.

#### Column Extension Cost (empirical, P6.2)

Measured on five synthetic 100k-step per-language step corpora (`python_co_positions`, `js_sourcemap_minified`, `cpp_dwarf`, `rust_dwarf`, `cairo`) with the chosen `DeltaColumn` encoding:

* **Step stream raw growth:** +1.9% to +20.3% over the line-only baseline (depending on within-line column-motion density). Best case JS-minified (almost all within-line, +1.9%); worst case Rust DWARF (mostly line transitions, +20.3%).
* **Step stream after Zstd:** absolute cost stays under **1.0 byte/step** on every corpus (worst case 0.897 B/step on `js_sourcemap_minified`).
* **`paths.dat` growth (Layout A):** per-line offset table adds roughly `line_count × 1-2 bytes` per file. For a 10K-line trace, ~10-20 KB total — typically <1% of total trace size.

**Budget framing.** The earlier "≤10% total trace size increase" target was relative to a line-only baseline that compresses extraordinarily well (0.095-0.40 B/step after Zstd, dominated by `+1` line deltas that Zstd run-length-collapses). No column-aware encoding can stay within +10% of that figure, by construction. The correct budget formulation is in **absolute bytes-per-step**: column-aware traces under the chosen `DeltaColumn` encoding cost **< 1.0 B/step after Zstd** on every measured corpus, which on a 10M-step trace is on the order of ~10 MB of column data on top of the ~4 MB line-only baseline.

The synthetic-corpus numbers are subject to revision once column-aware recorders ship (Python `co_positions`, DWARF column extraction, Cairo). Re-run the bench against real traces before treating the absolute figures as final.

### Chunked Compression

Events are grouped into **chunks** of `chunk_size` records (default: 4096). Each chunk is independently Zstd-compressed. Chunks contain **only compressed data** -- no inline headers. All metadata lives in companion index streams.

```
steps.dat:  [compressed_chunk_0][compressed_chunk_1][compressed_chunk_2]...
steps.idx:  [chunk_size: u32][offset_0: u64][offset_1: u64]...
```

The companion index `steps.idx` starts with the records-per-chunk count (u32 LE), followed by one u64 byte offset per chunk. To seek to a specific record, compute `chunk = record_id / chunk_size`, read the byte offset from the index, and decompress only that chunk. See [seekable-zstd.md](seekable-zstd.md) for the full companion index format.

Default Zstd compression level: 3.

## Recorder Integration — Starting a Recording

`start(path, line)` is the first writer call a recorder makes after its paths are
registered, and it is **not** merely a cursor move. It emits three things, in this
order, and **every** implementation of it does so — the C ABI's
`trace_writer_start` as much as a safe wrapper, because a recorder should not
have to know which writer it was handed in order to produce a well-formed call
tree. A recorder therefore MUST NOT register `<toplevel>` itself: doing so opens
the frame twice.

1. the **`<toplevel>` function record**, at `(path, line)`;
2. the **opening `Call`** of that function, with no arguments;
3. the **entry step** at `(path, line)`.

```
Function { path_id, line, name: "<toplevel>" }   -> function_id 0
Call     { function_id: 0, args: [] }            -> call_key 0, depth 0
Step     { path_id, line }                       -> the entry step
```

### `<toplevel>` is the call tree's root and its id is fixed

`<toplevel>` MUST be the **first** function a recording interns, so that it receives
**`function_id` 0**, and its call MUST be the first call, so that it receives
**`call_key` 0** at **`depth` 0**. Readers rely on this: the call tree is rooted at
`call_key` 0, and a recording whose first call is a user function has no root to hang
the tree from. Writers SHOULD assert the identity rather than assume it — the
reference implementation does exactly that, with
`assert!(function_id == TOP_LEVEL_FUNCTION_ID)` where `TOP_LEVEL_FUNCTION_ID` is
`FunctionId(0)`.

The name is literally `<toplevel>`, in angle brackets. It is not a placeholder for the
entry function's own name: a recording of a program whose entry point is `main` has
BOTH a `<toplevel>` frame at depth 0 and a `main` frame at depth 1, and a reader
showing a call tree shows the former as the root.

### The entry step is part of `start`, not the recorder's first `register_step`

A recording contains **one more step than the recorder emitted** — the entry step,
which `start` emits. A recorder that calls `start(p, 1)` and then `register_step` N
times produces **N + 1** steps.

This is stated normatively because the two reference writers disagreed about it, in
opposite directions, and neither was wrong by its own documentation:

- the pure-Rust `AbstractTraceWriter::start` registered `<toplevel>` and its call but
  emitted **no step**;
- the Nim C ABI's `trace_writer_start` emitted **the step** — its docstring says
  *"Record the initial step (entry point)"* — but registered **no `<toplevel>`**.

A container written by one and read by a consumer written against the other is off by
one step, or has no call-tree root, with nothing in the container to say so. Both
behaviours are defensible readings of *"start recording at the entry point"*, which is
why the contract is pinned here rather than left to each writer's own documentation.

### Consequences a recorder author should expect

- **Step counts.** Any assertion of the form *"the container has as many steps as the
  host emitted"* is off by one. Compare against `emitted + 1`, or against the
  positions the recorder asked for, which does not depend on the count at all.
- **Function and call counts.** A recording with no user function calls still has
  **one** function and **one** call.
- **The first step's position** is the entry point's, not the first event the
  recorder produced.

---

## Recorder Integration — Staging Values

A writer that buffers one step at a time — `register_step` opens a step, and the
next step, call, or return flushes it together with everything staged in between
— will be handed variables at moments when no step is open. This is not a
recorder behaving badly. It is the normal shape for two very common cases:

* **A binding whose value is only known after a call returns.** `let x = f()`
  reaches the writer as `call` / `step` / `return` / `variable x`, because the
  value does not exist until `f` has returned, and the return has already
  flushed the step.
* **A call's arguments.** Recorders stage each argument as a step variable as
  well as a call argument, and they do it *before* `register_call`, at a moment
  when the caller's own step is typically already flushed.

Those values are said to be staged **in the gap**, and what a writer does with
them is a correctness question, not a housekeeping one.

### The rule

**Values staged while no step is open MUST attach to the next step the writer
emits, and the writer MUST NOT emit a step of its own to carry them.**

Carrying them forward is correct on the format's own terms: `StepValues` is "all
variable values visible at this step", and the next step is the first step at
which they are visible. It also needs no new policy — it is what a line-only
writer has always done.

Two tempting alternatives are both wrong, and both have shipped:

**Do not park them on a `DeltaColumn`.** A `DeltaColumn` is a column-only nudge
on the preceding step, not a logical step — `logicalStepCount` excludes it for
exactly that reason. A value record written parallel to one sits at an exec
index that no logical step occupies: it is unreachable from `variables_at` for
every step in the trace, so the values are written and then never readable. The
container finalizes, decodes cleanly, holds one value record per exec index, and
is *short* — see `conformance-testing.md` §"A record that is empty for two
different reasons". **This is worse than discarding them**, because a discard at
least leaves the value count matching the step count, whereas this leaves a
container in which nothing at all indicates something is missing.

**Do not synthesize a step somewhere else to hold them.** Emitting one at the
callee's declaration site, or at the previous step's position, or at line 1, puts
a step in the exec stream that the recorder never asked for and the program never
executed. A reader that seeks to it lands where the program never was; the step
count stops matching what the recorder emitted, and no recorder author can
predict it, because whether the extra step appears depends on whether a value
happened to be staged at that moment. It is the same class of defect as folding a
column into a line-only address: a plausible position that is not a real one.

### Where the recording ends with values still staged

Carry-forward needs a terminus. When a recording ends with values staged and no
further step coming, those values **MUST NOT be silently discarded** — a writer
that drops them finalizes a container that is short in exactly the way described
above, with the same absence of any signal.

Two shapes are in use, and they are not equivalent; see §"Known Issues —
Column-Aware" for the open question. A writer that has not yet emitted the last
step's value record can still amend it, which costs no extra step. A writer that
has already emitted it cannot, and has to add one final step at the last recorded
position instead.

Where there is no last position either — values staged by a recorder that never
recorded a single step — there is nothing truthful to attach them to, and the
writer MUST fail the close by name rather than finalize a container that silently
omits them.

### What this asks of a recorder

A recorder does not have to rely on the writer getting this right, and the
recorders that read best do not. Holding a value until the next step is known and
registering it there makes the recorder's own intent explicit, keeps it correct
against writers that have not been fixed, and is the only form in which the
recorder — rather than the writer — decides which step a value belongs to.

---

## Recorder Integration — Column-Aware Steps

This section is the integration contract for recorders that want to emit
column-aware traces. The wire-format pieces (tag 0x07 `DeltaColumn`,
`paths.dat` Layout A, `meta.dat` bit 4) are documented above; this
section pins down the *recorder-side* API surface and the per-language
coverage matrix.

### Canonical Recorder Integration Pattern

The canonical sequence — driven by the Rust safe wrapper in
`codetracer-trace-format` (`NimTraceWriter`) — is:

```rust
// Once, before start(): opt in to column-aware encoding. Sets
// meta.dat bit 4 and switches the writer's step encoder to emit
// DeltaColumn (tag 0x07) records.
writer.enable_column_aware_steps();

// Once per source path, before start(): register the path together
// with the per-line byte-length table that the reader needs to
// decode (line, column) from global_position_index. `lengths[i]`
// is the byte length of line i (1-based source index) plus 1 for
// the "one past EOL" position.
writer.register_path_with_line_lengths(path, &lengths);

// Start recording at the entry point.
writer.start(path, line);

// Per step: emit a column-aware step. When `column` is Some(c),
// the writer threads the column through register_step + a
// follow-up register_delta_column so the step lands at (line, c).
// When `column` is None, the writer falls back to a line-only
// step and the reader surfaces column=None for that step.
for (path, line, column) in steps {
    writer.register_step_with_column(path, line, column);
}
```

**Why the split (`register_step` + `register_delta_column`).** Prior to
the column-aware navigation campaign (C1), `NimTraceWriter::register_step_with_column`
dropped the `column` argument on the floor — it emitted a line-only
step. The C1 fix threads the column through as two separate writer
calls: `register_step` (which emits `AbsoluteStep` or `DeltaStep`
addressing `(line, column=1)`), followed by `register_delta_column`
(which emits `DeltaColumn` with `column - 1` as the delta). The result
on the wire is a `(step, column-delta)` pair that decodes to the exact
`(line, column)` the recorder asked for. Recorders MUST call the safe
wrapper rather than driving the two FFI symbols directly unless they
own the bookkeeping for tracking the previous column themselves.

### A column needs a file with a column axis

`register_path_with_line_lengths` is what gives a file a column axis. A file
registered without one — an empty table, because the source was not readable at
the path the recorder named — is sized in the position space by the line-only
fallback, where **one address is one line**. A column delta added to such an
address does not name a column. It names a later line, and the result decodes as
`line + column - 1` with nothing in the container to say the position was ever
about a column.

This is neither hypothetical nor rare. A recorder that resolves source paths out
of debug information gets unreadable paths whenever it records somewhere other
than the build machine, and a recorder fed generated or fetched sources may have
no local file at all. Measured in the Solana recorder: every file went untabled,
and every step after the first reported a line that was wrong by its own column
— decoding cleanly, with plausible line numbers.

**A writer MUST NOT fold a column into the address of a file that has no
per-line table.** Readers already decline to decode a column for such a file and
fall back to line-only, so a writer that accepts one is producing positions its
own reader refuses to read.

What happens to the rest of the call depends on what else it carries:

* **A column arriving as part of a step** — `register_step_with_column`, or the
  single combined event a writer folds it into — **keeps the step**. Emit it at
  its line, line-only, and report the dropped column on the writer's diagnostic
  channel. The line is a position the recorder actually observed; refusing the
  whole step to punish the column throws that away, and a missing step is far
  harder to notice than a missing column.
* **A column arriving on its own** — a bare `DeltaColumn` cursor move — has no
  step to keep, so the call is refused with an error naming the file.

The two entry points are easy to fix by halves. A writer that guards only the
bare cursor move has not closed anything: the combined step-plus-column call is
the one the canonical recorder wrapper drives, so it is the one every recorder
actually reaches, and a guard that misses it leaves the defect live on the only
path in use. **Both entry points, or neither.**

**A recorder SHOULD NOT offer the column at all** for a file it knows has no
table — and it does know, being the party that supplied, or failed to supply,
the lengths. Tracking which paths came back with a non-empty table and passing
`None` for the rest keeps the diagnostic channel quiet, and is the only form
that is also correct against a writer that has not been fixed yet.

### FFI Symbols (column-aware extensions)

The column-aware extension adds four C FFI symbols to the writer ABI.
All are no-ops on column-unaware traces (they fail-closed without
corrupting the trace if called on a writer that has not had
`trace_writer_enable_column_aware_steps` invoked).

```c
// Opt in to column-aware encoding. Must be called before start().
// Sets meta.dat bit 4 (FLAG_HAS_COLUMN_AWARE_STEPS) and switches
// the writer's step encoder. Idempotent.
void trace_writer_enable_column_aware_steps(trace_writer_t handle);

// Register a source path together with its per-line byte-length
// table. Must be called before start() for every path the trace
// will reference. `lengths` is an array of `line_count` u32 values;
// `lengths[i]` is the byte length of line i+1 (1-based) plus 1.
// The writer copies the table into paths.dat Layout A.
void trace_writer_register_path_with_line_lengths(
    trace_writer_t handle,
    const char* path,
    const uint32_t* lengths,
    uint32_t line_count);

// Emit a column-aware step in one FFI call. Equivalent to a step at
// (path, line) followed by a register_delta_column for `column - 1`
// when `has_column != 0`. When `has_column == 0`, behaves like the
// legacy line-only ct_assignment.
void ct_assignment_with_column(
    trace_writer_t handle,
    const char* path,
    uint32_t line,
    uint32_t column,
    int has_column);

// Emit a DeltaColumn (tag 0x07) record after a Step. `column_delta`
// is the signed delta from the previous step's column. The writer
// MUST have been put into column-aware mode via
// trace_writer_enable_column_aware_steps; calling this on a
// column-unaware writer is a no-op (the writer records a one-shot
// diagnostic).
void trace_writer_register_delta_column(
    trace_writer_t handle,
    int32_t column_delta);
```

The Rust safe wrapper (`NimTraceWriter` in the `codetracer-trace-writer`
crate) re-exports these as `enable_column_aware_steps`,
`register_path_with_line_lengths`, `register_step_with_column` (which
internally chains `register_step` and `register_delta_column`), and
`register_delta_column`. The Nim canonical writer
(`codetracer-trace-format-nim`) exposes the same surface under the
`writeColumnAware*` family.

### WASM DWARF Subtlety

The WASM recorder consumes DWARF emitted by the target language's
compiler. For Rust → WASM, `rustc` emits column-refinement DWARF rows
**without `is_stmt`** for positions inside a sub-expression — i.e. the
column changes between rows but the row is not flagged as a statement
boundary. A naive PC → `(file, line, column)` indexer that filters on
`is_stmt == true` (the usual heuristic for "this row is a steppable
statement") will silently drop these refinements and end up emitting
only line-granularity columns.

The column-aware WASM recorder MUST index *all* DWARF line-program rows
(including `is_stmt == false`) so that column data is available to the
step predicate. The step predicate itself can still gate on `is_stmt`
when deciding whether to *emit* a step, but the underlying PC → source
map must carry every row's column.

This subtlety affects only the WASM recorder today; native DWARF
recorders that consume the same compiler output (when those land) will
inherit the same requirement.

### Per-Recorder Integration Matrix

Column-aware status of each in-tree recorder as of this spec revision.
"PASS" recorders set `FLAG_HAS_COLUMN_AWARE_STEPS` (bit 4) and SHOULD
also set the two capability bits (`FLAG_SUPPORTS_COLUMN_BREAKPOINTS`,
`FLAG_SUPPORTS_COLUMN_MOTIONS` — bits 6 and 7) whenever their step
predicate is genuinely per-statement. "Not supported" recorders leave
bit 4 clear and the capability bits clear. See `internal-files.md`
§"Column-Aware Capability Flags" for the contract.

| Recorder | Status | Notes |
|----------|--------|-------|
| JavaScript (V8) | PASS | Multi-statement-per-line distinguishable via V8 source positions; in production |
| EVM / Solidity (M-evm) | PASS | Solidity sourcemap entries carry column; in production |
| Solana SBF (M-sol) | PASS | SBF DWARF; in production |
| Cairo (M-cairo) | PASS | Cairo compiler emits column for each instruction; in production |
| Flow / Cadence (M-flow) | PASS | Cadence parser exposes column on each statement node; in production |
| PolkaVM (M-polkavm) | PASS | PolkaVM DWARF; in production |
| Move (M-move) | PASS | Move compiler emits column for each bytecode; in production |
| WASM (M-wasm) | PASS | DWARF column rows; see §"WASM DWARF Subtlety" above |
| Noir (M-noir-v2) | PASS | Unblocked via the codetracer Noir compiler fork; in production |
| Nim compile-time (M-nim) | PASS | Nim macro AST has column on every node; in production |
| Cardano / Aiken | NOT SUPPORTED | Aiken parser is line-oriented; column data is not produced upstream |
| Leo (Aleo) | NOT SUPPORTED | Aleo parser drops the second statement on a multi-stmt line, so per-column distinction is unrecoverable |
| TON Tolk | NOT SUPPORTED | Same root cause as Leo — parser drops the second statement on a multi-stmt line |
| Ruby | NOT SUPPORTED | `TracePoint` fires once per line; no sub-line callback hook exists |
| Circom | NOT SUPPORTED | Compiler emits no column information anywhere in its source-position pipeline |
| Fuel / Sway | NOT SUPPORTED | Sway compiler does not currently emit columns; revisit when upstream lands columns |
| Miden MASM | NOT SUPPORTED | Step predicate is line-only by construction (one MASM instruction per line) |
| Shell recorders | WONTFIX | Shell traces have no statement positions to attach columns to |
| Native MCR | OUT OF SCOPE | MCR uses a different replay model; columns flow through `debug.dat` DWARF at replay time, not through CTFS step records |

**Upstream-blocked recorders.** None currently. The Noir recorder was
previously blocked on upstream column support; it was unblocked via the
codetracer Noir compiler fork during this campaign and is now in the
PASS column.

A "NOT SUPPORTED by language constraint" recorder MUST leave bit 4
clear in `meta.dat` rather than emit synthetic column=1 values — the
contract is that a column-aware trace's columns mean something, and a
recorder that cannot produce meaningful columns must opt out of the
extension entirely.

## Known Issues — Column-Aware

Open issues in the writer and recorder pipeline as of this spec revision, most
of them surfaced by the column-aware navigation campaign. These are
recorder/writer bugs, not wire-format bugs — the on-wire format is stable; fixes
land in the writer crates without requiring trace re-recording. Entries that have
been closed are kept when the way they were wrong is itself worth knowing.

### `ct_print` drops `call_entry` past `stepCount`

The writer-side close-time flush is asymmetric between the step stream
and the call stream. When `ct_print` is invoked at trace shutdown, any
`call_entry` event whose step index is greater than the writer's
recorded `stepCount` is dropped instead of being flushed to `calls.dat`.
Symptom: the very last function call in a short trace is missing from
the call tree even though its steps appear in `steps.dat`.

Workaround: emit a final no-op step before tearing down the writer so
the last `call_entry` lands within `stepCount`. Fix tracked in the
writer crate's close-path.

### Writer's pending-value-after-`DeltaColumn`: trailing variable lost

**Closed.** The mechanism this described — a `DeltaColumn` record flushing the
pending buffer between a `register_step` and a *trailing* `register_variable` —
stopped existing when `register_delta_column` was folded into the pending line
step. The step and its column became one combined event, so a trailing
`register_variable` now lands in the same flush window as the step it annotates.

What outlived it was a different defect in the same pipeline, which is why this
entry stayed plausible for so long: values staged *in the gap*, where no step is
open at all, were parked on a synthesized `DeltaColumn` nudge and became
unreachable from every step in the trace. That is ruled out by §"Recorder
Integration — Staging Values", which now says where a staged value goes instead
of leaving each writer to invent a policy.

The entry is kept rather than deleted because of how it came to be wrong. A
regression test for exactly this shape had existed for months and had never run
— it was never added to the test runner's file list. It named the right pipeline
and the wrong defect, and nothing contradicted it, because nothing executed it.
See `conformance-testing.md` §"A gate that cannot pass".

### Trailing values at close: two shapes, one of them adds a step

Open. §"Recorder Integration — Staging Values" requires that values still staged
when a recording ends are not discarded. Both reference writers honour that —
differently.

* A writer still buffering the last step's value record amends that record. No
  extra step; the values surface on the final step.
* A writer that has already emitted the record cannot amend it, and emits one
  additional step at the last recorded position to carry them instead. Same
  values, one more step, and a `last_step_id` naming a duplicate position.

A container written by one is therefore a step longer than one written by the
other from identical recorder calls — which any byte-for-byte differential
between the two will see the moment a fixture ends with a staged value. Pick one
before a recorder starts depending on the count.

### A writer freed without being closed loses staged values silently

Open. The terminus that saves trailing values runs on the close path only. A
caller that releases the writer handle without closing it first — which is what a
wrapper's destructor does when `close()` was never called — finalizes the
container through the release path, where the terminus is not reached. The staged
values are dropped, and the release path has no return channel to report it, so
the only evidence is a value stream that is short.

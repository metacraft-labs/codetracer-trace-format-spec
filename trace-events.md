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
| 0 | AbsoluteStep | global_line_index: varint | ~3 bytes |
| 1 | DeltaStep | delta: signed varint | 2 bytes |
| 2 | Raise | exception_type_id: varint, message_len: varint, message: bytes | varies |
| 3 | Catch | exception_type_id: varint | ~2 bytes |
| 4 | ThreadSwitch | thread_id: varint | 2 bytes |

Step records do not carry `call_key`. To find a step's enclosing call, use proportional (interpolation) search on `calls.dat` — each call record stores `[first_step_id, last_step_id]` ranges. This is O(log log C), typically 2-3 iterations, and avoids doubling the step record size.

Raise is emitted when an exception is raised (before unwinding). Catch is emitted when a `try/except` handler catches the exception.

#### 2. Value Stream (`steps.dat`) — variable values, parallel-indexed by step

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

The value stream is indexed in parallel with the execution stream — record N in `steps.dat` corresponds to step N in `steps.dat`. For steps with no variables (possible), the record is empty (just a zero count).

#### 3. Call Stream (`calls.dat`) — call tree records, one per function call

Each record represents a complete function call with entry/exit information.

| Field | Type |
|-------|------|
| call_key | varint |
| function_id | varint |
| parent_key | varint (-1 for root) |
| first_step_id | varint |
| last_step_id | varint |
| depth | varint |
| children_count | varint |
| children_keys | [varint] × children_count |
| args | streaming CBOR (or empty if no args) |
| return_value | streaming CBOR (or VoidReturn marker) |
| raised_exception | optional: streaming CBOR (if call ended with unhandled raise) |

Call records are written when the function returns (not at call entry), so they contain complete information. The `call_key` is a sequential index assigned at call entry.

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
| Values | `steps.dat` | Variable values per step | Point lookup (parallel to exec) | 50-500 bytes |
| Calls | `calls.dat` | Call tree | Random access by call_key | 20-200 bytes |
| IO Events | `events.dat` | I/O event log | Paginated scan | 20-1000 bytes |
| Interning | `*.dat` + `*.off` | Paths, functions, types, names | Loaded at startup | Total 1-5MB |

#### Benefits

1. **Event log loads instantly**: `events.dat` is independent, small, directly paginated
2. **Call tree loads independently**: `calls.dat` is indexed by call_key, no step scanning needed
3. **Step navigation is fast**: `steps.dat` records are tiny (2-4 bytes), so chunks hold thousands of steps
4. **Value loading is on-demand**: `steps.dat` only loaded for the current step's variables
5. **Streams compress independently**: DeltaStep-heavy `steps.dat` compresses extremely well; value-heavy `steps.dat` gets different Zstd settings

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
| 0 | `AbsoluteStep` | `global_line_index: varint` | Execution stepped to a source line (full state at chunk/function boundaries) |
| 1 | `DeltaStep` | `delta: signed varint` | Compact step encoding — signed delta from previous step's global line index |
| 2 | `Raise` | `exception_type_id: varint`, `message_len: varint`, `message: bytes` | Exception raised (before unwinding) |
| 3 | `Catch` | `exception_type_id: varint` | Exception caught by a try/except handler |
| 4 | `ThreadSwitch` | `thread_id: varint` | Execution switched to a different thread |

### Value Stream Events (`steps.dat`)

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
| `call_key` | varint | Sequential index assigned at call entry |
| `function_id` | varint | Reference to interning table |
| `parent_key` | varint (-1 for root) | Parent call's call_key |
| `first_step_id` | varint | First step in this call |
| `last_step_id` | varint | Last step in this call |
| `depth` | varint | Call stack depth |
| `children_count` | varint | Number of child calls |
| `children_keys` | [varint] × children_count | Child call keys |
| `args` | streaming CBOR (or empty) | Function arguments |
| `return_value` | streaming CBOR (or VoidReturn marker) | Return value |
| `raised_exception` | optional: streaming CBOR | Present if call ended with unhandled raise |

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

## Compact Step Encoding

Step events use two variants for efficient encoding:

### AbsoluteStep (Tag 0)

Used at function entry, after large jumps, or when the delta would exceed DeltaStep's range.

```
[Tag: 0x00] [global_line_index: varint]
Total: 3-4 bytes typical (1 tag + 2-3 varint bytes)
```

### DeltaStep (Tag 1)

Used for consecutive steps within the same function or nearby code. Stores the signed delta from the previous step's global line index.

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

### Compression Impact

In a typical trace, ~80-90% of steps are sequential lines within a function (delta +1 or small positive). With DeltaStep:

- Most steps: 2 bytes (down from 17 bytes) — 8.5x reduction
- Function entry/return: 3-4 bytes (AbsoluteStep with varint)
- Weighted average: ~2-3 bytes per step

Combined with Zstd compression on the already-compact delta stream, effective per-step storage drops below 1 byte.

### Chunked compression

In split-binary mode, events are grouped into **chunks** of `chunk_size` events (default: 4096). Each chunk is independently Zstd-compressed with an inline 16-byte header:

```
+-------------------------------------------+
| Chunk Header (16 bytes)                   |
|  compressed_size:  u32 LE (4 bytes)       |
|  event_count:      u32 LE (4 bytes)       |
|  first_geid:       u64 LE (8 bytes)       |
+-------------------------------------------+
| Compressed Data (compressed_size bytes)   |
|  (Zstd-compressed concatenated events)    |
+-------------------------------------------+
```

Multiple chunks are concatenated back-to-back in the `events.log` file (after the HEADERV1 prefix). The chunk headers enable GEID-based seeking: to find a specific event, scan chunk headers to find the chunk whose `first_geid` range covers the target, then decompress only that chunk.

Default Zstd compression level: 3.

## CBOR Encoding (legacy)

In CBOR mode, each `TraceLowLevelEvent` is serialized as a complete CBOR value using `serde`-derived serialization (tagged enum encoding). Events are concatenated and streamed through a Zstd seekable encoder (via `zeekstd`), with frames flushed every 64 KiB of uncompressed data by default.

The CBOR format uses serde's default tagged-enum representation. Each event is a CBOR map with a single key (the variant name) whose value contains the fields.

## events.log internal header (HEADERV1)

Both split-binary and CBOR modes prefix the `events.log` file with an 8-byte header before any compressed data:

```
Byte:  0     1     2     3     4     5     6     7
     +-----+-----+-----+-----+-----+-----+-----+-----+
     | C0  | DE  | 72  | AC  | E2  | 01  | 00  | 00  |
     +-----+-----+-----+-----+-----+-----+-----+-----+
```

- Bytes 0-4: CTFS magic (`C0 DE 72 AC E2`)
- Byte 5: `0x01` -- events.log format version 1
- Bytes 6-7: Reserved, must be `0x00`

This is distinct from the outer CTFS container header. Readers verify this prefix before attempting to decode events.

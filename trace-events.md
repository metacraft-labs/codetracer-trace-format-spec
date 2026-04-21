# Trace Events

CodeTracer records program execution as a stream of `TraceLowLevelEvent` values. Each event captures one atomic action: a step to a source line, a function call, a variable binding, etc.

## TraceLowLevelEvent Variants

| Tag | Variant | Fields | Description |
|-----|---------|--------|-------------|
| 0 | `Step` | `path_id: usize`, `line: i64` | Execution stepped to a source line |
| 1 | `Path` | `path: String` (PathBuf) | Register a source file path (must appear before any Step referencing it) |
| 2 | `VariableName` | `name: String` | Intern a new variable name |
| 3 | `Variable` | `name: String` | Intern a variable name (legacy, backward compat) |
| 4 | `Type` | `record: TypeRecord` | Register a type (must appear before Value referencing it) |
| 5 | `Value` | `variable_id: usize`, `value: ValueRecord` | Full value for a variable |
| 6 | `Function` | `path_id: usize`, `line: i64`, `name: String` | Register a function (must appear before Call referencing it) |
| 7 | `Call` | `function_id: usize`, `args: Vec<FullValueRecord>` | Function call |
| 8 | `Return` | `return_value: ValueRecord` | Function return |
| 9 | `Event` | `kind: EventLogKind (u8)`, `metadata: String`, `content: String` | I/O or log event |
| 10 | `Asm` | `lines: Vec<String>` | Assembly instructions |
| 11 | `BindVariable` | `variable_id: usize`, `place: i64` | Bind a variable to a memory place |
| 12 | `Assignment` | `record: AssignmentRecord` | Variable assignment or parameter passing |
| 13 | `DropVariables` | `ids: Vec<usize>` | Drop multiple variables (end of scope) |
| 14 | `CompoundValue` | `place: i64`, `value: ValueRecord` | Experimental: compound value at a place |
| 15 | `CellValue` | `place: i64`, `value: ValueRecord` | Experimental: cell value at a place |
| 16 | `AssignCompoundItem` | `place: i64`, `index: usize`, `item_place: i64` | Experimental: assign to compound item |
| 17 | `AssignCell` | `place: i64`, `new_value: ValueRecord` | Experimental: assign to cell |
| 18 | `VariableCell` | `variable_id: usize`, `place: i64` | Experimental: associate variable with cell |
| 19 | `DropVariable` | `variable_id: usize` | Drop a single variable |
| 20 | `ThreadStart` | `thread_id: u64` | A new thread started |
| 21 | `ThreadExit` | `thread_id: u64` | A thread exited |
| 22 | `ThreadSwitch` | `thread_id: u64` | Execution switched to a different thread |
| 23 | `DropLastStep` | *(none)* | Discard the previous Step (workaround for append-only streams) |

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

### TypeRecord

- `kind: TypeKind` (u8 enum -- Seq, Set, HashSet, OrderedSet, Array, Varargs, Struct, Int, Float, String, CString, Char, Bool, Literal, Ref, Recursion, Raw, Enum, Enum16, Enum32, C, TableKind, Union, Pointer, Error, FunctionKind, TypeValue, Tuple, Variant, Html, None, NonExpanded, Any, Slice)
- `lang_type: String` -- the language-level type name
- `specific_info: TypeSpecificInfo` -- one of:
  - `None`
  - `Struct { fields: Vec<FieldTypeRecord> }` where each field has `name: String`, `type_id: usize`
  - `Pointer { dereference_type_id: usize }`

### EventLogKind (u8 enum)

| Value | Kind |
|-------|------|
| 0 | Write |
| 1 | WriteFile |
| 2 | WriteOther |
| 3 | Read |
| 4 | ReadFile |
| 5 | ReadOther |
| 6 | ReadDir |
| 7 | OpenDir |
| 8 | CloseDir |
| 9 | Socket |
| 10 | Open |
| 11 | Error |
| 12 | TraceLogEvent |
| 13 | EvmEvent |

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
[Tag: 0x00] [global_line_index: u64 LE]
Total: 9 bytes
```

### DeltaStep (Tag 24)

Used for consecutive steps within the same function or nearby code. Stores the signed delta from the previous step's global line index.

```
[Tag: 0x18] [delta: signed varint]
Total: 2 bytes typical (1 tag + 1 varint for delta ±63)
```

The signed varint uses zigzag encoding: `(delta << 1) ^ (delta >> 63)`, then unsigned LEB128.

| Delta range | Varint size | Total event size |
|-------------|------------|-----------------|
| ±63 | 1 byte | 2 bytes |
| ±8191 | 2 bytes | 3 bytes |
| ±1048575 | 3 bytes | 4 bytes |
| Larger | Use AbsoluteStep | 9 bytes |

### Encoding Rules

1. The first step in a trace is always AbsoluteStep
2. After a Call event, the next step is AbsoluteStep (new function context)
3. After a Return event, the next step is AbsoluteStep (returning to caller)
4. All other steps use DeltaStep if the delta fits in 3 varint bytes (±1048575), otherwise AbsoluteStep

### Compression Impact

In a typical trace, ~80-90% of steps are sequential lines within a function (delta +1 or small positive). With DeltaStep:

- Most steps: 2 bytes (down from 17 bytes) — 8.5x reduction
- Function entry/return: 9 bytes (same as before)
- Weighted average: ~3 bytes per step

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

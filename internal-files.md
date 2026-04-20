# Internal Files

A CTFS container (`.ct` file) stores several named files. This document describes the conventions for each.

## Core Files

### `events.log`

The event stream -- the main payload of a trace file.

**Structure**: An 8-byte HEADERV1 prefix followed by compressed event data.

```
[HEADERV1: 8 bytes][compressed event data...]
```

The HEADERV1 prefix is `C0 DE 72 AC E2 01 00 00` (CTFS magic + format version 1 + 2 reserved bytes).

The compressed data format depends on the serialization mode:

- **Split-binary** (default): Concatenated chunks, each with a 16-byte inline header followed by Zstd-compressed split-binary event data. See [trace-events.md](trace-events.md) for the chunk header layout and per-event encoding.
- **CBOR** (legacy): Seekable Zstd stream of concatenated CBOR-serialized events. See [seekable-zstd.md](seekable-zstd.md).

### `events.fmt`

A plain-text marker indicating the serialization format of `events.log`.

| Content | Meaning |
|---------|---------|
| `split-binary` | Split-binary encoding with chunked Zstd compression |
| `cbor` | CBOR encoding with seekable Zstd compression |

Readers check this file to determine which decoder to use for `events.log`.

### `meta.json`

Trace metadata serialized as JSON.

```json
{
  "workdir": "/home/user/project",
  "program": "/usr/bin/myapp",
  "args": ["--flag", "value"]
}
```

Fields:
- `workdir` (string): Working directory of the recorded process
- `program` (string): Path to the recorded executable
- `args` (array of strings): Command-line arguments

### `paths.json`

Registered source file paths, serialized as a JSON array of strings.

```json
[
  "/home/user/src/main.rs",
  "/home/user/src/lib.rs",
  "/home/user/src/utils.rs"
]
```

The array index corresponds to the `PathId` used in `Step`, `Function`, and other events. Paths are registered by `Path` events during recording and written here at finalization.

## Native Recorder Files

The following files are used by the native recorder for binary/debug information. They are not written by the standard trace writer but may be present in `.ct` files produced by the native recorder.

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
| `build_id` | build_id_len | raw bytes (typically 20 bytes for ELF build-id) |
| `path_len` | 1-10 | LEB128 varint |
| `path` | path_len | UTF-8 string |

Followed by type-specific fields:

- **DebugSymbol**: `binary_ref` (u64 LE) -- Base40-encoded CTFS name of the parent binary
- **SourceFile**: `compilation_dir_len` (LEB128 varint) + `compilation_dir` (UTF-8 string)
- **Binary**: no additional fields

### `platform.bin`

Platform description for the machine that recorded the trace.

**Header** (4 bytes):

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic: `50 4C 41 54` ("PLAT") |

**Fixed fields** (20 bytes):

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

**Variable fields** (after offset 24):

| Field | Encoding |
|-------|----------|
| `libc_name` | LEB128 length + UTF-8 string |
| `kernel_version` | LEB128 length + UTF-8 string |

### `mmap.bin`

Memory mapping table -- every mapped region in the recorded process.

**Header** (8 bytes):

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic: `4D 4D 41 50` ("MMAP") |
| 4 | 4 | Entry count (u32 LE) |

**Each entry** (33 bytes, fixed-size for O(1) indexing):

| Offset | Size | Field |
|--------|------|-------|
| +0 | 8 | `address` (u64 LE): start address of the mapping |
| +8 | 8 | `size` (u64 LE): size in bytes |
| +16 | 8 | `binary_ref` (u64 LE): Base40-encoded CTFS name of the mapped binary (0 if anonymous) |
| +24 | 8 | `file_offset` (u64 LE): offset within the binary file |
| +32 | 1 | `permissions` (u8): bit 0=read, bit 1=write, bit 2=execute, bit 3=private (vs shared) |

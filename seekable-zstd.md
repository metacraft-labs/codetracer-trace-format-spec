# Seekable Zstd Compression

CodeTracer uses Zstd seekable compression for the CBOR event encoding mode. This allows random access to events without decompressing the entire stream.

The seekable format is implemented by the [`zeekstd`](https://github.com/nicholasgasior/zeekstd) crate.

## Format Overview

The seekable format splits data into independently-compressed Zstd frames. A seek table (stored as a Zstd skippable frame) maps frame boundaries, enabling decompression of arbitrary byte ranges without reading the entire file.

The format is compatible with standard Zstd: any Zstd decoder can decompress a seekable file by simply ignoring the skippable seek table frame. The seek table only adds random-access capability.

## Frame Structure

A seekable Zstd stream consists of:

1. **One or more Zstd compressed frames** -- each independently decompressible
2. **A seek table frame** -- a Zstd skippable frame containing the index

```
[Zstd Frame 0][Zstd Frame 1]...[Zstd Frame N-1][Seek Table Frame]
```

### Seek Table Frame

The seek table is stored in a Zstd skippable frame. Two layouts exist:

**Foot format** (classic, placed at end of file):

| Field | Size | Description |
|-------|------|-------------|
| `Skippable_Magic_Number` | 4 bytes | `0x184D2A5E` |
| `Frame_Size` | 4 bytes | Size of the rest of this frame |
| `Seek_Table_Entries` | 8 bytes each | One per compressed frame |
| `Seek_Table_Integrity` | 9 bytes | Frame count + descriptor + magic |

**Head format** (v0.1.1, for standalone files):

| Field | Size | Description |
|-------|------|-------------|
| `Skippable_Magic_Number` | 4 bytes | `0x184D2A5E` |
| `Frame_Size` | 4 bytes | Size of the rest of this frame |
| `Seek_Table_Integrity` | 9 bytes | Frame count + descriptor + magic |
| `Seek_Table_Entries` | 8 bytes each | One per compressed frame |

### Seek Table Integrity (9 bytes)

| Field | Size | Description |
|-------|------|-------------|
| `Number_Of_Frames` | 4 bytes | Number of compressed frames (u32 LE) |
| `Seek_Table_Descriptor` | 1 byte | Bit 7: Checksum_Flag (deprecated in v0.1.1), bits 6-2: reserved (must be 0), bits 1-0: unused |
| `Seekable_Magic_Number` | 4 bytes | `0x8F92EAB1` |

### Seek Table Entries (8 bytes each)

| Field | Size | Description |
|-------|------|-------------|
| `Compressed_Size` | 4 bytes | Compressed size of this frame (u32 LE) |
| `Decompressed_Size` | 4 bytes | Decompressed size of this frame (u32 LE) |

Cumulative sum of `Compressed_Size` values gives the byte offset of each frame in the compressed stream.

## CodeTracer Usage

### CBOR mode (legacy)

In CBOR mode, the `zeekstd::Encoder` streams CBOR-serialized events through seekable Zstd compression. Configuration:

- **Frame size policy**: `FrameSizePolicy::Uncompressed(threshold)` -- a new Zstd frame is started after `threshold` bytes of uncompressed input. Default threshold: 64 KiB.
- **Compression level**: 3

The encoder writes compressed frames into a shared in-memory buffer. Periodically (when uncompressed data exceeds the flush threshold), the current Zstd frame is finalized and the compressed output is drained into the CTFS `events.log` file.

At finalization, `encoder.finish()` flushes any remaining data and writes the seek table as a trailing skippable frame.

### Split-binary mode (default)

Split-binary mode does **not** use seekable Zstd. Instead, it uses the chunked compression scheme described in [trace-events.md](trace-events.md), where each chunk of events is independently Zstd-compressed with an inline 16-byte header. This provides GEID-level seeking without the overhead of a separate seek table.

## Reference

The seekable Zstd format is specified in full at [`zeekstd/seekable_format.md`](https://github.com/nicholasgasior/zeekstd/blob/main/seekable_format.md), derived from the [original Facebook/Meta specification](https://github.com/facebook/zstd/blob/dev/contrib/seekable_format/zstd_seekable_compression_format.md).

All numeric fields are little-endian.

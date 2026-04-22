# Compression and Seeking in CTFS

CTFS uses per-chunk Zstd compression with companion index streams to provide random access within compressed data. This document describes how compression and seeking work.

## Chunked Compressed Table

Records are grouped into chunks of configurable size (per-stream), each chunk independently compressed with Zstd. This is the standard compression model for all CTFS data streams.

### Chunk Format

Chunks contain **only compressed data** -- no inline headers. All metadata lives in the companion index.

```
foo.dat:  [compressed_chunk_0][compressed_chunk_1][compressed_chunk_2]...
```

Each chunk is independently decompressible (no cross-chunk dependencies). All chunks except the last contain exactly `chunk_size` records. The last chunk contains the remainder.

### Companion Index Stream (.idx)

For each chunked data stream `foo.dat`, a companion index `foo.idx` is stored as a separate CTFS internal file:

```
foo.idx:  [chunk_size: u32][offset_0: u64][offset_1: u64][offset_2: u64]...
```

- `chunk_size` (u32 LE, 4 bytes): number of records per chunk, configurable per stream
- Each subsequent u64 LE (8 bytes): byte offset of that chunk within `foo.dat`

Different streams use different chunk sizes based on their record sizes -- `steps.dat` (2-4 byte records) benefits from larger chunks, `calls.dat` (100+ byte records) from smaller ones.

### Seeking -- O(1)

To read record N:

1. Compute `chunk_number = N / chunk_size`
2. Read `foo.idx[4 + chunk_number * 8]` -- the byte offset of chunk start
3. Read `foo.idx[4 + (chunk_number + 1) * 8]` -- next offset (or `foo.dat` file size for last chunk)
4. Read `foo.dat[offset..next_offset]` -- the compressed chunk
5. Decompress with Zstd
6. Access record `N % chunk_size` within the decompressed data

This is O(1) -- the chunk number is computed directly from the record index (no binary search needed). Two u64 reads from the index + one compressed chunk read. With block caching, the index reads are typically free (the entire index fits in one or two blocks for most traces).

### Derived Information

All of these are computable from the index without any redundant fields:

- **Record N is in chunk** `N / chunk_size`
- **Chunk C starts at** `foo.idx[C]` (u64 read)
- **Chunk C ends at** `foo.idx[C+1]` (or `foo.dat` file size for last chunk)
- **Compressed size of chunk C** = `foo.idx[C+1] - foo.idx[C]`
- **First record in chunk C** = `C * chunk_size`
- **Record count in chunk C** = `chunk_size` (or `total_records - C * chunk_size` for last chunk)

The index is 8 bytes per chunk plus a 4-byte header. For a stream with 10,000 chunks, the index is ~80 KB.

## Streaming: Available During Active Recording

The index is written **incrementally** as chunks complete. No finalization step is required.

### Writer Protocol

1. Accumulate records into a buffer
2. When buffer reaches `chunk_size` records:
   a. Encode records
   b. Compress with Zstd
   c. Append u64 byte offset (current position in `foo.dat`) to `foo.idx`
   d. Write compressed data to `foo.dat`
   e. Sync both `foo.dat` and `foo.idx` file entry sizes for concurrent readers

Concurrent readers see new chunks as soon as the index entry is synced. Seeking is available during active recording with no finalization step.

## Network Access

When reading over a network, the companion index enables efficient chunk-level fetching:

1. Fetch the index (a single small CTFS file, typically one or two blocks)
2. Compute chunk number from record ID
3. Read the chunk's byte offset from the local index
4. Issue a single HTTP range request for the target chunk
5. Decompress locally

This avoids fetching the entire data stream.

## Configuration

Default Zstd compression level: 3.

Chunk sizes are configurable per stream. Smaller chunks give finer seek granularity but lower compression ratio. Larger chunks compress better but require decompressing more data per seek. Default: 4096 records per chunk.

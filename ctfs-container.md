# CTFS Binary Container Format

CTFS (CodeTracer File System) is a simple block-based container format that stores multiple named files in a single `.ct` file. It is designed for append-friendly, seekable storage of trace data.

## Overview

A CTFS file is divided into fixed-size **blocks**. Block 0 is the **root block** and contains the header, extended header, and file entry table. Subsequent blocks hold either file data or mapping (index) blocks.

```
+------------------------------------------------------------------+
| Block 0 (root block)                                             |
|  [Header: 8 bytes]                                               |
|  [Extended Header: 8 bytes]                                      |
|  [File Entry 0: 24 bytes]                                        |
|  [File Entry 1: 24 bytes]                                        |
|  ...                                                             |
|  [File Entry N-1: 24 bytes]                                      |
|  [Zero padding to block_size]                                    |
+------------------------------------------------------------------+
| Block 1..M (mapping and data blocks)                             |
+------------------------------------------------------------------+
```

## Header (8 bytes)

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 5 | `magic` | `C0 DE 72 AC E2` -- identifies a CTFS file |
| 5 | 1 | `version` | Format version. Current: `0x03`. Readers also accept `0x02`. |
| 6 | 1 | `compression` | Compression tag for file data (see below) |
| 7 | 1 | `encryption` | Encryption tag (see below) |

```
Byte:  0     1     2     3     4     5     6     7
     +-----+-----+-----+-----+-----+-----+-----+-----+
     | C0  | DE  | 72  | AC  | E2  | ver | cmp | enc |
     +-----+-----+-----+-----+-----+-----+-----+-----+
```

### Compression method (byte 6)

| Value | Method |
|-------|--------|
| `0x00` | None |
| `0x01` | Zstd |
| `0x02` | LZ4 (reserved, not yet implemented) |

### Encryption method (byte 7)

| Value | Method |
|-------|--------|
| `0x00` | None |
| `0x01` | AES-256-GCM (reserved, not yet implemented) |

### Version history

- **v2**: Original version. Bytes 6-7 were reserved (always `0x00 0x00`).
- **v3**: Added compression and encryption tag bytes. v3 readers accept v2 files by interpreting `0x00` as None/None.

## Extended Header (8 bytes)

Immediately follows the header at offset 8.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 8 | 4 | `block_size` | Block size in bytes (u32 LE). Must be 1024, 2048, or 4096. Default: 4096. |
| 12 | 4 | `max_root_entries` | Maximum number of file entries in block 0 (u32 LE). Default: 31. |

```
Byte:  8     9    10    11    12    13    14    15
     +-----+-----+-----+-----+-----+-----+-----+-----+
     |    block_size (LE)    | max_root_entries (LE)   |
     +-----+-----+-----+-----+-----+-----+-----+-----+
```

## File Entry (24 bytes each)

File entries start at offset 16 (immediately after the extended header) and there are `max_root_entries` slots. An empty (unused) entry has all fields set to zero.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| +0 | 8 | `size` | File size in bytes (u64 LE) |
| +8 | 8 | `map_block` | Root mapping block number (u64 LE). 0 = unused entry. |
| +16 | 8 | `name` | Base40-encoded filename (u64 LE). See below. |

```
Byte:  +0    +1   ...   +7    +8   ...  +15   +16  ...  +23
     +-----+-----+---+-----+-----+---+-----+-----+---+-----+
     |    size (u64 LE)    | map_block (u64 LE)  | name (u64 LE)   |
     +-----+-----+---+-----+-----+---+-----+-----+---+-----+
```

The total root block usage is: `8 (header) + 8 (ext header) + 24 * max_root_entries`. The remainder of block 0 is zero-padded to `block_size`.

## Base40 Filename Encoding

Filenames are encoded into a single `u64` using base-40 arithmetic. This limits filenames to **12 characters** from the following 40-character alphabet:

| Index | Character |
|-------|-----------|
| 0 | NUL (padding/terminator) |
| 1-10 | `0`-`9` |
| 11-36 | `a`-`z` |
| 37 | `.` |
| 38 | `/` |
| 39 | `-` |

### Encoding algorithm

Given a filename string `s` of length <= 12:

```
result = 0
for i in 0..12:
    if i < len(s):
        idx = alphabet_index(s[i])
    else:
        idx = 0
    result += idx * (40 ^ i)
```

The first character occupies the lowest-order bits (little-endian digit order). An empty string encodes to 0.

### Decoding algorithm

```
chars = []
while encoded > 0:
    remainder = encoded mod 40
    encoded = encoded / 40
    if remainder == 0:
        break   // trailing NUL = end of name
    chars.append(alphabet[remainder])
result = join(chars)
```

## Block Mapping Model

Each file's data is stored across one or more data blocks. The `map_block` field in the file entry points to the **root mapping block**, which is always a **level-1** block.

### Definitions

- **N** = `block_size / 8` -- number of u64 entries per block
- **usable** = `N - 1` -- entries available for pointers (the last entry is reserved for the chain pointer)

With the default block size of 4096: N = 512, usable = 511.

### Level structure (bottom-up chain)

The mapping uses a bottom-up chain model with up to 5 levels:

- **Level 1** (root mapping block): `entries[0..usable-1]` are direct pointers to data blocks. `entries[N-1]` points to a level-2 block (0 if not needed).
- **Level 2**: `entries[0..usable-1]` each point to a level-1 mapping block. `entries[N-1]` points to a level-3 block.
- **Level k** (up to 5): `entries[0..usable-1]` each point to a level-(k-1) block. `entries[N-1]` chains to level-(k+1).

### Capacity formulas

| Level | Data blocks addressable | With block_size=4096 (usable=511) |
|-------|------------------------|-----------------------------------|
| 1 | usable | 511 (~2 MiB) |
| 2 | usable^2 | 261,121 (~1 GiB) |
| 3 | usable^3 | 133,432,831 (~509 GiB) |
| 4 | usable^4 | ~68 billion (~260 TiB) |
| 5 | usable^5 | ~35 trillion |

Total addressable with all 5 levels: sum of usable^k for k=1..5.

### Block resolution algorithm

To find the data block at logical index `block_index`:

1. Start at `current_block = root_mapping_block`, `level = 1`, `idx = block_index`.
2. Compute `cap = usable^level`. If `idx < cap`, this index belongs at the current level. Go to step 4.
3. Otherwise: `idx -= cap`, `level += 1`. Follow the chain pointer at `current_block[N-1]` to the next-level block. If the pointer is 0, the block does not exist. Repeat from step 2.
4. Navigate down within the level:
   - If `level == 1`: the data block number is `current_block[idx]`.
   - If `level > 1`: `sub_cap = usable^(level-1)`, `entry = idx / sub_cap`, `sub_idx = idx % sub_cap`. Follow `current_block[entry]` to a level-(level-1) block and recurse with `sub_idx`.

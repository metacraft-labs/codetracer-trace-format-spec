# CodeTracer Trace Format Specification

This repository is the source-of-truth specification for the CodeTracer trace format. It documents the binary layouts, encoding schemes, and file conventions used by CodeTracer to record and replay program execution traces.

## Documents

| Document | Contents |
|---|---|
| [ctfs-container.md](ctfs-container.md) | CTFS binary container format: magic, headers, file entries, block mapping, Base40 encoding |
| [trace-events.md](trace-events.md) | `TraceLowLevelEvent` enum, split-binary encoding, CBOR legacy encoding |
| [seekable-zstd.md](seekable-zstd.md) | Zstd seekable compression format as used by CodeTracer |
| [internal-files.md](internal-files.md) | Conventions for files stored inside a CTFS container |

## Implementations

- **Rust** (primary): [`codetracer-trace-format`](https://github.com/nicholasgasior/codetracer-trace-format) -- `codetracer_ctfs`, `codetracer_trace_types`, `codetracer_trace_writer`, `codetracer_trace_reader`
- **Nim** (secondary): [`codetracer-trace-format-nim`](https://github.com/nicholasgasior/codetracer-trace-format-nim) -- `codetracer_ctfs` Nim package

# CodeTracer Trace Format Specification

This repository is the source-of-truth specification for the CodeTracer trace format. It documents the binary layouts, encoding schemes, and file conventions used by CodeTracer to record and replay program execution traces.

## Documents

| Document | Contents |
|---|---|
| [ctfs-container.md](ctfs-container.md) | CTFS binary container format: magic, headers, file entries, block mapping, Base40 encoding |
| [trace-events.md](trace-events.md) | The split streams (`steps.dat`, `values.dat`, `calls.dat`, `events.dat`), value encoding, recorder integration rules; the removed `events.log` |
| [seekable-zstd.md](seekable-zstd.md) | Zstd seekable compression format as used by CodeTracer |
| [internal-files.md](internal-files.md) | Conventions for files stored inside a CTFS container |
| [recorded-entry-identity.md](recorded-entry-identity.md) | Optional checked program-entry call identity; legacy entry selection stays unchanged when absent |
| [Trace-Filters.md](Trace-Filters.md) | Cross-language trace filter contract: schema, hot-path requirement, provenance |
| [conformance-testing.md](conformance-testing.md) | How to test an implementation, and how such tests fail to fail — written from the defects that got through |
| [measurements/2026-10-format-efficiency.md](measurements/2026-10-format-efficiency.md) | The measurements behind the 2026-10 revision: step encoding, `step-map.ns`, `meta.dat`'s path list, small and empty members, `events.dat` kinds |

## Tools

- [`tools/ctfs-measure`](tools/ctfs-measure) -- the measurement harness: an independent `.ct` reader, the candidate encodings, native and WASM decode benchmarks, and the scripts that rebuild the recording corpus (`corpus/build_corpus.sh`) and the report (`run_measurements.sh`).

## Implementations

- **Rust** (primary): [`codetracer-trace-format`](https://github.com/metacraft-labs/codetracer-trace-format) -- `codetracer_ctfs`, `codetracer_trace_types`, `codetracer_trace_writer`, `codetracer_trace_reader`
- **Nim** (secondary): [`codetracer-trace-format-nim`](https://github.com/metacraft-labs/codetracer-trace-format-nim) -- `codetracer_ctfs` Nim package

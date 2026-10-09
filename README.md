# CodeTracer Trace Format Specification

This repository is the source-of-truth specification for the CodeTracer trace format. It documents the binary layouts, encoding schemes, and file conventions used by CodeTracer to record and replay program execution traces.

## Scope

This specification defines only the structure of the CTFS container format, and the records used by
the open-source recorders that produce materialized traces. It does not describe how the Multi-Core
Recorder (MCR) uses CTFS: which members an MCR recording writes, their layout, and its per-thread,
checkpoint and page streams. That is specified in
[`codetracer-specs`](https://github.com/metacraft-labs/codetracer-specs) under
`spec/Trace-Files/CTFS-Binary-Format.md`, and it is subject to change in each release.

The boundary between the two repositories, and how a change that spans both is landed, is stated in
`codetracer-specs/spec/Trace-Files/README.md`. In short: a change to the container itself (blocks,
the root directory, keyed members, namespaces, publication and the live coordination page, writer
architectures, durability, appends) is made here first, and every producer and consumer is then
updated to it; a change to what MCR stores in a container is made in `codetracer-specs`, using only
mechanisms specified here.

## Documents

| Document | Contents |
|---|---|
| [ctfs-container.md](ctfs-container.md) | CTFS binary container format: headers, the fixed root directory, file entries, block mapping, Base40 encoding, live publication through the live coordination page and reader obligations, dead space, closed-container appends, keyed members, namespaces, writer architectures |
| [ctfs-keyed-families.md](ctfs-keyed-families.md) | Members that hold families growing with a recording: the cost model, the properties of the data, the abstract families (dense, clustered, sparse exact, sparse ordered, static) and their candidate structures, the in-place publication they need, and what the benchmarks that choose a structure per family must measure |
| [trace-events.md](trace-events.md) | The split streams (`steps.dat`, `values.dat`, `calls.dat`, `events.dat`), value encoding, recorder integration rules; the removed `events.log` |
| [seekable-zstd.md](seekable-zstd.md) | Zstd seekable compression format as used by CodeTracer |
| [internal-files.md](internal-files.md) | Conventions for files stored inside a CTFS container by the materialized-trace recorders, and the member catalogue (which family each member belongs to) |
| [recorded-entry-identity.md](recorded-entry-identity.md) | Optional checked program-entry call identity; legacy entry selection stays unchanged when absent |
| [Trace-Filters.md](Trace-Filters.md) | Cross-language trace filter contract: schema, hot-path requirement, provenance |
| [conformance-testing.md](conformance-testing.md) | How to test an implementation, and how such tests fail to fail — written from the defects that got through |
| [measurements/2026-10-format-efficiency.md](measurements/2026-10-format-efficiency.md) | The measurements behind the 2026-10 revision: step encoding, `step-map.ns`, `meta.dat`'s path list, small and empty members, `events.dat` kinds |

## Tools

- [`tools/ctfs-measure`](tools/ctfs-measure) -- the measurement harness: an independent `.ct` reader, the candidate encodings, native and WASM decode benchmarks, and the scripts that rebuild the recording corpus (`corpus/build_corpus.sh`) and the report (`run_measurements.sh`).

## Implementations

- **Rust** (primary): [`codetracer-trace-format`](https://github.com/metacraft-labs/codetracer-trace-format) -- `codetracer_ctfs`, `codetracer_trace_types`, `codetracer_trace_writer`, `codetracer_trace_reader`
- **Nim** (secondary): [`codetracer-trace-format-nim`](https://github.com/metacraft-labs/codetracer-trace-format-nim) -- `codetracer_ctfs` Nim package

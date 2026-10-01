# Test Fixtures

Binary `.ct` fixture files for validating trace format implementations against the spec. Both the Nim and Rust implementations can use these files to verify correct reading/writing behavior.

## minimal_trace.ct

A small but representative CTFS container produced by the Nim `TraceWriter` API (`codetracer-trace-format-nim`). It uses split-binary encoding with seekable Zstd compression.

### Events contained (in order)

| # | Event kind | Details |
|---|-----------|---------|
| 0 | Path | `/src/main.nim` (pathId 0) |
| 1 | Path | `/src/math_utils.nim` (pathId 1) |
| 2 | Function | `main` at pathId 0, line 1 (functionId 0) |
| 3 | Step | pathId 0, line 1 |
| 4 | Call | functionId 0, no args |
| 5 | Step | pathId 0, line 3 |
| 6 | Step | pathId 0, line 4 |
| 7 | Value | variableId 1, Int(42) typeId 7 |
| 8 | Step | pathId 1, line 10 |
| 9 | Value | variableId 2, String("hello") typeId 9 |
| 10 | Return | None value |

### Metadata

- **recording_id**: UUIDv7 minted at fixture generation time (M-REC-1).
  Pre-1.0 the value is not stable across regenerations; M-REC-12 will
  freeze a well-known constant.
- **program**: `factorial`
- **args**: `["5"]`
- **workdir**: `/home/user/demo`

### Internal CTFS files

Container version 5 (`ctfs-container.md` §1); every member here fits one block,
so each `MapBlock` is its data block with the direct tag (§2).

- `events.log` -- legacy unified event stream, compressed with seekable Zstd
- `events.fmt` -- legacy stream format marker, the string `split-binary`
- `meta.dat` -- version 6 (`internal-files.md` §"Metadata (meta.dat)"): recording id, program, args, workdir; no path list
- `paths.dat` / `paths.off` -- the two source paths, in id order

This fixture predates the split-stream CTFS layout. Current materialized traces store execution records in `steps.dat`, values in `values.dat`, call records in `calls.dat`, and event-log records in `events.dat`, each with the corresponding companion index where the stream is chunked.

### How it was generated

```
cd codetracer-trace-format-nim
nim c -r tests/generate_spec_fixture.nim fixtures/minimal_trace.ct
```

The generator source is at `codetracer-trace-format-nim/tests/generate_spec_fixture.nim`. It uses `newTraceWriter` with `chunkThreshold = 64` (all events fit in a single chunk).

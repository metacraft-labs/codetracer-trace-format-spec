# Test Fixtures

Binary `.ct` fixture files for validating trace format implementations against the spec. Both the Nim and Rust implementations can use these files to verify correct reading/writing behavior.

## minimal_trace.ct

A small but representative split-stream container, written by the Nim split-stream writer
(`codetracer-trace-format-nim`, `multi_stream_writer.nim`). It reproduces byte for byte: the
recording id is fixed.

### What it records

- Paths: `/src/main.nim` (path 0), `/src/math_utils.nim` (path 1).
- Types: `int` (type 0), `string` (type 1). Variable names: `x` (0), `msg` (1).
- Function `main` at `/src/main.nim:1` (function 0).
- A call of `main`, then four steps: `/src/main.nim` lines 1, 3 and 4, and `/src/math_utils.nim`
  line 10. Step 2 carries `x = 42` (int), step 3 carries `msg = "hello"` (string).
- A return with no value.

### Metadata

- **recording_id**: `0192f8a0-0000-7000-8000-000000000001`
- **program**: `factorial`
- **args**: `["5"]`
- **workdir**: `/home/user/demo`

### Internal CTFS files

Container version 5 (`ctfs-container.md` §1); every member here fits one block, so each `MapBlock`
is its data block with the direct tag (§2).

- `paths.dat`/`.off`, `funcs.dat`/`.off`, `types.dat`/`.off`, `varnames.dat`/`.off` -- the interning
  tables (`internal-files.md` §"Interning Tables")
- `steps.dat`/`.idx`, `values.dat`/`.idx`, `calls.dat`/`.idx`, `events.dat`/`.idx` -- the runtime
  streams (`trace-events.md`); `events.dat` is empty
- `meta.dat` -- version 6: recording id, program, args, workdir
- `step-map.ns` -- the breakpoint index, written at close

It has no `events.log` or `events.fmt`: those members are not part of the format, and a reader
refuses a container that carries them (`trace-events.md` §"Removed members").

### How it was generated

```
cd codetracer-trace-format-nim
nim c -r -p:src tests/generate_spec_fixture.nim fixtures/minimal_trace.ct
```

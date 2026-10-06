# Nim span helper comments recommend forbidden presence-bit gating

| | |
|---|---|
| Status | open |
| Recorded | 2026-10-06 |
| Observed in | codetracer-trace-format-nim @ 19cad035585cbc0845b84ddc11aa3b824825dfaa |
| Area | `MetaDatContents.hasSpanStream`; `hasSpanStreamFiles` |

## Observed

The `hasSpanStream` field documentation in `src/codetracer_trace_writer/meta_dat.nim` says that a clear bit means there are no span streams, that readers must report zero spans, and that they must not look for the files. The `hasSpanStreamFiles` documentation in `src/codetracer_trace_writer/span_stream.nim` instead recommends gating callers on `meta.dat` bit 13 and describes the structural lookup as diagnostic-only.

These are documentation observations at the named source revision. They do not establish a writer defect or a failure of the structural lookup implementation. No production source or reader oracle was changed when recording this issue.

## Expected

`internal-files.md`, **“Stream-presence flags are a hint, not a gate”**, at frozen authority `974ab479f502eb34138f8168b501f566c9bf71ee` defines bits 8..13 and 15 as optional hints. Readers MUST NOT gate stream reading on those bits or reject a structurally present stream whose bit is clear. `findFile` and the file entry's `Size` supply the authoritative presence and readable-progress information.

The helper comments should describe that contract, including lazy stream creation after the metadata header is committed. A clear bit alone cannot require zero spans or prohibit structural discovery. Container version and metadata schema version remain distinct; this issue does not authorize additional container-version support.

## Evidence

Read the two named symbols from the exact `19cad035585cbc0845b84ddc11aa3b824825dfaa` tree and compare them with the frozen specification heading. The same normative heading is present in the owning specification checkout at `30b15f757ddd00453b578c43aa598615d26fe1ed`.

Before filing, the specification repository was synchronized to `latest`; open records and deleted issue history were searched for `hasSpanStream`, span hints, and span flags. No existing issue for these helper comments was found.

## Related

The distinct CodeTracer consumer issue records stale UI assertions (published CodeTracer specs commit `400494e4ef8f087d6e87a2f69db6ae4c8352a988`). Its measured consumer failure is not attributed here to the writer, nor transferred as qualification of this helper lane.

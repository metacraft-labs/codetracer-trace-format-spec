# Nested-Trace Correlation Record

Status: wire contract, authored 2026-08-18 (CodeTracer GDScript recorder
milestone **N1**).

A **nested trace** is a materialized CTFS trace produced *inside* a process that
is itself being recorded by another backend. The motivating case is the patched
Godot GDScript recorder (see
`codetracer-specs/Recording-Backends/GDScript-Recorder.md`): a patched engine
running under the Multi-Core Recorder (`ct-mcr record`) emits a source-level
GDScript `.ct` (steps / values / calls) *while* the MCR records the same process
natively. The two traces describe the same execution at two altitudes — native
machine events and GDScript source events — and a consumer needs to cross
between them (zoom from a native frame into the GDScript step that produced it,
and back).

This document defines the **correlation record**: the on-disk contract that lets
a nested materialized trace be joined to its parent native MCR trace. It is a
pure wire contract — fields, encoding, and the resolution rule. It does **not**
define UI behaviour (native↔VM zoom is the consumer feature, owned by the
GDScript backend spec once shipped, tracked by milestone N2) nor the recorder's
in-process mechanics (owned by `GDScript-Recorder.md`).

The design rationale (why a join at the `GDScriptFunction::call` boundary, how it
composes with EV-M visual replay and MCR) lives in
`codetracer-specs/Planned-Features/Nested-Trace-Correlation.md`.

## 1. Coordinate reuse — what a join key is built from

The correlation record introduces **no new time coordinate**. It reuses the two
coordinates the native MCR trace already assigns to every native event, defined
in [internal-files.md](internal-files.md):

- **GEID** — the *global event id*. In an MCR container it is the key of
  `geid.idx` (the GEID-to-checkpoint index, [internal-files.md] §"Multi-Core
  Recorder (MCR) Traces"). GEID is globally orderable: the total order across
  threads is reconstructed by sorting on the numeric GEID, so a GEID identifies
  a unique point in the native execution and indexes directly into the
  checkpoint / replay machinery (`geid.idx` → `cp.dat`/`cp.off`).
- **tick** — the native trace's *within-thread progress* coordinate, derived per
  the `meta.dat` **tick source** (`tick_source` / `tick_source_str` in the MCR
  extended-fields block, [internal-files.md] §"Extended Fields"). Ticks
  disambiguate order *within* a single GEID's span (multiple native
  instructions can share the coarser GEID granularity depending on the recorded
  tick source).

Both are **native-trace-internal** coordinates today; there is no cross-trace
correlation primitive. This document adds exactly one thing: a way to *import* an
`(GEID, tick)` pair, sampled from the parent native trace at the instant a nested
event fires, into the nested trace so the two traces share one coordinate system.

The join key is the pair:

```
JoinKey = (geid: u64, tick: u64)
```

`geid` is the primary key (globally orderable, indexes `geid.idx`); `tick` is the
secondary key used only to break ties inside one GEID span.

## 2. Where the record is stored — the join event

A correlation record is carried as an **IO event** in the nested trace's
`events.dat` stream (the record shape defined in
[trace-events.md](trace-events.md) §"IO Event Stream Records"): a
`(kind, step_id, metadata, content)` tuple. This reuses the existing special-event
channel (the same channel the GDScript recorder's GF10 async markers and GF13
diagnostics use) — **no new CTFS stream and no C-ABI extension are required**.

A join event is identified by a fixed **content prefix**:

```
ct-nested-join:<parent-kind>
```

where `<parent-kind>` names the parent recorder that supplies the coordinates
(e.g. `gdscript` for the GDScript-under-MCR case; the token is opaque to the
resolver and used only for diagnostics/filtering).

### 2.1 Fields

| Field | Location | Type | Description |
|-------|----------|------|-------------|
| `kind` | IO event `kind` | u8 (EventLogKind) | `TraceLogEvent` (12) — a neutral log kind that does not perturb the write/read kinds recorders rely on. |
| `step_id` | encoded in payload (see below) | u64 | The nested-trace step this join binds to. |
| `geid` | payload | u64 | Parent native GEID sampled when this nested event fired. |
| `tick` | payload | u64 | Parent native tick (per `meta.dat` tick source) at the same instant. |
| `site` | payload | enum | The nested-VM boundary that produced the join (§2.2). |
| `thread` | payload | u64 | The nested recorder's thread id for the emitting event (attribution; `1` == main). |

**Encoding.** The join fields are written in **both** the IO event `content` and
`metadata`, redundantly, so both a text decoder (`ct-print`) and a structured
consumer (the db-backend CTFS reader) can recover them:

- `content` = the human/tool-readable form, self-describing so `ct-print --full`
  surfaces the whole key (the reader does **not** surface `metadata`):

  ```
  ct-nested-join:<parent-kind> geid=<u64> tick=<u64> step=<u64> site=<site> thread=<u64>
  ```

- `metadata` = the same fields as a compact space-separated `key=value` list
  (`geid=<u64> tick=<u64> step=<u64> site=<site> thread=<u64>`), the form a
  structured consumer parses.

`step_id` is written **explicitly in the payload** rather than relying on the IO
event's implicit `step_id` field, because the multi-stream writer buffers one
"pending step" (for late-arriving column deltas / values), so the implicit
`step_id` lags the just-registered line by one. The recorder computes the
correct value as `trace_writer_next_step_index() - 1` (identical to the GF10
async-marker convention; see `codetracer_trace_writer.h`
`trace_writer_next_step_index`). Consumers MUST read `step` from the payload.

### 2.2 Join sites

`site` records which nested-VM boundary produced the join. A consumer uses it to
know what the native GEID *means* relative to the GDScript step:

| `site` | Boundary | Meaning |
|--------|----------|---------|
| `call-enter` | `GDScriptFunction::call` entry | The native coordinate at which this GDScript frame began. |
| `call-exit` | `GDScriptFunction::call` exit (both normal and yield/await-resume exits) | The native coordinate at which this GDScript frame returned. |
| `native-call` | a native-call opcode (`OPCODE_CALL` and the method-bind / native-static call opcodes) | The native coordinate at which this GDScript step called into engine/native code — the crossing where the native MCR trace *is* the continuation of this source step. |

The `native-call` site is the load-bearing one for zoom: it marks the exact
GDScript step whose execution *becomes* native machine events, so a native frame
inside that engine call resolves back to this GDScript step.

## 3. Resolution rule

Let `P` be the parent native MCR trace (with `geid.idx`) and `N` the nested
trace. Let `J = (geid=g, tick=t, step=s, site, thread)` be a join event in `N`.

### 3.1 nested → native

Given a nested step `s` (or the join event `J` at that step), resolve to the
native event that hosts it:

1. Take the join event `J` whose `step == s` (for a `native-call` site: the join
   recorded at `s`; for a frame, the `call-enter` join of the enclosing frame).
2. Look up `g` in `P`'s `geid.idx` → the checkpoint/native event current at that
   GEID. That native event (and its `cp.dat` checkpoint) is the native execution
   point that produced nested step `s`.
3. If two native events share the GEID granularity, use `t` (the tick) to select
   the precise one within the GEID's span.

Result: the native frame / instruction the user zooms *out* to.

### 3.2 native → nested

Given a native GEID `g'` (e.g. a native frame the user is inspecting in `P`),
resolve to the GDScript step it came from:

1. Consider `N`'s join events sorted by `geid` (they are emitted at call /
   native-call boundaries in **GEID-monotonic order** within a thread — see
   §3.3 — so this sort is cheap / already ordered per thread).
2. Select the join event `J*` with the **greatest `geid ≤ g'`** (binary search).
   That is the nested event active at-or-before the native coordinate `g'`.
3. `J*.step` is the GDScript step; `J*.site` says whether `g'` fell inside a
   frame's body (`call-enter` seen, matching `call-exit` not yet passed) or at a
   native-call crossing.

Result: the GDScript step the user zooms *in* to. An exact `geid == g'` match
(the common case when `g'` is itself a `native-call` crossing) is the precise
hit; the `≤` rule degrades gracefully when the native frame is between two
recorded GDScript boundaries.

### 3.3 Ordering invariant the resolver relies on

Within one thread, join events are emitted in the same order the native events
occur, so their `geid` values are **monotonically non-decreasing** in emission
(= `step`) order. This is what makes §3.2 a binary search and §3.1 a direct
`geid.idx` lookup. Across threads, the numeric GEID total order (the same
property MCR uses to reconstruct global order) keeps the join keys globally
orderable; the `thread` field attributes each join to its emitting thread for
per-thread sub-ordering.

## 4. Absence and standalone traces

A nested trace recorded **standalone** (no parent MCR context) contains **zero**
join events: the recorder samples no `(geid, tick)` and emits nothing. Such a
trace is byte-identical to one produced before this contract existed. A consumer
that finds no `ct-nested-join:` events treats the trace as un-nested (no
correlation available) — the flag is the *presence* of at least one join event,
not a `meta.dat` bit.

This mirrors the "registers none → byte-for-byte unchanged" discipline the span
stream uses ([trace-events.md] request/interval spans): correlation is opt-in and
additive.

## 5. Producer requirements (informative)

The authoritative producer contract lives in `GDScript-Recorder.md`; summarized
here so the wire contract is self-contained:

- The producer samples `(geid, tick)` from the parent native recorder's
  in-process context interface. For the CodeTracer MCR this is the exported
  `ct_mcr_now(CtMcrCoordinates*)` symbol
  (`codetracer-native-recorder/ct_interpose/.../trace_context.nim`), which
  reports the live GEID when `recordingAvailable` and `hasGeid` are set. The
  producer MUST treat the interface as **weak/optional** (resolve at runtime;
  absent when standalone) so §4 holds.
- The producer emits a join event at each `site` in §2.2, binding it to the
  current step via the `trace_writer_next_step_index() - 1` rule.
- The producer MUST NOT allocate its own native GEID out of band; it samples the
  coordinate the parent already assigned. (A future producer that needs an
  *authoritative* native event of its own — so that native→nested has a real
  native anchor rather than only the sampled cursor — uses the parent recorder's
  marker-emit primitive, e.g. `ct_mcr_mark_span_start/end`, which allocates a
  real GEID+tick and emits a native event. That path is an N2 concern.)

## 6. Open dependency notes (N1 → N2)

- **tick fidelity.** `ct_mcr_now` today exposes the live **GEID** but not a
  dedicated per-thread **tick** field (it exposes wall/monotonic clocks). Until
  the interposer exports a tick reader (or the producer allocates its own
  native event via `ct_mcr_mark_span_start/end`, which returns an authoritative
  GEID+tick), the `tick` in a real MCR run is the parent's monotonic-time
  sample; the primary `geid` key is fully authoritative. This gap closes in N2.
- **native anchor.** A true native→nested anchor (a native event that *points at*
  the nested trace) requires the producer to emit a native marker via the parent
  recorder (N2), not just sample the read-only cursor. N1 defines the contract
  and proves it against a **synthetic** native trace (a controlled `geid.idx`
  stand-in); the full MCR constellation is N2.

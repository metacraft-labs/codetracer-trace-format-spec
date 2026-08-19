# Nested-Trace Correlation

Status: **revised 2026-08-19.** The original `(GEID, tick)` join-key record (a
`ct-nested-join:` special event carrying a native coordinate sampled at record
time) is **SUPERSEDED**. It is retained below, marked, for readers of already
authored material — but it is **not** the mechanism to build. The converged
design records **no** correlation in the stream: the native↔VM association is
**derived at replay time**, bounded by a span, from the CTFS writer's per-event
chokepoints. This document now describes that mechanism and preserves the retired
one for reference.

The consumer UX (the native↔VM **language switch**) is owned by
`codetracer-specs/Planned-Features/Mixed-Trace-Debugging.md`; this document is the
format-side contract it rests on.

## 0. What a nested trace is

A **nested trace** is a materialized CTFS trace produced *inside* a process that
is itself being recorded natively by MCR. The motivating case is the patched
Godot GDScript recorder: a patched engine running under `ct-mcr record` emits
source-level GDScript steps/values/calls *while* MCR records the same process
natively. The two describe the same execution at two altitudes — native machine
events and VM source events — and a consumer crosses between them (view the
native position under a VM step, and vice versa).

## 1. The converged correlation mechanism

Correlation rests on three pieces, **none** of which is a per-event coordinate
record in the stream:

1. **One container (not two).** The materialized VM streams live in the **same**
   `.ct` as MCR's native streams (they differ by filename, so they coexist).
   `codetracer-specs/Planned-Features/Mixed-Trace-Debugging.md` §2. This is what
   makes "the native trace" and "the VM trace" one artifact whose coordinates are
   relatable at all.

2. **Crossing spans (coarse, seekable).** Each native→VM→native crossing is a
   `SpanRecord` in the existing span stream (`spans.dat`/`spans.idx`/`spantype.ns`,
   `meta.dat` bit 13 `FlagHasSpanStream`) — the same mechanism the HTTP-request
   panel uses. A crossing span's `start_step`/`end_step` bound the materialized
   steps executed inside it, so the span **is** a *(process, thread, step range)*
   coordinate. Spans answer "which crossing are we inside?" in `O(log C)` without
   decoding step data, and they **bound** the correlation search. See
   `Trace-Files/CTFS-Request-Span-Streams.md` and Mixed-Trace-Debugging.md §3.

3. **Writer chokepoints (fine, observable at replay).** The shared CTFS writer's
   per-event functions — `registerStep` (`trace_writer_register_step`),
   `registerCall` (`trace_writer_register_call`), `registerReturn`
   (`trace_writer_register_return[_cbor]`) — are ordinary non-inlined exported
   functions that **emit no stream events and do no syscalls per call** (they
   append to in-memory buffers; bytes hit disk only at `trace_writer_close`). MCR
   can therefore **observe them at replay** without perturbing deterministic
   replay. Mixed-Trace-Debugging.md §4.

### 1.1 Resolution — derived at replay, locally, no global table

Correlation is computed **at replay time, around the seek point, bounded by the
enclosing span** (Mixed-Trace-Debugging.md §6). There is:

- **no** record-time coordinate sampling (no `ct_mcr_now` snapshot per event),
- **no** join events in any stream (no `ct-nested-join` records), and
- **no** precomputed per-line index or global lookup table.

MCR instruments the writer chokepoints during replay (an observation-only patch)
and reads off the crossings in the bounded window:

- **native → VM step.** Find the crossing span covering the native position; the
  VM step is the one whose `registerStep` crossing is current within that span.
- **VM step → native position.** Seek natively to the point where that VM step's
  `registerStep` fires (bounded by the span).

Because the window is bounded by the span rather than the whole recording, both
directions are **local** computations, and both compose with the black-box
forward/reverse continue-to-breakpoint primitive
(`ReplaySession::step(Action::Continue, forward)`).

### 1.2 Non-normative optimization

"Which span / which VM frame / which step ordinal are we at" can be read directly
from memory MCR has already recreated at the seek point — the recorder's
per-thread bookkeeping (the shadow call stack `ct_shadow_sp`; the MCR single
per-thread TLS state block). This can short-circuit the bounded scan. It is a
**performance optimization, not a requirement**; a conforming implementation may
ignore it, and the normative model stays "bounded local scan of chokepoint
crossings within the span" — independent of MCR's internal memory layout.

### 1.3 Scope: MCR only

This mechanism is **MCR-only**. RR is out of scope for combined native↔VM traces:
without a recorded correlation, re-deriving the association on RR lacks MCR's
precise-memory / omniscient-DB machinery. The container and spans are
backend-neutral, so RR is not precluded by the format — it is simply not in
scope. Mixed-Trace-Debugging.md §7.

## 2. Absence and standalone traces

A materialized trace recorded **standalone** (no MCR context) carries **no**
crossing spans and lives in its own container; it is byte-identical to a
pre-existing materialized `.ct`. A native recording with no nested VM is
byte-identical to today's native `.ct`. The single-container init and the crossing
spans are strictly additive and gated on the nested (under-MCR) context and the
`FlagHasSpanStream` bit respectively.

---

## Appendix A — SUPERSEDED: the `(GEID, tick)` join-key record

> **This section documents the retired mechanism** (authored 2026-08-18, GDScript
> recorder milestone N1). It is **not** the design to implement. It is preserved
> because earlier recorder code (N1/N2/N3) and milestone notes reference it; that
> code is to be **unwound** in favor of §1. Do not build new consumers against it.

The retired contract imported a native coordinate into the nested trace at record
time and stored it as a special event:

- **Join key** `(geid: u64, tick: u64)` — the MCR `geid.idx` global event id and
  the `meta.dat` tick-source coordinate, sampled from the parent recorder's
  in-process interface (`ct_mcr_now`, weak `dlsym`) at the instant a nested event
  fired. A future "authoritative anchor" path additionally allocated a real native
  event via `ct_mcr_mark_span_start`/`ct_mcr_mark_span_end`.
- **Storage** — a `ct-nested-join:<parent-kind>` record on the nested trace's
  `events.dat` IO-event channel, carrying `geid`, `tick`, `step`, `site`,
  `thread` in both `content` and `metadata`.
- **Join sites** — `call-enter` / `call-exit` (`GDScriptFunction::call`
  entry/exit) and `native-call` (native-call opcodes).
- **Resolution rule** — nested→native looked up `geid` in the parent `geid.idx`;
  native→nested took the greatest join with `geid ≤ g'` (binary search over a
  GEID-monotonic join list), which required decoding the parent index and building
  a GEID-ordered join list — i.e. a **global lookup table** per direction.

**Why it was superseded.** It sampled and stored a coordinate on **every** nested
event (record-time cost and stream bloat proportional to steps), it required a
separate `nested_correlation` module to decode the native `geid.idx` and build the
per-direction ordered join list (the global table §1.1 avoids), it depended on
gaps that never fully closed (a per-thread tick reader; a real native anchor under
a live MCR run), and it duplicated — as a bespoke per-event record — information
the converged design derives for free at replay from the spans (coarse) and the
writer chokepoints (fine). The retired producer/consumer machinery
(`ct_mcr_now`/`ct_mcr_mark_span_*` sampling at the call boundary, the
`ct-nested-join` emission, the separate-file `nested_correlation` decoder) is
marked SUPERSEDED in `GDScript-Recorder.milestones.org` (N1, N2) and is to be
removed; the GDScript recorder itself (G2–GF13/GT1) and `res://` source-bundling
are unaffected and survive.

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
(`ReplaySession::step(Action::Continue, forward)`). This span-bounded scan is the
**format-only fallback**: it needs nothing but the on-disk container, so it is
what a consumer without recreated memory uses, and it is the contract the fast
path (§1.2) must agree with.

### 1.2 The primary resolution mechanism: recreated in-memory bookkeeping (seek → O(1))

The questions a consumer actually asks at a moment — *which crossing span, which
VM frame, which step ordinal within it, and (under cooperative scheduling) which
logical context is live* — are answered **directly, in O(1), by reading the
recorder's in-memory bookkeeping at the seek point.** This is the canonical
mechanism for the MCR path, not merely an optimization layered on §1.1's scan.

It is O(1) for the same reason CodeTracer navigation is O(1) in general: **a seek
restores process memory.** The recorder keeps its bookkeeping in ordinary process
memory — the shadow call stack (`ct_shadow_sp` / `ct_shadow_stack.c`: a `__thread`
pointer plus the `.bss` `ct_shadow_stacks[]` registry) and the single per-thread
TLS state block (the MCR "Set A" single-TLS-slot architecture,
`MCR-Recorder-Architecture-Invariants.md` A1/A2). MCR's checkpoint+restore
snapshots *every writable mapping* (heap, stack,
`.bss`, `mmap`s) plus the `fs_base`/`gs_base` TLS pointers, and a seek restores the
nearest checkpoint and re-executes forward to the exact tick
(Multi-Core-Recorder.md §11–§12). So at any landing moment those structures hold
**exactly** the values they held at that moment during recording; reading one is a
field access — no stream decode, no index lookup, no scan whose cost grows with the
recording.

This is why the query must **not** be modeled as reader-side state accumulated by
stepping there: on a cold jump the consumer has walked nothing, yet memory
restoration has reconstructed everything. The altitude/crossing/context state a
consumer needs is therefore whatever the recorder is *required* to keep in that
in-memory bookkeeping (§1.5 pins the cooperative-scheduling additions); the
consumer reads it rather than deriving it.

**The on-disk format stays independent of MCR's memory layout.** Elevating the
memory read to primary is a statement about the *consumer's* cost model, not the
*format contract*. The durable artifact is still the span stream (§1 — coarse and
seekable) plus the chokepoints: backend-neutral, and the only inputs available to
a consumer *without* recreated memory (a pure-DB reader of a standalone
materialized trace, §2). For such a consumer §1.1's span-bounded scan is the
equivalent, conforming fallback that yields the same answer more slowly. **The two
paths must agree** — the memory read is the fast path the MCR consumer takes, the
scan is the contract that keeps the format honest.

### 1.3 Scope: MCR only

This mechanism is **MCR-only**. RR is out of scope for combined native↔VM traces:
without a recorded correlation, re-deriving the association on RR lacks MCR's
precise-memory / omniscient-DB machinery. The container and spans are
backend-neutral, so RR is not precluded by the format — it is simply not in
scope. Mixed-Trace-Debugging.md §7.

### 1.4 Streaming correctness: open crossings and the live edge

All CodeTracer functionality must work while the target is still recording, and a
crossing is a live interval, so the span-stream projection is written
**open-at-begin, settled-at-close**: on entering a crossing the recorder appends a
`SpanRecord` with `flags.open` set and `end_step = 0`; on returning it appends a
record with the **same `span_id`** carrying the final `end_step` (last-record-wins
per `span_id`, `CTFS-Request-Span-Streams.md`). Emitting the record only at close
would leave the *in-flight* frame — exactly the moment a load-while-recording
session sits in — invisible on the fallback path until it returns.

An open crossing has no `end_step` yet, so a consumer on the fallback path treats
it as covering `[start_step, recording_head]` — the live edge
(`ReplaySession::recording_head`), never `0` and never `+∞`. This is not merely a
streaming nicety: `end_step` is the **bound of the expensive direction** (a
VM→native seek is MCR replay-from-checkpoint, §6 / Mixed-Trace-Debugging.md §5). An
unbounded open span would turn that bounded-local search into a scan of the whole
tail, so clamping an open crossing to the live edge is what keeps the fallback
O(local) at the live edge. The memory-read path (§1.2) is unaffected: "which
crossing am I in" is a field in restored memory whether or not the crossing has
returned yet.

### 1.5 Cooperative scheduling (multiple logical contexts on one OS thread)

A single OS thread may run **multiple logical execution contexts** that yield to
each other cooperatively — VM coroutines/fibers (e.g. Lua coroutines), or two
scripting altitudes interleaved on one host thread. Their materialized steps
**interleave on the one global step timeline**, so a crossing's
`[start_step, end_step]` for context A *contains* context B's steps. The interval
test alone no longer decides membership, and "which context is live at step N"
becomes the load-bearing question.

**On the memory-read path (§1.2) this is a non-problem.** At the landing moment,
restored memory already reflects which context is running — either the recorder's
own per-context bookkeeping or the VM's own current-coroutine pointer (also in the
restored heap). The consumer reads the current context and its current crossing in
O(1); interleaving never has to be untangled from the step ranges, because the
answer is not derived from the ranges.

Two requirements make this sound, and both are per-crossing / per-yield — never
per-step — so neither taxes the hot path:

1. **The recorder's crossing bookkeeping is keyed by logical context, not OS
   thread.** The shadow stack and the Set-A TLS block are per-OS-thread
   (`MCR-Recorder-Architecture-Invariants.md` A1), so on one OS thread two
   coroutines would otherwise share them. The VM identifies the logical context
   when it opens a crossing (it passes a context id through the crossing API), and
   the recorder maintains the current-crossing stack **per logical context**.

2. **The on-disk projection carries the owning context**, so the fallback path
   (§1.1) — which has no recreated memory — can still disambiguate. A VM crossing
   span's `thread_id` is the **logical context id** the VM assigned (for a
   non-cooperative VM this coincides with the OS thread), and `structural` bit 0
   `contiguous_on_one_thread` is set **false** whenever the context's steps
   interleave with another's on the global timeline — the explicit signal that
   `[start_step, end_step]` is *not* a contiguous ownership set and membership must
   be filtered by owning context. The fallback recovers `owner(N)` from the step
   stream's `ThreadSwitch` markers (coarse — one per yield, so a bounded local scan
   or a small companion index suffices), then filters covering crossings to that
   context before taking the innermost.

No per-step context tag is added to the step stream: that would tax the hot path
for a query the memory-read path answers for free and the fallback answers from the
coarse (per-yield) switch markers.

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

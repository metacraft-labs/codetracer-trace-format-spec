# Conformance Testing

How to test an implementation of this format, written from the defects that got
through.

Everything below is derived from a single campaign that aligned the two
reference writers to this specification. Every example is a real defect, and
every one of them sat behind a test suite that was passing.

## The thesis

**A check fails to fail when the thing it examines is produced by the same
mistake it is meant to catch.**

That is the whole of it. A test is a comparison, and a comparison is only
evidence when its two sides are independent. Where the expected value is
derived — directly or by a chain of steps — from the same code that produces
the actual value, the comparison holds no matter what either side does, and the
test reports success for the same reason it would report success if it were
deleted.

This is not a statement about carelessness. Every case below was written by
someone who understood the format, and several sit in files whose own comments
warn about the adjacent version of the mistake.

## The case that should be read first

`calls.dat` frames a call's arguments as a count followed by
`(varname_id, value_len, value)` triples. The two reference writers filled those
triples differently. One wrote **one entry per argument**, each with that
argument's interned name. The other wrote **one synthetic entry** under
`varname_id` 0 holding the whole argument vector as a single serialized blob,
and its own comment said so:

```rust
encode_varint(0, out); // varname_id (synthetic: whole-args blob)
```

A reader built for one convention, given a container written under the other,
kept the **first argument**, discarded every argument after it, and threw away
every name. It surfaced only as a type mismatch deep inside a CBOR parse. Where
the shapes had happened to agree, it would have lost arguments in silence.

There was a differential test. It compared the two writers' `calls.dat`
**byte for byte**, and it passed — for years — because both of its arms passed
`vec![]` for the arguments. An equality over two empty things is not evidence of
agreement; it is the absence of a question.

The part worth sitting with: **the same test file already documented this exact
lesson, one table over.** Its metadata-`args` comparison had been vacuous for
the same reason, someone found it, and they wrote a paragraph explaining that
the fixture now passes real arguments *because* the empty case could not have
caught a writer that dropped them. That paragraph was a screen away from the
call-argument comparison that was empty for the identical reason, and it did not
save it.

Prose does not generalise on its own. A structural check does.

## The shapes

These are the forms the defect took. They are listed as shapes rather than as
rules because the next one will not look exactly like any of them.

**A helper that returns nothing.** A readback helper recorded a program, checked
the container's magic bytes, and returned `vec![]`. Every assertion behind it
was unreachable, and each test short-circuited on
`if events.is_empty() { return; }`. Three recorders carried one. Between them,
**47 assertions** had not run in months, and the expectations behind them had
drifted to a format the recorders no longer emitted.

**A decoder for a format nothing writes.** Assertions matching on
`type == "Step"` — an encoding that had been retired. They did not fail; they
matched nothing and reported zero, which is indistinguishable from a program
that did nothing.

**A control whose needle stopped matching.** A test asserting that some value
appears, where the value's spelling had since changed. The assertion still runs,
still passes if written as "not absent", and now proves nothing.

**A gate that cannot pass.** A pre-commit hook pointing at a manifest path that
did not exist in that repository. It failed for everyone, so everyone bypassed
it, so nothing was checked. A CI job cloning five sibling repositories at a
branch name that four of them did not have: both jobs died at checkout, and
their verdict was never about the code. A dependency check demanding an exact
commit, which failed on every legitimate advance of that dependency. **A gate
everyone must bypass is worse than no gate**, because it also consumes the
attention that would otherwise notice its absence.

**An identifier that is in range but wrong.** A writer handed out type ids from
a private counter while the interning table advanced its own. Both were valid
indices, both resolved to a real record, and every value written after they
diverged reported a type it had never been given. Nothing failed. A count of
types was right. A round-trip of any single value was right. The only question
that could expose it — *which* type does this value resolve to — was one no test
asked. The identical defect existed for function ids, and attributed calls to
the function next door.

**A record that is empty for two different reasons.** A value record carrying no
variables is exactly what a step with no variables looks like. When a writer
dropped everything staged after its last step, the container finalized, held one
value record per step, decoded cleanly, and was short. The shortfall was visible
only to a test that knew *which* values should have been there.

**A placeholder that looks like data.** When an id could not be resolved, a
reader named it `type_0` and carried on. A test then pinned `["felt", "Word",
"type_0"]` as the expected type table — enshrining the fabrication as a
requirement, in a recording that registers exactly two types.

## What worked

Two defences caught these repeatedly. Both are cheap.

### Assert names, not counts

A count survives almost every mistake above. A name survives none of them.

The reader that kept only a call's first argument passed every count-based
check: the call existed, the record decoded, the arity was plausible. It failed
the moment a test asked what the arguments were **called**. Likewise, the type
ids that resolved to the wrong record passed every count; they failed a test
that asked which type each id resolved to.

> **Where a record carries an identifier into a table, a conformance test SHOULD
> assert the resolved entry, not the identifier and not the number of them.**

This is why the round-trip tests in this format's implementations assert
`["board", "depth", "flags"]` rather than `args.len() == 3`, and assert that
each registered type resolves to the kind *and* the name it was registered with.

### Prove the test bites

A test that has never failed has never been observed to work. Before trusting a
new regression test, put the defect back and watch it go red.

This is not ceremony. In this campaign it found a byte-level assertion that
demanded a float64 where the encoder legitimately emits the shortest form that
round-trips — the test was wrong, not the code, and only running it against the
bug revealed which. It also found that a suite's *own* vacuity check had gone
vacuous.

> **A regression test SHOULD be demonstrated to fail against the defect it
> describes, in the same change that introduces it.**

### Two smaller habits

**Prefer a positive test to an absence test.** Reading a container's format from
the *absence* of a legacy file works only while that file still exists to be
absent. Removing it flipped every such predicate at once. Dispatch on what a
container declares, not on what it lacks.

**Do not let one artifact answer for two.** A recorder that wrote its records
into a container beside the recording, and a test suite that read them back from
there, agreed with each other perfectly while the real recording was missing
every one of those records. Read back the artifact the product consumes.

The same shape appears wherever one job has two implementations. A dump tool
grew a second copy of its renderer so tests could call it in-process without
shelling out to the binary — and the tests then asserted against the copy, which
no user runs. Over the following months the shipped binary gained an ordering
fix, a column-aware position decoder, and a diagnostic channel; the copy the
tests exercised gained none of them, and separately grew a field the binary has
never printed. Every one of those was a passing test suite describing a program
nobody executes. Two producers of one output need something that compares them,
or they are not two implementations of a rule — they are two rules.

## What this asks of a conformance suite

1. **Cross-read.** An implementation's own round-trip proves self-consistency
   and nothing else. Read every container this format defines with the *other*
   implementation's reader. Where no such test exists, assume the path is
   broken — the path every recorder in this campaign actually used had no such
   test, and it was broken in four separate ways.
2. **Assert the resolved value.** Names, kinds, and spellings, not counts.
3. **Assert non-degeneracy.** Where a fixture's emptiness would make an
   assertion vacuous, assert that the fixture is not empty, in the same test.
4. **Refuse rather than default.** A decoder that cannot resolve something
   should say so by name. Every placeholder in this campaign was eventually
   pinned by a test as though it were data.
5. **Exercise a gate in both directions.** Confirm a correct input passes *and*
   that a deliberately broken one fails. An unexercised gate is indistinguishable
   from an absent one until something needs it.

# CTFS Keyed Member Families

> **Scope.** Like the rest of this specification, this document defines the structure of the CTFS
> container and the records of the open-source recorders that produce materialized traces. It
> does not describe how the Multi-Core Recorder (MCR) uses CTFS. MCR's families are assigned to the
> abstract families below in `codetracer-specs` (`spec/Trace-Files/CTFS-Binary-Format.md` §2), and
> that assignment is subject to change in each release. The boundary between the two repositories is
> `codetracer-specs/spec/Trace-Files/README.md`.

> **Status (2026-10-26): decided (`CKF-3`).** This document
> states the goals the container must keep (§1), the cost model every keyed structure pays (§2), the
> properties of the data that permit cheaper structures (§3), the anatomy shared by every keyed
> member (§4), the abstract families and their candidate realizations (§5), how a writer architecture
> constrains them (§6), and what the benchmarks that choose a realization per family must measure and
> how they decide (§7). The benchmark suite of §7 reported on 2026-10-14 and 2026-10-24
> (`tracing-formats-benchmarks` `agents` `4855de2`, `results/ctfs_keyed/REPORT.md`). The decisions,
> each with its evidence, what would reverse it, and the items the evidence left open, are §8. The
> byte layout of every decided realization is §5.8. A writer emits a keyed layout only for a family
> that §8 assigns to it, and only from its producer's rollout (for MCR, `CKF-5`). The owner's
> answers of 2026-10-08 to the questions this document raised are §9.
>
> *Until 2026-10-26 this block read: "design specification, realizations pending benchmark … It does
> **not** choose a realization for any family, and it defines no byte layout that a writer may emit
> yet."*
>
> The container rules this document relies on are normative now and are in
> [ctfs-container.md](ctfs-container.md): the root directory is fixed at creation (§1), in-place
> record publication (§6), the writer architectures (§6), and keyed members (§7a). Which family each
> member of a materialized trace belongs to is [internal-files.md](internal-files.md) §"Member
> Catalogue".

---

## 0. Why Keyed Members

A CTFS container names its members in a **root directory** whose size is fixed when the container is
created (`ctfs-container.md` §1). Growing it by relocating data blocks was tried in October 2026 and
retired: it moved blocks under live readers and rewrote the region every reader trusts
(`codetracer-specs/issues/2026-10-07-ctfs-root-directory-growth-landed-without-a-spec-change.md`).

Some members come in **families whose size grows with the recording**: one stream per thread, one set
of parts per checkpoint, one entry per hashed page, one bucket per correlation key, one list per hit
source line. A family does not get one root member per element. It gets **one keyed member** (or a
small fixed set of them): a member that maps a `u64` key to a fixed-size record, and through the
record to a value, to an index of the family's own, or to a container stream.

The design question is not "which index structure", asked once. It is asked per family, because the
families differ in exactly the properties that decide it: how the keys are distributed, whether the
structure changes while the recorded program runs, whether a reader in another process follows it
live, which lookups are asked, and how large the values are. §3 names those properties; §5 groups the
families by them.

---

## 1. Goals The Container Keeps

These are the container's goals (`ctfs-container.md`, "Goals and properties", where each is numbered
`G1` to `G15`), restated here as obligations on keyed members. A realization that breaks one is not a
candidate, however it benchmarks. G14 (compressed storage) obliges nothing beyond §5.1's D3.

| Goal | What it obliges a keyed member to do |
|---|---|
| **G1 Self-contained, no sidecars** | Every index, table and value lives in members of the same container. A close-time index is a member, not a file beside the `.ct`. |
| **G2 Append-mostly; nothing moves** | Records, values and index nodes are appended. A block number, once published, names the same block for the container's life. The bytes that change in place are an enumerated set (`ctfs-container.md` §6, "What is mutated in place"), and every keyed realization adds its in-place stores to that set by name. |
| **G3 Single writer per member, lock-free** | A member, and so a keyed member and everything it owns, has one writer at a time. The only shared mutable state across members is block allocation (`NextFreeBlock`), and under a multi-process writer architecture (§6) the root state and any shared key allocator. No locks, no CAS retry loops; atomic fetch-and-add and atomic stores only. |
| **G4 Live out-of-process readers** | A reader on the writer's machine attaches to the live coordination page and never observes a torn, half-published or relocated record as valid; a reader that cannot attach follows seal-published state from the file under the torn-read rule (`ctfs-container.md` §6, "In-place record publication"). A realization says which of its words change in place, so they are mirrored in the page. |
| **G5 Crash durability at seal granularity** | A writer that dies leaves every record published by its last seal readable and correct (`ctfs-container.md` §6, "Durability"). A record that was mid-update when the writer died is settled by a stated rule, never by guessing. |
| **G6 Bounded per-operation work where the recorded program can feel it** | No operation performed inside the recorded process, or on a path that can stall it, takes time proportional to the size of the family or the container: no global rehash, no whole-image rebuild, no directory doubling, no compaction, no root growth. Every live operation has a stated worst-case bound. §3, P9 says which paths that covers. |
| **G7 Random access with few block reads** | A point lookup costs a small, stated number of block reads, cold and warm (§2). Block-aligned layout keeps every read an HTTP range request. |
| **G8 A fixed root that one fetch reveals** | The root region never grows. A family costs a fixed, small number of root entries regardless of its size. |
| **G9 Streaming-compatible** | A keyed member is readable while it is written; no finalization step is needed to read what was published. A close-time index may be added at close, but its absence leaves the family readable by the live structure or a scan. |
| **G10 Compact-profile compatibility** | Every keyed member either has a defined compact form (`ctfs-container.md` §1d-§1f) or a writer converting to the compact profile refuses it by name. A stored container block number (§2.3, form b) has no meaning in a compact container and is dropped or rewritten there. |
| **G11 Encryption-aware** | Nothing here requires reading a record before the key is available; keyed members are member bytes like any other. |
| **G12 Deterministic where the specification claims it** | Where a container claims that two writers given the same input produce the same bytes (`ctfs-container.md` §1f, §5, and §6 "Block placement", which makes a split-stream writer's whole container a function of the recording), a keyed member's layout, and where its appends fall in the placement order, must be a function of its inputs and not of timing. Key assignment under concurrency (§6.3) and several appending writers (W2, the hybrid, a multi-producer drainer) are the cases to watch: they forgo the property (`ctfs-container.md` §6, W2 rule 8). |
| **G13 Implementable everywhere it is read** | Nim and Rust write; Nim, Rust, Go, C#, TypeScript and Python read. A realization is judged by the reader it forces on all of them, not only by the writer. |
| **G15 Bounded dead space** | A realization states the dead bytes each operation can leave (superseded nodes, a replaced node, a dropped live index) and its dead fraction at close; nothing is reused in place; a repack removes it (§4.6; `ctfs-container.md` §6, "Dead space"). |

---

## 2. The Cost Model (normative statements, not a recommendation)

### 2.1 Random access inside a member is not free

A record number gives a **logical** offset by arithmetic. Turning that offset into a file position
walks the member's block mapping (`ctfs-container.md` §4):

| Member size (`BlockSize = 4096`) | Mapping reads before the data block |
|---|---|
| at most one block (direct, §2) | 0 |
| up to 511 data blocks, about 2 MiB (level 1) | 1 |
| up to 261,632 data blocks, about 1 GiB (level 2) | 2 |
| larger | one more per level, at most 5 |

The block mapping **is an index**: a fixed-fan-out radix tree over logical block numbers. So a family
whose keys are dense needs no *second* index -- no key-to-record search structure -- because the
container's own mapping already turns a record number into a block. It does not need *no* index.

### 2.2 What makes the walk cheap in practice

- **Interior mapping blocks are append-only except at the frontier.** A mapping slot changes once,
  from `0` to its final value, and a slot that is set never changes (`ctfs-container.md` §6, "What is
  mutated in place"). A reader may therefore cache every non-zero mapping slot for the container's
  life, and re-read only the frontier slots it holds as `0`. With a warm cache the walk costs no read.
- **A fixed-size record never straddles a block (normative for every keyed member).** Records of size
  `S` are packed `floor(BlockSize / S)` to a block, and the remainder of each block is padding that a
  writer sets to zero and a reader ignores. Record `r` is in logical block
  `first_record_block + r / per_block`, at byte `(r mod per_block) * S` of that block. A record lookup
  with a warm mapping cache therefore costs exactly one data-block read, and a record's bytes are
  always one contiguous range of one block, which the publication protocol of `ctfs-container.md` §6
  needs.
- A member header, where a realization has one, occupies whole records' worth of space at the start of
  the first block, or a block of its own; it never shifts the record arithmetic by a non-multiple of
  `S`.

### 2.3 How a stored reference addresses its target

An index entry, a table slot, or a record that points at another record, a value or a stream must say
*where*. Because the root is fixed and no block moves (`ctfs-container.md` §1), a container block
number is stable for the container's life, so there are three forms. Which form each family uses is
**a benchmark decision** (§7), not one this document makes.

| Form | Stored | Read cost | What it costs elsewhere |
|---|---|---|---|
| **(a) member-relative** | a record number, or a byte offset within a member | the mapping walk of §2.1 (0 reads warm) plus the target block | Nothing. The reference survives every operation that copies a member into another container (below), unchanged, and is the only form a compact container can hold |
| **(b) absolute** | a container block number and a slot (record index or byte offset within the block); in a sharded container also the shard | one read, no walk | Every stored position must be rewritten by every operation that copies the member into another container. It addresses within one block only: a range or a value spanning blocks still needs the mapping. It forces write order: the target block is written, and its number known, before the reference is stored. It makes the member meaningless in a compact container |
| **(c) both** | (a), plus (b) as a **droppable cache** | (b) when the cache is valid, (a) otherwise | The (b) part may be zeroed, or rebuilt, by a copy operation instead of being rewritten; a reader validates it (the target's key must match, §4.4) and falls back to (a) when it is zero or fails |

**The copy operations** that a form (b) or (c) reference must survive are every operation that writes
a member into a container other than the one it was born in: conversion to the compact profile
(`ctfs-container.md` §1e-§1f), slicing and export (producer tools), assembly of a container from
content-addressed parts, and appending a copied member to a closed container (`ctfs-container.md` §5).
A realization in form (b) must say, for each, whether the reference is rewritten (and at what cost) or
the operation refuses.

**Rules that hold whatever the form (normative).** A stored container block number is validated
exactly as a mapping pointer is: never `0`, never inside the root region, never past the container's
whole blocks (`ctfs-container.md` §4, "Null block pointers on the read path"). A stored block number
also obeys the address rules a mapping pointer does: it is written only after the block it names.

---

## 3. Properties Of The Data That Permit Cheaper Structures

Each property is stated with the families it applies to. The realizations of §5 exploit them; a
benchmark workload (§7) is parameterized by them.

**P1. Monotonic, append-only streams.** Most of a recording is streams that only grow: event streams,
chunk indexes, step streams. Such a stream changes in place in exactly two words (its `Size` and, on
the direct-to-mapped transition, its `MapBlock`), and a reader's view of it is a prefix that only
lengthens. A record that refers to such a stream is updated in place but never changes the order or
membership of its family.

**P2. Key shape.**

- **Dense:** the keys are `0 .. n-1` (or `base .. base+n-1`), possibly with a few holes. Checkpoint
  ordinals, bundled-file ordinals, interning ids and chunk numbers are dense by construction.
- **Clustered:** the keys fall in a few dense runs separated by large gaps. A key that is
  `(owner << k) | local` with dense `local` is clustered.
- **Sparse:** the keys are spread over a large space with no exploitable runs -- digests and hashes
  (uniform by construction), and memory addresses (clumped into mappings, sparse within and between
  them).

**P3. Writer-assigned keys.** When the writer creates every element of a family, it may *assign*
the key: number the elements `0, 1, 2, ...` in order of first appearance. A natural key that is
clustered or sparse (a thread id, an address, a hash) then becomes an attribute stored in the record,
and the family becomes dense, with a small natural-key-to-dense-key table where natural-key lookup is
needed. This is available exactly when one allocator assigns the keys; §6.3 says what that takes when
several processes write.

**P4. Order-preserving and order-changing updates.** An **order-changing** update adds a key, removes
one or changes one: it changes which keys exist or where a search finds them. An **order-preserving**
update changes a record whose key and position are fixed: a stream's `Size` growing, a `MapBlock`
transition, a fixed-size value overwritten, an extent's length growing. Order-preserving updates are
**in-place stores** at a fixed position and are published by `ctfs-container.md` §6's record
protocol. Only order-changing updates need an index structure to change, and they are where pauses
and torn structural reads come from.

**P5. Fill-once slots.** In a direct-addressed table (dense, or a fixed-fan-out radix over a clustered
or sparse key) adding a key fills a slot that has never held anything. That is an order-*preserving*
store in disguise: the slot changes once, from all-zero to its final value, exactly like a mapping
slot. Fill-once structures need no copy-on-write and no rebalancing, and a reader distinguishes
"absent" (zero) from "present" without a version; a slot read while it is being filled is caught by
§4.4's key check, not by a version.

**P6. Lifecycle.**

| Lifecycle | Written | Live readers while written? | Pause-sensitive (G6)? |
|---|---|---|---|
| **live, in-process** | by a thread of the recorded process | yes | yes, directly |
| **live, out-of-process** | by a recorder process draining the recorded process | yes | yes, through back-pressure (P9) |
| **close-time** | once, when the recording closes, from the complete key set | no | no; close latency matters |
| **query-time** | by a consumer after the fact, possibly incrementally, persisted into the container | only that consumer, usually | no; query latency matters |
| **derived offline** | by a tool that copies or slices a closed container | no | no |

A family may have one structure per lifecycle phase: a live structure while recording and a
close-time index over the same records (§5.6).

**P7. Lookup kinds.** Exact match; ordered (predecessor, successor, range scan, interval stabbing);
enumeration in key order or in creation order; enumeration of what changed since a reader's last
refresh.

**P8. Value size classes.**

| Class | Size | Stored |
|---|---|---|
| **V0 inline** | fits the record (up to 16 bytes in practice) | in the record |
| **V1 small** | tens to hundreds of bytes, fixed once written | a run in a value heap, referenced by `(offset, length)` |
| **V2 growing small** | starts small and grows by appends (a correlation bucket, a hit list) | an append-only extent chain in a value heap; the record holds the head and a length that grows |
| **V3 large or live** | one block or more, or growing while a reader follows (an event stream) | its own container stream, referenced by a stream record `(size, map_block)` |

**P9. Where pauses are felt.** G6 binds every structure updated on a thread of the recorded process.
It binds a **drainer** too: a drainer that stalls stops consuming the shared-memory rings it drains,
the rings fill, and the recorded threads block on them. A drainer can absorb a stall shorter than its
rings' headroom, so its bound is looser but it exists, and the benchmark measures both (§7). Only
close-time, query-time and offline structures are free of G6.

**P10. One container writer or several.** Whether one process writes the whole container or several
do (§6) decides whether a single allocator can assign dense keys (P3), whether block allocation and
the root are in-process or cross-process atomics, and where the per-update cost lands.

---

## 4. Anatomy Of A Keyed Member

Every realization in §5 is assembled from four parts. A family uses the parts it needs.

### 4.1 The record store (always)

Fixed-size records of size `S`, a multiple of 8, packed per block as §2.2 says, addressed by
**record number**. Normative for every realization:

1. **Records never move.** A record, once its bytes are written, stays at its position for the
   container's life. No split, rehash, compaction or rebalancing moves a record; what moves, if
   anything, is an index entry that refers to it (§4.3).
2. **A record's key never changes and a record is never reused for another key.** Removing a key is
   an order-preserving update of the record's state word to *tombstone*, not the reuse of its slot.
3. **A record is written before anything makes it reachable**: before a `Size` that covers it is
   published, before an index entry or table slot that names it is stored.
4. **A reference into another family is published only after its target is durable.** A record that
   refers to a value, record or stream of another keyed member (a snapshot record referring to pages
   in a page store, for instance) is published -- in the page or in the file -- only after the target
   has been published at a seal at or before the referring record's own seal. So a container cut at any
   seal never holds a reference whose target it lacks.
5. **In-place updates of a record use `ctfs-container.md` §6, "In-place record publication"**:
   published in the live coordination page under the page's sequence counter, written to the file at
   seals. A record carries no sequence word on disk; the realization names the words it updates in
   place, and the page mirrors exactly those.

**Record kinds.** A record carries its key (unless the key is its record number), a state
(`empty`, `present`, `tombstone`), the publication words, and a payload of one of these kinds:

| Kind | Payload | Value class |
|---|---|---|
| `VALUE` | the value itself | V0 |
| `EXTENT` | `(offset, length)` into a value heap, or the head of an extent chain and a total length | V1, V2 |
| `STREAM` | `(size, map_block)` of a container stream, with `FileEntry`'s three `map_block` forms (`ctfs-container.md` §2) | V3 |
| `MULTI` | a fixed number of the above (for example the two streams of a chunked table, or a fixed set of parts) | mixed |

### 4.2 The key map (per family)

How a key becomes a record number:

- **identity or arithmetic** (dense keys): `r = key - base`;
- **a fixed-fan-out table** (clustered or sparse keys): fill-once slots (P5), possibly two levels;
- **a search structure** (sparse keys): a hash, trie or tree whose entries hold record numbers (§5.3,
  §5.4);
- **a static index** built at close (§5.5).

### 4.3 Index structures hold record numbers, and have their own publication rule

A search structure stores `(key or key fingerprint, reference)` pairs where the reference addresses a
record (§2.3). It never stores the record itself. So when an index changes shape -- a bucket splits, a
node grows, a leaf splits -- what moves is a reference of 8 to 16 bytes, and the record stays put.

Normative: **an index change must never make a key that a following reader could already find
unfindable**, and never let a reader accept a reference to the wrong record. A realization states
which of these mechanisms it uses for each kind of change:

- **copy-on-write with a root flip**: changed nodes are appended and published, then one reference is
  switched (`ctfs-container.md` §8 is the existing instance);
- **fill-once**: the change only sets slots that were empty (P5);
- **in-place with the record protocol**: the node's changing words are mirrored in the live
  coordination page and published there under its sequence counter, and reach the file at seals;
- **link-before-shrink** (B-link, Lehman-Yao): a split publishes the new right node and the link to it
  before the old node stops covering the moved keys.

### 4.4 Self-validating references

A reader that follows a reference to a record compares the record's stored key with the key it looked
up, and treats a mismatch as "not found here", retrying from the index. A reference that was read torn
then lands on a wrong record (refused by the key comparison), outside the member (refused by the bound
check) or on an empty record (refused by its state). This makes every fill-once slot and every
form-(c) cache safe against a torn read **for exact-match lookups**, without mirroring the index in
the live coordination page, and is what makes a reader that cannot attach safe on those lookups. It does not cover an ordered lookup, which must not skip a key; ordered structures need §4.3's
mechanisms.

### 4.5 The value heap (V1 and V2 values)

An append-only member of value bytes. A V1 value is one run; a V2 value is an extent chain:
fixed-capacity extents, each written once and linked from the previous extent's tail word (a fill-once
store) or listed in the record. Appending to a V2 value writes bytes into the free tail of its last
extent (or a new extent, then the link), then publishes the record's new length -- an order-preserving
update. A value never moves; a value that outgrows its extent continues in a new one. (The pooled
sub-block scheme of `ctfs-container.md` §8, which *copies* a value to a larger slot, is not used for a
value a reader may be following.)

### 4.6 Dead space (normative)

Records never move, but other things a realization writes can stop being reachable: the old path of a
copy-on-write node change, a trie or hash node replaced by a larger one, an extendible-hash directory
replaced by its doubling, a live index dropped when a static one is built at close, a whole member
rewritten. Under G2 those blocks are never reused in place (`ctfs-container.md` §6, "Dead space").
Every realization MUST state:

- the dead bytes each kind of operation can leave (for example one root-to-leaf path per
  copy-on-write commit; one node per HAMT node growth; the old directory per extendible-hash
  doubling; zero for fill-once tables, records and value-heap extents);
- the dead fraction of the family's bytes at close, as a function of the workload parameters;
- whether it keeps, drops or rebuilds a live structure at close, and the bytes that leaves dead.

A family whose dead fraction can exceed the container's repack threshold (RECOMMENDED 25%) is either
repacked at close by its producer or rejected by the benchmark's space rule (§7.3, rule 4, which counts
dead bytes as overhead). There is no container-level free list: `ctfs-container.md` §1 explains why the
area block 0 once reserved for one was removed.

---

## 5. The Abstract Families

### 5.0 The taxonomy, and why these five

Two questions decide a realization, and they are independent:

1. **Does a lookup need a search structure, and of which kind?** That is fixed by the key shape (P2,
   P3) and the lookups asked (P7): none for dense keys; a fixed-fan-out table for clustered keys; a
   hash or trie for sparse exact-match; an ordered tree or trie for sparse ordered lookups.
2. **Does the structure change while a reader follows, or inside the recorded program?** That is the
   lifecycle (P6): live structures carry G4, G5 and G6; static ones carry none of them and can be
   bulk-built, compact and branch-free.

Live structures split along question 1 into four families; static structures are one family, because
once nothing changes the key shape changes only which static index is smallest, not the protocol.

| Family | Key shape | Lookups | Lifecycle | Search structure |
|---|---|---|---|---|
| **F1 Dense** | dense, or writer-assigned (P3) | exact, enumeration | any | none: the record number is the key (§2.1) |
| **F2 Clustered** | a few dense runs | exact, enumeration | live | a fixed-fan-out table of fill-once slots |
| **F3 Sparse exact** | sparse | exact | live or query-time incremental | a hash or trie that never pauses |
| **F4 Sparse ordered** | sparse | predecessor, range, interval | live or query-time incremental | an ordered tree or trie |
| **F5 Static index** | any | any | close-time, query-time bulk, offline | a bulk-built structure |

Other cuts were considered and rejected: by value class (it decides the record kind, §4.1, not the
structure, and it is orthogonal: a dense family can have any value class), and by producer (the same
producer has families in every row). A family that is dense *because* the writer assigns its keys is
F1, not F2 or F3: P3 is the cheapest optimization available and the taxonomy is meant to make it the
default.

Every family obeys §4.1 to §4.5. Each subsection below gives the family's invariants, how a live
reader follows it, its torn-read rule, the candidate realizations, and their trade-offs.

### 5.1 F1: Dense-keyed

**Definition.** Keys are `0 .. n-1` (or start at a fixed base), assigned in creation order or near
it. Holes are allowed and cost one empty record each.

**Invariants.** Record `k` is the record of key `k`. Adding key `n` appends record `n`; adding a key
past the end pre-extends with empty records. A record is filled once and then updated only in place.

**Live reader.** The record count is the member's `Size` divided into records; a reader re-reads
`Size` at each refresh, reads new records, and re-reads the records it follows (from the live
coordination page when attached, whose sequence counters say whether anything changed). An empty
record (state `empty`) is a hole or a record not yet published.

**Torn-read rule.** Every in-place record update follows `ctfs-container.md` §6, "In-place record
publication". A new record is written whole before the `Size` that covers it, so a reader never reads a
record past `Size`.

**Candidate realizations.**

| Id | Realization | Notes |
|---|---|---|
| **D1** | **Record array**: one member of fixed records, record `k` at the §2.2 position | the default; 0-1 reads warm |
| **D2** | **Record array plus value heap**: D1 with `EXTENT` records into a sibling heap member | the existing "Variable-Size Record Table" (`internal-files.md`) is D2 with an offset array as the records and no in-place updates |
| **D3** | **Sealed record chunks**: records grouped into compressed chunks with a chunk index (`ctfs-container.md` §7) | only for records that never change after their chunk seals; compressed, so not addressable in place |
| **D4** | **Creation-order table with explicit keys** (the "stream directory" drafted on 2026-10-07): records carry their key, unsorted, found by scan or by a reader-built index | handles keys that are not dense at the cost of an O(n) scan per cold lookup; kept as the baseline every F1 and F2 candidate is measured against |

**Trade-offs.** D1 is optimal when the fill ratio (keys present / key span) is high. Its cost is one
record per hole, so a dense family whose natural keys have holes should use writer-assigned keys (P3)
rather than natural ones. D4 tolerates any key set but reads every record on a cold lookup: 2,000
threads with two streams each are 2,000 records of 24 bytes, 12 blocks, per cold lookup.

### 5.2 F2: Clustered-keyed

**Definition.** Keys form a few dense runs at bases far apart, and the writer does not, or cannot,
assign keys (P3) -- for example because natural keys are allocated by independent processes.

**Invariants.** The map from key to record number is a fixed-fan-out table: each level indexes a fixed
number of key bits, and each slot is filled once (P5). A run's records occupy a contiguous range of
record numbers reserved when the run's first key appears.

**Live reader.** It re-reads the table's frontier slots (those it holds as `0`) and the records it
follows. A filled slot is cached for good.

**Torn-read rule.** Slots are fill-once and their targets self-validating (§4.4). Records follow the
record protocol.

**Candidate realizations.**

| Id | Realization | Notes |
|---|---|---|
| **C1** | **Two-level table**: a top-level array indexed by `key >> k`, whose slots name a segment of `2^k` records | one extra read cold; the segment size `2^k` is the cluster granularity and wastes the unused part of each segment |
| **C2** | **Radix table**: fixed fan-out per level over the key's bits, depth fixed by the key width, like the block mapping itself | handles any key distribution with bounded depth; wastes a node per sparse branch |
| **C3** | **Remap to dense**: writer-assigned dense record numbers (F1) plus a natural-key table, live (C1 or C2 over the natural key) or close-time (F5) | turns the family into F1 whenever one allocator exists; the natural-key table is small and can be static |

**Trade-offs.** C1 is compact when runs are few and wide; C2 when keys are spread. C3 is the cheapest
whenever one writer assigns the keys (P3), and it moves natural-key lookup off the hot path.

### 5.3 F3: Sparse exact-match, live

**Definition.** Keys are sparse (hashes, digests, addresses), lookups are exact, and the structure is
updated while it is read or inside the recorded program.

**Invariants.** Records are appended in creation order (F1 by creation number) and never move. A
search structure maps keys (or fingerprints) to record numbers. No insert performs work proportional to
the number of keys: **no global rehash**, which is why plain resizable hash tables were rejected for
in-process use (a resize pauses the recorded program).

**Live reader.** Looks a key up through the current structure, confirms the key in the record (§4.4),
and on a miss during a structural change retries once after re-reading the structure's root words.

**Torn-read rule.** References are self-validating (§4.4). Node or bucket updates that are not
fill-once use the record protocol or copy-on-write (§4.3).

**Candidate realizations.**

| Id | Realization | Worst-case insert | Notes |
|---|---|---|---|
| **H1** | **Linear hashing** (Litwin) | splits one bucket per insert, so bounded; overflow chains bound lookups only on average | buckets hold `(fingerprint, record number)`; a split moves references, not records |
| **H2** | **Extendible hashing** | a directory doubling is O(directory) unless the directory is itself a two-level table filled once | bucket splits are local; the directory is the pause risk |
| **H3** | **Split-ordered list** (Shalev-Shavit) | bounded; the bucket array grows by doubling, lazily, and the list never reorganizes | the list nodes can *be* the records (a `next` word per record, fill-once), so inserting is a pointer store |
| **H4** | **Hash trie / HAMT** over the key's bits | bounded by depth (`64 / bits_per_level`); no resize ever | with fixed-size nodes of fill-once slots it needs no copy-on-write; with compressed (bitmap) nodes a node grows by copy and pointer switch |
| **T1** | **Crit-bit / Patricia trie** | bounded by key width | ordered as a bonus; one internal node per key |
| **T2** | **ART or qp-trie** | bounded by key width | ordered; node growth (4, 16, 48, 256) replaces a node and switches its reference |
| **B1** | **B+tree with in-place leaves** (§5.4) | a split is O(node), bounded | ordered; listed because one structure serving F3 and F4 may beat two |

**Trade-offs.** Hash structures give the fewest reads per exact lookup and the smallest nodes, but no
order. H4 with fill-once nodes is the closest to the container's own mapping (P5) and has no
structural update a reader can tear; its cost is space on sparse branches. H1 and H2 have the best
space but buckets that change in place. H3 has the cheapest insert and an unusual reader (it follows a
list). T1 and T2 serve F4 as well.

### 5.4 F4: Sparse ordered or interval, live or incremental

**Definition.** Keys are sparse and lookups need order: predecessor (the interval containing a point),
successor, range scans.

**Invariants.** Records never move. The structure orders references; changing it must never hide a key
from a reader that is scanning in order (§4.3: copy-on-write or link-before-shrink, never a bare
in-place shift).

**Live reader.** Descends from a published root (double-read of root slots, `ctfs-container.md` §8) or
follows right-links after a split; confirms the record's key.

**Torn-read rule.** A leaf or node updated in place is a record under the record protocol; a reader
that sees a node mid-update retries the node, not the whole descent.

**Candidate realizations.**

| Id | Realization | Notes |
|---|---|---|
| **B1** | **B+tree with in-place leaves and right-links** (an `NSB2` page format): leaf entries at fixed positions (interleaved `(key, reference)` or references at a fixed base), so an insert shifts nothing a reader already read; a leaf's changing words mirrored in the live coordination page; splits publish the right leaf and the link before shrinking the left | `NSB1`'s leaf layout ("all keys, then all descriptors") shifts every descriptor on an insert and cannot be updated in place under a reader |
| **B2** | **Copy-on-write B+tree** (`NSB1`, `ctfs-container.md` §8) | correct today; every insert copies a leaf-to-root spine (about 7.5 KiB of superseded pages per key measured by the Nim builders), so suitable only for low insert rates |
| **T1, T2** | crit-bit, ART or qp-trie (§5.3) | ordered iteration; node replacement needs a reference switch |
| **M1** | **Monotone run**: when keys arrive in increasing order, records appended in key order plus a fence array of each block's first key (F1) | no search structure at all; a predecessor lookup is a binary search over the fences; only applicable when the insert order is the key order |

**Trade-offs.** B1 has the fewest reads for range scans and the best fan-out, at the cost of the most
complex publication. B2 is simple and already implemented but pays a spine copy per insert. M1 is free
when its precondition holds.

### 5.5 F5: Static index (close-time, query-time bulk, offline)

**Definition.** The index is built once from the complete set of records, after the last insert, and
published by one root-entry publication. Nothing reads it while it is built and nothing changes it
afterwards (a query-time consumer that changes it rebuilds and republishes it whole).

**Invariants.** G6 does not apply. Build time and memory at close (or at query time) are the costs.
The index references records by form (a) or (c) (§2.3). The live structure it replaces, if any, may be
dropped at close or kept.

**Live reader.** None during the build. A reader that finds the index absent (a crashed recording)
falls back to the live structure or a scan (G9).

**Torn-read rule.** None needed: the index is published once, after it is complete, by the member's
entry.

**Candidate realizations.**

| Id | Realization | Notes |
|---|---|---|
| **S1** | **Sorted record array with block fences**: records (or references) in key order, plus a fence array of first keys, optionally in Eytzinger order | the `step-map.ns` shape; exact and ordered; one or two reads warm |
| **S2** | **Bulk-loaded B+tree** (`NSB1` `bulkLoad`) | what `linehits.tc`, `corrmark.ns`, `cppages.ns` and `snappages.ns` use today |
| **S3** | **Static hash**: bucketized open addressing at a fixed load, or a minimal perfect hash (CHD, BBHash, PTHash) over the final key set | exact only; one read per lookup at low space |
| **S4** | **Succinct monotone sequence** (Elias-Fano) for monotone keys | smallest; ordered; slower to decode |
| **S5** | **F1 itself** when the keys are dense | no index |

**Trade-offs.** S3 for exact lookups at the smallest read count; S1 or S2 when order is needed; S4
when space dominates and keys are monotone. The benchmark decides.

### 5.6 Two structures over one record store

A family may keep a live structure (F2 to F4) while recording and build a static index (F5) over the
same records at close. Both refer to the same record numbers; the live one is never rewritten to
become the static one. A reader prefers the static index when it is present and falls back to the
live structure otherwise. A realization that does this states which structure a writer keeps after
close and whether the live one may be omitted by a converter (for example to the compact profile).

### 5.7 A sparse family that must be durable at every seal

A family whose records other records reference live (rule 4 of §4.1) cannot wait for close to become
readable. If its keys are sparse it has three shapes, and which one a family uses is a benchmark
decision (§7):

| Shape | While recording | A reader of a container cut at a seal | Dead space |
|---|---|---|---|
| **Live index** (F3) | records appended in first-seen order; a never-pausing hash or trie (H1-H4, T1-T2) updated per insert and published in the page, and in the file at each seal | looks a key up in the index as of that seal; one structure to read | per the realization (§4.6); none for fill-once nodes |
| **Index rebuilt per seal** (F5 at seal granularity) | records appended in first-seen order; at each seal a static index (S1-S3) over every record so far is built and published, the previous one becoming dead | looks a key up in the last published index; one structure to read | the previous index at every seal: quadratic in the number of seals unless bounded, so a realization rebuilds only every `k` seals or levels its runs (next row) |
| **Leveled static runs** (log-structured merge) | at each seal a small static index over the records first seen since the previous seal is published as a new run; runs are merged geometrically, so each record is re-indexed `O(log n)` times | probes the runs newest first, `O(log n)` runs at most; a key is found in exactly one run | the merged runs: bounded by a constant factor of live index bytes |

In every shape a reader of a cut container finds every record published at or before the cut and no
reference whose target is missing (§4.1, rule 4). A record first seen after the last seal is absent,
not dangling. The shapes differ in the reader (one index, or several runs), the per-seal cost on the
writer (an insert per key; a rebuild; a small build plus amortized merges) and dead space.

### 5.8 The byte layouts of the decided realizations (normative)

Decided on 2026-10-26 (`CKF-3`, §8). This section gives the byte layout of every realization §8
adopts. A producer's catalogue names the members, defines its records' fields, and says which
realization each family uses: [internal-files.md](internal-files.md) §"Member Catalogue" for
materialized traces, and `codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md` §2 for MCR.

The layouts are taken from the implementation the benchmark measured: `tracing-formats-benchmarks`
`agents` `4855de2`, `ctfs_keyed/src/` (cited below as `src/…`), whose report is
`results/ctfs_keyed/REPORT.md` (cited as REPORT.md). The Nim ports in `nim_finalists/` write the same
bytes, and the Rust reader verified every container they wrote (REPORT.md §15). Where
the benchmark took a shortcut, this section says so and specifies the production form instead.

#### 5.8.0 Conventions for every layout below

- **Integers** are little-endian. A `u64` field sits at a member offset that is a multiple of 8,
  unless a layout says otherwise.
- **References are member-relative** (form (a), §2.3; §8, DR-10). A reference is a record number, a
  node number (a node is one 4096-byte block of its member, node `n` at member offset `n * 4096`), or
  a byte offset into a heap member. The only container block numbers in a keyed member are those of
  its keyed streams (`MapBlock` and their mapping, `ctfs-container.md` §7a rule 6).
- **Records** follow §2.2: `per_block = floor(4096 / S)`, record `r` at member offset
  `(r div per_block) * 4096 + (r mod per_block) * S`, and each block's tail is zero. The number of
  records is the number of whole records below the member's `Size`:
  `(Size div 4096) * per_block + min((Size mod 4096) div S, per_block)` (`src/store.rs:700-719`).
  A new record is written whole, and the zero tail of the block before it if the record does not fit
  there, before any `Size` covers it (`src/store.rs:731-742`).
- **A header member** is exactly 256 bytes: 32 `u64` words `w0` to `w31`, with unused words zero. It
  holds a realization's root words, which are updated in place. The writer writes it whole, in one
  256-byte write, at every seal where a word changed (`src/families/mod.rs:80-107`). Its words are
  mirrored in the live coordination page (`ctfs-container.md` §6) like any in-place words. A header
  member whose `Size` is 0 (never sealed) reads as all-zero words (`src/families/mod.rs:109-121`).
- **Index blocks** (B1's nodes, H1's buckets) are whole 4096-byte blocks of their own member. A
  member that holds nodes is padded with zeros to a multiple of 4096 before a node is appended
  (`src/families/btree.rs:134`).
- **The hash** that H1 and S3 use is splitmix64 (`src/util.rs:30-35`), over `u64` arithmetic modulo
  `2^64`: `z = k + 0x9E3779B97F4A7C15; z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9;
  z = (z ^ (z >> 27)) * 0x94D049BB133111EB; h(k) = z ^ (z >> 31)`.
- **A seal runs in phases.** Each phase hands its buffered bytes to the operating system in block
  order. It then publishes the in-place words and root entries that cover those bytes, in the page
  first and in the file after (`ctfs-container.md` §6, "Writing through the page"). A phase begins only
  after the previous phase's writes have returned (`src/families/mod.rs:129-136`). Each realization
  below lists its phases. They are what makes every key a reader could already find still findable
  after a kill at any point of a seal (§4.3). The crash matrix tested this with 201 kills per shape
  (REPORT.md §14).
- **Before the first seal, an empty family.** A keyed member that is absent, or whose header or
  record member has `Size` 0, reads as an empty family. A lookup answers "absent", and an enumeration
  returns nothing. A reader MUST NOT refuse it as damage. (The crash matrix logged 9 refusals of this
  kind, all killed before any seal. REPORT.md §3.2 and §14 found that the reader should answer
  "empty" instead.)
- **Shrinking an index block: the hole fill (production form).** Sometimes a split leaves an index
  block holding fewer entries than before: a B1 node, or an H1 bucket block, whose moved entries are
  already published in another block. The writer then does not rewrite the surviving entries in new
  positions:
  1. It fills each vacated slot below the new count with a surviving entry taken from the highest
     slot at or above the new count, one entry (16 bytes) per write.
  2. Only then does it store the new count.

  So every surviving entry is, at every instant, at a slot below whichever count a reader holds. A
  slot read torn holds one of two things. It may hold a moved entry, whose key the published high
  key or bucket address now sends elsewhere. Or it may hold a survivor that is still at its old slot
  too. An exact-match lookup confirms every candidate against its record (§4.4), so neither case can
  give a wrong answer. *Benchmark shortcut:* the harness rewrote the block whole, sorted for B1
  (`src/families/btree.rs:170-175`) and compacted for H1 (`src/families/hash.rs:116-125`, `:216-219`).
  That is safe against a kill, because each rewrite is one whole-block write, and the 201-kill matrix
  saw no FAIL (REPORT.md §14). It is not safe for a reader that reads the block while it is rewritten,
  and the benchmark measured no such reader on these blocks (REPORT.md §3.3).
- **Appending entries: entries, then count (production form).** The new entries' slots are written
  first, and the count after them, in a separate write. *Benchmark shortcut:* the harness buffered
  both in one flush. The flush writes a block's ranges in ascending offset order (`src/store.rs:497-500`),
  so the count went to the file before the entries. A file-only reader could then see a count that
  covers zeroed slots. The record check absorbs that, but a producer MUST NOT rely on it.

#### 5.8.1 D1, the record array, and D2, the value heap (F1)

- **D1.** One member of fixed-size records of size `S`, a multiple of 8. Record `k` is the record of
  key `k` (or `k - base`). The producer's catalogue defines the record's fields, which of them are
  updated in place, and whether a record can be empty. A family whose records can be empty (a hole, or
  a reserved record) carries a **state** field that the record defines: `0` empty, `1` present,
  `2` tombstone. A family that never leaves a hole needs none: record `r` is present when it lies below
  the member's `Size`.
- **D2.** D1 plus a **value heap**: a member of value bytes, appended and never moved (§4.5). A V1
  value is an `EXTENT`, `(offset: u64, length: u64)`, in the record. A V2 value is an extent chain
  (§5.8.6). A value is written to the heap before the record that references it.
- **In-place updates** are the ones the producer's record definition names. Each is published through
  the page as `ctfs-container.md` §6 says, with the record kind's store order. A record that is written
  whole once and never changed (a snapshot record, a bundled-file record) has no in-place words. Only
  its member's `Size` changes.
- **Dead space:** none.

#### 5.8.2 C1, the two-level table over a clustered key (F2)

Two members: the **record member**, which is D1 with the producer's records, and the **top table**.
The segment size is `2^k` records, and `k` is fixed per family by the producer's catalogue (MCR's
per-thread family: `k = 6`, §8, DR-3).

- **The top table** is an array of `u64` slots. Slot `i`, at member offset `8 * i`, belongs to segment
  `i = key >> k`. Its value is the segment's first record number plus 1, or `0` when the segment has
  not been reserved. Every slot is fill-once (P5).
- **Reserving a segment.** The writer appends `2^k` zero records to the record member. They are
  covered by its `Size`, each in state empty. At the seal, it then appends zero slots to the top table
  up to slot `i`, and stores slot `i` in one 8-byte write. The records come before the slot
  (`src/families/dense.rs:155-175`, `:264-293`). Segments are reserved in the order their first key
  appears.
- **The record of a key** is `(slot - 1) + (key & (2^k - 1))`. A reader confirms it by the key field
  of the record and by its state (`src/families/dense.rs:379-391`, `:418-425`). A key whose slot lies
  past the top table's `Size`, a slot of `0`, a record at or past the record count, a key mismatch or
  an empty state each mean "absent".
- **Creating a key's record** fills a reserved, empty record in place: the whole record is written to
  the file, and then its state becomes present. That state change is an in-place update, mirrored in
  the page.
- **Enumeration** reads every record and skips the empty ones.
- **Cost:** every hole in a segment is an empty record. REPORT.md §4 measured 50% to 55% of records
  wasted on holed sets.
- **Dead space:** none.

#### 5.8.3 B1, the B-link tree with in-place nodes (the `NSB2` node format)

B1 serves a sparse family that must be durable at every seal (§5.7, the "live index" shape). It has
two members, a **header** and a **node member**. Its references are record numbers in the family's
record member.

**Header** (256 bytes, §5.8.0):

- `w0` is the root's node number.
- `w1` is the node count: node numbers `0 .. w1 - 1` are allocated.
- `w1 = 0` means an empty tree, which is the state before the first seal. Node 0 is the initial root
  leaf.

**Node** (one 4096-byte block of the node member, node `n` at `n * 4096`; `src/families/btree.rs:64-77`):

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | `kind`: `1` leaf, `0` internal |
| 1 | 1 | `0` |
| 2 | 2 | `count: u16`, the entries in use, at most 254 |
| 4 | 4 | `0` |
| 8 | 8 | `high`: every key this node covers is below it; `0xFFFF_FFFF_FFFF_FFFF` means no bound (the rightmost node of its level) |
| 16 | 8 | `right`: the right sibling's node number plus 1; `0` for none |
| 24 | 8 | `first`: internal, the child for keys below every separator; leaf, `0` |
| 32 + 16·j | 8 | `key[j]` |
| 40 + 16·j | 8 | `ref[j]`: leaf, a record number; internal, a child's node number |

- **Entries are unsorted.** Each node keeps its entries in the order they arrived, so an insert shifts
  nothing a reader may already have read. Slots at or above `count` are pending, and a reader ignores
  them.
- **A leaf** maps `key[j]` to record `ref[j]`. Several entries may share a key (a truncated key), and
  each is a candidate.
- **An internal node** sends key `k` to the `ref` of the largest `key[j]` that is at most `k` and below
  `high`, and to `first` when there is none.

**Insert** (`src/families/btree.rs:79-131`). The writer descends to the leaf through its in-memory
copy.

- If `count < 254`, it appends the entry at slot `count`.
- Otherwise the node splits:
  1. Sort its 254 entries and the new one by `(key, ref)`. Let `mid = 127`, and let `sep` be the key
     at `mid`.
  2. A leaf keeps `[0, mid)` and gives `[mid, 255)` to a new right node. An internal node keeps
     `[0, mid)` and gives `(mid, 255)` to the new right node, whose `first` is the `ref` at `mid`.
  3. The new node takes the old node's `high` and `right`. The old node takes `high = sep` and a
     `right` that names the new node.
  4. `(sep, new node)` is inserted into the parent. When the root splits, a new internal root is
     created with `first` the old root and the one entry `(sep, new node)`.
- Node numbers are allocated in creation order.

**Seal** (`src/families/btree.rs:132-183`, `nim_finalists/pagestore_bench.nim:366-477`). A seal runs
after the family's new records are published (§5.8.0, phases).

- **Phase A.** Every node created since the last seal is written whole. The node member's `Size` is
  published.
- **Phase B.** For every node that existed before:
  - If it split since the last seal, store its new `high` and `right` in one 16-byte write at offset
    8. This is link-before-shrink.
  - Otherwise, if entries were appended, write the new slots, then `count` (§5.8.0, "entries, then
    count").

  Then the header's `w0` and `w1`. Publish.
- **Phase C.** Each node that split since the last seal shrinks to its left half by the hole fill
  (§5.8.0). Publish.

The writer keeps each node's physical slot order in memory, so that phase C can apply the hole fill.

**Lookup** (`src/families/btree.rs:208-279`):

1. Read the header (from the page when attached).
2. Descend from `w0`. At each node, if `key >= high` and `right != 0`, move to node `right - 1` on the
   same level. Otherwise, at a leaf, scan slots `[0, count)` for the key; at an internal node, take the
   child by the rule above.
3. Confirm every candidate against its record (§4.4).

A reader refuses as damage a `count` above 254, or a `kind` other than 0 or 1.

A link to a node whose block lies past the node member's `Size` as the reader last read it, or a
descent that visits more nodes than that `Size` holds, may mean only that the reader's view is stale.
Phase A publishes new nodes before phase B links them, but a reader can read the `Size` before phase A
and the link after phase B. So the reader first re-reads the header and the member's entry and
descends again, and only if it recurs is it damage. *(The benchmark's reader checked only `count`,
`src/families/btree.rs:199-206`.)*

A lookup that finds nothing re-reads the header and descends once more before it answers "absent"
(§5.3, "Live reader").

**Attached readers.** B1's in-place words are:

- the header's `w0` and `w1`, mirrored as one 2-word slot;
- each node's word 0 (`kind` and `count`), `high` and `right`, mirrored as one 3-word slot per node,
  in record-mirror extents keyed by the node member's root entry index and the node number.

A node change goes to the file first (the slots, the hole fill) and to the node's mirror slot after
(`seq` odd, payload, `seq` even). An attached reader reads a node's block from the file between two
loads of that node's mirror `seq`, and reads the node again when they differ. *Not measured:* the
KF-P runs followed the file alone, without the page (REPORT.md §3.3). §8.3, O-6, names the
measurement.

**Dead space:** none. No node is ever abandoned (REPORT.md §3.2, rule 4: 0).

**Copies:** every reference is a record number or a node number, so a copy operation copies the
members unchanged.

#### 5.8.4 H1, linear hashing (F3, live)

Three members: a **header**, the **bucket member** (primary blocks) and the **overflow member**.

**Header** (256 bytes):

| Word | Field |
|---|---|
| `w0` | level `L` |
| `w1` | split pointer `p` |
| `w2` | bucket count `nb = 2^L + p` |
| `w3` | key count |

`nb = 0` means an empty table (`src/families/hash.rs:209-214`).

**Bucket block** (4096 bytes; `src/families/hash.rs:13-23`):

| Offset | Size | Field |
|---|---|---|
| 0 | 2 | `count: u16`, at most 255 |
| 2 | 6 | `0` |
| 8 | 8 | `next`: the next overflow block's number plus 1; `0` for none |
| 16 + 16·i | 8 | `key[i]` |
| 24 + 16·i | 8 | `ref[i]`: a record number |

Bucket `b`'s primary block is at `b * 4096` in the bucket member. Overflow block `o` is at `o * 4096`
in the overflow member, numbered in allocation order. A block dropped before it was ever written
still occupies its number, and is extended as zeros (`src/families/hash.rs:202-207`).

- **Address** (`src/families/hash.rs:86-93`): `hk = h(key)`; `b = hk mod 2^L`; if `b < p`, then
  `b = hk mod 2^(L+1)`.
- **Insert** (`src/families/hash.rs:94-174`):
  1. Append the entry to the first block of `b`'s chain whose `count` is below 255. If there is none,
     allocate the next overflow block and link it from the chain's last block.
  2. Increment the key count. If it now exceeds `0.75 * nb * 255`, split bucket `p`:
     - every entry of `p`'s chain with `h(key) mod 2^(L+1) = p + 2^L` moves to the new bucket
       `p + 2^L`;
     - trailing overflow blocks left empty are dropped, and become dead;
     - `p` increases by 1, and when it reaches `2^L`, `L` increases by 1 and `p` returns to 0.

  At most one split happens per insert, so no insert does work proportional to the table.
- **Seal** (`src/families/hash.rs:175-224`, `nim_finalists/corrmark_bench.nim:86-158`):
  - **Phase A.** Buckets created since the last seal are written whole, with their chains. Every
    overflow block allocated since the last seal is written whole, and the overflow member is
    extended to its allocated blocks. Publish.
  - **Phase B.** The header. Publish.
  - **Phase C.** In pre-existing blocks: appended entries, then counts; removals by the hole fill; and
    every `next` that changed. Publish.
- **Lookup** (`src/families/hash.rs:242-269`): read the header, compute the address, and scan
  `[0, count)` of the primary block and of every block its `next` chain reaches. Each candidate is
  confirmed against its record. A chain longer than the overflow member's block count is damage.
- **Attached readers:** the header's four words, as one mirror slot, and each block's word 0 and
  `next`, as one 2-word slot per block.
- **Dead space:** the overflow blocks that splits drop: 74 to 438 KB at 10^6 correlation markers
  (REPORT.md §7, rule 4).

#### 5.8.5 S3, the static bucketized hash (F5)

S3 is one member written once, after its last record exists, and published by one root-entry
publication. The member is exactly `nb` buckets of 4096 bytes. *The benchmark embedded the image at an
offset and kept `nb` in a header (`src/workloads/kfm.rs:184-191`). In the production form the member
holds only the image, and `nb` is its `Size div 4096`.*

- **Size:** `nb = max(1, ceil(n / (511 * 0.9)))` for `n` entries (`src/families/statics.rs:14-23`).
- **Bucket:**

  | Offset | Size | Field |
  |---|---|---|
  | 0 | 2 | `count: u16`, at most 511 |
  | 2 | 1 | `overflow: u8`; `1` when a probe passed this bucket full |
  | 3 | 5 | `0` |
  | 8 + 8·i | 4 | `fp[i] = h(key) >> 32` |
  | 12 + 8·i | 4 | `ref[i]`: a record number (`u32`) |

- **Build** (`src/families/statics.rs:27-55`). Entries are inserted in ascending `(key, ref)` order,
  so that the image is a function of its entries (G12). *The benchmark inserted them in hash-map
  iteration order (`src/workloads/kfm.rs:185`), which is not deterministic.* Each entry goes into
  bucket `h(key) mod nb`. When that bucket is full, the entry probes to `(b + 1) mod nb`, setting
  `overflow` on every full bucket it passes.
- **Lookup** (`src/families/statics.rs:57-80`):
  1. Start at `h(key) mod nb`.
  2. Collect every `ref` whose `fp` matches.
  3. Continue to the next bucket while `overflow` is set, for at most `nb` buckets.

  Each candidate is confirmed against its record.
- **Limit:** a family that can hold `2^32` records or more MUST NOT use S3. Its writer refuses,
  naming the member.

#### 5.8.6 V2 values: extent chains in a value heap

An extent of `E` bytes (`src/workloads/kfm.rs:4-10`, `:104-145`):

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | `next`: the heap offset of the next extent; `0` for none |
| 8 | 4 | `used: u32` |
| 12 | 4 | `0` |
| 16 | `E - 16` | `floor((E - 16) / entry_size)` entries |

A successor extent is always appended after its predecessor, so it never lies at heap offset 0.

- **The record** holds the head extent's offset, the tail extent's offset and the entry count `n`.
  The producer may add a byte count.
- **To append**, the writer does one of two things:
  - writes the entry into the tail extent's next free slot and updates its `used`;
  - or appends a new extent, written whole with the entry in it, and then stores the old tail's `next`
    (a fill-once store).

  Then it updates the record's tail, count and bytes in place.
- **A reader takes the entry count from the record, never from `used`.** Extents fill in order, so
  extent `i` of a chain holds `min(per_extent, n - i * per_extent)` entries. `used` is the writer's
  convenience. A reader that sees it disagree with that ignores it.

#### 5.8.7 M1, the monotone run (F4, keys inserted in ascending order)

M1 has two members. The **records**, in key order, are D1, and each record carries a key, its value
and a state. The **fences** member holds one `u64` per record block: the key of that block's first
record, appended when that record is appended (fill-once) (`src/workloads/kfv.rs:89-104`).

- **Predecessor of `t`** (`src/workloads/kfv.rs:230-286`):
  1. Binary-search the fences for the last one at or below `t`.
  2. Scan that block for the last record whose key is at or below `t`.
  3. If that record is a tombstone, continue with the predecessor of its key minus 1.
- **An insert whose key does not exceed the last key** is refused by the writer, naming the member.
  M1 applies only when keys arrive in ascending order.
- **Dead space:** none.

#### 5.8.8 Realizations kept as they are specified elsewhere

- **S2** is the `NSB1` image of `ctfs-container.md` §8a, unchanged.
- **B2** is the `NSB1` copy-on-write tree of `ctfs-container.md` §8, with its "Live and incremental
  publication" commit: a commit appends only the pages it changed and flips a root slot.

---

## 6. Writer Architectures And What They Do To Families

The generic rules are `ctfs-container.md` §6, "Writer architectures". This section states their
consequences for keyed members.

### 6.1 W1: one container writer

One process writes the container and hosts its live coordination page; producers in other processes
hand it their data (through shared-memory rings, for instance). `NextFreeBlock`, the root state and
every key allocator are that writer's state, kept in the page so that readers can see them, and
"multiple writers" means threads of that process writing different members. Consequences:

- Every family whose elements the writer creates can have **writer-assigned dense keys** (P3), in
  order of first appearance.
- Every in-place store is made by one process, so each page sequence counter has one writer.
- The writer's own stalls are back-pressure on the producers (P9).

### 6.2 W2: several writer processes over a shared root state

Each producing process writes its own members directly into the container file, at offsets, with
`pwrite` or `WriteFile`. The processes coordinate through the live coordination page, which holds the
root state (`NextFreeBlock`, the entries, the mirror of in-place words, and any family counters).
Consequences:

- A member still has one writer (G3): a family whose elements are written by several processes is
  either partitioned into per-process members or has records that each process owns individually.
- A family-wide dense key needs an atomic counter in the shared page.
- Per-update costs (compression, file I/O, publication) land on the producing process, which for a
  recorder is the recorded program.

A **hybrid** keeps the root state with one coordinator (it alone writes the root region and assigns
family keys) and lets producers write their members' data blocks themselves.

**W1g** is W1 with sealing moved into the producers. A producer encodes and compresses its own chunks
into shared memory, and the one container writer only writes them. The container is W1's. It is the
form a producer that may make no file-system call can take (MCR's Windows stage 0).

**W2 with helper threads.** A W2 producer may move its per-chunk work (compressing, writing,
publishing) from the thread that seals a chunk to helper threads of its own process. The container is
W2's, so this is not a format choice. The arrangement MCR uses is `w2hf`: one polling helper per
recorded thread, a queue of 32 raw chunks per thread, and an inline fallback. Its measurements are in
§8.2 (DR-11), and its normative form is in `codetracer-specs` `CTFS-Binary-Format.md` §2.1.

### 6.3 Dense keys under concurrency, and determinism

A dense key assigned by a shared counter (W2), or by a W1 writer that numbers elements in the order it
first sees them from several producers, depends on timing: two runs of the same program can number the
same elements differently. That is acceptable only where the key is a storage detail. Normative: **a
reader or a replay MUST take a writer-assigned key from the container, never recompute it by
allocating again.** Where the specification claims byte-identical output for identical input (G12),
the writer-assigned order must be a function of the input (for example, assigned at close in natural-key
order), or the claim is withdrawn for that family by name. Block placement is the same question one
level down: a W2 or hybrid writer claims blocks in the order processes reach the counter, so its
container is never byte-reproducible (`ctfs-container.md` §6, "Block placement" and W2 rule 8).

---

## 7. What The Benchmarks Must Measure, And How They Decide

The choice of realization for each family, of the address form (§2.3) and of the writer architecture
is made by a synthetic benchmark suite
in `tracing-formats-benchmarks` (domain `ctfs_keyed`), whose plan is that repository's
`ctfs_keyed/PLAN.md`. This section is the requirement that plan implements; the plan adds workloads,
harness and procedure.

### 7.1 Workloads

One workload per family, **parameterized from measured key distributions**, not from guesses: thread
counts and thread-id sets, checkpoint counts, page-store key counts, correlation keys per recording,
hit lines against total lines, interval insert patterns, write-address distributions. The plan names
the corpus each parameter comes from; a parameter with no measured source is marked as such and swept.

### 7.2 Metrics (each per family, per candidate, per address form)

1. **In-process insert and update latency**: p50, p99, p99.9 and **maximum** over at least 10^6
   operations, on the writer thread, with the container in a real file. The maximum is the pause
   metric of G6.
2. **Block reads per lookup** by an out-of-process reader through an instrumented block source, cold
   (empty cache) and warm under a **bounded** mapping-and-block cache (16, 64 and 256 blocks), for
   each address form.
3. **Space overhead**: container bytes per key beyond the payload bytes, at each key count.
4. **Behaviour under a concurrent live reader**, both attached to the live coordination page and
   unable to attach: lag (time from the writer's publication to the reader's acceptance), retry rate,
   and **torn-read detection**: a harness that tears writes deliberately (writes a word's bytes in
   random order and pauses mid-update) while readers hammer; the required count of accepted torn values
   is zero for an attached reader, and is measured (with the content validations of the torn-read rule)
   for a reader that cannot attach.
5. **Close-time build cost**: time and peak memory to build each F5 candidate from the final record
   set, at each key count.
6. **Copy cost** for forms (b) and (c): time and bytes written to copy the family into a new
   container (compact conversion, slice, export), and correctness of the copied references.
7. **Crash behaviour**: a writer killed at random points; the container must read correctly through
   the last seal, and an in-flight record must be settled by the stated rule.
8. **Writer architecture** (W1, W2, hybrid): end-to-end write throughput; latency added on the
   producing thread per event and per sealed chunk (p50, p99, max); drainer CPU and memory against
   per-process cost; live-reader lag; crash tests killing one writer process mid-chunk; scaling with
   process and thread counts.
9. **The live coordination page**: the cost of publishing through it (per update and per seal,
   against publishing to the file alone), its resident size at each key count, how often it
   overflows, and writer-death recovery (time for a survivor or a reader to detect a dead writer and
   settle its words; that the file reads correctly through each writer's last seal with the page
   gone).
10. **Dead space**: unreachable bytes as a fraction of the container at close, and its growth per
    operation and per query-time persist, for each candidate (§4.6).

### 7.3 Decision rules

Applied in order, per family:

1. **Correctness gates (hard).** Zero accepted torn reads by an attached reader; zero lost or wrong
   records after a crash through the last seal; byte-exact round trip including after each copy
   operation. A candidate that fails any gate is out.
2. **Pause gate (hard, live families only; decided 2026-10-08).** Maximum insert or update latency on
   the writer thread at or below **69 µs** (the average cost of one sealed-chunk publication,
   `ctfs-container.md` §6 "Durability"), at every key count in the workload up to its stated maximum.
   A structure whose maximum grows with the key count fails, whatever its median.
   **How "maximum" is read (owner, accepted 2026-10-19, worded 2026-10-20).** The benchmark found
   that, read literally, this rule excludes every shape that is durable at every seal. A seal writes
   the index blocks its new pages dirtied, and that is hundreds of 4 KiB writes, while the budget fits
   one. Meanwhile a timed no-op on the shared host reached 122 µs (REPORT.md §3.1). The rule is
   therefore read structurally:
   - **A pause counts only if it recurs at the same operation in every run**, with the key stream
     seeded so that the same operation index is the same structural event in every run. Its time is
     the **minimum over the runs**, and it must be at most 69 µs.
   - **A seal's index cost must be bounded per new page, and must not grow with the store.** This is
     tested two ways. Within a run, the later seals' first tenth is compared with their last tenth
     while the store grows about 5.9×. Across key counts, the seal cost per new page is compared.
     The report failed a shape whose seals grew more than 2× within a run (REPORT.md §3.2). That
     threshold decided DR-2.
   - Host jitter does not survive the minimum, and a deterministic cost does, whether it is the
     realization's own or its allocator's. Single maxima stay in the report's tables as evidence, and
     decide nothing.
3. **Reads.** Among the survivors, the fewest warm block reads per lookup at p99 under the 64-block
   cache; then cold reads. Ties within 10% go to the next rule.
4. **Space** (live and dead bytes together, metrics 3 and 10), then **build cost** (F5), then
   **insert p99**.
5. **Economy.** A realization already chosen for another family wins a tie within 10% on rules 3 and
   4 together, because every realization is implemented in every reader (G13).

For the address form: form (a) unless form (b) or (c) removes at least one block read per lookup at
p99 warm *and* its copy cost (metric 6) adds at most **10%** to every copy operation the family
undergoes (decided 2026-10-08). For the writer architecture: W1 unless W2 or the hybrid lowers the producing thread's p99
added latency without raising its maximum, passes every crash test, and is possible on every recorder
arm that writes the family (an arm where it is not possible keeps W1, and the spec then carries both).

The decisions are recorded in §8, with the benchmark report they rest on.

---

## 8. Decisions

Recorded on 2026-10-26 (`CKF-3`). They rest on three sources:

- `CKF-2`'s report: `tracing-formats-benchmarks` `agents` `4855de2`, `results/ctfs_keyed/REPORT.md`
  (cited below as "REPORT"), including its "Addendum-helper" section.
- Its decision records DR-2 to DR-12, in that repository's root `REPORT.md` §0.
- The owner's rulings of 2026-10-08 (§9), 2026-10-19, 2026-10-20 and 2026-10-26.

Every figure below is quoted from REPORT, with its section. The hosts behind them are a Windows 11
workstation on ReFS (Ryzen 9 9950X, shared with other agents) and WSL2 ext4 on the same machine
(REPORT §1). No macOS figure exists.

The MCR families are assigned to these realizations, with their records and member names, in
`codetracer-specs` `spec/Trace-Files/CTFS-Binary-Format.md` §2. The rows below name an MCR family only
to say where the evidence comes from.

### 8.1 What is decided

| Family | Realization (byte layout) | Address form | Families that use it | Decided |
|---|---|---|---|---|
| F1 | D1 records, with a D2 heap where a value is variable (§5.8.1) | (a) | MCR snapshots (records plus a `positions` heap), MCR bundled files (D1). The F1 members of materialized traces are unchanged ([internal-files.md](internal-files.md) §"Member Catalogue") | DR-4, DR-5 |
| F2 | C1, `k` fixed per family (§5.8.2) | (a) | MCR per-thread streams, `k = 6` | DR-3; accepted by the owner, 2026-10-26 |
| F3, durable at every seal | B1, the "live index" shape of §5.7 (§5.8.3) | (a) | the MCR page store | owner, 2026-10-20 (DR-2) |
| F3, live, V2 values | H1 live (§5.8.4) with V2 extent chains (§5.8.6), plus S3 built at close (§5.8.5) | (a) | MCR's live correlation index | DR-6 |
| F4, query-time | B2 incremental: §8's commit, with a repack at the end of the writer's session. M1 (§5.8.7) where keys provably arrive ascending | (a) | MCR `coverage.tc` | DR-8 (M1 open, §8.3 O-5) |
| F5 | S2 (`ctfs-container.md` §8a), unchanged. S3 (§5.8.5) over an H1 family at close | (a) | `memwrites.tc` (S2); `linehits.tc` (S2; F1 deferred by the owner, 2026-10-26, §8.3 O-1); materialized `corrmark.ns` (S2) | DR-9, DR-7, DR-6 |
| Address form | (a), member-relative, for every family's index references | — | all | DR-10 |
| Rule 2's reading | Structural: a pause counts only if it recurs at the same operation in every run, timed as the minimum over the runs, and it must be at most 69 µs. A seal's index cost must be bounded per new page and must not grow with the store (§7.3, rule 2) | — | every live family | owner, 2026-10-19 (accepted); worded 2026-10-20 |
| Writer architecture | W2 where the producing processes may make file-system calls, with per-thread helpers and an inline fallback (`w2hf`). W1g on arms whose producers may not, and beyond 15 producer processes. W1 elsewhere (`ctfs-container.md` §6, "Writer architectures"; MCR's arms: `codetracer-specs` `CTFS-Binary-Format.md` §2.1) | — | MCR | owner, 2026-10-19; `w2hf` measured on request (REPORT, Addendum-helper) |
| The live coordination page | Byte layout version 1, 16 MiB default capacity, 16 writer slots (`ctfs-container.md` §6, "The page: contents") | — | every live writer | DR-12 |
| What a file-only follower is guaranteed | Provisional results until close. A follower whose results must be correct before close attaches to the page, directly or through a server that attaches (`ctfs-container.md` §6, "Reading") | — | every follower | DR-12 |
| In-place publication | the live coordination page; no sequence word on disk | — | — | owner, 2026-10-08 (§9) |
| Free list root area | removed (`R = 0`) | — | — | 2026-10-08, under the owner's rule (§9) |
| `memwrites.tc` lookups | exact address only: sparse exact (F3 live, F5 static) | — | — | owner, 2026-10-08 (§9) |
| Budgets | pause ≤ 69 µs per operation in the recorder; copy-time rewrite ≤ 10% | — | — | owner, 2026-10-08 (§9) |
| OQ-5, byte reproducibility | not claimed while recording; a canonical form is produced on demand by a repack tool, at close or on export (§9; O-12) | — | MCR | owner, 2026-10-26 |

A writer emits a keyed layout of §5.8 only for a family this table assigns to it. A producer adopts
it in its own rollout: for MCR, `CKF-5`.

### 8.2 The decision records: evidence, and what would reverse each

**DR-2. The page store: B1.** Runner-up H1. The owner chose B1 on 2026-10-20, by accepting rule 2's
structural reading (§7.3), under which B1 is the pick. The margin is narrow.

- *Rule 1.* No durable shape lost a published page, returned a wrong value or left a dangling
  reference, in 201 kills per shape (4,623 kills, about 60% of them inside a seal), on Windows and, for
  B1 and H1, on Linux. The controls `drop-data` and `h1-shrink-first` were caught (REPORT §3.2, §14).
- *Rule 2.*
  - B1 has no structural insert pause up to 1.6·10^7 pages. Its one recurring slow insert, 32 µs at
    1.6·10^7, is under the budget.
  - Its seal cost per new page stays bounded across store sizes: 4.3, 5.9 and 4.0 µs at 3·10^4,
    2.6·10^5 and 1.6·10^7.
  - Its seal grows within a run by 1.95× and 1.76× on Windows and 1.93× and 1.87× on Linux.
  - H1 grows by 2.04× and 1.88× on Windows and 3.71× and 2.57× on Linux, failing the within-run test.
  - Both H2 variants fail at 1.6·10^7 (recurring 361 µs and 104 µs).
  - The rebuilt and leveled shapes fail the seal test: they grow 3× to 16× within a run (REPORT §3.2).
- *Rule 3, at C = 64, p99:*
  - KF-P1, the measured workload: B1 1 block, H1 2.
  - 3·10^4 pages: 2 against 3.
  - 2.6·10^5: 3 and 3. Cold reads break the tie, 12 against 13.
  - 1.6·10^7: H1 6, B1 8 (REPORT §3.2).
- *Rule 4:* 48.1 bytes per key against H1's 57.2, with no dead bytes (REPORT §3.2).
- *Costs:* durability costs B1 0.77 to 1.11 of a checkpoint's own page writes, because each new page
  dirties one index block (REPORT §3.1 item 4, §3.4 P-P2).
- *What would reverse it* (REPORT §3.6):
  1. H1's within-run seal growth at or below 2× on a quiet bare-metal Linux host (PLAN §6). Rule 3 then
     ties B1 and H1 at 2.6·10^5.
  2. The owner judging H1's address computation and bucket chain the simpler reader, in that tie.
  3. Checkpoint-heavy stores of millions of distinct pages, where H1 reads 6 blocks against 8. KF-P1's
     largest recording holds 29,574.

  §8.3 O-4 names these measurements.

**DR-3. The per-thread streams: C1 keyed by ctTid, `k = 6`.** Runner-up D1 with a C1 ctTid table.
Close. Accepted by the owner on 2026-10-26. How MCR assigns ctTids, and the recorder unit expected to
remove C1's weak case of holed ctTid sets, are `codetracer-specs` `CTFS-Binary-Format.md` §2.3's
business.

- *Rule 1:* 0 FAIL in 201 kills for each of 10 candidates, after the driver's flush-order fix
  (REPORT §4, §14).
- *Rule 2:* no create, update or seal recurs above 69 µs, for any candidate at any size. The worst
  structural create is 43 to 49 µs, for 1,024-record segments. Update p99 is 0.2 to 0.7 µs and seal p99
  2 to 10 µs (REPORT §4).
- *Rule 3:* on dense sets (93.8% of the survey), C1, C2 and D1-C1 tie at 0 warm reads up to 2,047
  threads. Cold reads break the tie for C1, 3 against 5. On holed sets, C1 reads 1 against D1-C1's 0
  at 2,047 threads. At 10,000 threads C1 reads 1 against 2 (REPORT §4).
- *Rule 4:*
  - C1 wastes 50% to 55% of its records on holed sets.
  - It enumerates them at twice the reads, 65 against 33.
  - It is the smallest on dense sets: 143 KB against 180 KB at 2,047 threads.
- *The page's cost:* between −1.2 and +0.7 µs on a seal's p99. It never overflowed its 16 MiB default.
  It is resident at 184 KiB up to 2,047 threads, and 504 to 824 KiB at 10,000 (REPORT §4).
- *What would reverse it:* holed thread sets becoming common, for example multi-process recordings
  with 1,024-stride clusters. D1-C1 wins those on warm reads and on enumeration, and keeps a lookup by
  stream index (REPORT §4, "Why close").

**DR-4. Snapshots: D1 records with a D2 heap for `positions`.** Runner-up D4.

- By ordinal, cold, D1 reads 2 to 4 blocks at every size, against D4's 23 at 542 snapshots, 88 at
  2,180 and 3,976 at 100,000 (REPORT §5).
- Lookup by geid, a binary search over the records, is equal for both: 18 reads cold and 13 warm at
  100,000. That falsifies P-C1 (REPORT §5).
- *What would reverse it:* nothing in the grid. The refinement REPORT §5 proposes is a per-block geid
  fence array, about 2 warm reads at 100,000 snapshots (§8.3 O-8).

**DR-5. Bundled files: D1.** The control, root-member bundling, fills the bounded root at file 110 of
the grid's 1,024. D1 locates a file in 1 cold read, 2 at 1,024 files (REPORT §6). Nothing in the grid
would reverse it.

**DR-6. The live correlation index: H1 live, and S3 built at close.** Runner-up H4 fill-once 64.

- *Rule 1:* 0 FAIL in 201 kills each for H1, H3 and H4-64 (REPORT §7).
- *Rule 2:* H1 has no insert that recurs above 20 µs, at any size. Today's whole-image rebuild has a
  structural insert pause that grows with the markers: 27, 128 and 319 µs at 300, 1,000 and 3,000.
  Its dead bytes grow quadratically, to 394 MB at 3,000 markers. It fails at 1,000 markers (REPORT §7,
  P-M1).
- *Seals* every 64 markers cost about 9 µs per marker sealed, bounded by the markers per seal: rule
  2's structural reading.
- *Rule 3, at C = 64, p99, 10^6 markers:* H1 8 against H3's 15 and H4's 13. With S3 at close, 6. At 80%
  repeats, H1 12, with S3 11. The S3 build costs 20 to 25 ms at 10^6 markers (REPORT §7).
- *B6:* the Nim H1 writes containers the Rust reader verified key by key (500,006 keys at 10^6), with
  insert p99 0.5 to 0.6 µs (REPORT §15).
- *What would reverse it:* nothing in the grid.

**DR-7. `linehits.tc`: deferred by the owner (2026-10-26). S2 stays.**

- The report decides F1 (one D2 record per global line, hit or not) for line-only traces with 10% or
  more of their lines hit, and S2 below that:
  - at 10^7 lines, F1 reads 2 blocks p99 warm at every hit fraction, and 4 cold;
  - S2 reads 1 warm at 1% hit and 3 at 10% and above;
  - F1 costs 16 bytes per line against S2's 0.25 to 21.9 (REPORT §8).
- That decision is conditional on the distribution of hit fractions, which `CKF-2` could not measure
  (REPORT §8, §18). Adopting F1 changes a materialized-trace member, so every recorder that writes
  through the shared library would have to advance its pin.
- The owner deferred it on 2026-10-26. S2 stays, and nothing is adopted on an unmeasured input.
  A `CKF` follow-up measures the real hit distribution first (§8.3 O-1). F1's layout is §5.8.1's D2
  with a 16-byte record per line, `(payload_offset + 1, payload_len)`, where `0` means not hit
  (`src/workloads/kfw.rs:287-295`).
- *What would decide it:* §8.3 O-1. If most traces are below about 5% hit, F1 is dropped (REPORT §8).

**DR-8. `coverage.tc`: B2 incremental, with a repack at the end of the writer's session.** M1 when
intervals provably arrive in tick order.

- *In random order, persisting every 100 inserts, at 10^6 intervals:*
  - incremental B2 reads 10 blocks p99 at C = 64 (21 cold, 87 at most);
  - B1 reads 1,580 and T2 3,492, because a predecessor walks every tombstone below it;
  - B2 writes 5,346 bytes per insert and leaves 5.3 GB dead (99% of the file), so it is decided only
    with the repack, a whole-image §8a build of 174 to 567 ms at 10^6 (REPORT §9).
- *In tick order:* M1 reads 4 to 5 blocks, writes 24 bytes per insert, leaves nothing dead and persists
  in 33 µs p99 (REPORT §9).
- *What would reverse it:* the measured insert order of real coverage sessions, or B1 with a range
  tombstone, which has the smallest random-order footprint (48 bytes per insert, nothing dead) once its
  walks are bounded (§8.3 O-5).

**DR-9. `memwrites.tc`: S2, as today.** Runner-up S3, rebuilt per persist. Close.

- At C = 64, S2 reads 0, 3 and 3 blocks p99 at 10^4, 10^6 and 10^7 addresses, against S3's 1, 3 and 4.
- S3 is half the size (73 against 134 bytes per address) and persists 4 times faster (370 ms against
  1.6 s at 10^7). Rule 4 is not reached (REPORT §10).
- *What would reverse it:* a measured address distribution, since the grid's is a synthetic mixture
  (§8.3 O-7), or the owner weighing persist time or space.

**DR-10. The address form: (a), member-relative, for every family.**

- Form (b) saves no warm read: 3 and 3 at 2.6·10^5, and 4 and 4 at 4·10^6.
- It costs every copy 18% to 34% more: export +31% and +34%, compact conversion +27% and +18%
  (REPORT §11). Both conditions of §7.3's address-form rule fail.
- *What would reverse it:* a family where form (b) saves a warm read. It was measured on the page store
  only, the family with the most references (§8.3 O-11).

**DR-11. The writer architecture: W2 with `w2hf`, W1g where W2 is impossible.** Runner-up W1.

- *Against W1* (REPORT §13):
  - W2 carries 1.4× to 3.0× W1's throughput at 8 to 32 producer threads on Windows, and 2.8× to 17×
    at 16 and 32 threads on Linux.
  - Its event p99 is 0.1 µs, against W1's 0.0 to 0.3 µs at 10^6 events/s and 1.7 to 10.5 µs at 10^4.
  - Its event max stays at the no-op baseline, where W1's producers stall on full rings for 10.8 to
    27.9 ms on Windows and 14.6 to 579 ms on Linux.
  - It keeps 100% of a recording whose coordinator is killed, against W1's 44%, in 50 kills on Windows
    and 25 on Linux.
- *W2's price:* a per-chunk pause on the recorded thread of 0.12 to 2.1 ms p99, and 10 to 128 ms when
  32 threads seal at once (REPORT §13.2).
- *`w2hf`, measured at the owner's request* (REPORT, Addendum-helper H.0 to H.5): one polling helper
  per recorded thread, a queue of 32 raw chunks per thread, and an inline fallback that publishes the
  queue and the thread's own chunk in order.
  - **No structural pause** at any of the 12 Windows points with a CPU per recorded thread, where W2
    paused 135 to 596 µs at all 12; and none at the 6 Linux points, where W2 paused 275 µs to 143 ms.
  - Throughput is 0.94× to 1.05× W2's on Windows and 0.90× to 1.66× on Linux.
  - 0 FAIL in 100 kills on Windows and 50 on Linux. A killed coordinator loses nothing.
  - The fallback bounds a stall at 33 chunk publications. The worst measured stall is 10.5 to 12.4 ms
    on Windows at 8 to 16 threads, on 0.5% to 1.4% of chunks. These stalls do not recur at the same
    chunk.
- *What `w2hf` cannot do:* make spare cores. With 32 recorded threads on 32 logical CPUs every W2
  variant keeps a structural pause (`w2hf` 12.9 to 15.3 ms at 10^6 events/s), and nothing measured
  meets 69 µs there (REPORT H.0, H.5).
- *Limits:* W2 is impossible where producers may not make file-system calls (Windows stage 0) and
  beyond 15 producer processes, because the page has 16 writer slots (REPORT §13.1 item 5). W1g, whose
  producers compress into shared memory for one writer, is possible everywhere.
- *What would reverse it:* REPORT §13.3 names one condition, the owner applying the 69 µs budget to
  W2's per-chunk pause below W1's drainer ceiling. `w2hf` removes that pause wherever helpers have
  cores, so it now applies only to hosts without spare cores. §8.3 O-2 and O-3 are what remains open.

**DR-12. The live coordination page, and OQ-5 (evidence; recommendation adopted).**

- *Attached readers:* 0 torn accepts in 7.9·10^10 reads on Windows and 1.1·10^11 on Linux, under
  natural and hostile (byte-split) writers (REPORT §12).
- *File-only followers:* none in natural mode, under one write per word, on ReFS and ext4. Under
  byte-split seal writes, 1,864 to 100,495 torn accepts on Windows and 3,171 to 246,172 on Linux, of
  which 91.7% to 100% passed content validation (REPORT §12, §12.2).
- *The page's cost:* p99 0.1 µs per update and max 160 µs (one preemption; the no-op reaches the same).
  Attached lag p99 is 0.4 µs, against the file-only follower's 93 to 117 µs.
- *A dead writer* is detected by pid at p99 29 ms on Windows and 0.94 ms on Linux. A hung writer is
  detected by its heartbeat after 10 s, the rule's own bound (REPORT §12).
- *Adopted:* REPORT §12.2's recommendation, written into `ctfs-container.md` §6, "Reading". (The root
  report's DR-12 recommendation cites "§12.3"; the section is §12.2, and a one-line correction to
  `tracing-formats-benchmarks` accompanies this decision.)
- *OQ-5:* W1s and W2s, which append in global-event-id order, wrote byte-identical containers at 12 of
  12 points on both operating systems. Their cost is memory: a reorder buffer of up to 34 MB at
  32 producers × 10^6 events/s. Producer latency is unchanged (REPORT §13.4). §9 records the answer.
- *Not measured:* macOS (31-character `shm_open` names, arm64 ordering), the cannot-attach variants
  (another namespace, another logon session, an HTTP server), and SMB or NFS (REPORT §18; §8.3 O-10).

### 8.3 Open items: what the evidence left open, and the measurement that would close each

Each item names its decision record. None blocks `CKF-4`. Each says what changes when it closes.

- **O-1. `linehits.tc`'s hit distribution (DR-7).**
  - *Open:* whether F1 is adopted for line-only traces at 10% hit or more.
  - *Measurement* (owner, 2026-10-26: a `CKF` follow-up, tracked in `codetracer-specs`
    `milestones/CTFS-Keyed-Families.milestones.org` under `CKF-3`): the `(lines, hit lines)` pair of
    every line-only trace in the scripting recorders' corpus traces. Those recorders write
    `linehits.tc` through the shared writer library. The pairs give the distribution of the hit
    fraction, which `CKF-2`'s B0 could not read (REPORT §18).
  - *Result:* if a material share of traces is at 10% or more, F1 is adopted per trace, chosen by the
    writer at close from the fraction it knows. If most are below about 5%, F1 is dropped.
- **O-2. W2's stall shortened by a fair hand-off (DR-11).**
  - *Open:* `w2hf1` (fall back by one chunk) measured the same 9 to 11 ms worst stall as `w2hf`. The
    likely reason, not traced, is the per-thread publication lock's unfairness (REPORT H.4).
  - *Measurement:* `w2hf1` with a hand-off in which the helper yields the publication lock to a waiting
    recorded thread, at Addendum session 5's points on Windows and the queue-depth points on Linux, 3
    runs each, with the structural test and the stall distribution.
  - *Result:* if the worst stall falls to about one chunk's publication (about 0.3 ms at 16 threads)
    with no structural pause, the fallback becomes `w2hf1`'s.
- **O-3. The 16 writer slots (DR-11, DR-12).**
  - *Open:* page version 1 has 16 writer slots, so W2 registers at most 15 producer processes beside its
    coordinator. A 16-process W2 or hybrid run could not start (REPORT §13.1 item 5, §18). Whether W2
    keeps its lead past 15 processes is unmeasured: the plan's grid went to `P = 64`, and the runs to
    `P = 16` for W1 and W1g only.
  - *Measurement:* KF-X at `P` of 16, 32 and 64 with a page whose writer-slot table is sized at creation
    (a page version 2), W2 and `w2hf` against W1g, with throughput, the structural test and the kill
    tests.
  - *Until then:* W1g beyond 15 producer processes.
- **O-4. B1 against H1 (DR-2).**
  - *Measurements:* KF-P2's within-run seal growth on a quiet bare-metal Linux host (PLAN §6), and a
    Linux CAS-K1 extract giving real store sizes and new pages per checkpoint (B0 had none).
  - *Result:* H1 at or under 2× growth, or stores of millions of pages, reopens DR-2 (REPORT §3.6).
- **O-5. `coverage.tc`'s insert order (DR-8).**
  - *Measurement:* the order in which the db-backend's `coverage_add` delivers intervals in real
    sessions.
  - *Result:* tick order makes M1 the realization. Also, B1 with a range tombstone, re-measured once
    built (REPORT §9).
- **O-6. B1's and H1's attached readers, and the hole fill (DR-2, DR-6).**
  - *Open:* the KF-P and KF-M runs followed the file alone, without the page (REPORT §3.3). So the
    node and bucket mirrors and the hole fill of §5.8.0 are unmeasured.
  - *Measurement:* KF-P2 at 2.6·10^5 and 1.6·10^7 pages, and KF-M at 10^6 markers, with the page
    attached and an attached follower:
    - the structural test of rule 2;
    - the page's resident size and overflow count;
    - zero accepted torn values, and zero misses of a published key, under KF-R's hostile injector.
- **O-7. `memwrites.tc`'s address distribution (DR-9).**
  - *Measurement:* two `omniscient-prep` runs' write addresses (B0 had none), re-running KF-W.
  - *Result:* S3 overtakes S2 if it reads no more blocks at C = 64.
- **O-8. Snapshot lookup by geid (DR-4).**
  - *Open:* P-C1 was falsified: 13 warm reads at 100,000 snapshots.
  - *Measurement:* KF-C with a per-record-block fence array of first geids.
  - *Result:* if it brings warm reads to about 2, the fence member is added to the snapshot family.
- **O-9. B6 for KF-T and KF-C (DR-3, DR-4).**
  - *Open:* the Nim ports of the KF-T and KF-C finalists were not written (REPORT §18).
  - *Measurement:* the B6 check: the Rust reader verifies the Nim containers, and Nim rule 2 is tested
    structurally.
- **O-10. Hosts not run (DR-12).**
  - *Open:* macOS (KF-R's arm64 ordering and its 31-character name limit), the cannot-attach variants,
    SMB and NFS, and a bare-metal Linux host (REPORT §18).
  - *Measurement:* each runs KF-R, and the page store's rule 2 where timing matters.
- **O-11. The address form on the other families (DR-10).**
  - *Open:* KF-A measured only the page store's index.
  - *Measurement:* KF-A on the correlation index and the per-thread family. That form (b) costs every
    copy more is measured; that it saves no read elsewhere is inferred (REPORT §11).
- **O-12. The canonical repack (OQ-5; owner, 2026-10-26). Owned by `CKF-4N`.**
  - *Open:* the order in which a canonical repack places members and blocks, so that its output is a
    function of the container's content. Every input to that order must itself be content, not
    timing. Examples of timing-dependent inputs are a page store's first-stored record order and an
    H1 table's split history.
  - *What closes it:* the order specified here, and a test. Two recordings of one execution, whose
    live containers differ, repack to byte-identical containers. A mutation arm leaves one
    timing-dependent order in place, and must be caught.

---

## 9. The Owner's Answers (2026-10-08)

- **OQ-1, the free list root area: removed.** The owner asked why it was invented and said to remove
  it only if nothing needs it. It was invented for a container-global, per-shard sub-block allocator;
  no implementation keeps or reads free-list state in block 0. The evidence is in
  `ctfs-container.md` §1, "The free list root area is removed". The owner's underlying concern, blocks
  on disk that are no longer used, is G15 and §4.6.
- **OQ-2, torn reads: the live coordination page.** The owner: "I see more and more value in the idea
  of having a shared memory page for coordination between multiple writers and multiple readers while
  the file is still being written. We can potentially implement these safety mechanism in the shared
  memory without increasing the footprint of the files." Specified in `ctfs-container.md` §6,
  "In-place record publication: the live coordination page". Sequence words on disk, 32-byte root
  entries and counters in `MapBlock` bits are rejected for this reason.
- **OQ-3, `memwrites.tc`: exact-address lookups only.** The owner's reason: a range query decomposes
  into exact queries, and the function models very efficient data breakpoints. `memwrites.tc` is
  sparse exact (F3 where it is built incrementally, F5 where it is built whole); no ordered structure is
  required for it.
- **OQ-4, budgets: accepted.** The in-recorder pause budget is 69 µs per operation (§7.3, rule 2);
  the copy-time rewrite overhead for an absolute-form reference is at most 10% (§7.3).

### OQ-5, decided by the owner (2026-10-26): not reproducible while recording; a canonical form on demand

The owner's ruling:

- **While it is being recorded, an MCR container is not byte-reproducible.** The writer is W2 with
  `w2hf` (§8.2, DR-11), not its serialized form W2s. A W2 container is not a function of the recording
  (`ctfs-container.md` §6, W2 rule 8), and MCR does not claim that it is.
- **A canonical, byte-reproducible form is produced on demand, by a repack tool.** It runs at close
  or on export, and its output is for caching and deduplication. A repack copies every live member of
  a closed container into a new container (`ctfs-container.md` §6, "Dead space"). The canonical
  repack also fixes the order in which it places members and their blocks, so that its output is a
  function of the container's content and not of the recording's timing. That placement order is not
  specified yet. It is open item O-12 (§8.3), owned by `CKF-4N`, whose library already offers the
  repack.

*Why not reproducible while recording.* Doing it live was measured, and costs memory, not producer
latency. W1s and W2s, which append in global-event-id order, wrote byte-identical containers at 12
of 12 points on Windows and on Linux, with a reorder buffer of up to 34 MB at 32 producers × 10^6
events/s (REPORT.md §13.4). A repack moves that cost off the recording path, to where a reproducible
form is needed.

The question as it was put on 2026-10-08:

- **OQ-5. Should MCR containers be byte-reproducible?** `ctfs-container.md` §6, "Block placement",
  makes a split-stream writer's container a function of the recording. An MCR recording is not: its
  drainer appends in the order it drains the threads' rings, which depends on timing, and the writer
  architectures that move work into the recorded processes (W2, the hybrid) claim blocks in the
  order processes reach a shared counter. Requiring reproducibility would rule out W2 and the hybrid
  for MCR (or require appends serialized in recording order, giving up what they save) and would make
  the drainer order its appends by recording position (global event id) rather than by arrival.
  Not requiring it keeps every writer architecture open; MCR's correctness never depended on it
  (replay reads content, not placement). The benchmark measures what serialized appends would cost
  (`tracing-formats-benchmarks` `ctfs_keyed/PLAN.md`, `KF-X`), so the owner can decide with numbers.

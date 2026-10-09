# CTFS Keyed Member Families

> **Scope.** Like the rest of this specification, this document defines the structure of the CTFS
> container and the records of the open-source recorders that produce materialized traces. It
> does not describe how the Multi-Core Recorder (MCR) uses CTFS. MCR's families are assigned to the
> abstract families below in `codetracer-specs` (`spec/Trace-Files/CTFS-Binary-Format.md` §2), and
> that assignment is subject to change in each release. The boundary between the two repositories is
> `codetracer-specs/spec/Trace-Files/README.md`.

> **Status (2026-10-08): design specification, realizations pending benchmark.** This document
> states the goals the container must keep (§1), the cost model every keyed structure pays (§2), the
> properties of the data that permit cheaper structures (§3), the anatomy shared by every keyed
> member (§4), the abstract families and their candidate realizations (§5), how a writer architecture
> constrains them (§6), and what the benchmarks that choose a realization per family must measure and
> how they decide (§7). It does **not** choose a realization for any family, and it defines no byte
> layout that a writer may emit yet. Each family's realization, its byte layout and its address form
> are added to §5 by the decision recorded in §8, after the benchmark suite of §7 reports. The
> owner's answers of 2026-10-08 to the questions this document raised are §9.
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

| Family | Members (`internal-files.md` §"Member Catalogue"; MCR in `codetracer-specs`) | Realization | Address form | Decided |
|---|---|---|---|---|
| F1 | — | pending | pending | — |
| F2 | — | pending | pending | — |
| F3 | — | pending | pending | — |
| F4 | — | pending | pending | — |
| F5 | — | pending | pending | — |
| In-place publication | (`ctfs-container.md` §6) | the live coordination page; no sequence word on disk | — | owner, 2026-10-08 (§9) |
| Free list root area | (`ctfs-container.md` §1) | removed (`R = 0`) | — | 2026-10-08, under the owner's rule (§9) |
| `memwrites.tc` lookups | (MCR catalogue; `internal-files.md`) | exact address only: sparse exact (F3 live, F5 static) | — | owner, 2026-10-08 (§9) |
| Budgets | §7.3 | pause ≤ 69 µs per operation in the recorder; copy-time rewrite ≤ 10% | — | owner, 2026-10-08 (§9) |
| Writer architecture | (`ctfs-container.md` §6) | W1 today; pending benchmark and OQ-5 (byte-reproducibility) | — | — |

Until a row is decided, writers keep the formats they write today (the catalogue says which), and no
writer emits a keyed layout from §5.

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

### Open for the owner

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

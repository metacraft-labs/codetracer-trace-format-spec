# Compact profile measurements, 2026-10

The measurements behind [`ctfs-container.md`](../ctfs-container.md) §1d, the compact profile's body.
Taken 2026-10-01/02 against a **container version 5** baseline, which matters: the direct-block tag
(version 5, §2 "Members of at most one block") already removed the mapping block of every member
that never outgrows one block, so a reduction measured against a version-3 or version-4 container
credits the compact profile with a saving that is already banked. The campaign's earlier
86,016-byte mapping-block figure is a version-3 figure for exactly that reason.

## Containers measured

| # | Container | Producer | Version | Bytes |
|---|---|---|---|---|
| A | a 20,000-step recording | `codetracer-trace-format-nim`'s own writer (`tests/test_compact_container_layout.nim`) | 5 | 110,592 |
| B | [`fixtures/minimal_trace.ct`](../fixtures/minimal_trace.ct) | the committed spec fixture's documented producer | 5 | 24,576 |

**What is NOT here.** The published BlockTracer container the campaign's introduction measures
(`/t/vl/3h/vl3h7u4w62wz3p4c44gpikxtbt/trace.ct`, Aztec, 21 members) is not in this workspace, so
its 188,416 / 18,851 / 17,544 / 14,839 figures could not be re-taken from the same bytes. Nothing
below is a correction *of* those numbers; the figures below are a second, independent sample, and
where they disagree with what the first sample predicted that is recorded as a finding rather than
smoothed over.

Container B is a minimal trace -- five members, 326 bytes of payload -- so it bounds the structural
claim from the small end and is not used for anything else. Container A is a real recording through
the real stream writers (step, value, call and I/O event streams, the interning tables and
`meta.dat`), but it is a SYNTHETIC program: its per-step value data is highly repetitive, which the
one-shot compression finding below turns out to depend on.

## Overhead: the structural reduction

Re-taken after the reference encoder existed, by `tests/test_compact_container_layout.nim`'s
`measure_the_overhead_reduction`.

| quantity | A | B |
|---|---|---|
| members | 17 (0 empty, 2 larger than one block) | 5 (0 empty, 0 larger than one block) |
| sum of stored member sizes | 38,045 | 326 |
| FULL container, version 5, measured | 110,592 | 24,576 |
| FULL container, version 5, **predicted** | **110,592** | **24,576** |
| COMPACT container, **predicted** (`28 + 24*N + sum`) | **38,481** | **474** |
| COMPACT container, measured | 38,481 | 474 |
| structural overhead, FULL | 72,547 (65.5%) | 24,250 (98.6%) |
| structural overhead, COMPACT | 436 (1.1%) | 148 (31.2%) |
| reduction, full to compact | 72,111 bytes, 65.2% (2.8x) | 24,102 bytes, 98.0% (51.8x) |
| zero bytes in the FULL container | 76,823 (69.4%) | 24,244 (98.6%) |

**The predictions and the measurements agree exactly, in both directions and on both containers.**
The compact prediction is §1d's own size identity, `Size = 28 + 24*N + sum(length)`, and the
encoder is asserted against it. The full-profile prediction is an independent structural model of
version 5 -- one root block, then per member nothing if it is empty, one data block if it fits one
block (the direct-block tag), and otherwise `ceil(size/blockSize)` data blocks plus the mapping
blocks needed to address them -- and it reproduces both measured containers to the byte (delta 0).
That is worth stating because the model is what makes the 65.2% attributable: the saving is one data
block per member plus block 0, NOT mapping blocks, of which container A has only two (8,192 bytes)
because fifteen of its seventeen members are direct at version 5.

## One-shot compression: the three representations

The campaign's sharper claim is about the serving path rather than the layout: that most of the
compact profile's compressed-size win is RAW members letting a one-shot compressor find redundancy
across members that per-member Zstd structurally cannot. Taken on container A, all three
representations from the same bytes:

| representation | bytes | gzip -9 | zstd -19 | xz -9e |
|---|---|---|---|---|
| the container as served (version 5, 69.4% zero bytes) | 110,592 | 19,140 | 17,660 | 17,224 |
| its members concatenated, every hole removed (as stored) | 38,045 | 18,169 | 17,309 | 16,632 |
| its members concatenated and RAW (per-member Zstd undone) | 353,605 | **48,793** | **18,557** | **9,228** |

Removing every hole is worth 971 bytes of gzip -9, 5.1% -- the same order as the 1,307 bytes / 6.9%
measured on the Aztec container, so that half reproduces.

### FINDING: making the members raw is NOT worth 15.4% here. Under gzip it costs 2.7x

The Aztec sample measured raw members as worth a further 2,705 bytes of gzip -9, 15.4%, on top of
hole removal. On container A the same step makes gzip -9 **2.7x worse** (18,169 to 48,793) and
zstd -19 7.2% worse (17,309 to 18,557). Only xz -9e improves, and it improves enormously: 16,632 to
9,228, a 44.5% reduction, and the smallest representation of this recording measured by anything.

The mechanism, and it is arithmetic rather than a mystery. Undoing per-member compression is a
trade: the input grows by the per-member compressor's ratio and the one-shot compressor then gets a
whole-file view of it. It wins exactly when the one-shot ratio exceeds the per-member ratio.

| | per-member ratio (raw / stored) | one-shot gzip -9 ratio on raw | verdict |
|---|---|---|---|
| Aztec container | 85,118 / 18,148 = 4.7x | 85,118 / 14,839 = 5.7x | raw wins |
| container A | 353,605 / 38,045 = 9.3x | 353,605 / 48,793 = 7.2x | raw loses |

Container A's value stream is the reason: 308,890 raw bytes stored in 27,075, an 11.4x ratio that
per-member Zstd already achieves because the data is repetitive by construction. There is very
little cross-member redundancy left for a one-shot pass to find, and gzip's 32 KiB window cannot
reach across 353 KB to find what there is. xz -9e, with a window orders of magnitude larger, can,
and does.

**What this does and does not change.** It does not touch §1d or the structural reduction above,
both of which are independent of any compressor. It does mean the claim "about two thirds of the
compact profile's serving win is raw members" is a property of ONE container measured with ONE
compressor, not a property of the format: on this container it is false for gzip and for zstd and
true, by a wider margin than claimed, for xz. The compressor column and the corpus are therefore
both load-bearing, and settling this needs the cross-corpus benchmark rather than a second sample --
which is what the campaign's benchmark milestone exists for, with the browser-transparency column
beside it, since xz is not a `Content-Encoding` any browser performs and gzip is.

One caveat on the raw row's fairness, stated because it is small but real: the raw concatenation
retains each chunked table's `.idx` member (3,168 bytes across the four tables), whose offsets
describe the compressed layout and mean nothing once the frames are inflated. A writer that emitted
genuinely raw members would not write them in that form. It is 0.9% of the raw row's input and
cannot account for the direction of the result.

## Reproducing

The structural table, and the two verification arms behind it:

```bash
cd codetracer-trace-format-nim
nix develop --command bash -c \
  'CCP2_KEEP_FIXTURES=1 nim c -r -d:release -p:src tests/test_compact_container_layout.nim'
# leaves tmp_compact_container_layout/{full.ct,compact.ct}
```

The one-shot table, from those two files. `compact.ct` IS the hole-free concatenation plus a
28-byte header and a 24-byte-per-member directory, so the middle row is taken from a payload-only
concatenation to keep the three rows comparable:

```python
import struct, subprocess
AL = "\x000123456789abcdefghijklmnopqrstuvwxyz./-"   # ctfs-container.md §3
def dec(v):
    out = []
    for _ in range(12):
        v, r = divmod(v, 40); out.append(AL[r])
    return "".join(out).rstrip("\x00")

d = open("tmp_compact_container_layout/compact.ct", "rb").read()
n = struct.unpack_from("<I", d, 24)[0]
mem, order = {}, []
for i in range(n):
    name, off, ln = struct.unpack_from("<QQQ", d, 28 + 24 * i)
    nm = dec(name); mem[nm] = d[off:off + ln]; order.append(nm)

def unzstd(b):
    return subprocess.run(["zstd", "-d", "-c", "-q"], input=b,
                          capture_output=True, check=True).stdout

stored, raw = bytearray(), bytearray()
for nm in order:
    p = mem[nm]; stored += p; out = p
    idxn = nm[:-4] + ".idx"
    # A chunked compressed table (§7) is concatenated Zstd frames in `.dat`
    # with their offsets after a u32 chunk size in `.idx`.
    if nm.endswith(".dat") and idxn in mem and p[:4] == b"\x28\xb5\x2f\xfd" \
            and len(mem[idxn]) >= 12 and (len(mem[idxn]) - 4) % 8 == 0:
        idx = mem[idxn]; k = (len(idx) - 4) // 8
        offs = [struct.unpack_from("<Q", idx, 4 + 8 * j)[0] for j in range(k)]
        offs.append(len(p))
        out = b"".join(unzstd(p[offs[j]:offs[j + 1]]) for j in range(k))
    raw += out
open("concat_stored.bin", "wb").write(bytes(stored))
open("concat_raw.bin", "wb").write(bytes(raw))
```

```bash
for f in tmp_compact_container_layout/full.ct concat_stored.bin concat_raw.bin; do
  printf '%-22s %-9s %-9s %-9s %s\n' "$f" "$(stat -c%s "$f")" \
    "$(gzip -9 -c "$f" | wc -c)" "$(zstd -19 -c -q "$f" | wc -c)" \
    "$(xz -9e -c "$f" | wc -c)"
done
```

---

# CCP-4: the writer's threshold, and a container whose members are genuinely raw

Added 2026-10-02 when the profile-choosing writer landed. Everything above is about a compact
container built by CONVERTING a full one, whose members therefore still carry the full container's
per-member zstd frames -- the finding recorded as "a compact container's members are NOT raw today".
The containers below are the first ones written compact from the start, so they are the first
measurement of the raw-member representation that is not a reconstruction of it.

Produced by `codetracer-trace-format-nim/tests/test_profile_threshold_choice.nim` through
`src/codetracer_profile_writer.nim`. Reproduce with:

```bash
cd codetracer-trace-format-nim
nix develop --command bash -c \
  'CCP4_KEEP_FIXTURES=1 nim c -r -d:release -p:src tests/test_profile_threshold_choice.nim'
# leaves tmp_profile_threshold_choice/ with both containers of every pair
```

## C: a 12,000-event recording, written both ways

Paths, steps and calls; 17 members in neither case -- five, since this writer's member set is
`events.log`, `events.fmt`, `meta.dat`, `paths.dat`, `paths.off`. The two files are the SAME
recording: `raw_members_compact.ct` is the compact profile with raw members, `raw_members_full.ct`
is the full profile with the writer's ordinary chunked zstd.

| quantity | COMPACT (raw members) | FULL (per-member zstd) |
|---|---|---|
| container at rest | 204,582 | 61,440 |
| sum of member payloads | 204,434 | 33,858 |
| zstd frame magics in the whole image | **0** | 3 |
| `gzip -9` | 36,471 | 33,797 |
| `zstd -19` | **22,886** | 33,507 |
| `xz -9e` | **20,624** | 34,004 |
| `brotli -q 11` | **23,463** | 33,462 |

`events.log` in the compact container is 203,741 bytes and is byte-identical to the full container's
own `events.log` with all three of its chunks inflated and concatenated -- asserted, not assumed, so
"raw" means *the bytes the other profile would have compressed* rather than *the bytes this writer
chose to emit*.

### The arithmetic rule from the campaign's RESOLVED section reproduces, and it predicts the gzip reversal

The rule is that raw-plus-one-shot pays if and only if the one-shot ratio exceeds the per-member
ratio. This container's per-member ratio is 204,434 / 33,858 = **6.04x**. Applying the rule per
compressor, against the compact rows above:

| compressor | one-shot ratio on the raw image | vs 6.04x per-member | predicted | measured |
|---|---|---|---|---|
| `gzip -9` | 204,434 / 36,471 = 5.61x | below | raw loses | raw loses, 7.9% worse |
| `zstd -19` | 204,434 / 22,886 = 8.93x | above | raw wins | raw wins, 31.7% better |
| `xz -9e` | 204,434 / 20,624 = 9.91x | above | raw wins | raw wins, 39.3% better |
| `brotli -q 11` | 204,434 / 23,463 = 8.71x | above | raw wins | raw wins, 29.9% better |

Four for four, including the sign. This is a sixth container and the first non-reconstructed one,
and the rule holds on it -- which is worth more than another win, because the rule is what the
campaign actually decided on and a sample that only confirmed the conclusion would not have tested
it. The per-member ratio here is 6.04x, between the five production traces' 3.1x--6.1x and
container A's 9.30x, and the outcome is correspondingly between them: raw wins under three
compressors and loses under the one whose 32 KiB window cannot see across a 204 KB payload.

### FINDING: brotli is NOT best in every row here. `xz -9e` beats it by 12%

The campaign's RESOLVED section records brotli as "best in EVERY row" of its tables. On this
container it is not: `xz -9e` is 20,624 against brotli's 23,463, 12.1% smaller, and `zstd -19` at
22,886 also beats it. Brotli is still comfortably better than `gzip -9` (23,463 against 36,471,
35.7%) and it is still the right choice, but the reason is the browser-transparency column and not
a size win it does not have on this sample. Recorded because "best in every row" is the kind of
claim a reader will quote, and on a sixth container it is false. This is a SYNTHETIC recording, so
it is a second sample rather than a refutation of the five production traces -- the same caveat
container A carries, and the same reason the benchmark milestone must keep both populations.

## D: the boundary, and the three figures a publisher needs

A recording 6 bytes under the 1 MiB default threshold (61,675 identical steps, chosen so the raw
size is 1,048,570) written both ways. It is the most compressible recording this writer can produce,
which is what makes it the extreme case rather than a typical one.

| quantity | COMPACT (raw members) | FULL (per-member zstd) |
|---|---|---|
| container at rest | 1,048,670 | 16,384 |
| `gzip -9` | 2,763 | 358 |
| `zstd -19` | 268 | 280 |
| `xz -9e` | 432 | 360 |
| `brotli -q 11` | **192** | 285 |

**At rest the compact container is 64x larger. Served under `brotli`, it is 33% smaller.** Both
figures are of the same two files. This is the clearest available statement of why the three sizes
-- at rest, on the wire, and as the loader sees it -- have to be reported separately: the compact
profile's raw members are a bet on the serving path, and a compact archive that is NOT stored
pre-compressed is strictly worse than a full container on every axis except load-path simplicity.
`ctfs-container.md` §1e says so normatively; this is the measurement behind it.

Note also that `gzip -9` is 14x worse than `brotli -q 11` on the compact row (2,763 against 192)
while being only 1.26x worse on the full row. A 1 MB payload of a 17-byte repeating record is
exactly what a 32 KiB window handles badly, and it is the same mechanism as container A's reversal
seen on a corpus where raw still wins overall.

## E: the threshold's unit, measured

Two recordings of EQUAL raw size and very different compressibility: 4,700 plain `Step` events each,
79,995 raw bytes each (17 bytes per step plus the 95-byte `meta.dat` + `events.fmt` preamble). One
emits the same step every time; the other takes `pathId` and `line` from a fixed xorshift PRNG. Both
reach a 65,536-byte threshold at the same event and at the same measured size, 65,545 bytes.

| | total raw bytes | compressed by the writer's own chunked zstd | raw-byte rule at 65,536 | compressed-size rule at 65,536 |
|---|---|---|---|---|
| every step identical | 79,995 | 54 | full | compact |
| pseudo-random steps | 79,995 | 78,441 | full | full |

The compressed sizes are **1,452x apart** for the same raw size. A raw-byte threshold classifies the
two identically, which is the property §1e requires; a compressed-size threshold splits them. The
table is the control: without it the arm asserting "same path" would pass on either implementation.

## Reproducing the CCP-4 compression tables

The four compressed columns are taken from the kept fixtures with the CLI tools, so nothing in them
depends on the test binary. `brotli` is not in this repo's dev shell and is fetched for the reading:

```bash
cd codetracer-trace-format-nim/tmp_profile_threshold_choice
nix shell nixpkgs#brotli --command bash -c '
printf "%-30s %10s %10s %10s %10s %10s\n" file at-rest gzip-9 zstd-19 xz-9e brotli-11
for f in raw_members_compact.ct raw_members_full.ct boundary_under.ct boundary_under_oracle.ct; do
  printf "%-30s %10d %10d %10d %10d %10d\n" $f $(stat -c %s $f) \
    $(gzip -9 -c $f | wc -c) $(zstd -19 -q -c $f | wc -c) \
    $(xz -9e -c $f | wc -c) $(brotli -q 11 -c $f | wc -c)
done'
```

Taken twice, on two separate runs of the test, byte-identical both times -- the recordings are
generated from a written-out xorshift PRNG with a fixed seed precisely so that a compressed size can
be quoted at all.

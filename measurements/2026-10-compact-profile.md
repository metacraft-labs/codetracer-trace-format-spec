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

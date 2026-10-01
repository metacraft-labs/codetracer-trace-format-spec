#!/usr/bin/env python3
"""Turns the output of run_measurements.sh into the tables of
measurements/2026-10-format-efficiency.md.

usage: report.py OUT_DIR

Traces are grouped by the first path component of their label, i.e. the
corpus directory (one per recorder / language).
"""

import csv
import glob
import os
import statistics
import sys
from collections import defaultdict

RULES = [
    "abs-only",
    "spec-2026-09",
    "nim-writer",
    "rust-writer",
    "anchor-only",
    "min-ties-delta",
    "min-ties-abs",
    "delta-1byte",
    "delta-1byte-ties-abs",
]
SM_CANDS = ["v1", "v1-zstd", "v1-chunked", "gap", "gap-zstd", "gap-chunked", "packed-nozstd", "packed-rle-nozstd", "packed-rle", "packed"]


def rows(path):
    with open(path, newline="") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def group_of(label):
    return label.lstrip("./").split("/", 1)[0]


def pct(a, b):
    return f"{100 * (a - b) / b:+.1f}%" if b else "n/a"


def corpus_table(an):
    tr = rows(f"{an}/traces.tsv")
    g = defaultdict(lambda: [0, 0, 0, 0, 0, 0])
    for r in tr:
        for k in (group_of(r["trace"]), "ALL"):
            a = g[k]
            a[0] += 1
            a[1] += int(r["records"])
            a[2] += int(r["container_bytes"])
            a[3] = max(a[3], int(r["records"]))
            a[4] += int(r["column_aware"])
            a[5] += int(r["unanchored_chunks"]) > 0
    print("## Corpus\n")
    print("| corpus | traces | exec records | largest trace | column-aware traces | traces with an unanchored chunk | container bytes |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for k in sorted(x for x in g if x != "ALL") + ["ALL"]:
        a = g[k]
        print(f"| {k} | {a[0]} | {a[1]:,} | {a[3]:,} | {a[4]} | {a[5]} | {a[2]:,} |")
    print()


def steps_tables(an):
    st = rows(f"{an}/steps.tsv")
    by = defaultdict(lambda: defaultdict(lambda: [0, 0, 0, 0]))
    for r in st:
        n = int(r["records"])
        if n == 0:
            continue
        for k in (group_of(r["trace"]), "ALL"):
            a = by[k][r["rule"]]
            a[0] += n
            a[1] += int(r["raw_bytes"])
            a[2] += int(r["zstd_bytes"])
            a[3] += int(r["abs_records"])
    groups = sorted(x for x in by if x != "ALL")
    print("## `steps.dat`: bytes per exec record under each rule\n")
    print("zstd = the stream as stored: chunks of 4096 records, one level-3 frame each. Change is against the 2026-09 spec rule.\n")
    print("### Whole corpus\n")
    print("| rule | raw B/rec | stored B/rec | stored vs spec | AbsoluteStep share of position records |")
    print("|---|---:|---:|---:|---:|")
    spec = by["ALL"]["spec-2026-09"][2]
    pos = by["ALL"]["abs-only"][3] or 1
    for rule in RULES:
        a = by["ALL"][rule]
        print(f"| {rule} | {a[1]/a[0]:.3f} | {a[2]/a[0]:.4f} | {pct(a[2], spec)} | {100*a[3]/pos:.1f}% |")
    print()
    print("### Per corpus: stored bytes, change against the 2026-09 spec rule\n")
    print("| corpus | records | spec-2026-09 B/rec | " + " | ".join(r for r in RULES if r != "spec-2026-09") + " |")
    print("|---|---:|---:|" + "---:|" * (len(RULES) - 1))
    for k in groups:
        sp = by[k]["spec-2026-09"][2]
        n = by[k]["spec-2026-09"][0]
        cells = [pct(by[k][r][2], sp) for r in RULES if r != "spec-2026-09"]
        print(f"| {k} | {n:,} | {sp/n:.4f} | " + " | ".join(cells) + " |")
    print()
    # How many corpora each rule is within 1% of the best on.
    print("### Corpora on which each rule is within 1% of the smallest stored size\n")
    wins = defaultdict(int)
    for k in groups:
        best = min(by[k][r][2] for r in RULES)
        for r in RULES:
            if by[k][r][2] <= best * 1.01:
                wins[r] += 1
    print("| rule | corpora (of %d) |" % len(groups))
    print("|---|---:|")
    for r in RULES:
        print(f"| {r} | {wins[r]} |")
    print()


def decode_tables(out):
    envs = defaultdict(list)
    for p in sorted(glob.glob(f"{out}/decode.*.tsv")):
        env = os.path.basename(p).split(".")[1]
        envs[env].append(p)
    if not envs:
        return
    print("## `steps.dat` decode speed\n")
    print("ns per exec record, record-weighted over traces of at least 2000 records; best of the repeated passes per (trace, rule). `parse` decodes already-inflated chunks to absolute positions; the other columns include inflating every chunk.\n")
    for env, paths in sorted(envs.items()):
        best = {}
        for p in paths:
            for r in rows(p):
                key = (r["trace"], r["rule"])
                vals = [float(r["parse_ns_per_record"]), float(r["ruzstd_ns_per_record"]), float(r["czstd_ns_per_record"])]
                if key not in best:
                    best[key] = (int(r["records"]), vals)
                else:
                    n, old = best[key]
                    best[key] = (n, [min(a, b) if b == b else a for a, b in zip(old, vals)])
        agg = defaultdict(lambda: [0.0, 0.0, 0.0, 0])
        for (trace, rule), (n, v) in best.items():
            a = agg[rule]
            for i in range(3):
                a[i] += (v[i] if v[i] == v[i] else 0.0) * n
            a[3] += n
        print(f"### {env} ({len(paths)} passes, {len({t for t, _ in best})} traces)\n")
        print("| rule | parse | ruzstd + parse | C zstd + parse |")
        print("|---|---:|---:|---:|")
        for rule in RULES:
            a = agg.get(rule)
            if not a or not a[3]:
                continue
            cz = a[2] / a[3]
            print(f"| {rule} | {a[0]/a[3]:.2f} | {a[1]/a[3]:.2f} | {f'{cz:.2f}' if cz else 'n/a'} |")
        print()


def stepmap_tables(out, an):
    sm = rows(f"{an}/stepmap.tsv")
    agg = defaultdict(lambda: defaultdict(lambda: [0, 0, 0]))
    for r in sm:
        for k in (group_of(r["trace"]), "ALL"):
            a = agg[k][r["candidate"]]
            a[0] += int(r["stored_bytes"])
            a[1] += int(r["steps"])
            a[2] += 1
    print("## `step-map.ns`: stored bytes per step\n")
    print("Line-only traces only (a column-aware trace carries no step map). Candidates are described in `src/stepmap.rs`.\n")
    print("| corpus | maps | steps | " + " | ".join(SM_CANDS) + " |")
    print("|---|---:|---:|" + "---:|" * len(SM_CANDS))
    for k in sorted(x for x in agg if x != "ALL") + ["ALL"]:
        a = agg[k]
        n = a["v1"][1] or 1
        print(f"| {k} | {a['v1'][2]} | {a['v1'][1]:,} | " + " | ".join(f"{a[c][0]/n:.3f}" for c in SM_CANDS) + " |")
    print()
    for env in ("native", "wasm"):
        p = f"{out}/stepmap.{env}.tsv"
        if not os.path.exists(p):
            continue
        rs = [r for r in rows(p) if int(r["steps"]) >= 1000]
        agg = defaultdict(lambda: [0.0, 0.0, 0.0, 0, 0, 0])
        for r in rs:
            a = agg[(r["layout"], r["inflater"])]
            a[0] += float(r["load_ns"])
            a[1] += float(r["read_load_ns"])
            a[2] += float(r["cold_lookup_ns"])
            a[3] += int(r["steps"])
            a[4] += int(r["bytes"])
            a[5] += 1
        print(f"### Load and lookup, {env}: maps of at least 1000 steps\n")
        print("Full load = every list into a map, what the db-backend does at open. The read column adds reading the member's bytes from a file in the page cache first. One cold lookup = open plus one line, the hottest one, per map; summed over maps.\n")
        print("| layout | zstd decoder | bytes/step | full load ns/step | read + full load ns/step | one cold lookup, µs per map |")
        print("|---|---|---:|---:|---:|---:|")
        order = ["v1", "packed", "packed-rle", "packed-nozstd", "packed-rle-nozstd"]
        for (lay, inf), a in sorted(agg.items(), key=lambda kv: (order.index(kv[0][0]) if kv[0][0] in order else 99, kv[0][1])):
            print(f"| {lay} | {inf} | {a[4]/a[3]:.3f} | {a[0]/a[3]:.2f} | {a[1]/a[3]:.2f} | {a[2]/a[5]/1000:.1f} |")
        print()
        big = sorted({r["trace"]: int(r["steps"]) for r in rs}.items(), key=lambda kv: -kv[1])[:5]
        print(f"Largest maps, {env} (µs):\n")
        print("| trace | steps | layout | zstd decoder | bytes | full load | read + full load | one cold lookup |")
        print("|---|---:|---|---|---:|---:|---:|---:|")
        for t, n in big:
            for r in rs:
                if r["trace"] == t and r["layout"] in ("v1", "packed"):
                    print(f"| {t} | {n:,} | {r['layout']} | {r['inflater']} | {int(r['bytes']):,} | {float(r['load_ns'])/1000:.1f} | {float(r['read_load_ns'])/1000:.1f} | {float(r['cold_lookup_ns'])/1000:.1f} |")
        print()
    p = f"{out}/stepmap_sweep.txt"
    if os.path.exists(p):
        print("### `packed` chunk target\n")
        print("```")
        print(open(p).read().rstrip())
        print("```\n")
    p = f"{out}/latency/stepmap_latency.tsv"
    if os.path.exists(p):
        print("### Every candidate on the largest maps (native)\n")
        print("| trace | candidate | inflater | bytes | open µs | full load µs | one cold lookup µs |")
        print("|---|---|---|---:|---:|---:|---:|")
        for r in rows(p):
            print(f"| {r['trace']} | {r['candidate']} | {r['inflater']} | {int(r['stored_bytes']):,} | {float(r['open_ns'])/1000:.1f} | {float(r['load_all_ns'])/1000:.1f} | {float(r['cold_breakpoint_ns'])/1000:.1f} |")
        print()


def meta_and_blocks(an):
    tr = {r["trace"]: r for r in rows(f"{an}/traces.tsv")}
    mem = rows(f"{an}/members.tsv")
    print("## `meta.dat`'s path list\n")
    g = defaultdict(lambda: [0, 0, 0, 0])
    for r in tr.values():
        for k in (group_of(r["trace"]), "ALL"):
            a = g[k]
            a[0] += int(r["meta_bytes"])
            a[1] += int(r["meta_path_list_bytes"])
            a[2] += int(r["container_bytes"])
            a[3] += 1
    print("| corpus | traces | meta.dat bytes | of which the path list | path list share of meta.dat |")
    print("|---|---:|---:|---:|---:|")
    for k in sorted(x for x in g if x != "ALL") + ["ALL"]:
        a = g[k]
        print(f"| {k} | {a[3]} | {a[0]:,} | {a[1]:,} | {100*a[1]/max(1,a[0]):.0f}% |")
    print()
    print("## Small and empty members\n")
    per = defaultdict(lambda: [0, 0, 0, 0, 0])
    for m in mem:
        p = per[m["trace"]]
        p[0] += int(m["mapping_blocks"])
        p[1] += int(m["mapping_blocks_small_file_rule"])
        p[2] += 1
        p[3] += int(m["size"]) == 0
        p[4] += 0 < int(m["size"]) <= 4096
    g = defaultdict(lambda: [0, 0, 0, 0, 0, 0, []])
    for t, p in per.items():
        cb = int(tr[t]["container_bytes"])
        bs = int(tr[t]["block_size"])
        for k in (group_of(t), "ALL"):
            a = g[k]
            a[0] += 1
            a[1] += cb
            a[2] += (p[0] - p[1]) * bs
            a[3] += p[2]
            a[4] += p[3]
            a[5] += p[4]
            a[6].append((p[0] - p[1]) * bs / cb)
    print("Mapping blocks a container would not have under version 5's rules (one data block, no mapping block, for a member of at most one block; no block at all for an empty member).\n")
    print("| corpus | containers | members | empty | one block | container bytes | saved | saved share | median per container |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for k in sorted(x for x in g if x != "ALL") + ["ALL"]:
        a = g[k]
        print(f"| {k} | {a[0]} | {a[3]:,} | {a[4]:,} | {a[5]:,} | {a[1]:,} | {a[2]:,} | {100*a[2]/a[1]:.1f}% | {100*statistics.median(a[6]):.1f}% |")
    print()


def event_kinds(out):
    p = f"{out}/event_kinds.md"
    if os.path.exists(p):
        print("## `events.dat` kinds\n")
        print(open(p).read().rstrip())
        print()


def main():
    out = sys.argv[1]
    an = f"{out}/analyze"
    corpus_table(an)
    steps_tables(an)
    decode_tables(out)
    stepmap_tables(out, an)
    meta_and_blocks(an)
    event_kinds(out)


if __name__ == "__main__":
    main()

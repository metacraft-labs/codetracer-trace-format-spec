//! `ctfs-measure analyze --out DIR TRACE.ct...`
//!   Writes TSV tables into DIR:
//!   - traces.tsv    one row per container: identity, members, meta.dat
//!   - members.tsv   one row per member: size, data and mapping blocks
//!   - steps.tsv     one row per (container, step rule): raw and zstd bytes
//!   - stepmap.tsv   one row per (container, step-map candidate): bytes
//!   - packs/        per-(container, rule) compressed chunk packs for the
//!                   decode benchmark (`decode-bench`, native and wasm32)
//!
//! `ctfs-measure stepmap-latency --out DIR TRACE.ct...`
//!   Lookup latency per candidate: open, all-hits of a line, next-hit.
//!
//! Every encoding is checked to decode back to the positions it was built
//! from before its size is reported.

use ctfs_measure::calls::{self, CallRange};
use ctfs_measure::ctfs::{Container, frames, parse_idx};
use ctfs_measure::stepmap::{self, Candidate, Inflater, StepMap};
use ctfs_measure::steps::{self, CallMarks, DecodeStats, Rec, Rule};
use ctfs_measure::varint::get_u;
use ctfs_measure::zst;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Meta {
    version: u16,
    flags: u16,
    flags_ext: u32,
    size: usize,
    path_count: u64,
    path_list_bytes: usize,
}

/// The last `meta.dat` schema whose body carries the `paths_count` + `paths[]`
/// block after `recorder_id`. From version 6 `paths.dat` is the only list of
/// source paths and the block is ABSENT -- not empty, so there is no count
/// varint to consume (`internal-files.md` §"Metadata (meta.dat)", v6).
const META_PATH_LIST_LAST_VERSION: u16 = 5;

/// The highest `meta.dat` schema whose body this tool implements.
const META_MAX_VERSION: u16 = 6;

/// Parse a `meta.dat` header far enough to fill [`Meta`].
///
/// **Returns `Err` for a schema it does not implement, rather than `None`.**
/// It used to return `None` from a dozen `?`s, which the caller spelled
/// `.ok().and_then(parse_meta)` and then reported as `meta_version 0` -- a
/// version that does not exist, in a column a report reads. `ctfs-container.md`
/// §1c forbids exactly that shape ("treat an unknown scheme as `none`"), and
/// measured on a version-6 container it was already happening: with no MCR
/// block set, the bytes after `recorder_id` are the END of the payload, the
/// unconditional path-count read ran off the buffer, and the row said 0.
fn parse_meta(b: &[u8]) -> Result<Meta, String> {
    let missing = || "meta.dat: truncated header".to_owned();
    if b.get(0..4).ok_or_else(missing)? != b"CTMD" {
        return Err("meta.dat: bad magic bytes".to_owned());
    }
    let version = u16::from_le_bytes(b[4..6].try_into().map_err(|_| missing())?);
    if version > META_MAX_VERSION {
        return Err(format!(
            "meta.dat: schema version {version} is not supported; this tool reads up to \
             {META_MAX_VERSION}"
        ));
    }
    parse_meta_body(b, version).ok_or_else(|| {
        format!("meta.dat: schema version {version} header did not parse (truncated or not the layout this version declares)")
    })
}

fn parse_meta_body(b: &[u8], version: u16) -> Option<Meta> {
    let flags = u16::from_le_bytes(b.get(6..8)?.try_into().ok()?);
    let mut p = 8;
    let mut flags_ext = 0;
    if version >= 5 {
        flags_ext = u32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
        p = 12;
    }
    let skip_str = |p: &mut usize| -> Option<()> {
        let n = get_u(b, p)? as usize;
        *p += n;
        (*p <= b.len()).then_some(())
    };
    if version >= 3 {
        skip_str(&mut p)?; // recording_id
    }
    skip_str(&mut p)?; // program
    let argc = get_u(b, &mut p)?;
    for _ in 0..argc {
        skip_str(&mut p)?;
    }
    skip_str(&mut p)?; // workdir
    skip_str(&mut p)?; // recorder_id
    // The path list, through `META_PATH_LIST_LAST_VERSION` only. Branching on
    // the VERSION and not on a length, because a v6 body's next varint is
    // whatever the flags say follows -- or nothing at all -- and a small
    // plausible number read as a path count consumes the MCR block as path
    // strings before failing somewhere unrelated to the cause.
    let start = p;
    let pc = if version <= META_PATH_LIST_LAST_VERSION {
        let pc = get_u(b, &mut p)?;
        for _ in 0..pc {
            skip_str(&mut p)?;
        }
        pc
    } else {
        0
    };
    Some(Meta {
        version,
        flags,
        flags_ext,
        size: b.len(),
        path_count: pc,
        path_list_bytes: p - start,
    })
}

struct Trace {
    label: String,
    recs: Vec<Rec>,
    stats: DecodeStats,
    marks: CallMarks,
    calls: usize,
    column_aware: bool,
    stepmap: Option<(StepMap, bool)>,
}

fn load_steps(c: &Container) -> Result<(Vec<Rec>, DecodeStats), String> {
    let dat = c.read("steps.dat").map_err(|e| e.to_string())?;
    let idx = c.read("steps.idx").map_err(|e| e.to_string())?;
    let (cs, offs) = parse_idx(&idx).map_err(|e| e.to_string())?;
    let mut st = DecodeStats {
        chunk_size: cs,
        ..Default::default()
    };
    let mut recs = Vec::new();
    let mut cursor = None;
    let mut threads = BTreeSet::new();
    for f in frames(&dat, &offs) {
        let raw = zst::decompress(f);
        st.raw_bytes += raw.len() as u64;
        st.compressed_bytes += f.len() as u64;
        st.chunks += 1;
        steps::decode_chunk(&raw, &mut recs, &mut cursor, &mut st, &mut threads)?;
    }
    st.threads = threads.len() as u64;
    Ok((recs, st))
}

fn load_calls(c: &Container, n: usize) -> (CallMarks, usize) {
    let mut marks = CallMarks::default();
    let (Ok(dat), Ok(idx)) = (c.read("calls.dat"), c.read("calls.idx")) else {
        return (marks, 0);
    };
    let Ok((_, offs)) = parse_idx(&idx) else {
        return (marks, 0);
    };
    let mut calls: Vec<CallRange> = Vec::new();
    for f in frames(&dat, &offs) {
        if calls::parse_chunk(&zst::decompress(f), &mut calls).is_none() {
            return (marks, 0);
        }
    }
    marks.entry = vec![false; n];
    marks.ret = vec![false; n];
    for cr in &calls {
        if cr.parent < 0 {
            continue; // the toplevel frame is entered by the first step anyway
        }
        if let Some(e) = marks.entry.get_mut(cr.first as usize) {
            *e = true;
        }
        if let Some(r) = marks.ret.get_mut(cr.last as usize + 1) {
            *r = true;
        }
    }
    marks.known = true;
    (marks, calls.len())
}

/// Marks apply to the first *position* record at or after the marked exec
/// record: a call's first exec record may be a thread record.
fn spread_marks(recs: &[Rec], m: &mut CallMarks) {
    if !m.known {
        return;
    }
    for v in [&mut m.entry, &mut m.ret] {
        let mut pending = false;
        for (i, r) in recs.iter().enumerate() {
            pending |= v[i];
            v[i] = false;
            if pending && matches!(r, Rec::Pos { .. }) {
                v[i] = true;
                pending = false;
            }
        }
    }
}

fn synth_stepmap(recs: &[Rec]) -> StepMap {
    StepMap::build(recs.iter().enumerate().filter_map(|(i, r)| match r {
        Rec::Pos { pos, column: false } => Some((pos / 100_000, (pos % 100_000) as u32, i as i64)),
        _ => None,
    }))
}

fn label_for(p: &Path, corpus_root: Option<&Path>) -> String {
    match corpus_root.and_then(|r| p.strip_prefix(r).ok()) {
        Some(rel) => rel.display().to_string(),
        None => p.display().to_string(),
    }
}

fn load(p: &Path, root: Option<&Path>) -> Result<(Trace, Container, Option<Meta>), String> {
    let c = Container::open(p).map_err(|e| e.to_string())?;
    // A `meta.dat` this tool cannot parse is a REFUSAL, not an absent one: the
    // output has meta_* columns, so a container whose header did not parse
    // would contribute wrong numbers rather than no numbers.
    let meta = match c.read("meta.dat") {
        Ok(b) => Some(parse_meta(&b)?),
        Err(_) => None,
    };
    let column_aware = meta.as_ref().is_some_and(|m| m.flags & (1 << 4) != 0);
    let (recs, stats) = load_steps(&c)?;
    let (mut marks, ncalls) = load_calls(&c, recs.len());
    spread_marks(&recs, &mut marks);
    let stepmap = if column_aware {
        None
    } else {
        match c
            .read("step-map.ns")
            .ok()
            .and_then(|b| StepMap::decode_v1(&b))
        {
            Some(m) => Some((m, false)),
            None => Some((synth_stepmap(&recs), true)),
        }
    };
    Ok((
        Trace {
            label: label_for(p, root),
            recs,
            stats,
            marks,
            calls: ncalls,
            column_aware,
            stepmap,
        },
        c,
        meta,
    ))
}

fn tsv(dir: &Path, name: &str, header: &str) -> std::fs::File {
    let mut f = std::fs::File::create(dir.join(name)).unwrap();
    writeln!(f, "{header}").unwrap();
    f
}

fn write_pack(path: &Path, chunks: &[Vec<u8>], records: u64, positions: u64) {
    // [u64 records][u64 positions][u32 n]([u32 len][bytes])*
    let mut out = Vec::new();
    out.extend_from_slice(&records.to_le_bytes());
    out.extend_from_slice(&positions.to_le_bytes());
    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    for c in chunks {
        out.extend_from_slice(&(c.len() as u32).to_le_bytes());
        out.extend_from_slice(c);
    }
    std::fs::write(path, out).unwrap();
}

/// How a corpus sweep ended, per input, so that a sweep over a corpus NOTHING
/// can read is distinguishable from a sweep over no corpus at all.
///
/// # Why this type exists
///
/// Both sweeps below used to `continue` past an input they could not load —
/// `analyze` after an `eprintln!` whose stream `run_measurements.sh` redirects
/// into a log nothing reads, and `stepmap_latency` with no message at all. The
/// binary then exited 0 having written header-only TSVs, the report rendered a
/// report of nothing, and the shell script's `set -euo pipefail` could not help
/// because nothing had failed.
///
/// That is the same defect, in the same campaign, as a reader probe that cannot
/// tell "the reader is absent" from "the reader is present and refusing": a
/// missing input and an unreadable one have different remedies, and collapsing
/// them loses the remedy along with the finding. So there are THREE outcomes
/// here, and the one in the middle is fatal:
///
/// * `not_a_container` — no CTFS magic. The conformance kit's ASCII
///   placeholders are these, legitimately, and they are reported and skipped.
/// * `no_step_stream` — a container that READ, and carries no `steps.dat`.
///   This tool measures the step encoding, so such a container is outside its
///   scope rather than beyond its reach: the events.log-shaped bundles are
///   these. Reported and skipped, and kept apart from the row below because
///   "I read it and there is nothing here to measure" and "I could not read
///   it" have different remedies, which is the distinction this whole type is
///   for.
/// * `refused` — a container this tool could not decode. **A finding.** The
///   tool's reader is in this repository, so an input it cannot read is either
///   a format revision this reader has not followed or a corrupt artefact, and
///   both want someone's attention rather than a silent omission.
/// * `measured` — decoded, and in the output.
#[derive(Default)]
struct Census {
    measured: usize,
    not_a_container: Vec<String>,
    no_step_stream: Vec<String>,
    refused: Vec<(String, String)>,
}

impl Census {
    fn record(&mut self, path: &Path, outcome: Result<(), String>) {
        match outcome {
            Ok(()) => self.measured += 1,
            Err(e) if e.contains("not a CTFS container") => {
                self.not_a_container.push(path.display().to_string());
            }
            // Matched on the member NAME rather than on a generic "not found",
            // so a missing `steps.dat` stays a scope statement while a missing
            // `meta.dat` or a short mapping block stays a finding.
            Err(e) if e == "no member steps.dat" || e == "no member steps.idx" => {
                self.no_step_stream.push(path.display().to_string());
            }
            Err(e) => self.refused.push((path.display().to_string(), e)),
        }
    }

    /// Print the census and EXIT NON-ZERO if anything was refused, or if a
    /// non-empty corpus produced no measurement at all.
    ///
    /// Exits rather than returning a flag because every caller is the end of
    /// `main` and a flag is a thing a caller can drop. Exit code 3 is "the
    /// corpus did not resolve", distinct from a panic's 101.
    fn verdict(&self, what: &str, total: usize) {
        eprintln!(
            "{what}: {} of {total} inputs measured, {} not containers, {} without a step stream, \
             {} refused",
            self.measured,
            self.not_a_container.len(),
            self.no_step_stream.len(),
            self.refused.len()
        );
        for p in &self.not_a_container {
            eprintln!("  not a container (no CTFS magic), skipped: {p}");
        }
        for p in &self.no_step_stream {
            eprintln!("  read, but carries no steps.dat (outside this tool's scope), skipped: {p}");
        }
        for (p, e) in &self.refused {
            eprintln!("  REFUSED by this tool's own reader: {p}: {e}");
        }
        if !self.refused.is_empty() {
            eprintln!(
                "{what}: FAILED — {} input(s) are containers this tool could not read. A \
                 measurement taken with those omitted is a measurement of a different corpus, so \
                 this is an error and not a note. Fix the reader (tools/ctfs-measure/src/ctfs.rs) \
                 or re-record the input; do NOT re-run with them removed from the list.",
                self.refused.len()
            );
            std::process::exit(3);
        }
        if total > 0 && self.measured == 0 && self.no_step_stream.len() < total {
            eprintln!(
                "{what}: FAILED — a corpus of {total} input(s) produced zero measurements. \
                 Header-only output is not a result."
            );
            std::process::exit(3);
        }
    }
}

fn analyze(out: &Path, root: Option<&Path>, files: &[PathBuf]) {
    std::fs::create_dir_all(out.join("packs")).unwrap();
    let mut t_tr = tsv(
        out,
        "traces.tsv",
        "trace\tcontainer_bytes\tblock_size\tmembers\tmeta_version\tmeta_flags\tmeta_flags_ext\tmeta_bytes\tmeta_path_count\tmeta_path_list_bytes\tpaths_dat_bytes\tcolumn_aware\trecords\tpos_records\tabs\tdelta\tcolumn\tother\tthreads\tcalls\tcall_marks\tchunk_size\tchunks\tunanchored_chunks\tsteps_raw\tsteps_zstd\tstepmap_bytes\tstepmap_synth",
    );
    let mut t_mem = tsv(
        out,
        "members.tsv",
        "trace\tmember\tsize\tdata_blocks\tmapping_blocks\tmapping_blocks_small_file_rule",
    );
    let mut t_st = tsv(
        out,
        "steps.tsv",
        "trace\trule\trecords\tpos_records\tabs_records\traw_bytes\tzstd_bytes\tchunks\troundtrip",
    );
    let mut t_sm = tsv(
        out,
        "stepmap.tsv",
        "trace\tcandidate\tsteps\tlines\tpaths\tstored_bytes\tdirectory_bytes\tsynth",
    );
    let mut census = Census::default();
    for (n, f) in files.iter().enumerate() {
        let (tr, c, meta) = match load(f, root) {
            Ok(x) => {
                census.record(f, Ok(()));
                x
            }
            Err(e) => {
                census.record(f, Err(e));
                continue;
            }
        };
        let pos_records = tr
            .recs
            .iter()
            .filter(|r| matches!(r, Rec::Pos { .. }))
            .count();
        let paths_dat = c.entry("paths.dat").map(|e| e.size).unwrap_or(0);
        let sm_bytes = c.entry("step-map.ns").map(|e| e.size).unwrap_or(0);
        let m = meta.as_ref();
        writeln!(
            t_tr,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            tr.label,
            c.bytes.len(),
            c.block_size,
            c.entries.len(),
            m.map_or(0, |m| m.version),
            m.map_or(0, |m| m.flags),
            m.map_or(0, |m| m.flags_ext),
            m.map_or(0, |m| m.size),
            m.map_or(0, |m| m.path_count),
            m.map_or(0, |m| m.path_list_bytes),
            paths_dat,
            tr.column_aware as u8,
            tr.stats.records,
            pos_records,
            tr.stats.abs,
            tr.stats.delta,
            tr.stats.column,
            tr.stats.other,
            tr.stats.threads,
            tr.calls,
            tr.marks.known as u8,
            tr.stats.chunk_size,
            tr.stats.chunks,
            tr.stats.unanchored_chunks,
            tr.stats.raw_bytes,
            tr.stats.compressed_bytes,
            sm_bytes,
            tr.stepmap.as_ref().map_or(2, |s| s.1 as u8),
        )
        .unwrap();
        for e in &c.entries {
            let u = c.block_use(e);
            writeln!(
                t_mem,
                "{}\t{}\t{}\t{}\t{}\t{}",
                tr.label,
                e.name,
                e.size,
                u.data_blocks,
                u.mapping_blocks,
                u.mapping_blocks_small_file_rule
            )
            .unwrap();
        }
        let want = steps::positions(&tr.recs);
        for rule in Rule::ALL {
            let chunks = steps::encode(&tr.recs, &tr.marks, rule, 4096);
            let mut got = Vec::with_capacity(want.len());
            for ch in &chunks {
                steps::decode_positions(ch, &mut got);
            }
            let ok = got == want;
            if !ok && rule != Rule::Nim {
                panic!("{}: rule {} does not round-trip", tr.label, rule.name());
            }
            let comp: Vec<Vec<u8>> = chunks.iter().map(|ch| zst::compress(ch, 3)).collect();
            let abs_records = chunks
                .iter()
                .map(|ch| {
                    let mut p = 0;
                    let mut a = 0u64;
                    while p < ch.len() {
                        let t = ch[p];
                        p += 1;
                        match t {
                            0 => {
                                a += 1;
                                get_u(ch, &mut p);
                            }
                            1 | 7 | 3..=6 => {
                                get_u(ch, &mut p);
                            }
                            2 => {
                                get_u(ch, &mut p);
                                let n = get_u(ch, &mut p).unwrap() as usize;
                                p += n;
                            }
                            8 => {
                                get_u(ch, &mut p);
                                let n = get_u(ch, &mut p).unwrap();
                                for _ in 0..n * 3 + 1 {
                                    get_u(ch, &mut p);
                                }
                            }
                            _ => unreachable!(),
                        }
                    }
                    a
                })
                .sum::<u64>();
            writeln!(
                t_st,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                tr.label,
                rule.name(),
                tr.recs.len(),
                pos_records,
                abs_records,
                chunks.iter().map(|c| c.len()).sum::<usize>(),
                comp.iter().map(|c| c.len()).sum::<usize>(),
                chunks.len(),
                ok as u8
            )
            .unwrap();
            write_pack(
                &out.join("packs")
                    .join(format!("{n:04}.{}.pack", rule.name())),
                &comp,
                tr.recs.len() as u64,
                pos_records as u64,
            );
        }
        std::fs::write(out.join("packs").join(format!("{n:04}.label")), &tr.label).unwrap();
        if let Some((sm, synth)) = &tr.stepmap {
            if sm.steps() > 0 {
                for cand in Candidate::ALL {
                    let enc = stepmap::encode(sm, cand);
                    // Every candidate must give back every list.
                    let mut r = stepmap::Reader::open(cand, &enc.stored, Inflater::CZstd);
                    let mut buf = Vec::new();
                    let mut i = 0;
                    for (_, lines) in &sm.paths {
                        for (_, ids) in lines {
                            buf.clear();
                            r.hits(i, Inflater::CZstd, &mut buf);
                            assert_eq!(&buf, ids, "{} {}", tr.label, cand.name());
                            i += 1;
                        }
                    }
                    writeln!(
                        t_sm,
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        tr.label,
                        cand.name(),
                        sm.steps(),
                        sm.lines(),
                        sm.paths.len(),
                        enc.stored.len(),
                        enc.directory_bytes,
                        *synth as u8
                    )
                    .unwrap();
                }
                let dir = out.join("stepmaps");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join(format!("{n:04}.v1.bin")), sm.encode_v1()).unwrap();
                std::fs::write(dir.join(format!("{n:04}.label")), &tr.label).unwrap();
                for mode in stepmap::PackedMode::ALL {
                    let enc = stepmap::encode_packed_mode(sm, stepmap::PACKED_TARGET, mode);
                    std::fs::write(dir.join(format!("{n:04}.{}.bin", mode.name())), &enc).unwrap();
                    let mut pk = stepmap::Packed::open(&enc);
                    let all = pk.load_all(Inflater::CZstd);
                    assert_eq!(all.len(), sm.lines(), "{} {}", tr.label, mode.name());
                    for (pid, lines) in &sm.paths {
                        for (line, ids) in lines {
                            assert_eq!(all.get(&(*pid, *line)), Some(ids), "{} {}", tr.label, mode.name());
                        }
                    }
                    writeln!(
                        t_sm,
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        tr.label,
                        mode.name(),
                        sm.steps(),
                        sm.lines(),
                        sm.paths.len(),
                        enc.len(),
                        26 + 20 * pk.chunks.len(),
                        *synth as u8
                    )
                    .unwrap();
                }
            }
        }
        eprintln!(
            "[{}/{}] {} ({} records)",
            n + 1,
            files.len(),
            tr.label,
            tr.recs.len()
        );
    }
    census.verdict("analyze", files.len());
}

/// Times `f` until at least `min_ms` have elapsed, returning ns per call.
fn time_ns(min_ms: u128, mut f: impl FnMut()) -> f64 {
    // The machine is shared, so a mean over one window is at the mercy of
    // whatever else is running: take the best of several rounds instead.
    f(); // warm
    let rounds = 7;
    let per_round = (min_ms / rounds).max(5);
    let mut best = f64::INFINITY;
    for _ in 0..rounds {
        let t = Instant::now();
        let mut n = 0u64;
        while t.elapsed().as_millis() < per_round || n < 2 {
            f();
            n += 1;
        }
        best = best.min(t.elapsed().as_nanos() as f64 / n as f64);
    }
    best
}

fn stepmap_latency(out: &Path, root: Option<&Path>, files: &[PathBuf]) {
    std::fs::create_dir_all(out).unwrap();
    let mut t = tsv(
        out,
        "stepmap_latency.tsv",
        "trace\tcandidate\tinflater\tsteps\tlines\tstored_bytes\topen_ns\tall_hits_ns_mean\tall_hits_ns_hottest\tnext_hit_ns_mean\tnext_hit_ns_hottest\tcold_breakpoint_ns\tload_all_ns",
    );
    let mut census = Census::default();
    for f in files {
        let tr = match load(f, root) {
            Ok((tr, _, _)) => {
                census.record(f, Ok(()));
                tr
            }
            Err(e) => {
                census.record(f, Err(e));
                continue;
            }
        };
        let Some((sm, _)) = &tr.stepmap else { continue };
        if sm.steps() == 0 {
            continue;
        }
        let hottest = {
            let mut best = (0usize, 0usize);
            let mut i = 0;
            for (_, lines) in &sm.paths {
                for (_, ids) in lines {
                    if ids.len() > best.1 {
                        best = (i, ids.len());
                    }
                    i += 1;
                }
            }
            best.0
        };
        let nlines = sm.lines();
        let total_steps = tr.recs.len() as i64;
        for cand in Candidate::ALL {
            let enc = stepmap::encode(sm, cand);
            for inf in [Inflater::CZstd, Inflater::Ruzstd] {
                if matches!(cand, Candidate::V1 | Candidate::Gap) && inf == Inflater::Ruzstd {
                    continue;
                }
                let open = time_ns(30, || {
                    std::hint::black_box(stepmap::Reader::open(cand, &enc.stored, inf));
                });
                let mut r = stepmap::Reader::open(cand, &enc.stored, inf);
                let mut buf = Vec::new();
                // Mean over every line, each lookup on a cold one-chunk cache
                // would overstate a UI that sets one breakpoint; a warm cache
                // understates it. Lines are visited in a stride order so the
                // cache is mostly cold for chunked candidates.
                let stride = (nlines / 7).max(1) | 1;
                let order: Vec<usize> = (0..nlines).map(|k| (k * stride) % nlines).collect();
                let all_mean = time_ns(50, || {
                    for &i in &order {
                        buf.clear();
                        r.hits(i, inf, &mut buf);
                    }
                }) / nlines as f64;
                let all_hot = time_ns(30, || {
                    buf.clear();
                    r.hits(hottest, inf, &mut buf);
                });
                let mut scratch = Vec::new();
                let probes: Vec<(usize, i64)> = order
                    .iter()
                    .enumerate()
                    .map(|(k, &i)| (i, (k as i64 * 7919) % total_steps.max(1)))
                    .collect();
                let next_mean = time_ns(50, || {
                    for &(i, s) in &probes {
                        std::hint::black_box(r.next_hit(i, s, inf, &mut scratch));
                    }
                }) / probes.len() as f64;
                let next_hot = time_ns(30, || {
                    std::hint::black_box(r.next_hit(hottest, total_steps / 2, inf, &mut scratch));
                });
                // Cold path a UI pays for one breakpoint: open, find, all hits.
                let (p0, l0) = {
                    let e = &r.entries[hottest];
                    (e.path, e.line)
                };
                let cold = time_ns(30, || {
                    let mut r2 = stepmap::Reader::open(cand, &enc.stored, inf);
                    let i = r2.find(p0, l0).unwrap();
                    let mut b = Vec::new();
                    r2.hits(i, inf, &mut b);
                    std::hint::black_box(b);
                });
                let load_all = time_ns(60, || {
                    std::hint::black_box(stepmap::load_all(cand, &enc.stored, inf));
                });
                writeln!(
                    t,
                    "{}\t{}\t{:?}\t{}\t{}\t{}\t{:.0}\t{:.1}\t{:.0}\t{:.1}\t{:.0}\t{:.0}\t{:.0}",
                    tr.label,
                    cand.name(),
                    inf,
                    sm.steps(),
                    nlines,
                    enc.stored.len(),
                    open,
                    all_mean,
                    all_hot,
                    next_mean,
                    next_hot,
                    cold,
                    load_all
                )
                .unwrap();
            }
        }
        {
            let enc = stepmap::encode_packed(sm, stepmap::PACKED_TARGET);
            for inf in [Inflater::CZstd, Inflater::Ruzstd] {
                let open = time_ns(30, || {
                    std::hint::black_box(stepmap::Packed::open(&enc));
                });
                let mut pk = stepmap::Packed::open(&enc);
                let keys: Vec<(u64, u32)> = sm
                    .paths
                    .iter()
                    .flat_map(|(p, ls)| ls.iter().map(move |(l, _)| (*p, *l)))
                    .collect();
                let stride = (keys.len() / 7).max(1) | 1;
                let order: Vec<(u64, u32)> = (0..keys.len())
                    .map(|k| keys[(k * stride) % keys.len()])
                    .collect();
                let mut buf = Vec::new();
                let all_mean = time_ns(50, || {
                    for &(p, l) in &order {
                        buf.clear();
                        pk.lookup(p, l, inf, &mut buf);
                    }
                }) / keys.len() as f64;
                let hot_key = {
                    let mut best = ((0u64, 0u32), 0usize);
                    for (p, ls) in &sm.paths {
                        for (l, ids) in ls {
                            if ids.len() > best.1 {
                                best = ((*p, *l), ids.len());
                            }
                        }
                    }
                    best.0
                };
                let all_hot = time_ns(30, || {
                    buf.clear();
                    pk.lookup(hot_key.0, hot_key.1, inf, &mut buf);
                });
                let cold = time_ns(30, || {
                    let mut p2 = stepmap::Packed::open(&enc);
                    let mut b = Vec::new();
                    p2.lookup(hot_key.0, hot_key.1, inf, &mut b);
                    std::hint::black_box(b);
                });
                let load_all = time_ns(60, || {
                    let mut p2 = stepmap::Packed::open(&enc);
                    std::hint::black_box(p2.load_all(inf));
                });
                writeln!(
                    t,
                    "{}\tpacked\t{:?}\t{}\t{}\t{}\t{:.0}\t{:.1}\t{:.0}\t\t\t{:.0}\t{:.0}",
                    tr.label,
                    inf,
                    sm.steps(),
                    nlines,
                    enc.len(),
                    open,
                    all_mean,
                    all_hot,
                    cold,
                    load_all
                )
                .unwrap();
            }
        }
        eprintln!("latency {}", tr.label);
    }
    census.verdict("stepmap-latency", files.len());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: ctfs-measure (analyze|stepmap-latency) --out DIR [--root CORPUS_ROOT] (TRACE.ct... | --list FILE)";
    let cmd = args.get(1).expect(usage).clone();
    let mut out = None;
    let mut root = None;
    let mut files = Vec::new();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                out = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--root" => {
                root = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--list" => {
                let s = std::fs::read_to_string(&args[i + 1]).unwrap();
                files.extend(
                    s.lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(PathBuf::from),
                );
                i += 1;
            }
            f => files.push(PathBuf::from(f)),
        }
        i += 1;
    }
    let out = out.expect(usage);
    match cmd.as_str() {
        "analyze" => analyze(&out, root.as_deref(), &files),
        "stepmap-latency" => stepmap_latency(&out, root.as_deref(), &files),
        _ => panic!("{usage}"),
    }
}

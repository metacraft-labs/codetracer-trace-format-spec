//! `decode-bench PACK_DIR [LABEL_FILTER]`
//!
//! Decode speed of `steps.dat` under each candidate rule, from the packs
//! `ctfs-measure analyze` wrote. Builds natively and for `wasm32-wasip1`
//! (`--no-default-features`), so the same kernel is timed in both.
//!
//! For each (trace, rule) it prints, per record:
//! - `parse_ns`: decoding already-inflated chunks to absolute positions;
//! - `ruzstd_ns`: inflating every chunk with ruzstd, then decoding;
//! - `czstd_ns`: the same with C zstd (native builds only).

use ctfs_measure::steps::decode_positions;
use ctfs_measure::zst::ruzstd_decompress;
use std::time::Instant;

fn read_pack(path: &std::path::Path) -> (u64, u64, Vec<Vec<u8>>) {
    let b = std::fs::read(path).unwrap();
    let records = u64::from_le_bytes(b[0..8].try_into().unwrap());
    let positions = u64::from_le_bytes(b[8..16].try_into().unwrap());
    let n = u32::from_le_bytes(b[16..20].try_into().unwrap()) as usize;
    let mut p = 20;
    let mut chunks = Vec::with_capacity(n);
    for _ in 0..n {
        let l = u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize;
        p += 4;
        chunks.push(b[p..p + l].to_vec());
        p += l;
    }
    (records, positions, chunks)
}

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

/// `decode-bench stepmap DIR`: for the v1 map and every `packed` variant
/// `ctfs-measure analyze` wrote: a full load (what the db-backend does at
/// open), the same with the member's bytes read from the file first, and one
/// cold single-line lookup of the hottest line; with ruzstd (the decoder the
/// wasm32 reader uses) and, natively, C zstd.
fn stepmap_bench(dir: &std::path::Path, min_ms: u128) {
    use ctfs_measure::stepmap::{load_all, Candidate, Inflater, Packed, PackedMode};
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter_map(|n| n.strip_suffix(".v1.bin").map(str::to_string))
        .collect();
    ids.sort();
    println!("trace\tsteps\tlayout\tinflater\tbytes\tload_ns\tread_load_ns\tcold_lookup_ns");
    #[allow(unused_mut)]
    let mut infs = vec![Inflater::Ruzstd];
    #[cfg(feature = "native")]
    infs.push(Inflater::CZstd);
    for id in ids {
        let label = std::fs::read_to_string(dir.join(format!("{id}.label"))).unwrap_or(id.clone());
        let v1_path = dir.join(format!("{id}.v1.bin"));
        let v1 = std::fs::read(&v1_path).unwrap();
        let all = load_all(Candidate::V1, &v1, Inflater::Ruzstd);
        let steps: usize = all.values().map(|v| v.len()).sum();
        let hot = *all.iter().max_by_key(|(_, v)| v.len()).map(|(k, _)| k).unwrap();
        drop(all);
        let load = time_ns(min_ms, || {
            std::hint::black_box(load_all(Candidate::V1, &v1, Inflater::Ruzstd));
        });
        let read_load = time_ns(min_ms, || {
            let b = std::fs::read(&v1_path).unwrap();
            std::hint::black_box(load_all(Candidate::V1, &b, Inflater::Ruzstd));
        });
        let cold = time_ns(min_ms, || {
            let mut r = ctfs_measure::stepmap::Reader::open(Candidate::V1, &v1, Inflater::Ruzstd);
            let i = r.find(hot.0, hot.1).unwrap();
            let mut b = Vec::new();
            r.hits(i, Inflater::Ruzstd, &mut b);
            std::hint::black_box(b);
        });
        println!("{label}\t{steps}\tv1\t-\t{}\t{load:.0}\t{read_load:.0}\t{cold:.0}", v1.len());
        for mode in PackedMode::ALL {
            let path = dir.join(format!("{id}.{}.bin", mode.name()));
            let Ok(pk) = std::fs::read(&path) else { continue };
            for &inf in &infs {
                let uses_zstd = matches!(mode, PackedMode::GapZstd | PackedMode::RleZstd);
                if !uses_zstd && inf != Inflater::Ruzstd {
                    continue;
                }
                let load = time_ns(min_ms, || {
                    std::hint::black_box(Packed::open(&pk).load_all(inf));
                });
                let read_load = time_ns(min_ms, || {
                    let b = std::fs::read(&path).unwrap();
                    std::hint::black_box(Packed::open(&b).load_all(inf));
                });
                let cold = time_ns(min_ms, || {
                    let mut b = Vec::new();
                    Packed::open(&pk).lookup(hot.0, hot.1, inf, &mut b);
                    std::hint::black_box(b);
                });
                let infname = if uses_zstd { format!("{inf:?}") } else { "-".to_string() };
                println!(
                    "{label}\t{steps}\t{}\t{infname}\t{}\t{load:.0}\t{read_load:.0}\t{cold:.0}",
                    mode.name(),
                    pk.len()
                );
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("stepmap") {
        let min_ms: u128 = std::env::var("BENCH_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(150);
        stepmap_bench(std::path::Path::new(&args[2]), min_ms);
        return;
    }
    let dir = std::path::PathBuf::from(args.get(1).expect("usage: decode-bench PACK_DIR [FILTER]"));
    let filter = args.get(2).cloned();
    let min_ms: u128 = std::env::var("BENCH_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(150);
    // Traces below this many records are dominated by per-frame setup and
    // say nothing about the rule; `MIN_RECORDS` skips them.
    let min_records: u64 = std::env::var("MIN_RECORDS").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".pack"))
        .collect();
    names.sort();
    println!(
        "trace\trule\trecords\tpositions\tparse_ns_per_record\truzstd_ns_per_record\tczstd_ns_per_record"
    );
    let mut dec = ruzstd::decoding::FrameDecoder::new();
    for name in names {
        let (id, rule) = {
            let s = name.trim_end_matches(".pack");
            let (a, b) = s.split_once('.').unwrap();
            (a.to_string(), b.to_string())
        };
        let label = std::fs::read_to_string(dir.join(format!("{id}.label"))).unwrap_or(id.clone());
        if let Some(f) = &filter {
            if !label.contains(f.as_str()) {
                continue;
            }
        }
        let (records, positions, frames) = read_pack(&dir.join(&name));
        if records == 0 || records < min_records {
            continue;
        }
        let mut raws = Vec::new();
        let mut buf = Vec::new();
        for f in &frames {
            ruzstd_decompress(&mut dec, f, &mut buf);
            raws.push(buf.clone());
        }
        let mut out: Vec<u64> = Vec::with_capacity(4096 * 2);
        let parse = time_ns(min_ms, || {
            let mut n = 0;
            for r in &raws {
                out.clear();
                n += decode_positions(r, &mut out);
            }
            assert_eq!(n as u64, positions);
            std::hint::black_box(&out);
        });
        let ruz = time_ns(min_ms, || {
            for f in &frames {
                ruzstd_decompress(&mut dec, f, &mut buf);
                out.clear();
                decode_positions(&buf, &mut out);
            }
            std::hint::black_box(&out);
        });
        #[cfg(feature = "native")]
        let cz = time_ns(min_ms, || {
            for f in &frames {
                let raw = ctfs_measure::zst::decompress(f);
                out.clear();
                decode_positions(&raw, &mut out);
            }
            std::hint::black_box(&out);
        });
        #[cfg(not(feature = "native"))]
        let cz = f64::NAN;
        let r = records as f64;
        println!(
            "{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}",
            label,
            rule,
            records,
            positions,
            parse / r,
            ruz / r,
            cz / r
        );
    }
}

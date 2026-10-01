// Size and lookup cost of the specified step map (`packed-rle`, version 2) as a function of its chunk
// target. Reads the `NNNN.v1.bin` blobs `ctfs-measure analyze` wrote.
// Usage: stepmap_sweep STEPMAPS_DIR
use ctfs_measure::stepmap::{encode_packed_mode, Inflater, Packed, PackedMode, StepMap};
use std::time::Instant;

fn best_ns(mut f: impl FnMut()) -> f64 {
    f();
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let t = Instant::now();
        let mut n = 0;
        while t.elapsed().as_millis() < 10 || n < 2 {
            f();
            n += 1;
        }
        best = best.min(t.elapsed().as_nanos() as f64 / n as f64);
    }
    best
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let targets = [4 * 1024, 16 * 1024, 64 * 1024, 256 * 1024, usize::MAX];
    let mut tot = vec![0usize; targets.len()];
    let mut lookup = vec![0f64; targets.len()];
    let mut load = vec![0f64; targets.len()];
    let mut steps = 0usize;
    let mut n = 0;
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(".v1.bin")).collect();
    names.sort();
    for p in names {
        let m = StepMap::decode_v1(&std::fs::read(&p).unwrap()).unwrap();
        if m.steps() < 1000 {
            continue;
        }
        n += 1;
        steps += m.steps();
        let keys: Vec<(u64, u32)> = m.paths.iter().flat_map(|(p, ls)| ls.iter().map(move |(l, _)| (*p, *l))).collect();
        for (i, &t) in targets.iter().enumerate() {
            let enc = encode_packed_mode(&m, t, PackedMode::RleZstd);
            tot[i] += enc.len();
            // Mean cold lookup of one line (open + inflate one chunk + scan).
            let k = keys.len();
            let probe: Vec<(u64, u32)> = (0..k.min(64)).map(|j| keys[(j * 7919) % k]).collect();
            lookup[i] += best_ns(|| {
                for &(pp, l) in &probe {
                    let mut b = Vec::new();
                    Packed::open(&enc).lookup(pp, l, Inflater::CZstd, &mut b);
                    std::hint::black_box(b);
                }
            }) / probe.len() as f64;
            load[i] += best_ns(|| {
                std::hint::black_box(Packed::open(&enc).load_all(Inflater::CZstd));
            }) / m.steps() as f64;
        }
    }
    println!("{n} step maps with >= 1000 steps, {steps} steps");
    println!("target\tbytes/step\tmean cold one-line lookup ns (per map, averaged)\tload ns/step (averaged)");
    for (i, t) in targets.iter().enumerate() {
        let name = if *t == usize::MAX { "one frame".to_string() } else { format!("{} KiB", t / 1024) };
        println!("{name}\t{:.4}\t{:.0}\t{:.2}", tot[i] as f64 / steps as f64, lookup[i] / n as f64, load[i] / n as f64);
    }
}

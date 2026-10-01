// `events.dat` kind bytes across a corpus, and the stored size of the stream
// with each record's kind as written against the same records with the kind
// collapsed the way the Nim writer's 4-value IOEventKind API collapses it
// (0,1,2 -> 0; 3..=10 -> 4; 11 -> 11; 12,13 -> 12). Records are re-chunked
// 64 per level-3 frame, as written.
// Usage: event_kinds CORPUS_ROOT LIST_FILE
use ctfs_measure::ctfs::{frames, parse_idx, Container};
use ctfs_measure::varint::{get_u, put_u};
use ctfs_measure::zst;
use std::collections::BTreeMap;

fn collapse(k: u8) -> u8 {
    match k {
        0..=2 => 0,
        3..=10 => 4,
        11 => 11,
        12 | 13 => 12,
        x => x,
    }
}

/// The worst case for preserving kinds: every record's kind replaced by a
/// pseudo-random member of the class the Nim API collapses it into, so the
/// kind byte carries the most entropy those classes allow.
fn expand(k: u8, i: u64) -> u8 {
    let r = (i.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as u8;
    match k {
        0 => r % 3,
        4 => 3 + r % 8,
        12 => 12 + r % 2,
        x => x,
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let list = std::fs::read_to_string(&a[2]).unwrap();
    let mut by_group: BTreeMap<String, (u64, u64, u64, u64, BTreeMap<u8, u64>, u64)> = BTreeMap::new();
    for line in list.lines().filter(|l| !l.is_empty()) {
        let p = std::path::Path::new(line);
        let Ok(c) = Container::open(p) else { continue };
        let (Ok(dat), Ok(idx)) = (c.read("events.dat"), c.read("events.idx")) else { continue };
        let Ok((cs, offs)) = parse_idx(&idx) else { continue };
        let mut recs: Vec<Vec<u8>> = Vec::new();
        for f in frames(&dat, &offs) {
            let raw = zst::decompress(f);
            let mut q = 0;
            while q < raw.len() {
                let n = get_u(&raw, &mut q).unwrap() as usize;
                recs.push(raw[q..q + n].to_vec());
                q += n;
            }
        }
        let g = p.strip_prefix(&root).unwrap().iter().next().unwrap().to_string_lossy().to_string();
        let e = by_group.entry(g).or_default();
        let mut serial = 0u64;
        let mut enc = |mode: u8| -> u64 {
            recs.chunks(cs.max(1) as usize)
                .map(|ch| {
                    let mut raw = Vec::new();
                    for r in ch {
                        let mut r = r.clone();
                        serial += 1;
                        if mode == 1 {
                            r[0] = collapse(r[0]);
                        } else if mode == 2 {
                            r[0] = expand(collapse(r[0]), serial);
                        }
                        put_u(&mut raw, r.len() as u64);
                        raw.extend_from_slice(&r);
                    }
                    zst::compress(&raw, 3).len() as u64
                })
                .sum()
        };
        e.0 += 1;
        e.1 += recs.len() as u64;
        e.2 += enc(0);
        e.3 += enc(1);
        e.5 += enc(2);
        for r in &recs {
            *e.4.entry(r[0]).or_default() += 1;
        }
    }
    println!("| corpus | traces | records | stored bytes, kinds collapsed | kinds expanded at random within each class (upper bound) | cost | kind bytes seen (count) |");
    println!("|---|---:|---:|---:|---:|---:|---|");
    let mut t = (0u64, 0u64, 0u64, 0u64, 0u64);
    for (g, e) in &by_group {
        if e.1 == 0 {
            continue;
        }
        t.0 += e.0;
        t.1 += e.1;
        t.2 += e.2;
        t.3 += e.3;
        t.4 += e.5;
        let kinds: Vec<String> = e.4.iter().map(|(k, n)| format!("{k}:{n}")).collect();
        println!("| {g} | {} | {} | {} | {} | {:+} B | {} |", e.0, e.1, e.3, e.5, e.5 as i64 - e.3 as i64, kinds.join(" "));
    }
    println!("| ALL | {} | {} | {} | {} | {:+} B ({:+.3}%) | |", t.0, t.1, t.3, t.4, t.4 as i64 - t.3 as i64, 100.0 * (t.4 as f64 - t.3 as f64) / t.3.max(1) as f64);
}

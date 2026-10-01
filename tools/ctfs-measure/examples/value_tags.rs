// Counts values.dat event tags per corpus (Rust value_stream.rs layouts).
// Usage: value_tags CORPUS_ROOT LIST_FILE
use ctfs_measure::ctfs::{frames, parse_idx, Container};
use ctfs_measure::varint::get_u;
use ctfs_measure::zst;
use std::collections::BTreeMap;

fn skip(b: &[u8], p: &mut usize) {
    let n = get_u(b, p).unwrap() as usize;
    *p += n;
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let mut by: BTreeMap<String, BTreeMap<u8, u64>> = BTreeMap::new();
    for line in std::fs::read_to_string(&a[2]).unwrap().lines() {
        let p = std::path::Path::new(line);
        let Ok(c) = Container::open(p) else { continue };
        let (Ok(dat), Ok(idx)) = (c.read("values.dat"), c.read("values.idx")) else { continue };
        let Ok((_, offs)) = parse_idx(&idx) else { continue };
        let g = p.strip_prefix(&root).unwrap().iter().next().unwrap().to_string_lossy().to_string();
        let e = by.entry(g).or_default();
        for f in frames(&dat, &offs) {
            let raw = zst::decompress(f);
            let mut q = 0;
            while q < raw.len() {
                let n = get_u(&raw, &mut q).unwrap() as usize;
                let rec = &raw[q..q + n];
                q += n;
                let mut p = 0;
                while p < rec.len() {
                    let t = rec[p];
                    p += 1;
                    *e.entry(t).or_default() += 1;
                    match t {
                        0 => {
                            let k = get_u(rec, &mut p).unwrap();
                            for _ in 0..k {
                                get_u(rec, &mut p);
                                skip(rec, &mut p);
                            }
                        }
                        1 | 8 => {
                            get_u(rec, &mut p);
                            get_u(rec, &mut p);
                        }
                        2 => {
                            get_u(rec, &mut p);
                        }
                        3 => {
                            let k = get_u(rec, &mut p).unwrap();
                            for _ in 0..k {
                                get_u(rec, &mut p);
                            }
                        }
                        4 | 5 | 6 => {
                            get_u(rec, &mut p);
                            skip(rec, &mut p);
                        }
                        7 => {
                            get_u(rec, &mut p);
                            get_u(rec, &mut p);
                            get_u(rec, &mut p);
                        }
                        9 => {
                            get_u(rec, &mut p);
                            p += 1;
                            skip(rec, &mut p);
                        }
                        _ => skip(rec, &mut p),
                    }
                }
            }
        }
    }
    for (g, t) in by {
        println!("{g}: {}", t.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(" "));
    }
}

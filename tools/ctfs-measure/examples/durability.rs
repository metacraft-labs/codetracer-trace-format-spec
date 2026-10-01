// What it costs a writer to keep a container readable on disk as it records,
// replayed from real containers.
//
// For each recording, the members and the chunk boundaries of every chunked
// stream (a member with a companion `.idx`) are taken from the container
// itself. Recording is replayed as a sequence of *seals*: every chunk of every
// chunked stream, in order of how far through its own stream it lies, with the
// other members (interning tables, meta.dat) growing in proportion. The same
// final container (version 5 layout) is then written to a file on the
// working filesystem three ways:
//
//   buffered   all bytes in memory, written once at close (`write_all`)
//   per-seal   at every seal, the bytes appended since the last one are
//              written to their blocks, any mapping slot or block that changed
//              is written, and the changed root entries are written: the
//              container on disk is readable up to the last sealed chunk
//   per-seal+fsync  per-seal, plus an fdatasync at every seal (power-loss
//              durability, for comparison only)
//
// No variant fsyncs at close except per-seal+fsync. Times are best of three.
// Usage: durability CORPUS_ROOT LIST_FILE OUT_DIR
use ctfs_measure::ctfs::{Container, parse_idx};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::time::Instant;

const BS: usize = 4096;
const USABLE: usize = BS / 8 - 1;

struct Member {
    data: Vec<u8>,
    /// Sizes at which this member is published, in seal order: (progress in
    /// 0..=1, size). Chunked streams: one per chunk; others: proportional.
    marks: Vec<(f64, usize)>,
}

/// Block layout of the final container, allocated in the order the per-seal
/// writer allocates: blocks are claimed as bytes arrive.
struct Layout {
    entries: Vec<(u64, u64)>, // (size, map_block) per member
    data_blocks: Vec<Vec<u64>>,
    map_block: Vec<u64>,
    next: u64,
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&a[1]);
    let out_dir = std::path::PathBuf::from(&a[3]);
    std::fs::create_dir_all(&out_dir).unwrap();
    let tmp = out_dir.join("durability.tmp.ct");
    let mut by: BTreeMap<String, [f64; 7]> = BTreeMap::new();
    let mut tot = [0f64; 7];
    for line in std::fs::read_to_string(&a[2]).unwrap().lines() {
        let p = std::path::Path::new(line);
        let Ok(c) = Container::open(p) else { continue };
        let mut members: Vec<Member> = Vec::new();
        let names: Vec<String> = c.entries.iter().map(|e| e.name.clone()).collect();
        for e in c.entries.clone() {
            let Ok(data) = c.read_entry(&e) else { continue };
            let idxname = if e.name.ends_with(".dat") {
                Some(e.name.replace(".dat", ".idx"))
            } else {
                None
            };
            let mut marks = Vec::new();
            if let Some(i) = idxname.filter(|i| names.contains(i)) {
                if let Ok((_, offs)) = c.read(&i).and_then(|b| parse_idx(&b)) {
                    let n = offs.len().max(1);
                    for (k, _) in offs.iter().enumerate() {
                        let end = offs.get(k + 1).map(|x| *x as usize).unwrap_or(data.len());
                        marks.push(((k + 1) as f64 / n as f64, end));
                    }
                }
            }
            members.push(Member { data, marks });
        }
        // .idx members follow their .dat: one 8-byte offset per chunk.
        let idx_marks: Vec<Option<Vec<(f64, usize)>>> = c
            .entries
            .iter()
            .map(|e| {
                if !e.name.ends_with(".idx") {
                    return None;
                }
                let dat = e.name.replace(".idx", ".dat");
                let di = c.entries.iter().position(|x| x.name == dat)?;
                let m = &members.get(di)?.marks;
                Some(
                    m.iter()
                        .enumerate()
                        .map(|(k, (f, _))| (*f, 4 + 8 * (k + 1)))
                        .collect(),
                )
            })
            .collect();
        for (i, im) in idx_marks.into_iter().enumerate() {
            if let Some(im) = im {
                if let Some(m) = members.get_mut(i) {
                    m.marks = im;
                    let len = m.data.len();
                    if let Some(last) = m.marks.last_mut() {
                        last.1 = len;
                    }
                }
            }
        }
        // Seal points: union of all chunk-progress marks.
        let mut points: Vec<f64> = members
            .iter()
            .flat_map(|m| m.marks.iter().map(|x| x.0))
            .collect();
        points.push(1.0);
        points.sort_by(|a, b| a.partial_cmp(b).unwrap());
        points.dedup();
        let seals = points.len();
        let size_at = |m: &Member, f: f64| -> usize {
            if m.marks.is_empty() {
                // Proportional growth for unchunked members.
                ((m.data.len() as f64) * f).round() as usize
            } else {
                m.marks
                    .iter()
                    .filter(|x| x.0 <= f + 1e-12)
                    .map(|x| x.1)
                    .last()
                    .unwrap_or(0)
            }
        };
        // Build the final layout by replaying allocation.
        let nm = members.len();
        let mut lay = Layout {
            entries: vec![(0, 0); nm],
            data_blocks: vec![vec![]; nm],
            map_block: vec![0; nm],
            next: 1,
        };
        // Per-seal write plan: (offset, bytes) — computed during the replay.
        let mut file = vec![0u8; BS];
        file[0..5].copy_from_slice(&ctfs_measure::ctfs::MAGIC);
        file[5] = 5;
        file[8..12].copy_from_slice(&(BS as u32).to_le_bytes());
        let mut plan: Vec<Vec<(u64, Vec<u8>)>> = Vec::with_capacity(seals);
        let mut prev = vec![0usize; nm];
        for &f in &points {
            let mut writes: Vec<(u64, Vec<u8>)> = Vec::new();
            for (i, m) in members.iter().enumerate() {
                let s = size_at(m, f).min(m.data.len());
                if s <= prev[i] {
                    continue;
                }
                // Allocate blocks for [prev, s).
                let need = s.div_ceil(BS);
                while lay.data_blocks[i].len() < need {
                    let k = lay.data_blocks[i].len();
                    if k == 1 && lay.map_block[i] == 0 {
                        lay.map_block[i] = lay.next;
                        lay.next += 1;
                    }
                    if k >= USABLE {
                        // Deep mappings do not occur in this corpus's members;
                        // count the extra block without modelling the tree.
                        lay.next += (k % USABLE == 0) as u64;
                    }
                    lay.data_blocks[i].push(lay.next);
                    lay.next += 1;
                }
                let need_len = (lay.next as usize) * BS;
                if file.len() < need_len {
                    file.resize(need_len, 0);
                }
                // Data bytes.
                let mut o = prev[i];
                while o < s {
                    let bi = o / BS;
                    let off = o % BS;
                    let take = (BS - off).min(s - o);
                    let at = lay.data_blocks[i][bi] as usize * BS + off;
                    file[at..at + take].copy_from_slice(&m.data[o..o + take]);
                    writes.push((at as u64, m.data[o..o + take].to_vec()));
                    o += take;
                }
                // Mapping slots.
                if lay.map_block[i] != 0 {
                    let mb = lay.map_block[i] as usize * BS;
                    let first_new = if prev[i] == 0 { 0 } else { (prev[i] - 1) / BS };
                    for (k, b) in lay.data_blocks[i].iter().enumerate().skip(first_new) {
                        if k >= USABLE {
                            break;
                        }
                        file[mb + 8 * k..mb + 8 * k + 8].copy_from_slice(&b.to_le_bytes());
                    }
                    writes.push((
                        mb as u64,
                        file[mb..mb + 8 * lay.data_blocks[i].len().min(USABLE)].to_vec(),
                    ));
                }
                lay.entries[i].0 = s as u64;
                lay.entries[i].1 = if lay.map_block[i] != 0 {
                    lay.map_block[i]
                } else {
                    (1u64 << 63) | lay.data_blocks[i][0]
                };
                let eo = 16 + 24 * i;
                file[eo..eo + 8].copy_from_slice(&lay.entries[i].0.to_le_bytes());
                file[eo + 8..eo + 16].copy_from_slice(&lay.entries[i].1.to_le_bytes());
                writes.push((eo as u64, file[eo..eo + 16].to_vec()));
                prev[i] = s;
            }
            plan.push(writes);
        }
        let bytes = file.len();
        let best = |mut f: Box<dyn FnMut() -> u64>| -> (f64, u64) {
            let mut b = f64::INFINITY;
            let mut n = 0;
            for _ in 0..3 {
                let t = Instant::now();
                n = f();
                b = b.min(t.elapsed().as_nanos() as f64);
            }
            (b, n)
        };
        let t_buf = best(Box::new(|| {
            let mut fh = std::fs::File::create(&tmp).unwrap();
            fh.write_all(&file).unwrap();
            1
        }));
        let t_seal = best(Box::new(|| {
            let fh = std::fs::File::create(&tmp).unwrap();
            let mut n = 0;
            for w in &plan {
                for (o, b) in w {
                    fh.write_at(b, *o).unwrap();
                    n += 1;
                }
            }
            n
        }));
        let t_sync = best(Box::new(|| {
            let fh = std::fs::File::create(&tmp).unwrap();
            let mut n = 0;
            for w in &plan {
                for (o, b) in w {
                    fh.write_at(b, *o).unwrap();
                    n += 1;
                }
                fh.sync_data().unwrap();
            }
            n
        }));
        let g = p
            .strip_prefix(&root)
            .unwrap()
            .iter()
            .next()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let e = by.entry(g).or_default();
        let v = [
            1.0,
            bytes as f64,
            seals as f64,
            t_seal.1 as f64,
            t_buf.0,
            t_seal.0,
            t_sync.0,
        ];
        for k in 0..7 {
            e[k] += v[k];
            tot[k] += v[k];
        }
    }
    let _ = std::fs::remove_file(&tmp);
    println!(
        "| corpus | containers | MB | seals | writes | buffered ms | per-seal ms | per-seal cost | per-seal+fsync ms |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    let row = |g: &str, v: &[f64; 7]| {
        println!(
            "| {g} | {} | {:.1} | {} | {} | {:.1} | {:.1} | {:+.1} ms ({:.2}x) | {:.0} |",
            v[0],
            v[1] / 1e6,
            v[2],
            v[3],
            v[4] / 1e6,
            v[5] / 1e6,
            (v[5] - v[4]) / 1e6,
            v[5] / v[4],
            v[6] / 1e6
        )
    };
    for (g, v) in &by {
        row(g, v);
    }
    row("ALL", &tot);
}

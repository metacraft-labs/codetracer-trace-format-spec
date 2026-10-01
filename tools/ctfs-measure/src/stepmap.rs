//! `step-map.ns`: the v1 layout (`internal-files.md` §"`step-map.ns`", spec
//! commit 580b0be) and candidate storage encodings, each with the lookups a
//! reader performs, so size and latency are measured on the same code.
//!
//! Candidates:
//! - `v1`            the v1 blob, uncompressed (8 bytes per step id).
//! - `v1-zstd`       the v1 blob as one zstd frame; a lookup inflates it all.
//! - `v1-chunked`    v1 directory uncompressed; the i64 step-id lists zstd'd
//!                   in chunks (no delta coding) -- "like the other streams".
//! - `gap`           v1 directory uncompressed; lists delta-coded as varint
//!                   gaps, uncompressed.
//! - `gap-zstd`      `gap` with the list area as one zstd frame.
//! - `gap-chunked`   `gap` with the list area zstd'd in chunks of about
//!                   `CHUNK_TARGET` raw bytes, each list wholly inside one
//!                   chunk unless it alone exceeds the target.
//! - `packed*`       keys delta-coded inline with the lists, chunks of about
//!                   `PACKED_TARGET` raw bytes behind a small uncompressed
//!                   chunk table (see "`packed`" below), in four variants:
//!                   gap varints or run-length-coded gaps, each with or
//!                   without zstd. `packed-rle` (`PackedMode::RleZstd`) is
//!                   `step-map.ns` version 2 as specified, byte for byte.

use crate::varint::get_u_fast;
#[cfg(feature = "native")]
use crate::varint::put_u;

pub const MAGIC: u32 = 0x5354_4D50;

#[derive(Clone, Debug, Default)]
pub struct StepMap {
    /// (path_id, lines), ascending path_id; lines ascending, each with its
    /// ascending step ids.
    pub paths: Vec<(u64, Vec<(u32, Vec<i64>)>)>,
}

impl StepMap {
    pub fn steps(&self) -> usize {
        self.paths
            .iter()
            .flat_map(|p| p.1.iter())
            .map(|l| l.1.len())
            .sum()
    }
    pub fn lines(&self) -> usize {
        self.paths.iter().map(|p| p.1.len()).sum()
    }

    /// Builds from `(path_id, line, step_id)` in step order.
    pub fn build(hits: impl IntoIterator<Item = (u64, u32, i64)>) -> StepMap {
        let mut m: std::collections::BTreeMap<u64, std::collections::BTreeMap<u32, Vec<i64>>> =
            Default::default();
        for (p, l, s) in hits {
            m.entry(p).or_default().entry(l).or_default().push(s);
        }
        StepMap {
            paths: m
                .into_iter()
                .map(|(p, ls)| (p, ls.into_iter().collect()))
                .collect(),
        }
    }

    pub fn encode_v1(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&(self.paths.len() as u32).to_le_bytes());
        out.extend_from_slice(&18u64.to_le_bytes());
        let lines_start = 18 + 20 * self.paths.len() as u64;
        let mut lo = lines_start;
        for (pid, lines) in &self.paths {
            out.extend_from_slice(&pid.to_le_bytes());
            out.extend_from_slice(&(lines.len() as u32).to_le_bytes());
            out.extend_from_slice(&lo.to_le_bytes());
            lo += 32 * lines.len() as u64;
        }
        let mut so = lo;
        for (_, lines) in &self.paths {
            for (line, ids) in lines {
                out.extend_from_slice(&line.to_le_bytes());
                out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
                out.extend_from_slice(&ids[0].to_le_bytes());
                out.extend_from_slice(&ids[ids.len() - 1].to_le_bytes());
                out.extend_from_slice(&so.to_le_bytes());
                so += 8 * ids.len() as u64;
            }
        }
        for (_, lines) in &self.paths {
            for (_, ids) in lines {
                for id in ids {
                    out.extend_from_slice(&id.to_le_bytes());
                }
            }
        }
        out
    }

    pub fn decode_v1(b: &[u8]) -> Option<StepMap> {
        let u32_at = |o: usize| -> Option<u32> {
            Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
        };
        let u64_at = |o: usize| -> Option<u64> {
            Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
        };
        if u32_at(0)? != MAGIC {
            return None;
        }
        let pc = u32_at(6)? as usize;
        let pto = u64_at(10)? as usize;
        let mut paths = Vec::with_capacity(pc);
        for i in 0..pc {
            let o = pto + i * 20;
            let pid = u64_at(o)?;
            let lc = u32_at(o + 8)? as usize;
            let lo = u64_at(o + 12)? as usize;
            let mut lines = Vec::with_capacity(lc);
            for j in 0..lc {
                let e = lo + j * 32;
                let line = u32_at(e)?;
                let n = u32_at(e + 4)? as usize;
                let so = u64_at(e + 24)? as usize;
                let ids = (0..n)
                    .map(|k| u64_at(so + 8 * k).map(|v| v as i64))
                    .collect::<Option<Vec<_>>>()?;
                lines.push((line, ids));
            }
            paths.push((pid, lines));
        }
        Some(StepMap { paths })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Candidate {
    V1,
    V1Zstd,
    V1Chunked,
    Gap,
    GapZstd,
    GapChunked,
}

impl Candidate {
    pub const ALL: [Candidate; 6] = [
        Candidate::V1,
        Candidate::V1Zstd,
        Candidate::V1Chunked,
        Candidate::Gap,
        Candidate::GapZstd,
        Candidate::GapChunked,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Candidate::V1 => "v1",
            Candidate::V1Zstd => "v1-zstd",
            Candidate::V1Chunked => "v1-chunked",
            Candidate::Gap => "gap",
            Candidate::GapZstd => "gap-zstd",
            Candidate::GapChunked => "gap-chunked",
        }
    }
}

pub const CHUNK_TARGET: usize = 16 * 1024;

/// One line's directory entry as a reader holds it after open.
#[derive(Clone, Copy, Debug)]
pub struct LineEntry {
    pub path: u64,
    pub line: u32,
    pub count: u32,
    pub first: i64,
    pub last: i64,
    /// Offset of the list in the (decompressed) list area.
    pub off: u64,
    /// Chunk holding the list's first byte, for chunked candidates.
    pub chunk: u32,
}

/// An encoded step map: the bytes as stored, plus what a reader needs to
/// answer lookups once opened.
pub struct Encoded {
    pub cand: Candidate,
    /// Stored bytes (what goes into the container).
    pub stored: Vec<u8>,
    /// Size of the directory (header, path table, line entries, chunk index)
    /// within `stored`.
    pub directory_bytes: usize,
}

#[cfg(feature = "native")]
fn gap_list(ids: &[i64], out: &mut Vec<u8>) {
    let mut prev = -1i64;
    for &id in ids {
        put_u(out, (id - prev) as u64);
        prev = id;
    }
}

#[cfg(feature = "native")]
/// Lays out lists for a candidate; returns (list area raw, per-line offsets).
fn list_area(m: &StepMap, gap: bool) -> (Vec<u8>, Vec<u64>) {
    let mut area = Vec::new();
    let mut offs = Vec::new();
    for (_, lines) in &m.paths {
        for (_, ids) in lines {
            offs.push(area.len() as u64);
            if gap {
                gap_list(ids, &mut area);
            } else {
                for id in ids {
                    area.extend_from_slice(&id.to_le_bytes());
                }
            }
        }
    }
    (area, offs)
}

#[cfg(feature = "native")]
/// Chunk boundaries for a list area: lists are not split unless one exceeds
/// the target alone. Returns chunk start offsets (raw).
fn chunk_starts(offs: &[u64], area_len: usize) -> Vec<u64> {
    let mut starts = vec![0u64];
    for &o in offs.iter().skip(1) {
        if (o - *starts.last().unwrap()) as usize >= CHUNK_TARGET {
            starts.push(o);
        }
    }
    let _ = area_len;
    starts
}

#[cfg(feature = "native")]
/// Directory as stored by every candidate other than `v1`/`v1-zstd`:
/// header, path table, 32-byte line entries (offset into the list area),
/// then for chunked candidates `[u32 n][u64 raw_start, u64 stored_start]*n`.
fn directory(m: &StepMap, offs: &[u64], chunks: &[(u64, u64)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&(m.paths.len() as u32).to_le_bytes());
    out.extend_from_slice(&18u64.to_le_bytes());
    let mut li = 0u64;
    for (pid, lines) in &m.paths {
        out.extend_from_slice(&pid.to_le_bytes());
        out.extend_from_slice(&(lines.len() as u32).to_le_bytes());
        out.extend_from_slice(&li.to_le_bytes());
        li += lines.len() as u64;
    }
    let mut k = 0;
    for (_, lines) in &m.paths {
        for (line, ids) in lines {
            out.extend_from_slice(&line.to_le_bytes());
            out.extend_from_slice(&(ids.len() as u32).to_le_bytes());
            out.extend_from_slice(&ids[0].to_le_bytes());
            out.extend_from_slice(&ids[ids.len() - 1].to_le_bytes());
            out.extend_from_slice(&offs[k].to_le_bytes());
            k += 1;
        }
    }
    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    for (r, s) in chunks {
        out.extend_from_slice(&r.to_le_bytes());
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(feature = "native")]
pub fn encode(m: &StepMap, cand: Candidate) -> Encoded {
    use crate::zst::compress;
    match cand {
        Candidate::V1 => {
            let b = m.encode_v1();
            let dir = 18 + 20 * m.paths.len() + 32 * m.lines();
            Encoded {
                cand,
                stored: b,
                directory_bytes: dir,
            }
        }
        Candidate::V1Zstd => {
            let b = compress(&m.encode_v1(), 3);
            let n = b.len();
            Encoded {
                cand,
                stored: b,
                directory_bytes: n,
            }
        }
        Candidate::Gap | Candidate::GapZstd => {
            let (area, offs) = list_area(m, true);
            let mut dir = directory(m, &offs, &[]);
            let dlen = dir.len();
            if cand == Candidate::Gap {
                dir.extend_from_slice(&area);
            } else {
                dir.extend_from_slice(&compress(&area, 3));
            }
            Encoded {
                cand,
                stored: dir,
                directory_bytes: dlen,
            }
        }
        Candidate::V1Chunked | Candidate::GapChunked => {
            let gap = cand == Candidate::GapChunked;
            let (area, offs) = list_area(m, gap);
            let starts = chunk_starts(&offs, area.len());
            let mut frames = Vec::new();
            let mut chunks = Vec::new();
            let mut stored_off = 0u64;
            for (i, &s) in starts.iter().enumerate() {
                let e = starts.get(i + 1).copied().unwrap_or(area.len() as u64);
                let f = compress(&area[s as usize..e as usize], 3);
                chunks.push((s, stored_off));
                stored_off += f.len() as u64;
                frames.push(f);
            }
            let mut dir = directory(m, &offs, &chunks);
            let dlen = dir.len();
            for f in frames {
                dir.extend_from_slice(&f);
            }
            Encoded {
                cand,
                stored: dir,
                directory_bytes: dlen,
            }
        }
    }
}

/// A reader over an encoded step map: `open` parses the directory (and, for
/// whole-blob candidates, inflates what it must); `hits` returns one line's
/// step ids; `next_hit` the first hit after a step.
pub struct Reader<'a> {
    cand: Candidate,
    stored: &'a [u8],
    pub entries: Vec<LineEntry>,
    /// Inflated v1 blob or list area, for whole-frame candidates.
    whole: Vec<u8>,
    /// (raw_start, stored_start) per chunk, plus where the frames begin.
    chunks: Vec<(u64, u64)>,
    frames_base: usize,
    /// One-chunk cache: (chunk index, inflated bytes).
    cache: Option<(u32, Vec<u8>)>,
    dec: ruzstd::decoding::FrameDecoder,
    pub chunk_inflations: u64,
}

fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn rd_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Which zstd implementation a reader inflates with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inflater {
    Ruzstd,
    #[cfg(feature = "native")]
    CZstd,
}

fn inflate(dec: &mut ruzstd::decoding::FrameDecoder, which: Inflater, f: &[u8], out: &mut Vec<u8>) {
    match which {
        Inflater::Ruzstd => crate::zst::ruzstd_decompress(dec, f, out),
        #[cfg(feature = "native")]
        Inflater::CZstd => *out = crate::zst::decompress(f),
    }
}

impl<'a> Reader<'a> {
    pub fn open(cand: Candidate, stored: &'a [u8], which: Inflater) -> Reader<'a> {
        let mut dec = ruzstd::decoding::FrameDecoder::new();
        let mut whole = Vec::new();
        let mut entries = Vec::new();
        let mut chunks = Vec::new();
        let mut frames_base = 0;
        let dir: &[u8] = match cand {
            Candidate::V1Zstd => {
                inflate(&mut dec, which, stored, &mut whole);
                &whole
            }
            _ => stored,
        };
        let pc = rd_u32(dir, 6) as usize;
        let v1 = matches!(cand, Candidate::V1 | Candidate::V1Zstd);
        let mut total_lines = 0usize;
        let mut path_lines = Vec::with_capacity(pc);
        for i in 0..pc {
            let o = 18 + i * 20;
            let lc = rd_u32(dir, o + 8) as usize;
            path_lines.push((rd_u64(dir, o), lc));
            total_lines += lc;
        }
        let lines_base = 18 + 20 * pc;
        let mut k = 0;
        for (pid, lc) in &path_lines {
            for _ in 0..*lc {
                let e = lines_base + 32 * k;
                entries.push(LineEntry {
                    path: *pid,
                    line: rd_u32(dir, e),
                    count: rd_u32(dir, e + 4),
                    first: rd_u64(dir, e + 8) as i64,
                    last: rd_u64(dir, e + 16) as i64,
                    off: rd_u64(dir, e + 24),
                    chunk: 0,
                });
                k += 1;
            }
        }
        let after_lines = lines_base + 32 * total_lines;
        if !v1 {
            let n = rd_u32(dir, after_lines) as usize;
            for i in 0..n {
                let o = after_lines + 4 + 16 * i;
                chunks.push((rd_u64(dir, o), rd_u64(dir, o + 8)));
            }
            frames_base = after_lines + 4 + 16 * n;
            if !chunks.is_empty() {
                let mut c = 0usize;
                for e in entries.iter_mut() {
                    while c + 1 < chunks.len() && chunks[c + 1].0 <= e.off {
                        c += 1;
                    }
                    e.chunk = c as u32;
                }
            }
            if cand == Candidate::GapZstd {
                inflate(&mut dec, which, &stored[frames_base..], &mut whole);
            }
        }
        let _ = which;
        Reader {
            cand,
            stored,
            entries,
            whole,
            chunks,
            frames_base,
            cache: None,
            dec,
            chunk_inflations: 0,
        }
    }

    /// Index of `(path, line)`'s entry, by binary search over the directory.
    pub fn find(&self, path: u64, line: u32) -> Option<usize> {
        self.entries
            .binary_search_by(|e| (e.path, e.line).cmp(&(path, line)))
            .ok()
    }

    fn chunk_bytes(&mut self, c: u32, which: Inflater) -> &[u8] {
        if self.cache.as_ref().map(|x| x.0) != Some(c) {
            let (_, s) = self.chunks[c as usize];
            let end = self
                .chunks
                .get(c as usize + 1)
                .map(|x| x.1)
                .unwrap_or((self.stored.len() - self.frames_base) as u64);
            let f = &self.stored[self.frames_base + s as usize..self.frames_base + end as usize];
            let mut buf = self.cache.take().map(|x| x.1).unwrap_or_default();
            inflate(&mut self.dec, which, f, &mut buf);
            self.chunk_inflations += 1;
            self.cache = Some((c, buf));
        }
        &self.cache.as_ref().unwrap().1
    }

    /// All step ids of entry `i`, appended to `out`.
    pub fn hits(&mut self, i: usize, which: Inflater, out: &mut Vec<i64>) {
        let e = self.entries[i];
        let n = e.count as usize;
        match self.cand {
            Candidate::V1 | Candidate::V1Zstd => {
                let src: &[u8] = if self.cand == Candidate::V1 {
                    self.stored
                } else {
                    &self.whole
                };
                let o = e.off as usize;
                out.extend((0..n).map(|k| rd_u64(src, o + 8 * k) as i64));
            }
            Candidate::Gap | Candidate::GapZstd => {
                let base = if self.cand == Candidate::Gap {
                    self.frames_base
                } else {
                    0
                };
                let src: &[u8] = if self.cand == Candidate::Gap {
                    self.stored
                } else {
                    &self.whole
                };
                let mut p = base + e.off as usize;
                let mut prev = -1i64;
                for _ in 0..n {
                    prev += get_u_fast(src, &mut p) as i64;
                    out.push(prev);
                }
            }
            Candidate::V1Chunked | Candidate::GapChunked => {
                let gap = self.cand == Candidate::GapChunked;
                let raw_start = self.chunks[e.chunk as usize].0;
                let rel = (e.off - raw_start) as usize;
                let src = self.chunk_bytes(e.chunk, which);
                if gap {
                    let mut p = rel;
                    let mut prev = -1i64;
                    for _ in 0..n {
                        prev += get_u_fast(src, &mut p) as i64;
                        out.push(prev);
                    }
                } else {
                    out.extend((0..n).map(|k| rd_u64(src, rel + 8 * k) as i64));
                }
            }
        }
    }

    /// First hit of entry `i` strictly after `step`, if any. Uses the
    /// directory's first/last before touching the list, as a reader would.
    pub fn next_hit(
        &mut self,
        i: usize,
        step: i64,
        which: Inflater,
        scratch: &mut Vec<i64>,
    ) -> Option<i64> {
        let e = self.entries[i];
        if e.last <= step {
            return None;
        }
        if e.first > step {
            return Some(e.first);
        }
        match self.cand {
            Candidate::V1 | Candidate::V1Zstd => {
                let src: &[u8] = if self.cand == Candidate::V1 {
                    self.stored
                } else {
                    &self.whole
                };
                let o = e.off as usize;
                let (mut lo, mut hi) = (0usize, e.count as usize);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if (rd_u64(src, o + 8 * mid) as i64) <= step {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                Some(rd_u64(src, o + 8 * lo) as i64)
            }
            _ => {
                scratch.clear();
                self.hits(i, which, scratch);
                let k = scratch.partition_point(|&x| x <= step);
                scratch.get(k).copied()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `packed`: everything delta/varint-coded and zstd'd in chunks of whole line
// records, with a small uncompressed chunk table keyed by each chunk's first
// (path, line) so that one line can still be found without inflating the rest.
//
//   header (26 B): magic u32 | version u16 = 2 | chunk_count u32 |
//                  path_count u32 | line_count u32 | step_count u64
//   chunk table:   chunk_count x { frame_offset u64 | first_path u64 | first_line u32 }
//   frames:        one zstd frame per chunk (offsets relative to the first frame)
//   chunk content: line records, ascending (path, line):
//                  path_delta varint | line varint (absolute when path_delta > 0
//                  or first in chunk, else delta from the previous line) |
//                  count varint | count x gap varint (id - previous id; the
//                  previous id of a list's first element is -1)
// ---------------------------------------------------------------------------

pub const PACKED_TARGET: usize = 64 * 1024;

#[cfg(feature = "native")]
pub fn encode_packed(m: &StepMap, target: usize) -> Vec<u8> {
    encode_packed_mode(m, target, PackedMode::GapZstd)
}

/// Variants of `packed`, told apart (for measurement only) by the header's
/// version field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackedMode {
    /// Gap varints, chunks zstd'd.
    GapZstd = 5,
    /// Gap varints, chunks stored as they are.
    GapRaw = 3,
    /// Run-length-coded gaps -- (gap, repeat) varint pairs -- stored as they are.
    RleRaw = 4,
    /// Run-length-coded gaps, chunks zstd'd: `step-map.ns` version 2 as
    /// specified, byte for byte.
    RleZstd = 2,
}

impl PackedMode {
    pub const ALL: [PackedMode; 4] = [PackedMode::GapZstd, PackedMode::GapRaw, PackedMode::RleRaw, PackedMode::RleZstd];
    pub fn name(self) -> &'static str {
        match self {
            PackedMode::GapZstd => "packed",
            PackedMode::GapRaw => "packed-nozstd",
            PackedMode::RleRaw => "packed-rle-nozstd",
            PackedMode::RleZstd => "packed-rle",
        }
    }
    fn from_version(v: u16) -> PackedMode {
        match v {
            5 => PackedMode::GapZstd,
            3 => PackedMode::GapRaw,
            4 => PackedMode::RleRaw,
            2 => PackedMode::RleZstd,
            _ => panic!("packed version {v}"),
        }
    }
    fn zstd(self) -> bool {
        matches!(self, PackedMode::GapZstd | PackedMode::RleZstd)
    }
    fn rle(self) -> bool {
        matches!(self, PackedMode::RleRaw | PackedMode::RleZstd)
    }
}

#[cfg(feature = "native")]
fn rle_list(ids: &[i64], out: &mut Vec<u8>) {
    let mut prev = -1i64;
    let mut run: Option<(u64, u64)> = None;
    for &id in ids {
        let g = (id - prev) as u64;
        prev = id;
        run = match run {
            Some((rg, n)) if rg == g => Some((rg, n + 1)),
            Some((rg, n)) => {
                put_u(out, rg);
                put_u(out, n);
                Some((g, 1))
            }
            None => Some((g, 1)),
        };
    }
    if let Some((rg, n)) = run {
        put_u(out, rg);
        put_u(out, n);
    }
}

#[cfg(feature = "native")]
pub fn encode_packed_mode(m: &StepMap, target: usize, mode: PackedMode) -> Vec<u8> {
    let mut chunks: Vec<(u64, u32, Vec<u8>)> = Vec::new();
    let mut cur = Vec::new();
    let mut cur_first: Option<(u64, u32)> = None;
    let (mut prev_path, mut prev_line) = (0u64, 0u32);
    let mut lines = 0u32;
    for (pid, ls) in &m.paths {
        for (line, ids) in ls {
            if cur_first.is_none() {
                cur_first = Some((*pid, *line));
                prev_path = *pid;
                prev_line = 0;
            }
            let dp = pid - prev_path;
            put_u(&mut cur, dp);
            put_u(&mut cur, if dp > 0 { *line as u64 } else { (*line - prev_line) as u64 });
            put_u(&mut cur, ids.len() as u64);
            if mode.rle() {
                rle_list(ids, &mut cur);
            } else {
                gap_list(ids, &mut cur);
            }
            prev_path = *pid;
            prev_line = *line;
            lines += 1;
            if cur.len() >= target {
                let (p, l) = cur_first.take().unwrap();
                chunks.push((p, l, std::mem::take(&mut cur)));
            }
        }
    }
    if let Some((p, l)) = cur_first {
        chunks.push((p, l, cur));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(mode as u16).to_le_bytes());
    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    out.extend_from_slice(&(m.paths.len() as u32).to_le_bytes());
    out.extend_from_slice(&lines.to_le_bytes());
    out.extend_from_slice(&(m.steps() as u64).to_le_bytes());
    let frames: Vec<Vec<u8>> = chunks
        .iter()
        .map(|c| if mode.zstd() { crate::zst::compress(&c.2, 3) } else { c.2.clone() })
        .collect();
    let mut off = 0u64;
    for (c, f) in chunks.iter().zip(&frames) {
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&c.0.to_le_bytes());
        out.extend_from_slice(&c.1.to_le_bytes());
        off += f.len() as u64;
    }
    for f in frames {
        out.extend_from_slice(&f);
    }
    out
}

pub struct Packed<'a> {
    stored: &'a [u8],
    mode: PackedMode,
    /// (frame start, frame end, first_path, first_line)
    pub chunks: Vec<(usize, usize, u64, u32)>,
    dec: ruzstd::decoding::FrameDecoder,
    buf: Vec<u8>,
    ids: Vec<i64>,
}

impl<'a> Packed<'a> {
    pub fn open(stored: &'a [u8]) -> Packed<'a> {
        let mode = PackedMode::from_version(u16::from_le_bytes([stored[4], stored[5]]));
        let n = rd_u32(stored, 6) as usize;
        let base = 26 + 20 * n;
        let mut chunks = Vec::with_capacity(n);
        for i in 0..n {
            let o = 26 + 20 * i;
            let s = base + rd_u64(stored, o) as usize;
            let e = if i + 1 < n { base + rd_u64(stored, o + 20) as usize } else { stored.len() };
            chunks.push((s, e, rd_u64(stored, o + 8), rd_u32(stored, o + 16)));
        }
        Packed { stored, mode, chunks, dec: ruzstd::decoding::FrameDecoder::new(), buf: Vec::new(), ids: Vec::new() }
    }

    /// Calls `f(path, line, ids)` for every line record of chunk `c`, until
    /// `f` returns true.
    fn each(&mut self, c: usize, which: Inflater, mut f: impl FnMut(u64, u32, &[i64]) -> bool) {
        let (s, e, first_path, _) = self.chunks[c];
        let src: &[u8] = if self.mode.zstd() {
            inflate(&mut self.dec, which, &self.stored[s..e], &mut self.buf);
            &self.buf
        } else {
            &self.stored[s..e]
        };
        let rle = self.mode.rle();
        let ids = &mut self.ids;
        let (mut path, mut line) = (first_path, 0u32);
        let mut p = 0;
        while p < src.len() {
            let dp = get_u_fast(src, &mut p);
            let dl = get_u_fast(src, &mut p) as u32;
            if dp > 0 {
                path += dp;
                line = dl;
            } else {
                line += dl;
            }
            let n = get_u_fast(src, &mut p) as usize;
            ids.clear();
            ids.reserve(n);
            let mut prev = -1i64;
            if rle {
                while ids.len() < n {
                    let g = get_u_fast(src, &mut p) as i64;
                    let r = get_u_fast(src, &mut p);
                    for _ in 0..r {
                        prev += g;
                        ids.push(prev);
                    }
                }
            } else {
                for _ in 0..n {
                    prev += get_u_fast(src, &mut p) as i64;
                    ids.push(prev);
                }
            }
            if f(path, line, ids) {
                return;
            }
        }
    }

    /// One line's ids: inflates only the chunk that can hold it.
    pub fn lookup(&mut self, path: u64, line: u32, which: Inflater, out: &mut Vec<i64>) -> bool {
        let c = self.chunks.partition_point(|x| (x.2, x.3) <= (path, line));
        if c == 0 {
            return false;
        }
        let mut found = false;
        self.each(c - 1, which, |p, l, ids| {
            if (p, l) == (path, line) {
                out.extend_from_slice(ids);
                found = true;
                return true;
            }
            (p, l) > (path, line)
        });
        found
    }

    /// The db-backend's open: every list into a map.
    pub fn load_all(&mut self, which: Inflater) -> std::collections::HashMap<(u64, u32), Vec<i64>> {
        let mut m = std::collections::HashMap::new();
        for c in 0..self.chunks.len() {
            self.each(c, which, |p, l, ids| {
                m.insert((p, l), ids.to_vec());
                false
            });
        }
        m
    }
}

/// The db-backend's open for the other candidates: every list into a map.
pub fn load_all(cand: Candidate, stored: &[u8], which: Inflater) -> std::collections::HashMap<(u64, u32), Vec<i64>> {
    let mut r = Reader::open(cand, stored, which);
    let mut m = std::collections::HashMap::with_capacity(r.entries.len());
    for i in 0..r.entries.len() {
        let mut v = Vec::with_capacity(r.entries[i].count as usize);
        r.hits(i, which, &mut v);
        m.insert((r.entries[i].path, r.entries[i].line), v);
    }
    m
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;

    /// Version 2's record layout, by hand: one line, ids 0, 2, 4, 6, 7.
    /// The first gap is from -1; runs are maximal.
    #[test]
    fn version_2_line_record_is_the_specified_bytes() {
        let m = StepMap::build([0i64, 2, 4, 6, 7].map(|s| (0u64, 3u32, s)));
        let raw = encode_packed_mode(&m, PACKED_TARGET, PackedMode::RleRaw);
        // header 26 + one chunk-table entry 20, then the chunk's content.
        let content = &raw[26 + 20..];
        //            dp line count (gap,rep) (gap,rep) (gap,rep)
        assert_eq!(content, &[0, 3, 5, 1, 1, 2, 3, 1, 1]);
        let z = encode_packed_mode(&m, PACKED_TARGET, PackedMode::RleZstd);
        assert_eq!(&z[0..4], &MAGIC.to_le_bytes());
        assert_eq!(u16::from_le_bytes([z[4], z[5]]), 2, "the specified layout is version 2");
        let got = Packed::open(&z).load_all(Inflater::Ruzstd);
        assert_eq!(got[&(0, 3)], vec![0, 2, 4, 6, 7]);
    }

    /// A chunk closes after the record that takes it to the target, and the
    /// next record restates its line in full.
    #[test]
    fn chunks_close_after_the_record_that_reaches_the_target() {
        let hits = (0..40u32).flat_map(|l| (0..3i64).map(move |k| (0u64, l + 1, k * 100 + l as i64)));
        let m = StepMap::build(hits);
        let raw = encode_packed_mode(&m, 16, PackedMode::RleRaw);
        let p = Packed::open(&raw);
        assert!(p.chunks.len() > 1);
        for w in p.chunks.windows(2) {
            assert!((w[0].2, w[0].3) < (w[1].2, w[1].3));
        }
        let mut pk = Packed::open(&raw);
        assert_eq!(pk.load_all(Inflater::Ruzstd).len(), 40);
    }
}

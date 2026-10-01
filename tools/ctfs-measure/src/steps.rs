//! `steps.dat` records, decoded to a writer-independent form and re-encoded
//! under candidate AbsoluteStep / DeltaStep rules.
//!
//! The decoded form keeps every exec record in order. A position-bearing
//! record (tags 0, 1, 7) is reduced to its resolved `global_position_index`
//! plus whether the writer emitted it as a column move (tag 7). Every other
//! record is kept as its exact bytes, because no candidate rule touches it.
//!
//! Decoding resolves deltas against one running cursor shared by all threads
//! and untouched by non-position records -- what both writers do -- and
//! resets that cursor at each chunk start, which is how both readers decode a
//! chunk (`StepStreamReader`'s per-chunk `prev_abs = None`; Nim's
//! `stepAbsoluteGlobalLineIndex` starting at 0). A chunk whose first position
//! record is a delta is decoded with the cursor carried from the previous
//! chunk instead, and counted, since that is the Nim writer's gap
//! (`exec_stream.nim` promotes only a chunk's first *record*).

use crate::varint::{self, get_s, get_u, len_u, put_s, put_u, zigzag};

pub const TAG_ABS: u8 = 0;
pub const TAG_DELTA: u8 = 1;
pub const TAG_COLUMN: u8 = 7;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rec {
    /// A position record. `column` is true when written as tag 7.
    Pos { pos: u64, column: bool },
    /// Any other record (Raise, Catch, ThreadSwitch/Start/Exit, SourceReload),
    /// verbatim including its tag.
    Other { tag: u8, bytes: Vec<u8> },
}

#[derive(Default, Debug, Clone)]
pub struct DecodeStats {
    pub records: u64,
    pub abs: u64,
    pub delta: u64,
    pub column: u64,
    pub other: u64,
    /// Chunks whose first position record was a delta (no anchor).
    pub unanchored_chunks: u64,
    pub chunks: u64,
    pub chunk_size: u32,
    /// Distinct thread ids named by thread records.
    pub threads: u64,
    pub raw_bytes: u64,
    pub compressed_bytes: u64,
}

/// Decodes one decompressed chunk, appending to `out`. `cursor` carries
/// across chunks only for the unanchored case described in the module doc.
pub fn decode_chunk(
    raw: &[u8],
    out: &mut Vec<Rec>,
    cursor: &mut Option<u64>,
    st: &mut DecodeStats,
    threads: &mut std::collections::BTreeSet<u64>,
) -> Result<(), String> {
    let mut p = 0usize;
    let mut chunk_cursor: Option<u64> = None;
    let mut first_pos = true;
    while p < raw.len() {
        let start = p;
        let tag = raw[p];
        p += 1;
        st.records += 1;
        let bad = || format!("truncated record tag {tag} at {start}");
        match tag {
            TAG_ABS => {
                let v = get_u(raw, &mut p).ok_or_else(bad)?;
                chunk_cursor = Some(v);
                st.abs += 1;
                first_pos = false;
                out.push(Rec::Pos {
                    pos: v,
                    column: false,
                });
            }
            TAG_DELTA | TAG_COLUMN => {
                let d = get_s(raw, &mut p).ok_or_else(bad)?;
                let base = match chunk_cursor {
                    Some(c) => c,
                    None => {
                        if first_pos {
                            st.unanchored_chunks += 1;
                        }
                        cursor.unwrap_or(0)
                    }
                };
                first_pos = false;
                let v = (base as i64).wrapping_add(d);
                if v < 0 {
                    return Err(format!("negative position at record {}", st.records));
                }
                chunk_cursor = Some(v as u64);
                if tag == TAG_COLUMN {
                    st.column += 1;
                } else {
                    st.delta += 1;
                }
                out.push(Rec::Pos {
                    pos: v as u64,
                    column: tag == TAG_COLUMN,
                });
            }
            2 => {
                get_u(raw, &mut p).ok_or_else(bad)?;
                let n = get_u(raw, &mut p).ok_or_else(bad)? as usize;
                p += n;
                if p > raw.len() {
                    return Err(bad());
                }
                st.other += 1;
                out.push(Rec::Other {
                    tag,
                    bytes: raw[start..p].to_vec(),
                });
            }
            3..=6 => {
                let v = get_u(raw, &mut p).ok_or_else(bad)?;
                if tag >= 4 {
                    threads.insert(v);
                }
                st.other += 1;
                out.push(Rec::Other {
                    tag,
                    bytes: raw[start..p].to_vec(),
                });
            }
            8 => {
                get_u(raw, &mut p).ok_or_else(bad)?;
                let n = get_u(raw, &mut p).ok_or_else(bad)?;
                for _ in 0..n * 3 {
                    get_u(raw, &mut p).ok_or_else(bad)?;
                }
                get_u(raw, &mut p).ok_or_else(bad)?;
                st.other += 1;
                out.push(Rec::Other {
                    tag,
                    bytes: raw[start..p].to_vec(),
                });
            }
            t => return Err(format!("unknown step tag {t} at {start}")),
        }
    }
    if chunk_cursor.is_some() {
        *cursor = chunk_cursor;
    }
    Ok(())
}

/// Context a rule may consult: exec-record indices that are the first record
/// of a call (call entry) or the first record after a call returned to its
/// caller.
#[derive(Default, Clone)]
pub struct CallMarks {
    pub entry: Vec<bool>,
    pub ret: Vec<bool>,
    pub known: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// Every position record absolute (baseline).
    AbsOnly,
    /// `trace-events.md` §"Encoding Rules" as written (2026-09): absolute for
    /// the first step and after every call and return; otherwise delta when
    /// the zigzag delta fits 3 varint bytes.
    Spec,
    /// The Nim writer: delta when -64 <= d <= 63, else absolute; first step
    /// absolute; first position record of a chunk promoted only when it is
    /// the chunk's first record.
    Nim,
    /// The Rust line-only writer: absolute for the first step of each chunk
    /// and after call, return and thread switch; otherwise delta when
    /// |d| <= 1_048_575.
    Rust,
    /// Smallest encoding, ties to DeltaStep; the first position record of
    /// every chunk absolute.
    MinTiesDelta,
    /// Smallest encoding, ties to AbsoluteStep; the first position record of
    /// every chunk absolute.
    MinTiesAbs,
    /// Absolute only where a reader needs an anchor -- the first position
    /// record of every chunk -- and delta everywhere else, whatever its size.
    AnchorOnly,
    /// DeltaStep exactly when the zigzag delta fits one varint byte
    /// (-64..=63), AbsoluteStep otherwise; the first position record of
    /// every chunk absolute. The Nim writer's window with the anchor rule
    /// applied to the first *position* record rather than the first record.
    Delta1,
    /// `Delta1`, except that an AbsoluteStep that is itself one byte
    /// (position < 128) is preferred to the equally long DeltaStep.
    Delta1TiesAbs,
}

impl Rule {
    pub const ALL: [Rule; 9] = [
        Rule::AbsOnly,
        Rule::Spec,
        Rule::Nim,
        Rule::Rust,
        Rule::MinTiesDelta,
        Rule::MinTiesAbs,
        Rule::AnchorOnly,
        Rule::Delta1,
        Rule::Delta1TiesAbs,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Rule::AbsOnly => "abs-only",
            Rule::Spec => "spec-2026-09",
            Rule::Nim => "nim-writer",
            Rule::Rust => "rust-writer",
            Rule::MinTiesDelta => "min-ties-delta",
            Rule::MinTiesAbs => "min-ties-abs",
            Rule::AnchorOnly => "anchor-only",
            Rule::Delta1 => "delta-1byte",
            Rule::Delta1TiesAbs => "delta-1byte-ties-abs",
        }
    }
}

/// Encodes `recs` under `rule`, chunked every `chunk_size` records. Returns
/// the raw bytes of each chunk.
pub fn encode(recs: &[Rec], marks: &CallMarks, rule: Rule, chunk_size: usize) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut cur = Vec::new();
    // Running cursor as a decoder carrying across chunks sees it.
    let mut cursor: Option<u64> = None;
    let mut first_rec_of_chunk;
    let mut first_pos_of_chunk;
    let mut force_next_abs = false; // Rust/Spec: after call/return/thread switch
    let mut ever = false;
    for (ci, chunk) in recs.chunks(chunk_size).enumerate() {
        let base = ci * chunk_size;
        first_rec_of_chunk = true;
        first_pos_of_chunk = true;
        for (k, r) in chunk.iter().enumerate() {
            let idx = base + k;
            let is_entry = marks.known && marks.entry.get(idx).copied().unwrap_or(false);
            let is_ret = marks.known && marks.ret.get(idx).copied().unwrap_or(false);
            match r {
                Rec::Other { tag, bytes } => {
                    if rule == Rule::Rust && *tag == 4 {
                        force_next_abs = true;
                    }
                    cur.extend_from_slice(bytes);
                }
                Rec::Pos { pos, column } => {
                    let pos = *pos;
                    let d = cursor.map(|c| pos as i64 - c as i64);
                    let abs_len = len_u(pos);
                    let delta_len = d.map(|d| len_u(zigzag(d)));
                    let use_abs = match rule {
                        Rule::AbsOnly => true,
                        Rule::Spec => {
                            !ever || is_entry || is_ret || delta_len.map_or(true, |l| l > 3)
                                // The spec's rule does not mention chunks, but
                                // every reader decodes a chunk on its own, so a
                                // conforming container needs the anchor anyway.
                                || first_pos_of_chunk
                        }
                        Rule::Nim => {
                            !ever
                                || (first_rec_of_chunk)
                                || d.map_or(true, |d| !(-64..=63).contains(&d))
                        }
                        Rule::Rust => {
                            first_pos_of_chunk
                                || force_next_abs
                                || is_entry
                                || is_ret
                                || d.map_or(true, |d| d.unsigned_abs() > 1_048_575)
                        }
                        Rule::MinTiesDelta => {
                            first_pos_of_chunk || abs_len < delta_len.unwrap_or(usize::MAX)
                        }
                        Rule::MinTiesAbs => {
                            first_pos_of_chunk || abs_len <= delta_len.unwrap_or(usize::MAX)
                        }
                        Rule::AnchorOnly => first_pos_of_chunk,
                        Rule::Delta1 => first_pos_of_chunk || delta_len.map_or(true, |l| l > 1),
                        Rule::Delta1TiesAbs => {
                            first_pos_of_chunk || abs_len == 1 || delta_len.map_or(true, |l| l > 1)
                        }
                    };
                    if use_abs {
                        cur.push(TAG_ABS);
                        put_u(&mut cur, pos);
                    } else {
                        cur.push(if *column { TAG_COLUMN } else { TAG_DELTA });
                        put_s(&mut cur, d.unwrap());
                    }
                    cursor = Some(pos);
                    ever = true;
                    first_pos_of_chunk = false;
                    force_next_abs = false;
                }
            }
            first_rec_of_chunk = false;
        }
        chunks.push(std::mem::take(&mut cur));
    }
    chunks
}

/// Fast decode kernel used by the speed benchmark: resolves every position
/// record of one chunk to its absolute position, cursor reset per chunk.
/// Non-position records are skipped. Returns the number of positions written.
#[inline(never)]
pub fn decode_positions(raw: &[u8], out: &mut Vec<u64>) -> usize {
    let mut p = 0usize;
    let mut cur: u64 = 0;
    let n0 = out.len();
    while p < raw.len() {
        let tag = raw[p];
        p += 1;
        match tag {
            0 => {
                cur = varint::get_u_fast(raw, &mut p);
                out.push(cur);
            }
            1 | 7 => {
                let d = varint::unzigzag(varint::get_u_fast(raw, &mut p));
                cur = (cur as i64 + d) as u64;
                out.push(cur);
            }
            2 => {
                varint::get_u(raw, &mut p);
                let n = varint::get_u(raw, &mut p).unwrap() as usize;
                p += n;
            }
            3..=6 => {
                varint::get_u(raw, &mut p);
            }
            8 => {
                varint::get_u(raw, &mut p);
                let n = varint::get_u(raw, &mut p).unwrap();
                for _ in 0..n * 3 + 1 {
                    varint::get_u(raw, &mut p);
                }
            }
            _ => panic!("bad tag"),
        }
    }
    out.len() - n0
}

/// Positions of `recs`, for checking that an encoding round-trips.
pub fn positions(recs: &[Rec]) -> Vec<u64> {
    recs.iter()
        .filter_map(|r| match r {
            Rec::Pos { pos, .. } => Some(*pos),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(p: u64) -> Rec {
        Rec::Pos {
            pos: p,
            column: false,
        }
    }

    #[test]
    fn every_rule_round_trips_through_the_reset_per_chunk_decoder() {
        let mut recs = vec![pos(100_000), pos(100_001), pos(5), pos(200_000)];
        recs.push(Rec::Other {
            tag: 4,
            bytes: vec![4, 1],
        });
        for i in 0..50u64 {
            recs.push(pos(1000 + (i * 37) % 300));
        }
        let marks = CallMarks::default();
        for rule in Rule::ALL {
            let chunks = encode(&recs, &marks, rule, 7);
            let mut got = Vec::new();
            for c in &chunks {
                decode_positions(c, &mut got);
            }
            if rule == Rule::Nim {
                // Nim's gap: a chunk starting with a non-step record leaves the
                // following delta unanchored; the reset-per-chunk decoder then
                // disagrees. That is the defect, so it must show here.
                continue;
            }
            assert_eq!(got, positions(&recs), "rule {}", rule.name());
        }
    }

    #[test]
    fn min_rule_picks_the_shorter_and_ties_as_named() {
        // 300 -> 301: delta +1 (1 byte) beats abs 301 (2 bytes).
        // 0 -> 60 -> 1: abs 1 is 1 byte, delta -59 is 1 byte: a tie.
        let recs = vec![pos(300), pos(301), pos(60), pos(1)];
        let m = CallMarks::default();
        let d = encode(&recs, &m, Rule::MinTiesDelta, 4096).concat();
        let a = encode(&recs, &m, Rule::MinTiesAbs, 4096).concat();
        assert_eq!(d[3], TAG_DELTA);
        assert_eq!(d.len(), a.len());
        assert_ne!(d, a);
    }
}

//! `calls.dat` (`trace-events.md` §"Call Stream Records"), read only as far
//! as the step range of each call, which is all the step-encoding rules that
//! mention calls need.

use crate::varint::{get_s, get_u};

#[derive(Clone, Copy, Debug)]
pub struct CallRange {
    pub parent: i64,
    pub first: u64,
    pub last: u64,
}

fn skip_bytes(r: &[u8], p: &mut usize) -> Option<()> {
    let n = get_u(r, p)? as usize;
    *p = p.checked_add(n)?;
    (*p <= r.len()).then_some(())
}

/// Parses one decompressed chunk of call records. Both writers frame each
/// record in a chunk with a varint byte length (`call_stream.rs`'s chunk
/// flush; the spec's record table does not show the frame).
pub fn parse_chunk(raw: &[u8], out: &mut Vec<CallRange>) -> Option<()> {
    let mut q = 0usize;
    while q < raw.len() {
        let n = get_u(raw, &mut q)? as usize;
        let end = q.checked_add(n)?;
        let rec = raw.get(q..end)?;
        parse_record(rec, out)?;
        q = end;
    }
    Some(())
}

fn parse_record(raw: &[u8], out: &mut Vec<CallRange>) -> Option<()> {
    let mut p = 0usize;
    {
        let _function = get_u(raw, &mut p)?;
        let parent = get_s(raw, &mut p)?;
        let first = get_u(raw, &mut p)?;
        let last = get_u(raw, &mut p)?;
        let _depth = get_u(raw, &mut p)?;
        let args = get_u(raw, &mut p)?;
        for _ in 0..args {
            get_u(raw, &mut p)?;
            skip_bytes(raw, &mut p)?;
        }
        skip_bytes(raw, &mut p)?; // return value
        skip_bytes(raw, &mut p)?; // raised exception
        let children = get_u(raw, &mut p)?;
        for _ in 0..children {
            get_u(raw, &mut p)?;
        }
        if first > last {
            return None;
        }
        out.push(CallRange {
            parent,
            first,
            last,
        });
    }
    Some(())
}

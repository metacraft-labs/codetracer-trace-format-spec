//! Unsigned LEB128 and zigzag, as `trace-events.md` defines them.

#[inline]
pub fn put_u(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

#[inline]
pub fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

#[inline]
pub fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

#[inline]
pub fn put_s(out: &mut Vec<u8>, v: i64) {
    put_u(out, zigzag(v))
}

/// Encoded length of an unsigned varint.
#[inline]
pub fn len_u(v: u64) -> usize {
    if v == 0 {
        1
    } else {
        (64 - v.leading_zeros() as usize).div_ceil(7)
    }
}

/// Reads an unsigned varint at `*pos`, advancing it. `None` on truncation or
/// a value wider than 64 bits.
#[inline]
pub fn get_u(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        if shift == 63 && b > 1 {
            return None;
        }
        v |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// [`get_u`] with a one-byte fast path, for trusted, already-validated
/// input (the hot loops of the decode benchmarks). Panics on truncation.
#[inline(always)]
pub fn get_u_fast(buf: &[u8], pos: &mut usize) -> u64 {
    let b = buf[*pos];
    if b < 0x80 {
        *pos += 1;
        return b as u64;
    }
    get_u(buf, pos).unwrap()
}

#[inline]
pub fn get_s(buf: &[u8], pos: &mut usize) -> Option<i64> {
    get_u(buf, pos).map(unzigzag)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_len() {
        for v in [0u64, 1, 63, 64, 127, 128, 16383, 16384, u64::MAX] {
            let mut b = Vec::new();
            put_u(&mut b, v);
            assert_eq!(b.len(), len_u(v), "len of {v}");
            let mut p = 0;
            assert_eq!(get_u(&b, &mut p), Some(v));
        }
        for v in [0i64, -1, 1, -64, 63, -65, 64, i64::MIN, i64::MAX] {
            assert_eq!(unzigzag(zigzag(v)), v);
        }
        assert_eq!(zigzag(-64), 127);
        assert_eq!(zigzag(63), 126);
    }
}

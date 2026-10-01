//! A minimal CTFS container reader (`ctfs-container.md` §1–§4).
//!
//! Reads every writer revision seen in the corpus: all of them give every
//! member a level-1 mapping block, including empty members. A member whose
//! `MapBlock` points directly at its data block (the small-file layout, §2) is
//! recognised too, by `size <= BlockSize` together with the absence of a
//! plausible mapping table, so the reader also handles containers written
//! under the revised rule.

use std::fmt;

pub const MAGIC: [u8; 5] = [0xC0, 0xDE, 0x72, 0xAC, 0xE2];

#[derive(Debug)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
fn err<T>(s: impl Into<String>) -> Result<T, Error> {
    Err(Error(s.into()))
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub map_block: u64,
}

pub struct Container {
    pub bytes: Vec<u8>,
    pub version: u8,
    pub max_shards: u8,
    pub block_size: u64,
    pub entries: Vec<Entry>,
}

const ALPHABET: &[u8; 40] = b"\x000123456789abcdefghijklmnopqrstuvwxyz./-";

pub fn base40_decode(mut v: u64) -> String {
    let mut s = String::new();
    while v > 0 {
        let r = (v % 40) as usize;
        v /= 40;
        if r == 0 {
            break;
        }
        s.push(ALPHABET[r] as char);
    }
    s
}

/// Per-member block accounting under two layouts.
#[derive(Clone, Copy, Debug, Default)]
pub struct BlockUse {
    pub data_blocks: u64,
    /// Mapping blocks the member actually owns in this container.
    pub mapping_blocks: u64,
    /// Mapping blocks the member would own under §2's small-file rule plus
    /// `MapBlock = 0` for an empty member.
    pub mapping_blocks_small_file_rule: u64,
}

impl Container {
    pub fn open(path: &std::path::Path) -> Result<Container, Error> {
        let bytes = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        Self::parse(bytes)
    }

    pub fn parse(bytes: Vec<u8>) -> Result<Container, Error> {
        if bytes.len() < 16 || bytes[0..5] != MAGIC {
            return err("not a CTFS container (magic)");
        }
        let version = bytes[5];
        if bytes[6] != 0 {
            return err("encrypted container");
        }
        let max_shards = bytes[7];
        let block_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as u64;
        let max_root = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as u64;
        if block_size < 64 || block_size % 8 != 0 {
            return err(format!("implausible block size {block_size}"));
        }
        // Every reader and writer in the workspace places the entry array at
        // byte 16 whatever `max_shards` says (the free-list root area of §1 is
        // not implemented, and one writer stamps `max_shards = 1`), so this
        // reader does too.
        let count = if max_root == 0 { (block_size - 16) / 24 } else { max_root };
        let base = 16u64;
        let mut entries = Vec::new();
        for i in 0..count {
            let o = (base + i * 24) as usize;
            if o + 24 > bytes.len() {
                break;
            }
            let size = u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
            let map_block = u64::from_le_bytes(bytes[o + 8..o + 16].try_into().unwrap());
            let name = u64::from_le_bytes(bytes[o + 16..o + 24].try_into().unwrap());
            if size == 0 && map_block == 0 && name == 0 {
                continue;
            }
            entries.push(Entry {
                name: base40_decode(name),
                size,
                map_block,
            });
        }
        Ok(Container {
            bytes,
            version,
            max_shards,
            block_size,
            entries,
        })
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    fn block(&self, b: u64) -> Result<&[u8], Error> {
        if b == 0 {
            return err("null block pointer");
        }
        let s = (b * self.block_size) as usize;
        let e = s + self.block_size as usize;
        if e > self.bytes.len() {
            // The final block may be short in a container that is not a block
            // multiple; serve what is there.
            if s < self.bytes.len() {
                return Ok(&self.bytes[s..]);
            }
            return err(format!("block {b} past end of container"));
        }
        Ok(&self.bytes[s..e])
    }

    fn ptr(&self, block: u64, idx: u64) -> Result<u64, Error> {
        let b = self.block(block)?;
        let o = (idx * 8) as usize;
        if o + 8 > b.len() {
            return err("short mapping block");
        }
        Ok(u64::from_le_bytes(b[o..o + 8].try_into().unwrap()))
    }

    /// Whether `e`'s `MapBlock` is the data block itself (small-file layout).
    /// Every writer in the corpus allocates a mapping block, so this is only
    /// true for containers written under the revised rule; it is decided by
    /// whether slot 0 of the would-be mapping block is a plausible block
    /// number, which a data block of text or compressed bytes almost never is.
    fn is_direct(&self, e: &Entry) -> bool {
        if e.size > self.block_size {
            return false;
        }
        let nblocks = (self.bytes.len() as u64).div_ceil(self.block_size);
        match self.ptr(e.map_block, 0) {
            Ok(p) => !(p > 0 && p < nblocks),
            Err(_) => true,
        }
    }

    fn resolve(&self, e: &Entry, block_index: u64) -> Result<u64, Error> {
        let usable = self.block_size / 8 - 1;
        let mut idx = block_index;
        let mut level_block = e.map_block;
        let mut level = 1u32;
        loop {
            let cap = usable.saturating_pow(level);
            if idx < cap {
                break;
            }
            idx -= cap;
            level += 1;
            if level > 5 {
                return err("index past 5 levels");
            }
            level_block = self.ptr(level_block, usable)?;
        }
        let mut blk = level_block;
        while level > 1 {
            let sub = usable.saturating_pow(level - 1);
            blk = self.ptr(blk, idx / sub)?;
            idx %= sub;
            level -= 1;
        }
        self.ptr(blk, idx)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        let e = self
            .entry(name)
            .ok_or_else(|| Error(format!("no member {name}")))?
            .clone();
        self.read_entry(&e)
    }

    pub fn read_entry(&self, e: &Entry) -> Result<Vec<u8>, Error> {
        if e.size == 0 {
            return Ok(Vec::new());
        }
        if self.is_direct(e) {
            let b = self.block(e.map_block)?;
            return Ok(b[..e.size as usize].to_vec());
        }
        let bs = self.block_size;
        let mut out = Vec::with_capacity(e.size as usize);
        for i in 0..e.size.div_ceil(bs) {
            let b = self.block(self.resolve(e, i)?)?;
            let take = ((e.size - out.len() as u64).min(bs)) as usize;
            if take > b.len() {
                return err(format!("{}: short data block", e.name));
            }
            out.extend_from_slice(&b[..take]);
        }
        Ok(out)
    }

    /// Mapping blocks needed to address `nblocks` data blocks (§4 chain model).
    pub fn mapping_blocks_for(&self, nblocks: u64) -> u64 {
        let usable = self.block_size / 8 - 1;
        let mut total = 0u64;
        let mut remaining = nblocks;
        let mut level = 1u32;
        // At least the level-1 block exists once a mapping exists at all.
        if nblocks == 0 {
            return 1;
        }
        while remaining > 0 && level <= 5 {
            let cap = usable.saturating_pow(level);
            let here = remaining.min(cap);
            // A level-k block plus the level-(k-1) blocks under it.
            total += 1;
            let mut sub_blocks = here;
            for _ in 1..level {
                sub_blocks = sub_blocks.div_ceil(usable);
                total += sub_blocks;
            }
            remaining -= here;
            level += 1;
        }
        total
    }

    pub fn block_use(&self, e: &Entry) -> BlockUse {
        let data = e.size.div_ceil(self.block_size);
        let direct = e.size > 0 && self.is_direct(e);
        let mapping = if direct {
            0
        } else {
            self.mapping_blocks_for(data)
        };
        let small_rule = if data <= 1 {
            0
        } else {
            self.mapping_blocks_for(data)
        };
        BlockUse {
            data_blocks: data,
            mapping_blocks: mapping,
            mapping_blocks_small_file_rule: small_rule,
        }
    }
}

/// `[chunk_size: u32][offset: u64]...` (`ctfs-container.md` §7).
pub fn parse_idx(idx: &[u8]) -> Result<(u32, Vec<u64>), Error> {
    if idx.len() < 4 || (idx.len() - 4) % 8 != 0 {
        return err(format!("malformed .idx of {} bytes", idx.len()));
    }
    let cs = u32::from_le_bytes(idx[0..4].try_into().unwrap());
    let offs = idx[4..]
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    Ok((cs, offs))
}

/// Splits a chunked stream into its compressed frames.
pub fn frames<'a>(dat: &'a [u8], offs: &[u64]) -> Vec<&'a [u8]> {
    let mut v = Vec::with_capacity(offs.len());
    for (i, &o) in offs.iter().enumerate() {
        let end = offs.get(i + 1).copied().unwrap_or(dat.len() as u64);
        v.push(&dat[o as usize..end as usize]);
    }
    v
}

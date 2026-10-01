//! zstd decompression usable on wasm32.
//!
//! The Rust reader uses `ruzstd` on wasm32 (`codetracer_trace_reader`'s
//! Cargo.toml), so the WASM decode benchmark measures that decoder. Natively
//! both C zstd (what the native readers use) and ruzstd are measured.

use ruzstd::decoding::FrameDecoder;
use std::io::Read;

/// Decompresses one frame with ruzstd into `out` (cleared first).
pub fn ruzstd_decompress(dec: &mut FrameDecoder, frame: &[u8], out: &mut Vec<u8>) {
    out.clear();
    let mut src = frame;
    dec.reset(&mut src).expect("ruzstd: frame header");
    dec.decode_blocks(&mut src, ruzstd::decoding::BlockDecodingStrategy::All)
        .expect("ruzstd: blocks");
    let n = dec.can_collect();
    out.resize(n, 0);
    let got = dec.read(&mut out[..]).expect("ruzstd: collect");
    out.truncate(got);
}

#[cfg(feature = "native")]
pub fn compress(raw: &[u8], level: i32) -> Vec<u8> {
    // One-shot compression declares the content size in the frame header, as
    // `internal-files.md` §"Chunking and compression of the runtime streams"
    // requires of every chunk.
    zstd::bulk::compress(raw, level).expect("zstd compress")
}

#[cfg(feature = "native")]
pub fn decompress(frame: &[u8]) -> Vec<u8> {
    let n = zstd::zstd_safe::get_frame_content_size(frame)
        .ok()
        .flatten()
        .expect("frame declares its content size") as usize;
    zstd::bulk::decompress(frame, n).expect("zstd decompress")
}

//! Measurement support for CTFS format decisions.
//!
//! This crate is deliberately independent of both reference implementations
//! (`codetracer-trace-format`, `codetracer-trace-format-nim`): it reads a `.ct`
//! container with its own small reader, so a measurement cannot inherit a bug
//! from the code whose encoding it is judging, and it can read containers
//! written by any writer revision a recorder happened to ship with.
//!
//! Modules:
//! - [`ctfs`]: container reader (header, root directory, block mapping).
//! - [`varint`]: LEB128 / zigzag.
//! - [`steps`]: `steps.dat` decode to a writer-independent record list, and
//!   re-encoding under candidate AbsoluteStep/DeltaStep rules.
//! - [`calls`]: just enough of `calls.dat` to find call entries and returns.
//! - [`stepmap`]: `step-map.ns` construction and candidate storage encodings.
//! - [`zst`]: zstd decompression that works on wasm32 (ruzstd) and natively.

pub mod calls;
pub mod ctfs;
pub mod stepmap;
pub mod steps;
pub mod varint;
pub mod zst;

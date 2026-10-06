# Rust CTFS reader starts the directory inside declared free-list roots

Measured source: `codetracer-trace-format` at `f39e71016715b4ba67b626f4c971acfd6a766977`, `codetracer_ctfs/src/reader.rs::CtfsReader::open`. After reading the 16-byte combined header, the implementation immediately reads `max_root_entries` file entries. It does not use the parsed `Header.max_shards` to skip the free-list root region. This is a source-qualified defect observation; a genuine nonzero-shard native reproduction remains required before claiming runtime qualification.

`ctfs-container.md`, Block 0 Layout, requires version-5 entries to begin at `16 + R`, where `R = 7 * max_shards * 6`; zero-count auto-fill uses `(block_size - 16 - R) / 24`. Explicit entry counts may reserve contiguous root overflow blocks. At one shard the correct first entry is byte58, while the current Rust reader starts at byte16. The same source reads zero entries for an auto-fill declaration.

The existing unsharded Rust writer does not exercise this nonzero-shard reader boundary. The private immutable-snapshot seam currently retains the same initial directory assumption and cannot claim complete header-derived bounds until it is corrected and qualified.

Required repair: derive checked version-5 directory start and effective count from the actual parsed header, preserve explicit version refusal and existing unsharded bytes, validate prefix/directory arithmetic and complete physical bounds before entry allocation or reads. Genuine nonzero-shard, auto-fill, explicit small-count, root-overflow, truncated-prefix and misplaced-entry controls must preserve reserved roots and reject corruption. This does not introduce container-version6 support, change frozen consumer pins, or qualify the private successor.

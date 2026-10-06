# Rust CTFS writer treats auto-fill as zero capacity

Status: Open; source-qualified observation, no runtime repair qualified.

At `codetracer-trace-format` commit
`f39e71016715b4ba67b626f4c971acfd6a766977`,
`codetracer_ctfs/src/writer.rs::CtfsWriter::create_in_store` passes the literal
`max_root_entries` to `ExtendedHeader::new`, writes that many empty entries,
and stores it as the writer's capacity. `ExtendedHeader::new` admits zero.
`add_file` then rejects its first member because `files.len() >= 0`.

The normative `ctfs-container.md` Block 0 Layout defines serialized
MaxRootEntries zero as auto-fill: `(BlockSize - 16 - R) / 24` for the existing
version-5 layout, where `R = 42 * MaxShards`. An unsharded 4096-byte block has
170 slots. Zero therefore declares a usable automatic directory, rather than
an empty writer incapable of accepting a member.

This is distinct from the recorded Rust reader shard-prefix/auto-fill defect:
an independently corrected reader does not repair the writer's allocation or
admission. The source observation does not expand the existing writer's shard
API or container-version support, and no genuine writer reproduction or
successor qualification is claimed here.

Required bounded repair design: distinguish the serialized zero declaration
from its effective checked directory capacity, preserve explicit nonzero
directory behavior and existing unsharded bytes, and demonstrate actual
zero-declaration writer/member/reader round trips against independently
verified header/directory byte expectations. Preserve all original suites and
unsupported-version refusals. Do not silently change the frozen reader or
consumer pins, regenerate expected bytes from the repaired writer, or fold a
new shard/version feature into this correction.

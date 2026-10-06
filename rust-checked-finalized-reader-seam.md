# Checked finalized CTFS reader and immutable-image API

The implementation baseline is the clean owning Rust format commit f39e71016715b4ba67b626f4c971acfd6a766977. Format authority is TraceFormatSpec 974ab479f502eb34138f8168b501f566c9bf71ee. This amendment repairs finalized-reader completeness and representable field preservation; it does not add container version 6 support or advance any consumer pin.

## Same-image boundary

Preserve the nongeneric public CtfsReader and its existing open(path) behavior. Add CtfsReader::from_bytes(Vec<u8>) returning the same result/error type. The reader owns the supplied bytes and exposes no mutable access to its backing image. A private input enum implements the existing read/seek operations for File and Cursor<Vec<u8>>. Snapshot bounds derive from the owned byte length; live file and ConcurrentCtfsReader bounds retain their existing per-read behavior. Header and member validation applies equally to both inputs. Snapshot construction performs checked size arithmetic and refuses invalid directory allocation rather than reserving an attacker-declared capacity.

Add read_trace_from_reader(&mut CtfsReader), preserving combined-stream precedence and existing read_trace_from_ctfs(path) compatibility through delegation. Metadata and events can then be decoded from one owned image without reopening the path. The caller must bound its initial allocation before reading the container; this API does not claim a mutable file descriptor is immutable. Genuine tests mutate or replace the original path after snapshot construction and prove both metadata and events still refer to the original owned image.

## Finalized completeness and fields

Only actual marker absence retains documented legacy CBOR selection. Present unreadable or unknown events.fmt is a named error. Add a checked split decoder that accounts for all supplied bytes and rejects a valid prefix followed by an invalid tail. Preserve any deliberately unchecked compatibility API with its semantics explicitly documented; the finalized trace entry point uses the checked API.

Finalized chunk decoding validates complete frame headers, encoded sizes and record counts with checked arithmetic. Distinguish any declared partial/live decoding interface from finalized decoding. Preserve existing container whole-block tail rules: an incomplete unaddressable container block is distinct from an incomplete member frame.

Bridge every representable type-specific, event and value field without lossy UTF-8 replacement, unknown-kind coercion or silently skipped value tags. Preserve the existing forward-compatibility rule: unknown self-delimiting value tags >=10 are bounds-checked and skipped with a named diagnostic while decoding remaining known values; unknown tags <10 refuse. A checked result reports skipped events as incomplete, and an explicitly lossless caller can refuse that result. Unsupported non-self-delimiting wire values receive a named capability error identifying the member/tag/position. This is a new explicit completeness interface, not a claim that the specified bounded-skip policy itself is defective. Preserve valid forward, self and mutually recursive type references, append-order IDs, float bit identity, combined-stream precedence and existing public DTO compatibility. No fabricated defaults or partial-success database publication is allowed.

## Qualification

Use genuine writer/encoder, immutable byte images and filesystem boundaries. Full-field combined and split fixtures cover every modern event and RValue variant, nested values, nonempty type-specific records, names, flags, arguments and returns. Malformed controls cover prefix-plus-tail, truncated frames, counts, offsets, paired missing members, unknown markers/kinds/tags, invalid UTF-8, truncated directory prefixes and invalid block references. Assert specific errors and no complete-prefix acceptance or panic. Same-image controls bind metadata and events after actual source-path changes.

Run the complete original owning corpus alongside the new tests. Preserve all advertised native/platform gates, compiler/tool identities and solved-lock dependency principals. Source-only or private Linux evidence does not qualify other platforms. Container version 6, frozen caller semantics, existing Windows compiler prerequisites and monitoring gaps remain independent requirements. Owning publication and any consumer transition require review of the exact final source, complete generated lock and actual qualification receipts.

# Recorded program entry identity

An optional `entry.dat` member records which genuinely registered call is the program entry. It supplies replay authority that cannot be inferred from a literal synthetic root and its first child: legacy absorbed entries can execute their real body in the root before a later internal call. The literal `<toplevel>` root, all actual calls, arguments, steps, locals and returns remain unchanged.

## Binary member

`entry.dat` is an uncompressed optional container member, found by structural presence as specified in [Optional runtime members](internal-files.md#optional-runtime-members) and [Stream-presence flags](internal-files.md#stream-presence-flags-are-a-hint-not-a-gate). No `meta.dat` flag or version changes. Its absence declares no entry identity. A present empty member is malformed, never absence.

Exactly one record, with no trailing bytes:

```
magic:        4 bytes ASCII "CTEI"
version:      u16 little-endian = 1
reserved:     u16 little-endian = 0
call_key:     canonical unsigned varint
function_id:  canonical unsigned varint
entry_step:   canonical unsigned varint
```

Varints are shortest-form unsigned base-128 encodings, at most ten bytes with no overflow. Call and step ids must fit their nonnegative signed-64-bit domains; function ids must fit the reader's index domain and resolve to an actual function. Unknown version, nonzero reserved bytes, duplicate member/record, truncation, overlong/overflow varint, trailing bytes and out-of-domain ids MUST be refused naming `entry.dat`.

## Writer

A checked writer operation marks the CURRENT genuinely registered active call as entry, capturing that call's actual key, function id and entry step. Callers do not supply guessed ids or a function name. No active call, a completed call or a second different entry MUST fail without replacing the first identity. Repeating the identical identity may be idempotent. A recorder that never marks an entry emits no member, so its original container stays byte-identical.

The writer retains this identity and validates its final call/step binding. At close, after every existing close-time member including `corrmark.ns`, it creates and writes `entry.dat` once, before final sealing. This extends [block placement](ctfs-container.md#block-placement-the-container-is-a-function-of-the-recording) with one final append and preserves the ordering of every preexisting append. It does not rewrite `meta.dat`, which is complete at open. A marked identity that cannot be resolved or validated at finalization MUST fail the recording. There is no external sidecar.

Before close, no entry member is published; this extension supplies entry authority for a finalized recording, not a promise that a following reader knows entry identity early. Existing following behavior remains in effect until then. A genuine root-level program body may be explicitly marked as root0; unchanged recorders do not opt in implicitly.

## Checked reader and replay

Readers expose an optional `RecordedEntryIdentity` from the SAME immutable finalized image that supplies calls, functions and steps. A present identity MUST resolve to an actual call and function, the call's function id MUST match, the call's entry step MUST equal the declared step, and that actual step MUST belong to the named call. A nonroot entry MUST have a valid recorded parent chain to the synthetic root when one exists; cycles, missing parents and inconsistent depth/parent relationships MUST be refused. An explicitly marked root-level body is valid when its actual root call and step satisfy the same checks.

The reader MUST validate before using the identity, without reopening a path, searching for an expected function name, inferring an unused function or manufacturing a call/step. A malformed present identity MUST NOT fall back to legacy selection.

Replay selects the validated entry call and applies its existing first-executable-step semantics. If the member is absent, replay MUST preserve the previous entry selection exactly, including an absorbed legacy EVM root body, genuine outer/root-only bodies, callless/event-only recordings and ordinary Python, Ruby, JavaScript and BEAM behavior. Source parsing, constant step indices, child ordinal or variable absence are not substitutes for recorded entry authority.

### Legacy entry selection

"The previous entry selection" is this rule, stated here so it can be preserved. The entry is the first call in user source, except that the `<toplevel>` root (call key 0) is skipped when it **wraps** the program. It wraps the program when a step of a nested call comes before the root runs any code of its own. Walking forward from the root's entry step, the only root steps allowed before that nested step are:

- exec records with no source position, such as a thread start, which still occupy step ids;
- steps at the entry step's own path and line, provided the call that opens next is defined at that same path and line. Such a step is an anchor: the JavaScript recorder emits a step at the callee's definition line in the caller's frame before every call, and it starts the recording at the position of its first step, which is the anchor before `<module>`.

The walk gives up after eight root steps. A root still positioning by then does not wrap the program.

Any other root step means the root runs its own body, so it does not wrap the program. This is the absorbed-entry shape, and replay opens on the root. Both conditions of the anchor bullet matter here. An absorbed body may start on the entry line and then call a helper defined elsewhere. The EVM recorder defines each external call's function at the caller's line, so a body that ran one line before an external call stands at the definition position of the call that opens. Replay also opens on the root when the recording has no other call. Within the selected call, replay opens on the first executable step.

An earlier replay implementation looked only at the step after the root's entry step. On a JavaScript recording that step is a thread record, followed by the `<module>` anchor, so the root was never recognised as a wrapper. The recording then opened on the synthetic entry step, and the first step over ran past the whole program. That was a defect, not legacy behavior to preserve. The rule above is the selection that the "ordinary JavaScript behavior" requirement refers to.

Canonical `--full` emits a present validated identity as `metadata.entry` with keys `call_key`, `function_id`, `entry_step`. An absent identity adds no key and preserves the whole legacy document. Path stripping and every other field retain their existing semantics.

## Compatibility and rollout

New readers accept containers without the member unchanged. Old readers that ignore unknown optional members can read the unchanged existing streams in a new container, but do not gain entry-position correctness. Actual old-reader readability MUST be qualified on a genuine new recording; it is not assumed from the layout. Unknown extended metadata flags remain refused under the existing rule; this extension allocates none.

Entry-aware Nim/Rust/FFI and replay reader support must ship in the supported consumer tuple before a recorder opts in. Full source/header/compiler/link/runtime binding and original platform/monitor gates remain mandatory. This specification authorizes neither a borrowed branch write nor an unreviewed dependency-pin transition.

## Verification

Use genuine writer and checked finalized-reader APIs and real recordings. Required cases include exact balanced root/entry/internal calls and source/argument/local fidelity; a real parameterized entry; absent-member whole-document and replay equality for legacy absorbed EVM, root-body-with-child, root-only, callless and ordinary-language recordings; reverse/restore/step-over behavior; both reference writers producing equal bytes; and old-reader readability.

Independently corrupt each member shape/version/reserved/id/call/function/step/parent binding and prove named refusal, followed by genuine restoration. Preserve direct-writer primitive exact counts (root/add remains two calls) and all original assertions; change only genuine dispatcher fixtures whose producer contract changes. No mocked trees, golden rewriting, filtered suites or weakened counts.

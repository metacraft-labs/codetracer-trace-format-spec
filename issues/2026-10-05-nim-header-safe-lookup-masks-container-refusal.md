# Header-safe root lookup can turn a container refusal into optional absence

Measured at `68d32331d1b017ac5c2ab873d952c42b72558553` plus the seven reviewed sharded-root postimages: the unchanged original full `just test` fails `test_older_versions_are_refused`, reader door 0, by opening a CTFS version-2 container. Its preceding unchanged path-registration timing test passes with ratio 4.31; the earlier 8.63 failure remains a separate retained result.

A genuine archive of unmodified `68d32331`, using the same owning Nim 2.2.8 executable, release/ARC/compiler-prefix arguments and original test, passes all four version-refusal/current-positive groups. This is a regression in the proposed postimages, not a demonstrated released-tip defect.

## Expected

[ctfs-container.md §2, “Older versions are refused”](../ctfs-container.md) requires refusal of an unimplemented version before resolving members. The owning Nim reader deliberately declares container version 5 only. Container versions 2, 3, 4 and 6 must remain refused by name; support for version 6 is a separate unfinished capability, not authorized by this repair. [§1c](../ctfs-container.md) rejects defaults for unknown header values. The supported metadata schema remains 6, independently of the container-version gate.

## Cause and bounded repair

The reviewed low-level `findFileEntry` validates the root layout and returns `found=false` on an invalid header. The high-level `openNewTraceFromBytes` treats failed optional metadata lookup as absence and skips tables whose presence lookup returns false. Previously, presence lookup found the stamped container's tables and their `readInternalFile` version gate supplied the refusal. The new safe lookup must remain unchanged; the public reader must validate the version and root header before interpreting any failed optional lookup as absence.

Use the existing `ctfsVersionError` and header-derived `rootDirectoryLayout` at public open entry. Keep genuine version-5 optional absence, all original test assertions/flags, and the reviewed shard-prefix/count/overflow/append behavior. Do not widen accepted versions, change metadata decoding, or modify frozen consumer revisions.

## Evidence

- Current original full gate: `/tmp/promotion-campaign/trace-nim-single-original-full-qualification.log`; natural exit 1, owned SID empty, full source/index unchanged. Proof SHA-256 `cfe7f903c8592f52ea8f5745a43aa90a0ab43c1a360aa9b2647d6fbf6d652c43`.
- Unmodified original baseline: `/tmp/promotion-campaign/trace-nim-original68-version-_09lrbze/baseline.log`; exit 0, four groups pass, original and owning source/tool guards unchanged. Proof SHA-256 `e3718894895e0aa76362c5720ea95fb0bc2bfa29d2034fda19efb84151e2d9f1`.
- Compiler in both cases: owning `/nix/store/ih7dxzla05hxpfrd08x908gg6w6hg47y-nim-2.2.8/bin/nim`, SHA-256 `926a860d3c0fbb7bd048b9f7b39d6c2faf2de17a99f008458932ec01b4b2ed1f`.

After repair, run the unchanged original version-refusal test and the complete original suite including both FFI tasks. Component or timing successes do not qualify the whole suite or required platforms.

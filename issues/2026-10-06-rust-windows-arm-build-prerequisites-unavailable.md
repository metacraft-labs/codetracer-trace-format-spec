# Rust Windows ARM build prerequisites unavailable

Status: Open; genuine required-job failures, no native repair qualified.

At Rust format PR65 head `f39e71016715b4ba67b626f4c971acfd6a766977`,
the native Windows ARM job `111211182915` tested merge
`6a41d9c39d67f1666d90b5e1603fc0b6e303ea38`. Its actual setup action
`58da84d5af27408ca598ce69caa83b133e054592` resolved Codetracer
`34b96d03b9436a0538e0d526a9caaf458226c63f` from the committed lock.
The original pinned Cap'n Proto source bootstrap then refused because CMake
was absent from PATH. This fails before the native cargo corpus executes.

The distinct typed Windows ARM job `111211183222` at the same PR head
reports MSVC activation skipped because `vswhere.exe` was absent, then fails
the Rust MSVC target with `link.exe` not found. CMake provisioning alone is
not qualification of this separate compiler/linker/SDK requirement.

The expectations are the existing owning workflow's pinned Cap'n Proto
component requirement and `repro.nim`'s original whole-workspace cargo graph,
alongside the checked-reader seam's separate advertised native-platform
qualification requirement. Neither job is evidence of a reader test-body
defect, and neither failure permits removing a required action or test.

Actual raw logs are retained as
`/tmp/promotion-campaign/rust-f39-windows-arm64-111211182915.log` and
`rust-f39-windows-arm64-111211183222.log`. The exact committed helper source
and hashes are recorded in
`/tmp/promotion-campaign/rust-native-arm-prerequisite-source-inspection/source-binding.json`.

Proper repair requires genuine native ARM CMake plus a supported native ARM
MSVC compiler/linker and full Windows/UCRT/VC header/library closure, with
actual selected target/image/version/source evidence and unchanged original
native and typed cargo/FFI gates. The locked Codetracer helper declares MSVC
toolset14.51.36231 but permits other host-tool fallbacks and version warnings;
merely invoking it does not prove native ARM tool selection. No GNU ABI
substitution, emulation qualification, unpinned latest installer, fake linker
alias, test omission or monitor/quota bypass is justified.

The immutable766 catalog's official CMake4.3.3 ARM archive was independently
hash/member/PE inspected on Linux. That bounded observation is not native
extraction, runtime/version, Cap'n Proto compatibility, MSVC provisioning or
complete Windows qualification. The owning prerequisite design remains
pending substantive review; no workflow, compiler/source pin or consumer
dependency transition has been applied.

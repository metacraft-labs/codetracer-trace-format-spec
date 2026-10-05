# Trace-format Nim dev shell declares only lint

Measured product: `codetracer-trace-format-nim` at
`68d32331d1b017ac5c2ab873d952c42b72558553` plus the independently reviewed
eight-file root-directory/public-reader repair. That repair does not modify
`flake.nix`. This is a source configuration gap, not a claim that any absent
hook executed or passed.

The actual `preCommit` definition enables only `lint` with original
`just lint` entry and pre-commit stage. It imports no named standard-hook
provider; the owning repository has no tracked EditorConfig. Current
[repository requirements §4](https://github.com/metacraft-labs/metacraft-dev-guidelines/blob/78bb3425d85a119a4abd7c86a47830d13ff2c897/policies/repo-requirements.md)
requires adoption of the whole named `mcl-standard-hooks` set. The existing
lint-only generated config does not establish that adoption.

Fresh owning specs mainline `108d1233d1127d14573b6dd7ac4faab772ee8a55`, open
issues and full issue history were searched for hook/pre-commit/EditorConfig
before filing; no existing matching issue was found.

Implement the bounded [owning named-set specification](../nim-named-standard-hook-seam.md).
Preserve the original lint, complete compiler/dependency closure, owning
shell-install positive and foreign-cwd no-write tests, managed-hook ownership,
all original native/typed/shipping/platform gates, and all source/test flags.
Reject unknown hooks before writes. Native Unix construction must not be
reported as native Windows portability, and a successful root/reader suite
does not qualify this separate hook seam.

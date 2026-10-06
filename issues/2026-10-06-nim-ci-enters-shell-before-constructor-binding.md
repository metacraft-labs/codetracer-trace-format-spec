# TraceNim CI enters its guarded shell before explicit constructor binding

| | |
|---|---|
| Status | in-progress; private prerequisite qualification pending |
| Recorded | 2026-10-06 |
| Observed in | codetracer-trace-format-nim @ 19cad035585cbc0845b84ddc11aa3b824825dfaa |
| Area | Unix CI setup and owning native hook snapshot |

## Observed

The current CI installs Nix, enters the owning shell to expose Bash, and only afterward runs source Repro setup. It does not bind REPROBUILD_REPRO before that first entry. Actual Linux job112100075911 failed at snapshot; its underlying guard stderr was not uploaded. A genuine private exact19cad entry with the variable deliberately absent failed with `Explicit matching managed hook constructor is absent`. That is corroboration of the missing-binding path, not an assertion about the unobserved runner stderr.

## Expected

[nim-named-standard-hook-seam.md](../nim-named-standard-hook-seam.md) requires an actual matching constructor and preserves owning installation/foreign no-write refusals. [nim-ci-constructor-before-shell.md](../nim-ci-constructor-before-shell.md) defines the explicit committed766 prerequisite before first shell entry and narrow guard-only failure evidence. No ownership or test policy is relaxed.

## Evidence

Current job log: `/tmp/promotion-campaign/trace-19cad-current-failed-job-112100075911.log`. Private exact-source failure: `/tmp/promotion-campaign/trace-19cad-no-constructor-private.log`, captured known guard stderr: `/tmp/promotion-campaign/trace-19cad-no-constructor-private-guard-stderr.log`. The immutable package acquisition's private0 result is separate from CI authentication/platform acceptance. Source/template/config/unknown refusal checks remain mandatory.

Specs synchronized at e50f4238e6c0caf214fc33e40153a658aaf09c29; open issues and deleted issue history were searched. The existing foreign-Python installer issue is distinct. Windows actual GCC Invalid argument is a separate unproven cause.

## Suggested direction

Bind exact committed766 package/runtime/helper/protocol before first shell, preserve original later source helper/commands, and retain only the six known guard subprocess stderr labels in exclusive CI proof files. Never upload transaction snapshots, Git configuration or unrelated installer output. Preserve healthy current runs and qualify the new head through original suites.

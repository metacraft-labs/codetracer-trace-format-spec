# Installed Nim native runner rejects the canonical hook configuration

## Observed

At `codetracer-trace-format-nim` owning `19cad035585cbc0845b84ddc11aa3b824825dfaa` plus the three reviewed constructor-before-shell postimages, the unchanged original owning/foreign shell oracle and the complete configured `prek run --all-files` gate passed. A normal `git commit` then failed before any native hook executed: the installed generated `.repro-local` hook invokes the selected git-hooks library's `pre-commit` 4.5.1, whose validator rejects the canonical `ban-added-ct-recordings` hook's `language: unsupported`.

The owning shell separately declares Prek 0.2.17 and pre-commit 4.3.0. These are distinct from the library-default constructor package: a version-only command on PATH does not identify the installed hook runner. The actual 4.5.1 installed whole body and configuration were retained. No commit was created, configuration language was not rewritten, and current CI was not superseded.

## Expected

[nim-named-standard-hook-seam.md](../nim-named-standard-hook-seam.md) requires the complete named set, actual selected runner/factory/configuration authority, original ownership oracles, and genuine configured native gates. An installed native hook must execute that same complete configuration; success from a different manual runner is not native hook qualification.

## Proposed repair and qualification

Select the already-declared owning Prek package explicitly for the canonical configuration and derive factory invocation from the actual selected package's executable. Preserve the existing compiler/tool/source pins, both declared native runner closures, the exact prior native factory as legacy ownership authority, all named hooks/stages and original assertions. Retain this failed normal commit. Qualify actual generated whole bodies, ownership/refusal/rollback controls and native hook execution through a genuine commit before claiming publication readiness. No language substitution, exemption or managed-hook bypass is proposed.

## Evidence

`/tmp/promotion-campaign/trace-constructor-final-8mrbay4l/owning-normal-commit-attempt.log` retains the real refusal. `owning-gates-proof.json` records the preceding source-bound original oracle and configured Prek gate. `native-runner-package-api.log` records actual owning package versions and public installation options. The original constructor/private qualification receipts remain separate from this installed-runner defect.

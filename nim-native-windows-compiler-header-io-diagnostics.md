# Native Windows compiler/header I/O diagnosis

Status: Approved finite failure-only diagnostic; not a production fix or native suite acceptance.

At Nim producer ae0781373070db05c4ac6386fde3885f34ac7479, actual Windows x64 job112989555211 completes whole locked module acquisition and reaches original ct-print C compilation, then reports project codetracer_crossing_state.h, ordinary string.h and Nim nimbase.h Invalid argument with external GCC exit1. The cause, actual typed compiler image and released engine/source correspondence remain unproven. Preserve the failed build and every original compiler flag, input, action and assertion.

After an original Windows command fails, use the existing dev-exec wrapper to run a finite PowerShell diagnostic. Record resolved SDK GCC and Nim absolute file paths, complete hashes and compiler version, and hash/read the original Nim include header and project header. In one exclusive RUNNER_TEMP directory, compile two real minimal C translation units: string.h plus the project header, and string.h plus nimbase.h, with their original include prefixes. Record exact argv, exits and output hashes when produced. Revalidate all four observed files after compilation. Do not dump environment values, mutate original sources, change compiler/profile pins, replace headers, disable monitoring/quota or count this probe as passing the original tests.

SDK selected image is an observation distinct from typed action-profile image; match actual identities before causality claims. No Linux PowerShell parser result is native Windows qualification. Full original platform matrix remains mandatory; diagnostics execute only on failure and original reds remain red until a proper reviewed source/provisioning correction actually qualifies. No mocks are used.

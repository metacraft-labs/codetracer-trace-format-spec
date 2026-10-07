# Windows ct-print cannot resolve declared results source

At Nim producer `ac06faf4719d8c995ae4c3bab43765d2e1b1b947`, PR21 native Windows x64 typed job `112917099677` fails the original ct-print compile with `cannot open file: results` at `src/codetracer_ct_print.nim:24`. The selected archive is the native provisioned Nim2.2.10. This failure is distinct from the Windows ARM official Nim archive extraction failure and does not establish a C compiler/linker defect.

The same producer's `flake.lock` already declares the complete immutable `nim-results` b319652e98a198fa881ca70a76754c2dd6f09804, `nim-stew` 1a5d0b99209f50ff055d9b5216849ba0365f8cf5 and `nim-unittest2` 92e1c13dcfb6d7e4ea6fb624fe9cc31e3dc76813 source inputs. Its Unix selected Nim wrapper supplies their module paths. The native Windows typed action has not received that complete owning source closure.

The [approved complete source-input contract](../nim-native-windows-complete-source-inputs.md) requires whole existing locked trees and explicit module paths/inputs, with the original native Just/Nimble configuration receiving the same roots. It preserves all original Unix paths, public defaults, tests and compiler ABI. Source-only/Linux qualification cannot close the native Windows gate.

Evidence: exact raw job `/tmp/promotion-campaign/trace-ac06-job-112917099677.log`, original error at line671. Complete private Linux acquisition preserves all139 members of those three locked inputs and their Linux NAR identities; this is source construction evidence only. Source patch and native qualification remain pending.

Specs synchronized at e4ac3505610689b44d1a21c9a88863346cfb2f21. Open issues and deleted issue history were searched for results/complete Windows Nim source closure; the original Rust storage fixture source issue is distinct. No producer pins, source assertions or native matrix jobs were changed by this record.

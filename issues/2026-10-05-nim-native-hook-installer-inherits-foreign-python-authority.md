# Owning native hook installer inherits an older Python module

At `68d32331d1b017ac5c2ab873d952c42b72558553` plus the reviewed thirteen
postimages, original owning shell attempt 57956 fails the native hook census
after the genuine upstream installer returns zero. Its retained transaction
`trace-nim-hook-transaction-wdag4onp` reports `Unknown hook body or mode:
pre-commit`, then `UNSAFE restoration refused`; no rollback acceptance follows.

The selected upstream executable is pre-commit 4.5.1. A version-only diagnostic
with the actual final `nix print-dev-env --json` exported environment reports
4.3.0; removing only inherited PYTHONPATH/PYTHONHOME/NIX_PYTHONPATH reports4.5.1
from the identical executable. The installed body names pre-commit4.3.0 and
owning Bash, whereas the genuine factory expects4.5.1 and its different Bash.
The diagnostic is retained at
`/tmp/promotion-campaign/trace-nim-native-installer-python-authority-diagnostic.json`.

[nim-named-standard-hook-seam.md](../nim-named-standard-hook-seam.md) requires
the actual native configuration/arguments and complete body/mode authority,
original owning install-positive and foreign no-write oracles, and refusal
before unsafe restoration. Fix the child import authority and shared factory
Bash closure rather than accepting unexpected bodies or replacing the oracle.
Real private final-source controls, owning final hooks and original suites
remain required; no platform or complete-monitor qualification is claimed.

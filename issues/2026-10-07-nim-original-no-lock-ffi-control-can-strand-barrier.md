# Original no-lock FFI causal control can strand its barrier

At `ea955fafa9c7a8a9014b6fba3b19d8e198c5e95a` plus the private explicit-Zstd proposal, original `testFfiThreads` reached the deliberately invalid no-process-lock archive and remained blocked for more than 26 minutes. Child3532397 and its UID/birth/image/argv, four futex-waiting threads and raw output are retained in `traceea-zstd-action-input-private-ofk115fj`. No timeout or forced exit is a passing negative.

The original Nimble causal contract requires both shipped C hosts to succeed naturally and the TLS/no-lock variants to fail naturally. The concurrent host has four workers, 30000 steps and two barriers; early failure returns can leave peers waiting at the second barrier. The observed shape is consistent with a failed participant, but does not prove the individual failure path.

The reviewed private design is [bounded original FFI host controls](../nim-bounded-original-ffi-host-controls.md). It preserves workload/assertions, makes failed workers participate in both barriers, and requires an authoritative natural child result before causal acceptance. Private qualification and full final source review remain required; no owning adoption, original hung-child recovery, native Darwin or whole-suite green is claimed.

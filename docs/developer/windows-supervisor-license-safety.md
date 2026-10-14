# B3B2 local dependency and FFI review

The sole new direct dependency is the repository-local lab-windows-job path
crate, version 0.1.0, Rust 1.95. It has no dependencies. Its license is the same
workspace MIT license in [LICENSE](../../LICENSE); the Windows SDK ABI is called
through system Kernel32, which is not redistributed. No third-party package,
toolchain, SDK installation or historical release/license asset is changed.

The separate crate is necessary because Workbench inherits unsafe_code=forbid.
That setting is unchanged. The capsule denies unsafe in its root and permits it
only in [windows.rs](../../crates/lab-windows-job/src/windows.rs). ABI sizes are
checked for Windows x64; other Windows architectures are not qualified. Safe
Workbench state-machine/file-fixture code contains no unsafe.

## Reviewed invariants

- Unnamed Job, non-inherited handles, exact limit flags and readback, no breakaway.
- JOB_LIST attributes retain aligned storage and boxed handle value through list
  deletion. No spawn/Assign fallback exists. Initial process/thread ownership is
  registered before post-create checks; failed gates leave that exact suspended
  process available for one rollback request and original-handle observation.
- Membership and limits are checked again before ResumeThread; its previous
  suspend count must be exactly one. A possibly resumed failure stays contained.
- Unique Job/process/thread handles close once on successful checked retirement.
  A failed retirement retains the remaining ledger and blocks replacement.
  Drop is containment only; it is never process exit or GPU cleanup evidence.
- Process witnesses duplicate only SYNCHRONIZE/QUERY access from the original
  process, with inheritance disabled. Only the read-only witness is Send, justified
  by Windows kernel process-handle operations having no thread affinity. Job
  ownership itself is not Send/Sync and remains on the sole supervisor worker.
- Parent-death fixture reports an original child process handle identifier to its
  external guard. The guard duplicates from the still-live original parent process
  and validates the child PID; ordinary supervision never reopens a process by PID.
  This test-only facility is behind fixture-probes, not a production protocol.
- Executable/argument count, UTF-16 length and retained capacities are bounded.
  Quoting rejects embedded NUL/quotes and a terminal backslash. Absolute paths and
  at most eight tokens are required. Launch environment is a fixed, checked Windows
  directory query followed by a SystemRoot-only block; Runtime secrets and parent
  launch controls are not inherited.
- FFI structures, output pointers and buffers remain live through each call.
  Attribute storage is at most 4,096 bytes; UTF-16 command capacity is 9,243 units;
  executable/cwd capacities are 1,025 units each; environment is 1,040 units.
  No GL pointers, Runtime/client handles or writable model references enter here.

## Limits

Job containment is not a malicious-code security sandbox or a VRAM/Rust allocator
limit. Same-user trusted code can still open other services/files; the software
authority boundary and future private protocol need their own architecture guards.
OS calls may stall. One worker and one unresolved episode remain owned; no worker
replacement, termination retry or child restart is synthesized.

The fixture-only 32 MiB process/Job limit is inherited from B3A, not a deployment
cap. The two-process parent-death guard uses separate 64/128 MiB test limits.
Production GUI cap C remains unselected. Whole host Rust budget and terminal GPU
lifecycle remain BLOCKED. Native parent-death containment is not a per-GL-object
cleanup receipt. ACTIVATION_QUALIFIED=false.

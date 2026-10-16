# MP-1 bounded Windows Job accounting probe

Test-only x86_64 Windows/MSVC workspace. It calls the production
`lab_windows_job::Job::{new,prepare,resume,poll_exit,retire}` path. The opt-in
`accounting-probes` feature adds read-only counters, completion-port diagnostics
and a bounded CPU allocation fixture inside the audited ABI capsule. Default
Workbench builds do not enable it. No Runtime client, GUI toolkit or GPU allocator
is added. Existing launch flags and process/IPC retirement gates are unchanged.

Run from the repository root with Rust 1.95.0/MSVC, Python 3 and PowerShell:

```powershell
git worktree add --detach target/mp-1/render-2a-source b05007fc64efdfc2e675fd485d5f5a9ef2a13b11
cargo build --locked --manifest-path target/mp-1/render-2a-source/scripts/support/wgpu-proof/Cargo.toml
cargo build --locked --release --manifest-path target/mp-1/render-2a-source/scripts/support/wgpu-proof/Cargo.toml
pwsh -File scripts/support/job-accounting/qualify.ps1 -BuildProfile dev -Directory D:/rust_projects/lab-runtime/target/mp-1/reproduce-dev -ProofExecutable target/mp-1/render-2a-source/scripts/support/wgpu-proof/target/debug/lab-wgpu-proof.exe
pwsh -File scripts/support/job-accounting/qualify.ps1 -BuildProfile release -Directory D:/rust_projects/lab-runtime/target/mp-1/reproduce-release -ProofExecutable target/mp-1/render-2a-source/scripts/support/wgpu-proof/target/release/lab-wgpu-proof.exe
```

Use new absolute output directories; preserve earlier attempts. The worktree is
the exact historical executable source, not a checkout of current shared GUI
modules. Rebuilt executables have their own recorded hashes and are not claimed
to be the historical binaries. Omit `-ProofExecutable` for CPU-only reproduction.

Each profile builds identical CPU code in console and Windows GUI PE subsystems.
The latter creates no GUI; it distinguishes the automatic console-host effect
without changing the capsule's creation flags. Five cases per subsystem select
process/Job limits of 16/16, 16/64, 64/16 and 64/64 MiB (1, 4 or 16 MiB requests).
They reserve 128 MiB, commit without access, touch only admitted pages, reach a
denial, decommit, release and exit normally. At most 64 requests/140 barriers,
96 MiB admitted fixture pages and 20 s child/22 s controller deadlines are used.
The graphical negative control retains the historical 16 MiB caps and 30 s
containment/35 s original-exit deadline. There is no automatic cap increase.

`accounting.json` retains suspended, child-barrier and original-exit samples;
graphical samples are periodic at 100 ms. Fields are bytes except explicit counts
and elapsed times. Each API read is sequential; the sample timestamp precedes
those reads. No timestamp is a scientific measurement. API failures remain
explicit, including diagnostic members that disappear while enumerating.

Independent readings use `K32GetProcessMemoryInfo` with
`PROCESS_MEMORY_COUNTERS_EX`; current private commit, its peak, working set and
pool quotas remain separate. `VirtualQuery` verifies the denied region remains
`MEM_RESERVE`. `QueryInformationJobObject` classes 9, 1 and 3 report exact limits,
peaks, counts and member PIDs. A diagnostic PID is opened read-only, checked for
membership in this exact Job, then queried and closed; it never replaces the
original process handle. Completion messages are best effort; IDs 9/10 distinguish
observed process/Job denials, not termination. Exit requires the original handle
to signal and `GetExitCodeProcess` to succeed; retirement and diagnostic-port
`CloseHandle` are separate receipts.

Compile `abi.cpp` in an x64 Visual Studio Developer Command Prompt:

```text
cl /nologo /W4 /WX /EHsc scripts\support\job-accounting\abi.cpp /Fe:target\mp-1\abi.exe /Fo:target\mp-1\abi.obj
target\mp-1\abi.exe
```

This independent Windows SDK witness reports structure sizes, alignment, offsets,
classes and flags; the feature-gated Rust capsule checks matching layouts at
compile time. The independent lock uses only exact registry versions/checksums
already present in the root lock and introduces no product dependency.

`verify.py` checks retained native observations, allocation failure, unchanged
live commit, uncommitted denied pages, release/exit/handle receipts, helper
membership and the graphic peak delta. It intentionally does not assert
`PeakJobMemoryUsed <= JobMemoryLimit`: the probe demonstrates why that is not an
admission guarantee. Its 64 KiB tolerance applies only to separate process reads
around receipt/JSON heap activity, never to limit readback. Memory values and
completion-message availability are Windows-build observations, not a portable
counter or deployment-limit policy.

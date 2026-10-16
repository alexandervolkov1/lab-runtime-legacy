# MP-1: Windows Job commit accounting investigation

Status: **OPEN, narrowed by reproducible native evidence**, pending independent
review in [Draft PR #51](https://github.com/alexandervolkov1/lab-runtime/pull/51).
`ACTIVATION_QUALIFIED=false`. No release, production activation or memory
deployment policy is selected.

The CPU-only experiment reproduces an aggregate peak above the configured Job
limit. Two effects are directly observed on Windows 10 build **19045.6691**:

1. A console-subsystem child launched with the existing `CREATE_NO_WINDOW` path
   acquires a second Job member, `C:\Windows\System32\conhost.exe`, after resume.
   It contributes about 6.5 MB of private commit. The exact readback still says
   `ACTIVE_PROCESS=1`. A Windows-GUI-subsystem build of the same CPU code has one
   member and needs no GUI/GPU initialization.
2. When the **Job** limit rejects `VirtualAlloc(MEM_COMMIT)`, both Job peaks and
   the independently queried process commit peak can advance by the rejected
   request size. The API returns `NULL`, last error is 1455, current private commit
   stays at its previous level, and `VirtualQuery` says the rejected pages remain
   `MEM_RESERVE`. A tighter **process** limit rejects the request without that
   request-sized high-water increase in these controls.

The second result also occurs with **one process and no conhost**, separating the
failed-charge accounting effect from helper membership. It is evidence for a
transient accounting charge followed by rollback, not evidence that the fixture
received usable pages above its Job admission limit. The internal kernel update
order is an inference; no kernel implementation trace is claimed.

The same-source RENDER-2A DX12 negative control reproduces both effects. This
explains a mechanism capable of producing the historical discrepancy, without
requiring an ABI error, a GPU allocator leak or a successful over-limit allocation.
The exact historical attribution is **not established retroactively**: old runs
did not retain member identities/current private commit or the denied allocation
trace, and the original NVIDIA/9600X machine is not this AMD/6600H machine. MP-1
therefore remains OPEN rather than converting a plausible historical explanation
into a recovered observation.

## Provenance and TIME-1 merge

PR [#50](https://github.com/alexandervolkov1/lab-runtime/pull/50) was merged with
ordinary merge commit **`30a20e3867e1bb08e2aff3405a7015266add70ed`** after checking
remote HEAD `f7c41c4638c5841f954fb6a522ca8d4fe9d85467`, base
`5c0730c8f827de9b326cabaf4176ae21d2e3308b`, a clean working tree and all **112**
canonical SHA256 entries in `time-1-sha256.txt`. The merge has those two parents
and the reviewed HEAD's exact tree. Main was fast-forward synchronized and clean
before branch `investigate/mp-1-job-accounting` was created at that merge.
See [merge/checksum receipts](mp-1-evidence/time-1-merge.json).

The final probe executable source is
**`e84101adf8e273ee8e4058c9898a184b0a3087a0`**. Earlier attempts are retained but
are not substituted for that final qualification. Its independent workspace is
[`scripts/support/job-accounting`](../../scripts/support/job-accounting/README.md).
The opt-in `accounting-probes` feature adds diagnostics inside the existing
audited `lab-windows-job` ABI capsule; production callers do not enable it.
The reservation owner rejects use after release, with a native regression.
Product launch flags, admission, original-process exit and retirement behavior
are unchanged; comments now distinguish aggregate/member peaks and admission.

For the graphic control, an isolated, clean, detached worktree of the exact
RENDER-2A executable source **`b05007fc64efdfc2e675fd485d5f5a9ef2a13b11`** was built
with its historical lock. No current Presentation/GUI code was backported into
that proof and no GPU source was edited. These are **newly built binaries**, not
the historical executable identities. [Six PE/binary identities](mp-1-evidence/binary-identities.json)
record SHA256, sizes, x64 machine and console/GUI subsystem values. The current
machine is Ryzen 5 6600H, AMD Radeon Graphics driver `32.0.21030.2001`, Windows
10 Pro 22H2 `19045.6691`, Rust/Cargo 1.95.0 x86_64 MSVC, 4096-byte system pages.
The controller reports membership in an outer Job; its identity/limits are not
claimed. Inner-Job limit messages and exact member checks identify this probe's
own Job; no breakaway or outer-policy change is attempted.

## Experiment and ABI

The probe uses the production `Job::new -> prepare -> resume` path: limits set
and read back, non-inherited handles, stable `PROC_THREAD_ATTRIBUTE_JOB_LIST`,
`CreateProcessW` with `EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED |
CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT`, original-handle membership and
limit rechecks before `ResumeThread`. There is no interval of uncontained user
code and no PID-based substitution for the original process.

The independent Windows SDK C++ witness was compiled with `/W4 /WX` using SDK
`10.0.26100.0` / MSVC tool directory `14.44.35207` (`cl.exe` file version
`19.44.35227.0`). Its
[ABI output](mp-1-evidence/abi.json) agrees with Rust compile-time checks:

| Item | x64 SDK / Rust |
|---|---:|
| Extended information size/alignment | 144 / 8 |
| Basic information size; flags/active offsets | 64; 16 / 40 |
| Process/Job limit offsets | 112 / 120 |
| Process/Job peak offsets | 128 / 136 |
| STARTUPINFOEXW size | 112 |
| PROCESS_MEMORY_COUNTERS_EX size; PrivateUsage offset | 80; 72 |
| Basic accounting / completion association sizes | 48 / 16 |
| Enforced limit flags | `0x2308` |
| JOB_LIST attribute / creation flags | `0x2000d` / `0x08080404` |
| Query classes: extended / basic / PID / completion association | 9 / 1 / 3 / 7 |

Every sample checks exact cap/flag/member readback. Extended/basic returned byte
counts are checked; the bounded PID list and its returned length are retained.
It enumerates the **exact inner Job**, opens diagnostic members read-only,
rechecks their membership, queries image/memory and closes those handles.
Disappearance/query failures remain explicit. These handles never certify the
original child's exit. The diagnostic completion port is associated before
creation and drained with bounded zero-time calls; messages are observations,
not reliable lifecycle gates.

Per profile, five cap/request combinations run both console and GUI-subsystem
CPU binaries: 16/16, 16/64, 64/16 MiB with 1 MiB requests; 64/64 MiB with 4 or
16 MiB requests. Every case reserves 128 MiB, commits without touching, writes
one byte per committed system page, reaches refusal, decommits, releases and
exits normally. Limits remain finite: 64 requests, 96 MiB admitted fixture pages,
140 barriers, child watchdog 20 s, controller termination at 22 s and original
exit deadline 27 s. GUI-subsystem CPU binaries create **no window**.

The parent samples before resume and at child file barriers; graph samples are
periodic at 100 ms, not allocation tracing. `Instant` elapsed microseconds mark
the start of sequential reads, not a simultaneous snapshot or measurement time.
`K32GetProcessMemoryInfo(PROCESS_MEMORY_COUNTERS_EX)` separately retains current
and peak private commit, current and peak working set, pool quotas and fault
counts. All memory fields are bytes. Address reservation, commit charge,
resident pages and VRAM are not interchangeable.

## New observations and comparison

The cleanest causal control has one GUI-subsystem CPU process, process cap
67,108,864 B, Job cap 16,777,216 B and a denied 1,048,576 B request:

| Profile | Private before | Private after refusal | Independent process peak / Job peak | Outcome |
|---|---:|---:|---:|---|
| dev | 16,392,192 | 16,392,192 | 17,440,768 / 17,440,768 | NULL / 1455; Job message 10 |
| release | 16,359,424 | 16,359,424 | 17,408,000 / 17,408,000 | NULL / 1455; Job message 10 |

The increase is exactly the rejected 1 MiB request, while denied pages remain
reserved. Process-limited 16/64 MiB controls instead report message 9 and remain
below the process cap. The console controls have two members; the 64/64 MiB,
4 MiB-request case shows the same Job-refusal effect with a larger request.
Reserve-only succeeds above either commit cap without a 128 MiB commit increase.
Touch increases resident working set; decommit removes the admitted private
commit; release removes the address reservation. Raw phase/counter receipts
preserve the before/after values, not just this summary.

The historical and newly built **16/16 MiB** DX12 negative controls are:

| Run | PeakProcessMemoryUsed B | PeakJobMemoryUsed B | Difference B | Exit |
|---|---:|---:|---:|---:|
| historical dev, NVIDIA machine | 15,032,320 | 21,614,592 | 6,582,272 | 92 |
| historical release, NVIDIA machine | 14,954,496 | 21,524,480 | 6,569,984 | 92 |
| new dev, AMD machine | 13,746,176 | 20,307,968 | 6,561,792 | 92 |
| new release, AMD machine | 13,688,832 | 20,254,720 | 6,565,888 | 92 |

In the new samples, the difference **equals the independently queried conhost
private commit exactly**. Process peak also equals the original child's
`PeakPagefileUsage`. Its current private commit at the first failure sample is
only 2,498,560 B dev / 2,416,640 B release. Each run observes Job-limit message 10,
enters child code and records the same DX12 no-adapter error before publishing
a GPU root. At 30 s the controller requests termination, observes original exit
92 via its retained handle, then retires Job/process/thread objects to zero and
separately confirms diagnostic-port `CloseHandle`. A memory notification did not
terminate the child. No successful render or GPU-root cleanup is claimed by this
negative control.

The historical archive remains byte-for-byte SHA256
`872d21e4a5613c44735442cf8f334ae339c38392b78a9e00e897dab309a619c9`.
[Historical records](mp-1-evidence/historical-control.json) retain original paths,
per-record hashes, limits, entry/failure and exit/retirement receipts. A historical
6.5 MB delta is consistent with the newly measured helper contribution, but its
exact helper identity and failed-request size were not originally captured.

## Microsoft contract and the remaining uncertainty

[Extended Job limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)
distinguish per-process and aggregate caps and define the process peak over any
associated process. It is not an original-child counter. That page does not
specify whether a rejected charge can enter the high-water marks.
[Basic limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information)
describe memory admission failure and separate process/Job limit notifications.
These support the successful-vs-denied allocation distinction; they do **not**
document the transient peak update order observed here. Nor do they explain the
observed OS helper exceeding the active-count setting. Those two implementation
details are not promoted to universal Windows guarantees.

[VirtualAlloc](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualalloc)
separates reserving addresses from charging commit and from materializing pages
on access. [Process counters](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex)
distinguish commit, its lifetime peak and resident working set. These independent
APIs are why a large reserve or a high peak is not evidence of usable committed
pages above admission. [Process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)
describe `CREATE_NO_WINDOW` and `CREATE_SUSPENDED`; hiding a console window does
not document a promise of one accounting member.
[Console definitions](https://learn.microsoft.com/en-us/windows/console/definitions)
identify conhost as the Windows console server. The PE-subsystem control supplies
the empirical connection to this launch path; the documentation alone does not.

[Completion-port semantics](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_associate_completion_port)
make ordinary hard-limit messages best effort and warn about PID lifetime.
Their absence cannot prove non-occurrence; messages cannot replace original
handle waits. [JOB_LIST](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
specifies assignment during creation and attribute-storage lifetime. Existing
readback/membership gates agree with that contract. Pre-association operations
are outside retroactive examination by
[AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject);
this is not used as an explanation for the explicitly post-initialization CPU
refusals measured here.

No product enforcement defect was found in ABI, flags, calls, atomic Job
assignment, readback or resume order. The clarified implementation contract is
to install/check Windows commit-admission settings and preserve exact process
containment/retirement, **not** to guarantee that OS peaks, RSS, address space,
all OS-helper behavior or VRAM fit those numeric settings. It remains one owned
GUI process record and one Runtime client/scope; Job accounting can include an
OS helper. Closing application-owned handles is not a GPU-memory cleanup claim.

MP-1 stays OPEN because exact historical causal attribution and the undocumented
helper/peak-accounting behavior have not been independently confirmed. Missing
data are a member/commit/allocation trace on the original NVIDIA configuration
and a primary Microsoft explanation or kernel trace of helper admission and
failed-charge high-water accounting. The new finite probe and retained controls
provide a concrete starting point. Raising a cap, suppressing an assertion,
changing renderer or renaming a counter is not the proposed resolution.

## Verification, reuse and reproduction

New dev/release results: 10 CPU cases and one historical-source graphic negative
control per profile, all observation checks passing; capsule tests **2/2** per
profile (including native pipe and released-reservation regression); Supervisor
**7/7** and private IPC **22/22** per profile; capsule and probe all-target,
all-feature Clippy with `-D warnings`; workspace/probe fmt and diff check; SDK
ABI comparison; six binary/PE identities; dependency-lock and source comparisons.

[Source proof](mp-1-evidence/unchanged-source.json) checks 379 app/host/proof/Cargo
paths against merged TIME-1. Compared with TIME-1's qualified source
`5eaef186b6a7cf5582e77dafd90b3cd5351e7b5f`, only the host README differs in those
paths. The default capsule Windows implementation differs only in comments and
the false-by-default diagnostic module declaration. Therefore the full TIME-1
virtual Runtime/API replacement, stock WGPU/Glow dev/release, crash/restart and
Custom Glow evidence is **reused**, not described as newly run. The new native
Supervisor/IPC tests check original-exit, failed poll/termination, pending I/O,
retirement and restart gates again. Runtime, OutputAuthority, Recorder and public
Application/Workbench APIs are unchanged. No instrument connection was opened.

Follow the [probe commands and bounds](../../scripts/support/job-accounting/README.md)
with new absolute directories. `qualify.ps1` records source cleanliness,
environment and exact binary identities before verification. `verify.py` checks
the actual retained observations. It does not replace failed allocations with
successful results or apply a tolerance to cap readback.
[Gate logs/index](mp-1-evidence/gates.json), [canonical source inputs](mp-1-evidence/source-inputs.json),
[dependency review](mp-1-evidence/dependency-review.json), final summaries and raw
phase receipts are retained with a per-member SHA256/CRC/length manifest.
Preliminary runs and unsuccessful setup/audit attempts are labelled separately;
none is removed to obtain the final passing result. Exact source/evidence hashes
are listed in `mp-1-sha256.txt` (raw Git blobs, excluding that manifest itself).

The [raw archive](mp-1-evidence/raw-evidence.zip) contains **3,138** members,
including final and preliminary phase receipts, original gate logs and metadata.
It is **1,271,276 bytes**, SHA256
`ad57b6aa1bd1acdaf70c7c85913db5b9cdcbfee702561765700f2c12cf8cc77c`;
the [manifest](mp-1-evidence/raw-manifest.json) rechecks every member's length,
CRC32 and SHA256. Executable hashes identify retained local binaries; executable
files/build caches are not embedded in this evidence archive. Raw archive hashes
refer to original bytes; the separate canonical manifest refers to Git blobs.

This is an independent-review Draft. Deployment limits for stock WGPU/Glow,
other Windows builds, process-host policy changes, GUI controls, hardware
acceptance, M-CLEAN and activation remain outside this work.

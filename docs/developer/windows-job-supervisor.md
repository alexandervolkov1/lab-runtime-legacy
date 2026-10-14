# DGUI-4C7B3B2: Windows Job supervisor

Inactive production supervisor seam and Windows/MSVC no-GPU qualification.
ACTIVATION_QUALIFIED=false. This stage adds no renderer, GUI child host, production
IPC, snapshot bridge, Runtime operation or public API/DTO.

PR #37 was verified unchanged at 67211d1ae95e1b35f49c79c6c804ef57298fc0b0, with
main base 14bbe58aa54ef6b2320a6d11ccfb98cf2ee67697, made Ready and merged as
2aaf98fcd77a8b6bf73b2341d0b95298be401df6. The reviewed HEAD is its second parent.
Main was synchronized and clean before feature/dgui-4c7b3b2-windows-job-supervisor.

## Ownership and seam

[ParentPump](../../apps/lab-workbench/src/parent.rs) can install one optional
[Supervisor](../../apps/lab-workbench/src/supervisor/mod.rs). A second installation
is rejected even if the first worker stalls. ParentOwner and the Application
worker never create/wait/join a child or borrow a live OS backend. The optional
composition seam is not called by active v1, which remains cooperative.

~~~mermaid
flowchart LR
    A[Workbench API callers] --> M[One ParentPump / ParentOwner / dispatcher]
    M --> W[One Application worker]
    W --> R[Runtime: sole experiment and OutputAuthority owner]
    P[Parent root controller] -->|one bounded pending request| S[One supervisor OS worker]
    S --> J[One unnamed Job and original process/thread handles]
    J --> C[No-GPU fixture owner loop]
    C -->|bounded fixture challenge response| S
    S --> L[One latest status snapshot]
    L --> P
~~~

The worker owns one mutable lifecycle record and one WindowsBackend. The shared
latest-status slot is an immutable metadata snapshot, not a second writable model.
There is one last terminal diagnostic inside that bounded record, no history or
event queue. Status/error records contain fixed scalars and static operation names.
No Runtime client, sequencer, recovery journal, presentation or observations are
transferred to the child. Environment is SystemRoot-only; handle inheritance is
disabled. This is trusted-code containment, not a malicious-code security sandbox.

Root controller creates the sole OS worker outside model turns. Backend factories
must be inert: the production WindowsBackend default creates no native resource.
All Job/process creation, queries, resume, termination, file I/O and retirement
execute on that worker. The model-facing request/status methods use try_lock;
contention returns Busy. They perform no blocking OS waits or joins. A stalled
operation leaves its call/phase/incarnation and last progress timestamp observable.
Status becomes degraded after the monotonic observation deadline; no new worker
or launch is created to bypass it.

## Atomic native startup

[lab-windows-job](../../crates/lab-windows-job/src/lib.rs) is a local dependency
with no third-party dependencies; its [license/safety review](windows-supervisor-license-safety.md)
explains the isolated FFI and unchanged Workbench unsafe prohibition.

1. Create unnamed, non-inheritable Job. Configure KILL_ON_JOB_CLOSE,
   ACTIVE_PROCESS=1, PROCESS_MEMORY and JOB_MEMORY with explicit caller limits.
2. Read back exact limits and handle flags. Breakaway is never enabled.
3. Build aligned bounded attribute storage with a stable JOB_LIST value.
   CreateProcessW uses JOB_LIST, CREATE_SUSPENDED, a sanitized Unicode environment,
   explicit executable/cwd and disabled inheritance. No Assign/spawn fallback.
4. Store original process/thread handles before post-create checks. Check their
   non-inheritance, actual Job membership and limits while suspended.
5. Consume stop/root-close and roll back an expired startup before ResumeThread.
   Recheck membership/limits at resume; its previous suspend count must be one.
   Release primary thread handle checked.
6. Any post-create failure retains the exact process for one rollback request and
   original-handle observation. No suspended child is abandoned or resumed.

The fixture-only cap is 33,554,432 bytes per process/Job. The external parent-death
guard uses separate 64/128 MiB limits for two test processes. Neither is deployment
cap C. C remains unselected until actual renderer-child measurements and review.

Windows 10+ JOB_LIST support and nested constraints are required. Unsupported
attributes, denied Job creation/configuration/query or incompatible inherited
limits fail closed. Current real acceptance is Windows 10 Pro 19045 x64/MSVC;
unsupported-attribute/access/read failures are additionally fault injected. This
is not qualification of every Windows version or every nested-Job configuration.
See [creation attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)
and [nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs).

## Lifecycle and proof

~~~mermaid
stateDiagram-v2
    [*] --> Stopped
    Stopped --> Starting: explicit request, no old obligation
    Starting --> Ready: exact no-GPU challenge / advancing owner counter
    Starting --> Stopping: stop, startup failure or observation deadline
    Ready --> Stopping: explicit close, heartbeat or progress deadline
    Stopping --> TerminationRequested: one termination request
    Stopping --> ExitedHealthy: healthy fixture close plus original exit and retirement
    TerminationRequested --> ExitedContained: original exit plus retirement
    TerminationRequested --> ExitUnconfirmed: error or observation deadline
    Ready --> ExitedContained: unexpected original-process exit plus retirement
    ExitUnconfirmed --> ExitedContained: later original proof and retirement
    ExitUnconfirmed --> ExitUnconfirmed: retained obligation, replacement forbidden
    ExitedHealthy --> Starting: explicit restart
    ExitedContained --> Starting: explicit restart
    Starting --> Stopped: failed before any process, Job retired
~~~

Restart is explicit and admitted only from Stopped/confirmed terminal states.
Active, pending-stop, concurrent/full-slot and unconfirmed requests are rejected.
There is no automatic restart loop. Accepted stop replaces the pending restart;
an immediate stop cannot be downgraded by a repeated graceful request. Irreversible
parent shutdown has an atomic latch, rejects late restarts and requests stop even
when the metadata mutex is busy.

TerminateJobObject success is only request acknowledgement. The original process
handle must be signaled and GetExitCodeProcess must succeed. Neither elapsed time,
fixture/channel closure nor a healthy-close packet is an exit receipt.
[Microsoft termination semantics](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject)
do not upgrade process termination to per-GPU-object cleanup.

Before handle retirement finishes, status remains Stopping with the outstanding
call and ledger; replacement is rejected. ExitedHealthy/ExitedContained and final
diagnostic are published together only after checked retirement. The deterministic
retirement barrier protects this publication order. Counts during an in-flight OS
call are the last published counts, not proof that a partially created object is
absent. Starting and an unfinished worker always prevent successful root shutdown.

Native query/termination/wait failures preserve original startup/operation error
independently from cleanup error. ExitUnconfirmed retains one process/Job episode
and continues original-handle observation; no termination retry or second worker.
An unresolved handle-retirement failure keeps its remaining ledger and blocks
replacement without blind repeated close. Operator/OS recovery can be required.

ParentPump shutdown requests supervisor shutdown without waiting for its OS calls.
It preserves model/client failure first, and returns unconfirmed until child
retirement and supervisor worker completion are observed. Repeated shutdown stops
the client once. Drop requests the same asynchronous stop; if OS progress remains
unconfirmed, the sole worker capture continues owning that episode. It supplies no
success receipt and uses neither mem::forget nor forced Rust-thread termination.
Parent process death closes its kernel Job handles; external original-process
witnesses prove fixture containment separately.

## Watchdog and no-GPU protocol

Default application policy: challenge 250 ms, startup/progress observation 2 s,
graceful close 2 s, post-termination exit observation 2 s. The worker checks one
bounded turn at a time, with a finite Condvar wait; no busy-spin or missed-tick
replay. These are observation deadlines between calls, not OS/driver execution
bounds or a universal parent/child thread-exit guarantee.

The only implemented channel is the explicit no-GPU file fixture adapter:
one 64-byte maximum challenge file and one 96-byte maximum response file.
Incarnation, exact challenge and advancing owner counter are validated. A repeated
ack does not refresh heartbeat; a helper's fresh pong with unchanged counter does
not refresh owner progress. Healthy close also requires an advancing owner counter.
Freshness, process liveness and original-handle exit are distinct status facts.

The [fixture](../../scripts/support/windows-supervisor-fixture.rs) can freeze its
owner and let a helper continue answering. It remains OS-alive and heartbeat-fresh,
but the progress deadline requests containment. No renderer completion is inferred.
Ready/ExitedHealthy here describe this no-GPU protocol, not an admitted GUI snapshot
or a checked GL cleanup receipt. Named-pipe IPC, real renderer heartbeat and snapshot
bridge require subsequent independent stages.

## Failure matrix

| Scenario | Evidence/outcome | Replacement |
| --- | --- | --- |
| Normal no-GPU close | Advancing close receipt, original exit=0, zero handles; ExitedHealthy | Explicit only |
| Forced termination | One request, original exit=77, zero handles; ExitedContained | After proof |
| Missing executable | CreateProcessW failure, no process, Job retired; Stopped + diagnostic | Explicit only |
| Stop/post-create failure while suspended | Exact original rollback, executed marker absent, exit witnessed | After proof |
| Startup call returns after deadline | Rollback before ResumeThread; executed marker absent, original exit witnessed | After proof |
| Crash | Original exit=31; ExitedContained | After proof |
| Owner hang/startup silence | Deadline, graceful window, one termination and original exit | After proof |
| Helper pong, frozen owner | Heartbeat fresh, owner counter unchanged; progress failure then containment | After proof |
| Commit denial | 64 MiB allocation denied under 32 MiB fixture cap, error 1455, fixture exits 90 | After proof |
| Descendant spawn | ACTIVE_PROCESS=1 denies actual CreateProcess, error 1816 | No second child |
| Parent death | Inner last Job handle closes; original child witness signaled | Parent recovery is separate |
| Incompatible outer active-count Job | Atomic inner child creation rejected, no executed child | No fallback |
| Unsupported/denied Job setup/read | Injected fallible outcomes preserve error; no child/resume | No fallback |
| OS call stalled | One worker/episode, degraded status; parent API/model still progress in barrier fixture | Forbidden while pending |
| Termination ack without exit proof | ExitUnconfirmed; actual guardian proves fixture containment, supervisor proof withheld | Forbidden |
| Wait/retirement failure | Remaining obligation retained; later original exit can resolve wait failure | Forbidden until proof |
| Unexpected inert factory exit | WorkerPanic / ExitUnconfirmed; no replacement worker | Forbidden |

## Memory scopes

| Scope | Controlled bound / interpretation |
| --- | --- |
| Preview CPU | Unchanged 8,388,608 accounted bytes, historical CSV/Widget/Page gates |
| Supervisor ownership | One worker, Job, original process/thread record, latest status, request, last terminal diagnostic |
| Launch descriptor | At most eight arguments; string capacities <=4,096 bytes, executable/directory capacities <=1,024 bytes; UTF-16 token cap 1,024 |
| Native startup scratch | <=4,096 attribute bytes; command UTF-16 capacity 9,243 units; app/cwd 1,025 units each; environment 1,040 units; checked UTF-8 quoted command |
| Overlap during launch | One admitted descriptor and its directory copy; UTF-8 command <=33,818 bytes simultaneously with the UTF-16 and attribute buffers; no snapshot copy |
| Fixture data | Challenge <=64 bytes, response <=96, parser read <=97; fixed files, no history/snapshot copies |
| Fixture parse scratch | At most 48 whitespace fields from 96 bytes, bounded format/read allocations; allocator capacity/slack is not a whole host receipt |
| OS commit | Explicit process/Job caps, read-back peaks recorded independently; denial is not automatic process kill |
| Kernel/thread/native caches | Not a proven Rust allocation budget; active count does not cap thread count |
| Driver/VRAM/GPU lifecycle | No GPU loaded in fixtures; no new qualification |

Metadata sizes and observed process/Job peaks are in
[evidence](windows-job-supervisor-evidence.txt). Payload/capacity bounds describe
controlled storage and work; they do not include all allocator slack, thread
stacks, filesystem/kernel buffers or native libraries. Whole host Rust budget
remains BLOCKED; no Preview quota or numeric policy is increased.

## Qualification and remaining stages

The [native runner](../../scripts/test-windows-supervisor.ps1) compiles the fixture
against the exact local capsule and real supervisor source, then runs six real
process groups. Deterministic unit/barrier tests cover pending request priority,
wait/delete uncertainty, startup errors, duplicate/wrong-incarnation progress,
retirement publication, inert factory panic, root shutdown and independent parent
API/recovery progress. Test-only process witnesses are read-only and non-inherited.
All real negative scenarios assert original process exit or a failed atomic
creation with no execution, before returning; unresolved proof suppression is
explicitly simulated and guarded by a real original-handle witness.

Parent turn limits stay at 64 FIFO updates/rebuild steps and at most four distinct
external callers per fair owner turn. The 100 ms idle check and measured small
64-update fixture are scheduling observations, not a Windows latency guarantee.
Supervisor observations add no work to ParentOwner::turn. The final suites contain
523 tests: 502 enabled and 21 ignored; 13 existing virtual process fixtures and six
new native process groups were run explicitly, with the two historical M17.4
fixtures left separately BLOCKED.

~~~powershell
./scripts/test-windows-supervisor.ps1 -BuildProfile dev
./scripts/test-windows-supervisor.ps1 -BuildProfile release
cargo test -p lab-workbench --locked -- --test-threads=1
cargo test -p lab-workbench --release --locked -- --test-threads=1
cargo test -p lab-workbench --locked -- supervisor::tests parent::tests --test-threads=1 --nocapture
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
cargo clippy -p lab-workbench -p lab-windows-job --all-targets --all-features --locked -- -D warnings
~~~

Exact tested source HEADs, inventory, native and regression results are in
[evidence](windows-job-supervisor-evidence.txt) and [SHA256 inventory](windows-job-supervisor-sha256.txt).
The final evidence commit changes no Rust/Cargo/probe inputs.

Existing virtual v1 native smoke was also repeated in both profiles using its
automated Confirm workflow. Published JSON audits and inspected window captures
are linked in the evidence. One optimized attempt passed the GUI workflow but
failed the observer's exact two-session assertion: it observed a third complete
hello-only session and three GUI EOFs, one mutation and no status/retry. The
unchanged repeat passed. The extra connection's cause remains unqualified; neither
production GUI nor that observer assertion was changed. Captures are GDI client
images, not GL readback or a mouse/keyboard Confirm test.

M17.4 Recorder overlap remains the separately reproduced B3A baseline BLOCKED
case; its fixture/assertions/Recorder are unchanged. Terminal GPU lifecycle and
whole host Rust budget retain previous BLOCKED scopes. No GPU loss/driver stall,
Linux backend or arbitrary Windows/nested-Job profile is qualified. C remains
unselected. Required next reviews concern private IPC/cancellation, renderer
heartbeat/GL integration, real child memory measurements and opt-in virtual host
selection. None is started here. Runtime, OutputAuthority, public APIs/DTOs, B1,
FiniteTraceItem and active v1 operations are unchanged. No merge, release or B3B3.

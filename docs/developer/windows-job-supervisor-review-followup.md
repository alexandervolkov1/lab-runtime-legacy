# DGUI-4C7B3B2 independent review follow-up

Draft PR #38 remains unmerged. ACTIVATION_QUALIFIED=false.
Reviewed HEAD: `76471bebdcb147148c34fcff23fbf2cba686f120`.
B3B1 merge: `2aaf98fcd77a8b6bf73b2341d0b95298be401df6`.
Follow-up tested source: `c72e4440d5258679a8313e9894211948294373f7`.
The later report/evidence commit changes no Rust, Cargo or probe inputs.

## Lost accepted Restart

Previously `take()` emptied the pending slot before `start()` published Starting.
The initial `backend.retire()` could block with Stopped/confirmed terminal still
published. A second caller could successfully enqueue R2. The post-prepare
Stop-only match then consumed and discarded R2. This was a lost accepted command,
not merely delayed status publication.

Commit `53860cc` adds one fixed `Shared.launch_in_flight` flag. Dequeuing Restart
sets it under the admission mutex. Restart rejects that reservation as Active;
pending Stop still takes priority. The first subsequent publication writes the
new phase and clears the reservation under that same lock. There is no extra
queue, thread, process record or deferred launch. Shared inline size changes from
336 to 344 bytes; Status remains 232 and Worker 424 bytes on this MSVC build.
These are structure sizes, not a whole-host allocation claim.

`dequeued_restart_reserves_launch_during_initial_retire_and_stop_keeps_priority`
uses a real Supervisor worker and controlled fake Backend. A Barrier stops the
initial retire after R1 was taken, while the published phase is still Stopped.
R2 must be rejected. Four paths then observe ordinary Ready/termination, Stop,
root shutdown, or suspended startup failure. Each has one prepare/one obligation,
the cancelled paths have zero resumes, and the original exit proof gates terminal
publication. Only a new explicit Restart after retirement creates incarnation 2.
No sleep establishes event order.

## First containment after observation failure

Previously a `poll_exit()` error published ExitUnconfirmed, and `begin_stop()`
returned immediately for that phase. Therefore both an accepted explicit Terminate
and irreversible root shutdown could do nothing to a still-owned live child.
Uncertainty about exit was incorrectly treated as evidence that containment had
already been requested.

Commit `b19bc43` allows the first containment request from ExitUnconfirmed on
explicit Terminate or root shutdown, using the existing `termination_requests`
and original backend ownership. It preserves ExitUnconfirmed through a successful
request, the original error, and the same incarnation/Job. Termination errors use
cleanup diagnostics. Re-observing the original poll error does not fill that
cleanup slot. Repeated Terminate/root turns cannot send another request. No PID
lookup, process reopen, automatic replacement or extra worker is introduced.
Already confirmed exit and failed handle retirement do not initiate containment.

`observation_failure_allows_one_explicit_or_root_containment_until_original_proof`
observes Ready, injected poll failure, ExitUnconfirmed with zero requests, one
explicit/root request, repeated-request suppression and replacement refusal.
It covers both successful and failed termination. A retirement Barrier proves
that root completion remains false even after original exit is known, until
handle retirement finishes. Failure retains exactly one unresolved obligation;
external containment/recovered proof resolves it without a retry.

The seventh native no-GPU group exercises the explicit and root paths with a
real Windows Job. An independent witness duplicated from the original process
handle first proves the child is alive, then confirms exit 77 after the sole
real Job termination request. Supervisor observation failure is injected; after
it is removed the same original handle supplies proof, and gated retirement
precedes worker completion. This does not simulate a real driver-stalled kernel
termination and does not change any production API.

## Qualification

Windows 10 Pro 10.0.19045 x64; Rust/Cargo 1.95.0, native MSVC; locked
dependencies. Both profiles ran on the exact tested source above.

| Gate | Dev | Release |
| --- | --- | --- |
| Complete Workbench suite | 504 passed, 22 ignored; 156.49 s | 504 passed, 22 ignored; 7.06 s |
| Inventory | 526 tests | Same 526 names |
| Parent/supervisor/GUI/architecture selection | 41 passed | 41 passed |
| Existing non-M17.4 virtual process acceptance | 13 passed | 13 passed |
| Six existing native groups plus original-witness poll-failure group | 7 passed | 7 passed |
| Native v1 ordinary smoke, strict observer | PASS, 2 connections | PASS, 2 connections |
| Five borrowing compile-fail gates | PASS | PASS |
| Complete CPU Preview probe | PASS; 527.144 s including wrapper | PASS; 59.499 s including wrapper |
| Clippy, all targets/features, `-D warnings` | PASS | PASS |
| Runtime/Workbench builds and local capsule unit/doc checks | PASS | PASS |

Inventory names SHA256:
`dbf27ca9fbbb41e741929af6baa462c7ff945298111f3abe93a562e3ea36e8e7`
(sorted names, UTF-8 with LF).
Formatting, standalone fixture formatting, PowerShell AST, Python parsing,
protected-source equality, local documentation links and diff checks accompany
the raw evidence. No source-input change is hidden behind a documentation HEAD.

Geometry (119 cases), memory (125), frames (12), no-upload (4), pending retirement
(9), and Widget/Page/Unicode tables preserve the recorded accounting. Maximum
Preview charge remains **4,610,337 / 8,388,608 bytes**. CSV comparison preserves
all columns except the scheduling-dependent `wait_calls`; raw values remain
available. The pending table matches `TERMINAL_GPU_PENDING_*`, including 1664
inline host bytes. Older `FALLIBLE_RETIREMENT_PENDING_*` has 1496: commit `e2eb543`
added deletion diagnostics before B3B1. That historical difference is explicitly
recorded, not attributed to this supervisor fix or erased from old evidence.
Neither a CPU allowance nor whole-host/GPU qualification is expanded.

The [raw evidence directory](windows-job-supervisor-review/) contains command
receipts, the complete inventory, exact Git-blob and tested working-byte hashes
for 318 source inputs, CPU comparisons, binary snapshots' hashes, and the session
summary. `raw-evidence.zip` preserves unedited stdout/stderr, per-session audits,
smoke results, captures and generated probe evidence; `raw-manifest.json` checks
every archived byte. The original review evidence remains separately marked
historical in [windows-job-supervisor-evidence.txt](windows-job-supervisor-evidence.txt).

Source identity is not a claim of byte-identical rebuilt executables. The initial
smokes used the joint Runtime/Workbench build; the CPU wrapper subsequently
rebuilt Workbench independently at the same source HEAD, with different binary
hashes. Both executed hashes are retained. Additional strict native smokes passed
on the exact preserved final dev/release snapshots, and the verifier matches
those executed bytes to `binary-snapshots.json`. Baseline binaries have their own
source HEAD and matching execution hashes. No executable is released here.

An initial full-suite attempt at `f359fcc22351f48c9f0fe7aa82c9b03535763920`
exposed a test-observation race: TerminationRequested is published before the
backend call, so its counter could correctly still be zero. The run was stopped
after that failure and retained as failed evidence. Commit `c72e444` makes the
test wait for the completed worker turn before inspecting backend counters;
production code and lifecycle assertions are unchanged. This preserves the
distinction between a published request and call completion.

The first CPU-wrapper attempts used Windows PowerShell 5, which treated the
expected borrowing compiler stderr as a terminating NativeCommandError. These
failed harness runs are retained. Complete reruns using the already installed
PowerShell 7.6.6 pass both profiles without changing the borrowing assertions,
Rust toolchain, dependencies or probe sources.

## Hello-only session investigation

The historical
[release failure audit](windows-job-supervisor-v1-release-observer-limitation.json)
contains three client hellos and three GUI EOFs, one intentional mutation and no
status/Exact Retry. It records neither Runtime hello replies nor connection-state
timestamps. Attribution of that exact attempt remains **UNQUALIFIED**; a passing
repeat or a similar controlled reproduction cannot recover missing evidence.

The inspected path has one ClientHandle created by ParentOwner. The existing GUI
smoke transitions through AwaitRestored, Disconnect, AwaitStale, explicit Connect
with the retained scope, and ReattachedFresh. The disconnect fence cancels retry
and drains/rejects old commands before acknowledging Disconnected. A new manual
retained Connect starts one bounded reattach episode. `scope_in_use` in its hello
response invokes `reset_transport_only()`, schedules a retry within that same
absolute deadline, and preserves the scope. A second accepted Application
attachment is forbidden by Runtime SessionStore until the old connection detaches.
The following successful hello still requires the authoritative rebuild before
Fresh. None of those production files differs between B3B1 and this follow-up.

The mapping below follows that unchanged worker control flow; the wire frames
and per-connection monotonic times are observed, rather than newly instrumented
internal ClientHandle transitions:

| Observed event | Existing ClientHandle path | Application meaning |
| --- | --- | --- |
| First GUI EOF after explicit Disconnect | Disconnect fence clears retry/reattach, closes its one transport, acknowledges Disconnected | Old Runtime attachment may still await detach processing |
| Retained-scope hello on connection 2 | ManualRetained episode; Reattaching then AwaitingHello | Attempts the original scope; does not allocate another scope |
| `scope_in_use`, `accepted=false` | Reset transport only; Reattaching; retry in the same absolute deadline | No second accepted attachment and no mutation/status/Exact Retry |
| Same-scope hello result on connection 3 | AwaitingHello then Ready; authoritative rebuild follows | Original scope resumes after detach; Fresh is separately observed |

The observer now records per-connection request indices, monotonic open/close
times and hello responses, within its existing four-connection/256-request
bounds. An opt-in negative experiment holds the first Runtime-side connection
after GUI EOF until the next hello receives `scope_in_use`. This provides a
controlled test of delayed detach; it preserves the strict exact-two-session
assertion and must be reported as a failed smoke assertion, not counted as PASS.
Both native profiles were rebuilt at the exact B3B1 merge and at the follow-up
source, with binary SHA256 snapshots and explicit observer/binary HEADs. All six
ordinary runs (two baseline, four current including the final snapshots) passed
the unchanged strict assertion with two connections. All
four delayed-close experiments failed that assertion with three connections,
while the embedded GUI workflow passed. Each intermediate connection contained
only hello and received `scope_in_use` with `accepted=false`; the next successful
hello returned the original scope. Every run had one intentional mutation and
zero status/Exact Retry/repeated mutation submissions.

This classifies the controlled extra connection as an allowed retained-scope
reattach of the existing worker, not a duplicate accepted Application session.
It reproduces on B3B1 as well as B3B2. It does **not** conclusively identify the
unrecorded response in the historical attempt, which remains UNQUALIFIED. The
strict observer failures and that original audit remain available; no assertion
was changed to accept three connections. No production reconnect policy changed.

## Unchanged boundaries

Runtime, OutputAuthority, Recorder, public APIs/DTOs, B1, FiniteTraceItem and the
existing GUI v1 are unchanged. There is no B3B3/private IPC, snapshot bridge,
renderer child, GL integration, deployment cap selection, v2 activation or new
operator/automation API. Historical M17.4 Recorder overlap, terminal GPU lifecycle
and whole-host Rust budget remain BLOCKED in their previous scopes. No release,
merge or next-stage work is authorized by this report.

**READY FOR INDEPENDENT REVIEW** within the B3B2 no-GPU supervisor scope.
The two reported lifecycle blockers are fixed and qualified in dev/release;
the historical hello-only audit remains separately UNQUALIFIED. Keep PR #38
Draft. ACTIVATION_QUALIFIED=false.

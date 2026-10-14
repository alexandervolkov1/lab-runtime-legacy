# DGUI-4C7B3A: native GUI host decision

Status: **proposed for independent review**, Windows-first architecture preflight.
Recommendation: **B, one isolated GUI child supervised by Workbench**. This PR
does not implement that host, change public contracts or activate GUI v2.
`ACTIVATION_QUALIFIED=false`.

## Context and requirements

PR #35 was checked at `e2eb54300aa7c9908881bc80c901afc272aa2f4f`, made Ready and
merged normally as `145983461cb8c6754f43a8a221b36230b3bd6ef0`, preserving the
reviewed commit as second parent. Main was synchronized and clean before branch
`feature/dgui-4c7b3a-native-host-architecture`.

[B2B2 evidence](../../apps/lab-workbench/src/gui/readonly_v2/TERMINAL_GPU_OWNERSHIP.md)
qualifies actual rendering, healthy own-resource lifecycle and logical GPU
payload bounds. Unresolved completion/deletion, final WGL destruction and whole
native Rust allocation bounds remain BLOCKED. In particular, pinned glutin's
WGL Drop does not expose a checked final destruction result. Manual destruction
of a glutin-owned context would violate its ownership.

The operational requirement is to keep experiment authority independent of
renderer progress, contain a failed renderer, retain the Workbench document and
recovery state, and restart display without replaying control commands. It does
not require a fabricated per-object GPU cleanup receipt after forced termination.
If an absolute driver-time bound, exact VRAM ceiling or checked cleanup after
every device loss is required, **neither option satisfies it**.

## Options

| Concern | A: native v2 inside Workbench | B: supervised GUI child |
|---|---|---|
| Normal close | Existing checked own-object release; final WGL destruction still unobservable | Same healthy GL path in child; parent separately observes process exit |
| Unresolved deletion/device loss | Workbench must retain renderer root; safe in-process teardown is not qualified | Child retains root; parent may request OS termination and observe exit; no GPU cleanup receipt |
| Forced termination | A GL owner thread cannot be safely force-killed; killing Workbench loses presentation/API/client continuity | Terminate only the GUI Job; Workbench model/API/worker can continue |
| Recovery | A stuck root blocks replacement; whole Workbench restart changes its identity/session boundary | New display incarnation after confirmed old exit; parent document and Runtime recovery survive |
| Memory and queues | Native caches share Workbench address space; Job memory cap would also constrain its client/model | Separate child commit cap; application IPC and parent queues still need explicit bounds |
| Window/input/DPI | Direct native event integration; less IPC | Child owns top-level windows, IME, focus, DPI and raw input; parent receives bounded semantic state only |
| Runtime progress | GPU code and model pump share the current GUI host unless refactored | Requires an independent parent model pump and supervisor; child backpressure cannot wait on the Application worker |
| User model | One Workbench process/window | One logical Workbench/workspace/API, with a replaceable display window; no second presentation owner |
| Complexity | Fewer processes, unresolved terminal contract remains | Additional supervisor, private IPC, snapshots, packaging and fault tests; conventional OS mechanisms |
| Windows | Familiar eframe/glutin seam, same driver limitations | Job Objects, creation-time assignment, process handles and overlapped local IPC |
| Linux later | Native context problem remains | Same ownership/protocol; separately qualified cgroup/process-lifetime backend, different memory semantics |

## Decision and consequences

Select B for subsequent **small, independently reviewed** implementation PRs.
Keep the Workbench Application worker, dispatcher, presentation owner and recovery
in the parent. Move only display ownership to the child: OS window, renderer
HostSession, Preview, GL context, exact GPU ledger and immutable document/read
replicas. No laboratory authority, Runtime socket, sequencer or recovery journal
belongs to the child.

Use a top-level child window branded as the Workbench display. Do not embed or
reparent its HWND into v1, relay raw input to the parent, or keep a hidden v1 GL
renderer as a required model pump in the eventual v2 profile. The current model
pump is coupled to `WorkbenchApp::update`; extracting an independent bounded
owner is a real prerequisite, not an assumption already satisfied by spawning a
process. The current v1 GUI stays unchanged in this PR.

Windows Job Objects can enforce a child committed-memory limit and tie its
lifetime to the last owning Job handle. Configure an unnamed, non-inheritable
Job before creation, assign it with `PROC_THREAD_ATTRIBUTE_JOB_LIST`, and check
membership while suspended before executing the child. No breakaway flags or
uncontained fallback are permitted. The narrow dev/release
[OS probe](native-gui-host-probe.txt) validates these mechanisms on Windows 10
19045; it does not load a GPU driver.
See [Microsoft's Job documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
and [creation attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

Containment closes the practical user-mode ownership/lifetime problem under the
specified OS profile. It does **not** isolate the shared kernel, GPU driver,
compositor, total VRAM, parent input queues or every OS allocation. OS termination
may remain unconfirmed; retain that single incarnation and forbid replacement.
This degraded state is a supported failure result, not successful cleanup.

| Evidence or future guarantee | Status in this PR |
|---|---|
| Windows Job mechanism preflight | PASS, no-GPU fixtures in dev/release |
| Production supervised GUI containment | NOT IMPLEMENTED; integration admission remains BLOCKED |
| GPU rendering / healthy lifecycle / logical payload | Unchanged B2B2 PASS scope |
| Terminal GPU lifecycle | BLOCKED; process exit does not upgrade it |
| Whole host-owned Rust budget | BLOCKED; OS commit cap is a separate future contract |
| Exact driver/VRAM ceiling or universal termination time | No guarantee claimed |

## Integration admission and next PRs

1. **Independent parent model ownership:** offline/fake-client harness extracts
   the bounded dispatcher/model pump from rendering, preserves one Application
   worker and external `dispatch()` boundary, and proves progress with display
   permanently stalled. Do not switch the active v1 host.
2. **Production supervisor seam:** one child/Job record, atomic startup,
   membership/limit readback, monotonic watchdog, observed process exit, repeated
   close and parent-death tests. Test missing support/nested-Job rejection without
   launching an uncontained child. No GL yet.
3. **Private read-only IPC and snapshots:** closed versioned envelopes, bounded
   framing/reassembly, latest-only snapshots, coherent epoch/freshness and
   child-incarnation fences; slow reader/flood/truncation/cancellation tests.
4. **Isolated renderer child:** reuse B2B2's checked native seam and ledgers;
   qualify actual pixels, healthy deletion, retained unresolved root and separate
   OS containment. Measure actual child commit and controlled allocation peaks
   at fixed quotas before selecting any deployment cap.
5. **Opt-in virtual integration qualification:** exercise parent progress,
   renderer hang/crash/OOM, confirmed-exit restart, stale IPC rejection, DPI/IME,
   bounded lifetime cycles and API continuity. Activation is a separate decision
   after independent review, not a consequence of these PRs.

Every stage has numerical admission gates and failure outcomes in the
[host contract](native-gui-host-contract.md). Neither a job limit nor new budget
terminology changes B1, FiniteTraceItem or the 8,388,608-byte CPU Preview allowance.
No Typed Parameter API, Manual Output API, GUI Actions or ExperimentRunner work
is authorized here.

## Clojure compatibility

A future external Clojure client configures the **same parent Workbench** through
its language-neutral API. It does not implement rendering or attach to private
child IPC. Current wire v1 remains v1; proposed v2 version negotiation and atomic
presentation updates remain a separate implementation/review, following the
[v2 contract proposal](../generic-controls-v2-contract.md#6-version-selection-and-concurrent-clients).
The recommended fixed instance profile preserves default v1 and one presentation
owner; no lossy v1 shadow document or second Runtime client is introduced.

Child restart changes display incarnation only, not parent `workbench_id`,
presentation revision or Runtime mutation identity. Read snapshots can be sent
again. Mutation, status and Exact Retry are never regenerated from GUI restoration.
Existing dispatcher/session/recovery and OutputAuthority rules continue to apply.

The [ownership, lifecycle, failure/recovery diagrams and memory contract](native-gui-host-contract.md)
are part of this decision. No merge, activation or release is requested for B3A.

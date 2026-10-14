# DGUI-4C7B3B1: independent parent model pump

PR #36 was verified at `f5e2b38ffe437c7452f3e7d3ca104646f615460d`, made Ready and
merged normally as `14bbe58aa54ef6b2320a6d11ccfb98cf2ee67697`. The reviewed commit
is its second parent. Main was synchronized and clean before branch
`feature/dgui-4c7b3b1-independent-parent-pump`.

This implements the first prerequisite of the approved
[native host ADR](native-gui-host-adr.md#integration-admission-and-next-prs): a
working independently scheduled model owner with a renderer-free fake-client
harness. It does not select a new product host or activate GUI v2.
`ACTIVATION_QUALIFIED=false`.

## Implementation and single authority

[parent.rs](../../apps/lab-workbench/src/parent.rs) owns the model execution path:

- `ParentOwner<C>` owns one WorkbenchDispatcher, its one client/update receiver,
  RebuildCoordinator, operator/status/Exact Retry observers, selected presentation
  identity and optional WorkbenchEndpoint.
- `turn()` consumes up to 64 FIFO ClientUpdates, applies each once, routes its
  events, advances the existing workflow observers/rebuild in the original order,
  performs the existing implicit default-plot transition and services one fair
  external-owner turn. It requires no egui context, frame, window or GPU.
- `ParentPump<C>` moves that unique owner into one named thread. The running
  handle exposes neither model/dispatcher nor a mutable model borrow. The
  production `ParentPump<ClientHandle>::connect` constructor uses one existing
  Application worker and the existing endpoint with a renderer-neutral wake.
- `PumpSignal` holds one pending bit, one stop bit, a Mutex and Condvar. It has no
  callback backlog or payload queue. Client and endpoint producers publish the
  bit; the runner clears it before a turn, preserving a publish during processing.

```mermaid
flowchart LR
    A[One Application worker] -->|existing bounded FIFO updates| O[Unique ParentOwner: dispatcher and observers]
    N[Existing Workbench endpoint network thread] -->|bounded requests and detach notices| O
    O -->|existing bounded results/events| N
    A --> W[One coalesced wake bit]
    N --> W
    W --> R[ParentPump scheduler: no renderer dependency]
    R -->|exclusive ownership, bounded turn| O
```

There is no shared writable dispatcher mutex, second Runtime connection or cloned
RuntimeObservations map. Thread transfer moves the existing heap ownership. A
stopped owner can be retained after joining for diagnostics; it has no client or
API endpoint and is never another running authority. External requests still use
the sole `WorkbenchDispatcher::dispatch()` path. The dispatcher rebuild seam is
generalized only over its existing internal RebuildClient trait for qualification
with a fake client; no public API/DTO changes.

## Legacy v1 scope

WorkbenchApp now owns `ParentOwner<ClientHandle>` and delegates its model turn.
Its `drain_updates`, update staging Vec, workflow/rebuild application and direct
`endpoint.service_owner` path are removed. UI/operator methods access the same
owner's fields on the legacy GUI thread, preserving controls, Confirm, revision
fences, selection and presentation behavior. There is no parallel thread applying
updates to that legacy-owned object.

**The active v1 host retains its existing cooperative scheduling and repaint
adapter.** The independent runner is real implementation available to the future
parent root, exercised without GUI callbacks here; it is not concurrently started
beside v1. A stalled active-v1 event loop is not newly qualified as independent.
This follows the approved ADR's explicit requirement to keep the active v1 host
unchanged while proving the renderer-free parent harness. Closing the whole v1
Workbench remains parent shutdown. Closing the harness's separate display is not
parent shutdown.

The calls independent of GUI in the new runner are update receive/application,
external API service, workflow observations, rebuild/freshness barriers, default
presentation transition and root shutdown. GUI is a scheduler adapter, not the
definition or required caller of those operations.

## Bounds, wake and shutdown

| Item | Bound/policy |
|---|---|
| Application client workers / dispatchers / update receivers | One of each per parent root |
| Updates and observer/rebuild steps per turn | At most 64 each; no 64-update staging Vec |
| External service | One existing service_owner turn, at most four callers; existing rotating fairness cursor |
| Client command / update mailboxes | Existing 32 / 64 capacities, unchanged full/stop policies |
| External owner mailboxes | Existing 32 messages and 16,777,216-byte credits; caller/output/framing limits unchanged |
| Additional scheduling storage | One coalesced wake state and one join record; no callback/model-snapshot queue |
| Idle | Condvar wait with 100 ms fallback; stop interrupts it; no renderer wake required |
| Saturated update turn | Requests another turn through the same pending bit; external service still runs each turn |
| Root shutdown observation | Three-second application deadline, finite 10 ms waits; join only after is_finished |

Existing RebuildCoordinator advances at most one rebuild command per applied
update. Its projection/reference/controller/resource bounds remain unchanged.
All work inside an existing API operation retains its admitted JSON/document and
allocation bounds. A turn counter is not a universal instruction/time bound.
The 100 ms ADR target is a measurement reference, not a Windows scheduling or
driver guarantee. The committed evidence records the measured 64-update fixture;
it does not extrapolate that fixture timing to every maximum-payload operation.

Root shutdown first closes API ownership, then stops the existing client. No
Runtime shutdown, mutation, status or Exact Retry is synthesized. Repeated
shutdown preserves failure and performs no second client stop. A join timeout
retains the same join/owner obligation for later observation; no replacement
worker is started. The barrier regression checks that timeout is not successful
shutdown and later confirmed exit releases that exact owner. Trusted owner panic
and client-stop failure remain errors on subsequent shutdown calls.

Drop requests the same bounded stop and reports failure. It cannot force-kill a
Rust thread or establish a universal OS teardown bound; an unfinished thread
continues owning its capture if a root is forcibly dropped after failure. This is
not a successful cleanup receipt. This stage introduces no native child, GPU
resource or OS-process terminal qualification.

## Renderer-free evidence

[parent/tests.rs](../../apps/lab-workbench/src/parent/tests.rs) uses a bounded fake
client and the real loopback WorkbenchEndpoint. Its actual parent thread receives
updates and responds through the existing protocol while a separate fake display
thread is held at a Barrier. No eframe logic/ui or renderer callback is invoked.
Update-count Condvar barriers establish application before snapshot assertions;
socket timeouts are failure bounds, not sleeps used as progress evidence.

The tests verify:

- Full discovery/measurement/subscription/cursor freshness barrier without a GUI;
  Runtime update acceptance and external hello/status/presentation/recovery reads.
- Atomic presentation revision 1 -> 2 and rejection of a stale revision, with
  exact ambiguous recovery identities retained. The display exits while parent
  API remains usable and fake client stop count remains zero.
- No Runtime mutation/status/retry from absent display or snapshot restoration;
  only existing rebuild read/subscription commands arise from actual updates.
- Physical input split across writes; explicit mutation acceptance followed by
  caller detach; ambiguous projection and one explicit exact-target status call.
  Detach does not cancel acceptance or create local-rejected/retry submissions.
- Existing status/Exact Retry observer state across disconnect/reconnect,
  outcome_unknown and accepted/ambiguous records. Explicit status and Confirm
  submit once; subsequent idle/stale updates submit nothing automatically.
- Full fake update mailbox rejection, a producer inserting a 65th FIFO update
  during the turn, exactly 64 consumed, and the last update handled next turn.
- 10,000 notifications coalesced into one bit, publish-during-turn retained, idle
  Condvar waiting without GPU wake, and stop priority over pending work.
- Startup validation/thread-start failure, repeated shutdown, worker stop error,
  owner panic and deadline-unconfirmed join followed by real completion.

The legacy GUI drain regression now tests the real owner turn instead of the
removed GUI helper. Architecture guards reject a GUI-side update/application/
external-service path and enforce one production ClientHandle creation site in
the parent component. The stopped-renderer claim covers this deterministic
headless harness, not actual GPU device loss or a native child implementation.

## Validation and next prerequisite

Windows x86_64 MSVC dev/release results, operation counts, full regression scope
and CPU allocation comparisons are in [gate evidence](independent-parent-pump-evidence.txt).
The CPU probe wrapper includes the new parent module to compile the real v1
adapter; its scenarios, profiles, allocator and borrowing gates are unchanged.
No CPU budget/quota or numeric policy is raised. Preview allowance remains
8,388,608 accounted bytes, with the prior qualification preserved independently
from parent-thread/whole-host allocation claims.

An additional historical ignored M17.4 process-overlap gate remains **BLOCKED**:
its held Recorder Start fixture returns recording_unavailable before the second
mutation is accepted. The same failure was reproduced by compiling and running
the unmodified archived B3A merge source with the exact locked test artifacts,
without the new parent component. This gate drives dispatcher/rebuild directly.
Its original fixture/assertion remains unchanged; thirteen other virtual process
acceptance cases pass in each profile. A suitable acceptance-aware Recorder fault
fixture requires separate qualification, not weakened overlap assertions or a
Runtime change hidden in this refactor. The new parent accepted/detach/status
harness is separately PASS.

```powershell
cargo test -p lab-workbench --locked -- --test-threads=1
cargo test -p lab-workbench --release --locked -- --test-threads=1
cargo test -p lab-workbench --locked -- parent::tests --test-threads=1 --nocapture
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
cargo clippy -p lab-workbench --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

The next Windows Job supervisor still needs its own single-record lifecycle,
atomic Job-contained startup, limits/membership checks, independent bounded OS
work, heartbeat/observed-exit evidence and failure/parent-death qualification.
The future product parent root must retain workspace ownership and select the
independent scheduler, rather than retain the v1 repaint pump as its model owner.
Bounded supervisor/model status communication, private read-only snapshot IPC and
renderer child remain separate prerequisites. None is implemented here.

Runtime, OutputAuthority, public DTO/API, B1, FiniteTraceItem, dependencies and
active-v1 user operations are unchanged. GPU terminal and whole host Rust budget
verdicts remain BLOCKED. No Job supervisor, child launcher, named-pipe IPC, new v2
host, Typed Parameter API, Manual Output API, GUI Actions or ExperimentRunner is
started. One Draft PR is required for independent review; no merge, next stage or
release is authorized for B3B1.

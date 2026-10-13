# DGUI-4C7B2C4A: enforced frame ownership and host boundary

C4B2A implements [fallible retirement and persistent HostSession ownership](FALLIBLE_RETIREMENT.md).
That report supersedes the host constructor, infallible complete/drain and
retirement descriptions below. This file preserves the historical C4A contract
and evidence; the exclusive frame and generation bounds remain.

C4B1 follow-up: [native Glow preflight](NATIVE_GLOW_PREFLIGHT.md) corrects drain
arming for platform errors without uploads and identifies the infallible drain
contract as a GPU integration blocker. The C4A evidence below is historical.

C3 PR #30 was verified at `e42b970461791f9e792d5e434cf380d82ce9a26b`,
made Ready and merged normally as `7f11a34ce9997c8260c30e805d2c028c49d0ba93`.
The approved commit is the merge's second parent. Main was synchronized with a
clean worktree before creating `feature/dgui-4c7b2c4a-frame-ownership-seam`.

This stage implements the previously documented CPU ownership protocol. It adds
no native window, GPU backend, real texture handle or active v2 Workbench consumer.
`ACTIVATION_QUALIFIED = false`. C3's source/allocation audit remains applicable;
its caller-enforced frame protocol is superseded by this interface.

## Frame API and enforced ownership

```rust,ignore
let mut preview = Preview::new(&admitted, epoch); // explicit CPU-only backend
let mut frame = preview.run(&observations, bounded_input)?;
let generation = frame.generation(); // the Context that CREATED this output
let diagnostics = frame.report();    // preparation diagnostics, not a render ack
let meshes = frame.tessellate()?;    // borrowed, never an owned output queue
// inspect meshes here; do not retain copies in the Preview CPU budget
let outcome = frame.finish()?;      // CpuOnly, not GPU presentation evidence
```

`PreviewFrame` exclusively borrows `Preview`. `FullOutput` is private and owned
by a single `Option<FrameData>` in Preview. CPU meshes occupy that same frame
slot. Neither an owned FullOutput nor an owned mesh Vec escapes. Another run,
navigation/replacement/retirement or destruction of Preview is a Rust borrow
error while the guard remains usable. Native compile-fail gates verify these
constraints against the actual modules in both profiles.

The first tessellation attempt consumes the shapes using the generating Context
and its font atlas. Subsequent attempts fail and poison completion; no second
library call occurs. Pinned egui's API itself is infallible. The seam validates
returned mesh indices/positions/UV/clip rectangles, rejects callbacks and a
silently empty tessellation of nonempty shapes, and exposes an error result.
It does not recover arbitrary allocator OOM. The test-only fallible tessellator
closure exercises error cleanup without a production allocator hook or a
production `catch_unwind` safety claim.

`finish` consumes the guard, applies the host protocol below, releases CPU
output/meshes and returns a completion outcome only on success. `finish_cpu`
is available only for the explicit `CpuOnly` backend; it tessellates if needed
and uses the same completion/texture path. Preparation reports remain separate
from acknowledgement. Diagnostic geometry/allocation failures produce a
`degraded: true` completion, preserving other admissible curves; they never
become a successful claim that every trace rendered.

`discard` before or after tessellation retires the Context and texture namespace.
Dropping an unfinished guard does the same and records `Abandoned`, rather than
silently accepting it. There is one bounded `last_outcome` slot, no error history.
Even `mem::forget(frame)` cannot detach/leak the CPU payload: it stays in Preview,
and a next run returns `OutstandingFrame`. Explicit retirement or Preview Drop
cleans this slot. Forgetting the entire Preview is ordinary Rust resource leakage,
outside any RAII shutdown guarantee.

## Context generations and transitions

Every actual Context creation obtains a new, process-wide `ContextGeneration`.
Constructing Preview alone does not allocate a Context or spend a generation.
The atomic counter is shared by all Preview instances, never reset/reused, and
issued values include `u64::MAX` exactly once. Zero is a permanent exhausted
sentinel; exhaustion returns `GenerationExhausted` before any new Context/output.
Compare/exchange contention permits at most eight attempts, then returns
`GenerationContended`, with no automatic retry. Counter exhaustion is tested
with a local counter so tests cannot reset or poison the real global identity.
These identities are process-local, not durable or cross-process GPU tokens.

Context recreation for changed Details/Unicode fragments, displayed Runtime
unit text, Page/Plot and occurrence changes, different admitted document,
owner/revision/epoch, explicit retirement or processing failure gets a fresh
generation. Retaining the same Context retains its generation. Recreating
Preview cannot reuse an earlier namespace even with identical document/epoch.

A navigation press can select the next view while painting the current one.
`select_plot` changes logical selection only; it does not temporarily detach
Context or texture ownership. The returned transition frame retains the exact
old Context, generation and ledger. The next prepare retires those resources
before creating the selected view's Context. Page-to-Plot and Plot-to-Page
regressions assert generation identity as well as usable CPU meshes.

## Texture owner and backend obligations

`Preview::with_backend` owns a trusted `FrameBackend` through `TextureOwner`.
This is a testable host seam, not an eframe/Glow adapter. The default `CpuOnly`
sink owns no GPU resources and can return only a `CpuOnly` outcome. A mock
backend tests protocol ordering; its `HostCompleted` outcome is not GPU evidence.

Each backend key is `(ContextGeneration, TextureId)`. Numeric `Managed(0)` in a
new Context is a different resource. The owner contains eight fixed texture
slots (ID and dimensions), one generation and one in-flight flag. A frame admits
at most eight set IDs, sixteen ImageDelta updates and eight free IDs. Capacity
failure is explicit; there is no eviction, hidden growth, second generation or
retirement queue. The production font atlas uses one managed texture. These
caps do not add custom image widgets or authorize larger input/UI workloads.

The enforced sequence is:

1. Verify generation and delta-count bounds. Visit set IDs in deterministic
   sorted order using a fixed stack array, preserving per-ID delta order.
2. A full set registers its resource/dimensions **before** the backend call.
   A partial update requires an existing key and checked in-range coordinates.
   Set/upload may already be in flight even if it returns an error.
3. Pass borrowed platform output and the root viewport output (including repaint
   requests) to the backend. A backend must bound/handle these; ignoring required
   output must not return success. CPU-only inspection intentionally has no host.
4. Check that every mesh references a currently owned texture; callbacks are
   rejected. Submit one draw, then call `complete`. Successful `complete` means
   all dependent upload/draw uses have finished, not merely been enqueued.
5. Only after completion, apply free deltas. Unknown/already absent frees are
   harmless; no resource from another generation can be addressed. Clear consumed
   CPU deltas and drop the CPU frame. Only then can a next frame begin.

All failures abort the **entire generation**, without retrying any update.
Previously successful updates and the currently attempted resource remain in
fixed ledger slots. Still-unattempted deltas have no backend obligations.
The owner calls `drain`, then frees every owned key, including partially created
ones, and discards remaining CPU output before dropping Context. Thus the
backend need not infer how many operations to retry after an ambiguous error:
the only permitted action is withdrawal of that namespace. `drain` must include
uploads, submissions that failed after starting, and completion errors.

`drain` and `free` are non-panicking cleanup contracts. `drain` must establish
quiescence before returning; `free` must release the entire key. A real backend
must prove bounded shutdown/device-loss behavior or reject integration. The
mock's immediate completion is not evidence that a GPU can satisfy that contract.
There is deliberately no asynchronous outstanding-frame queue in C4A.

When Context retires without final free deltas, or Preview is destroyed, the
same owner releases **all** its keys. Context destruction alone is never treated
as freeing GPU resources. CPU `TexturesDelta::clear` is only CPU payload disposal;
it does not substitute for backend drain/free. Production frame code does not
call `drop_without_applying_deltas` to discharge host obligations.

## Ownership, allocation and lifetime audit

| Resource | Owner and lifetime | Allocation/reservation and bound | Failure/retirement |
|---|---|---|---|
| B1 traces and points | prepare only | Existing exact fallible reservations; 32 traces / 16,384 points; 512 KiB already included | No frame/report on shared projection failure; unchanged observations/TraceIdentity |
| Curve meshes, bounds and text | Existing production modules; current paint only | Unchanged finite bounds, at most 65,536 curve vertices / 98,304 indices; Unicode fragment 16 scalars | Local allocation diagnostic; no partial successful trace |
| Context, fonts, PlotMemory | Preview, one generation | C3 audit unchanged; library allocations remain infallible | Retire before replacement; no persistent generation/cache history |
| FullOutput and shapes | One private FrameData slot | Existing output heap; no clone or extra reservation | Drop only after completion or explicit namespace withdrawal |
| CPU meshes | Same FrameData slot, one attempt | Existing egui/epaint Vec growth and realloc overlaps; measured together with output | Cannot return owned Vec; failure prevents acknowledgement |
| Frame guard | Exclusive stack borrow | No heap; `must_use`; payload does not move into guard | Drop aborts; forgotten guard blocks run; Preview Drop still owns cleanup |
| Generation allocator | Process static | One AtomicU64, at most eight CAS attempts; no heap/history | Never wraps; static bounded failure |
| Texture ledger | Preview, one generation | Eight inline slots, fixed sorted-ID scratch; no heap reservation or realloc | Registers attempted sets, drains before free, retires whole namespace |
| Backend resources | Trusted host backend | Test backend fixed metadata only; real staging buffers, handles, fences and device memory **not qualified here** | Must satisfy complete/drain/free and bounded shutdown; cannot retain unaccounted clones |

The unchanged accounting formula is:

`accounted = realloc_overlap_peak + 127 * peak_blocks + 65_536 <= 8_388_608`.

The ledger/frame inline storage is included in the existing 65,536-byte fixed
charge, verified by a size regression. No new Preview heap buffer is hidden
outside the meter. Caller input allocations, backend-owned staging/copies,
window/event-loop state and GPU allocations require their own explicit host
budget; C4A's mock cannot qualify them. The scope remains Windows x86_64 MSVC,
the existing dev/release profiles, pinned egui/epaint/eframe 0.36.2 and
egui_plot 0.37.0, default fonts/DPI, viewport 1280×960, bounded input events and
one Plot/Context/output. Neither RSS nor arbitrary allocators/OS/fonts/DPI or
future versions acquire an 8 MiB guarantee.

## Verification evidence

**CPU MEMORY QUALIFIED remains valid for the stated CPU-only configuration.**
Both native profiles pass with the same maxima; no limit was raised and no
trace/point quota was reduced. This does not qualify an arbitrary `FrameBackend`
implementation: its own storage/copies/staging need an explicit host budget even
if the backend is moved into Preview rather than borrowed from a host owner.

| Native suite | Cases/phases per profile | Maximum accounted bytes |
|---|---:|---:|
| Preserved geometry qualification | 119 | 4,610,337 |
| Preserved C3 memory sequences/faults | 125 | 4,458,653 |
| Full Plot widget | 113 | 4,483,157 |
| New frame/host ownership | 12 | 4,355,499 |
| A2 Page probe, including CPU tessellation | 5,120 pages plus stress/lifetimes | 3,013,664 |

The combined maximum is **4,610,337 / 8,388,608 bytes**, leaving **3,778,271**.
At that geometry maximum: retained 616,264, phase peak 3,723,744, realloc-overlap
peak 4,439,264, peak blocks 831, shapes 146, vertices 69,540, indices 106,596,
zero geometry/allocation faults and zero post-drop bytes. New frame cases peak
at 626,386 retained, 3,590,538 phase, 4,170,202 overlap, 943 blocks, 202 shapes,
69,760 vertices and 106,926 indices. Expected host errors are reported separately;
all new lifetimes retain zero CPU bytes/blocks and zero owned mock texture keys.
New frame phase maxima include shutdown, not just pre-drop rendering.

The inline regression measures Preview 15,168 bytes (including its Page and
FrameData), TextureOwner<CpuOnly> 280, FrameData 640 and Page 13,800. With the
additional page plan, text scratch, bounds preflight and PlotView headers the
checked subtotal is 42,496 bytes, within the unchanged 65,536 fixed charge.

The complete targeted gate passes **299 tests in dev and 299 in release**:
the prior 283 plus sixteen new ownership/generation/ledger checks. All three
compile-fail checks pass in both profiles. Final mock-only same-frame partial
failure strengthening and shutdown-peak probe accounting were followed by
repeating all sixteen owner tests and twelve native frame cases in both profiles;
unchanged production/legacy gate evidence is reused. Workspace/standalone format,
all-target warning-denied Workbench Clippy, admission/conversion/export allocation
probe and diff checks pass. Active v1 source is unchanged and its regression
selection remains green; this is not a native-window or GPU smoke test.

Results are recorded in `FRAME_OWNERSHIP_GATES.txt` and the adjacent
`FRAME_OWNERSHIP_*` native evidence files. C3 CSV and earlier evidence are kept
unchanged. The full native launchers retain all 119 geometry cases, 125 C3
memory sequences, complete Plot widget probe and A1/A2/B1/B2/B2A cases. The
new twelve native frame scenarios cover Page, maximum Plot, 25 Context/epoch
lifetimes, discard/Drop before and after tessellation, forgotten guard shutdown
and four host failure points. The same native System allocator and realloc
overlap meter are used. Every complete new lifetime must retain zero bytes/blocks
above the existing singleton baseline.

Additional regressions check texture set/partial/free order, bounded capacity,
unknown/out-of-bounds partials, partial failure withdrawal, cross-generation ID
collision, missing final free, generation exhaustion, Unicode/runtime unit and
owner/document/occurrence transitions. C3's four deterministic curve reservation
failures additionally assert degraded completion; its two shared B1 reservation
failures assert failed preparation without acknowledgement. No fault hook was
added to production. Existing v1 GUI/Plot/hover/Confirm, admission/conversion/
export and architecture guards remain in the selected test gate.

```powershell
cargo test -p lab-workbench --locked -- gui::readonly_v2:: presentation:: gui::controls:: gui::app:: external::tests:: dispatcher::tests:: operator_boundary_tests:: model::read_binding:: --test-threads=1
# Repeat with --release.
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
# -FramesOnly selects the twelve new native cases; all modes check borrow failures.
./scripts/test-v2-allocation.ps1
cargo fmt --all -- --check
cargo clippy -p lab-workbench --all-targets --locked -- -D warnings
git diff --check
```

## Remaining integration blockers

- Implement and independently qualify a real GPU backend: texture upload/partial
  updates, fences, ambiguous submission, device loss, bounded drain/shutdown and
  final resource destruction. Perform a real GPU smoke test on a fixed platform.
- Budget host CPU staging/output processing, GPU resources and any proposed
  asynchronous frames/generations. C4A supports one serial generation/frame only.
- Integrate root viewport/platform output, redraw scheduling and presentation
  owner lifetime without borrowing the active v1 Context or spending its budget.
- Obtain separate integration/activation approval and complete the other required
  stages. No editors/drafts/actions/Confirm or external v2 protocol is enabled.

C4A is one implementation commit and a Draft PR for independent review. It does
not authorize C4A merge, C4B work, a release or activation.

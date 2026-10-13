# DGUI-4C7B2C3: CPU Plot memory qualification

Historical C3 qualification/evidence. C4A implements the caller protocol below
through [enforced frame ownership and texture retirement](FRAME_OWNERSHIP.md);
see that report for the current API and repeated native measurements.

This is the isolated CPU Preview qualification and the resource-ownership handoff
for a later, separately reviewed host stage. `ACTIVATION_QUALIFIED = false`.
No GPU backend, texture handle, host renderer, release, or Runtime work is added.

**Verdict: CPU MEMORY QUALIFIED**, within the configuration/accounting/ownership
scope below. A: finite structural bounds and contained resource lifetimes.
B: both native dev/release suites pass. Maximum combined accounted total is
**4,610,337 bytes**, including B1, below 8,388,608 by **3,778,271 bytes**.

C2 PR #29 was checked at `3bbaa4d3959e65c607bfe68fe26b6d859109bd0c`, made Ready,
and merged normally as `f3fbce6d8cf85612d6d7f1f457cbbca56c15534f`. The reviewed
commit is the merge's second parent. Main was fast-forwarded to that merge with
a clean worktree before creating the C3 branch.

## Scope and meaning of qualification

The two independent results are (A) source-derived finite work/storage and
lifetime containment and (B) native numerical allocation measurement. Neither
turns a measured maximum into a theorem about all future allocators or all legal
documents/interactions. Numerical results below apply to the reproducible suite
and the following fixed configuration:

- Native Windows 10 Pro 10.0.19045 x86_64 MSVC, Rust 1.95.0 (`59807616e`, LLVM 22.1.2), System
  allocator; no WSL, instrument, Runtime connection, worker or GUI host.
- Locked eframe/egui/epaint/emath/ecolor/default-fonts 0.36.2 and egui_plot 0.37.0.
  Workbench's existing features and Cargo.lock are unchanged. egui_plot has no
  default features (in particular, no serde PlotMemory persistence).
  The locked shaping/font dependencies include harfrust 0.12.0, skrifa 0.44.0
  and ahash 0.8.12; no dependency or feature is added.
- Existing dev/test profile: egui debug assertions off, overflow checks on;
  application/other crates keep normal dev checks. Ordinary optimized release
  is qualified independently. The launcher checks actual Cargo artifact profiles.
- One Preview, one isolated Context, one open Plot, one pass, fixed 1280 x 960
  logical viewport, default fonts/style/scale (one pixel per point), root viewport
  only. Plot requests 1200 x 540 logical points inside it.
- At most the first 64 input events: bounded pointer/button/wheel coordinates
  within +/-16,384; Plot-only zoom 0.1..=10. Input modifiers, keys, text, custom
  DPI/fonts/texture size and secondary viewports do not enter the Context.
  Incoming RawInput storage is supplied and budgeted by its caller; the ordinary
  suite constructs at most 64 events in bounded buffers. One additional stress
  phase explicitly charges a 4096-capacity input Vec containing one event.
- At most 32 traces and 16,384 prepared B1 PlotPoints. Long admitted Unicode
  fields remain losslessly reachable, 16 scalars / 64 raw UTF-8 bytes per Details
  fragment. Curves and Details are mutually exclusive paint modes.
- Sequential FullOutput and CPU primitive ownership. No caller-held output,
  cloned mesh, galley, Context, delta or texture-image history across frames.

The **single total allowance is 8,388,608 bytes**, including the 524,288-byte B1
reservation. B1 is never added again to measured heap bytes. The unchanged charge:

`accounted = realloc_overlap_peak + 127 * peak_blocks + 65_536`

The native hook measures every successful Rust System heap request in the window,
including Vec spare capacity, font/galley/atlas allocations, temporary buffers,
and old/new realloc overlap. 127 per block is the existing accounting policy
(rounding up to 64 plus 64 slack), not a measured Windows heap-manager theorem.
The extra realloc block is included in peak blocks. 65,536 charges inline state
and stack scratch. These are **accounted bytes, not RSS/committed virtual memory**;
Windows heap arenas, fragmentation, executable/static fonts and OS allocations
are not claimed to be physically capped at 8 MiB. There is no production global
allocation meter or post-allocation enforcement guard.

Borrowed admitted documents and RuntimeObservations belong to existing owners,
and fixtures/canonical comparison copies predate measurement. Fixed evidence
slots are prepared before the window and printed after it. No renderer category
is subtracted. The only prewarmed persistent initialization is the independently
attributed 24-byte/two-block ahash singleton (the A1 baseline probe is retained).
All Context allocations and complete retirement are measured above that baseline.

## Source-derived allocation and owner audit

Local production files are `plot_projection.rs`, `plot_bounds.rs`,
`finite_trace.rs`, `plot_widget.rs`, `plot_text.rs`, `page.rs` and `mod.rs`.
Exact dependency source paths below are relative to their pinned crate roots;
the [source manifest](PLOT_CPU_MEMORY_SOURCES.txt) records SHA-256 evidence, not a
copy or a dependency fork.

| Buffer / source | Owner and lifetime | Element/work bound and reservation | Overlap and allocation-failure behavior | Retained history |
|---|---|---|---|---|
| B1 `PlotView.traces` / points arena | One `run_plot`, dropped before return | <=32 inline/borrowed TraceViews, <=16,384 16-byte PlotPoints; checked 512 KiB preflight, separate `try_reserve_exact` | Trace allocation precedes target reads and points allocation. Any failure returns no PlotView/FullOutput/report; all already owned buffers drop. Native retained bytes verify both capacities. | None; source observations are borrowed, never rewritten |
| B1 target strings / typed read metadata | One trace resolution at a time | Admitted RuntimeRef strings; fallible exact copy of instrument/parameter; typed read scalar text prechecked <=512 bytes, validated units <=32/16 bytes | Two target strings and bounded typed clones may overlap the arena. Target-copy error refuses the frame; infallible typed clones use Rust allocation-error behavior. | None |
| B1 quotas/buffer references, bounds `Preflight`, status Cells | Stack of the current call | 32 slots each; two source scans <=262,144 visits; bounds scan <=16,384 points | No heap reservation, no allocation retry; bounds refusal diagnoses affected trace independently | None |
| `FiniteTraceItem`, aliases, PlotUi items | Plot show call | <=32 boxed borrowed items, three-byte ASCII aliases; egui_plot `PlotUi::add` grows item Vec | Small Box/String/Vec allocations are infallible; OOM may terminate. No sample copy in an item. | None |
| Curve `Mesh.vertices` / `indices` | Local builder, then Arc in Shape, then consumed by tessellator | For each nonempty trace S=max(n-1,1), reserve 4S vertices/6S indices with checked arithmetic and `try_reserve_exact`; total S<=16,384 | Both buffers and one fallible Shape slot reserved before publication. Failure of the second frees the first; no partial failed mesh. Arc allocation after reservation is infallible. | No curve cache or segment arena |
| Plot `shapes` / painter lists | egui_plot show and Context current paint pass | Grid <=12 marks per axis; <=32 curve meshes; one hover; finite UI below. `try_reserve(1)` for curve publication, other library pushes/extends infallible | Original curve buffers coexist with shapes, painter Vec capacity and later output Vec. Shape slots may realloc; meter charges old/new block. | PaintList capacities can persist for the same finite layer set; contents drain |
| egui_plot `memory.rs`, `plot.rs` | One Context PlotMemory, plus local loads/clones | Hidden HashSet <=32 IDs, one hovered ID, finite transform/auto flags, one entry per x/y axis thickness map | PlotMemory::load clones hidden sets/maps (twice inside show, then widget readback); config/legend copies overlap. HashMap/BTreeMap/String allocation is infallible. | One addressed Plot only; occurrence/epoch changes retire Context; no serialized history |
| egui_plot `overlays/legend.rs` | One show call | <=32 entries, seen-ID map <=32, aliases <=3 bytes; no user label keys | Legend config clone, entries/seen map and newly collected hidden set overlap stored/local PlotMemory. Infallible reserve/growth. | Only bounded hidden set / hovered ID persists |
| Axes and hover (`plot.rs`, `axis.rs`) | Current show call; galleys in Context | One axis each, <=12 grid marks each per spacer call; two queued bounds modifications; hover scans <=32 items and <=16,384 source slots, then one segment | Formatter strings have finite bounds (time <=48 bytes, y scientific, hover <=2048 fixed bytes before String copy). Default fixed grid callbacks replace range-sized work. Library vectors/format strings/tooltip allocations are infallible. | No sample/hover trajectory; current/previous text galleys only |
| Context widget/input/animation/maps | Preview owns the sole Context | One root viewport; one pass; finite UI IDs for one graph or one page/fragment; <=64 accepted events in a separate Vec | Context/RawInput copies/IdTypeMap/layout use infallible growth. Caller event storage overlaps the bounded filter buffer temporarily, but its spare capacity is not transferred to Context. Replacing Context frees the old one **before** constructing the new one. | Bounded current view capacities; egui global debug Id maps disabled by existing package profile |
| Fonts/glyphs/atlas (`epaint text/fonts.rs`, `text/font.rs`, `texture_atlas.rs`) | Context | Fixed default font faces/sizes/scale; graph alphabet is ASCII plus fixed library/UI glyphs; Details adds at most 16 scalars to a fresh Context. Atlas starts 2048x32 Color32, doubles height on demand. | Face/glyph maps, shaping scratch and atlas use infallible growth. Atlas delta clones full image or dirty region; atlas + delta overlap. Actual heap values are measured, not derived from a portable per-glyph byte constant. | Glyph caches/high-water buffers remain only within finite alphabet/style scope; Details key change retires them before another repertoire |
| Galley cache (`fonts.rs::GalleyCache`) | Context + Arc text Shapes | Bounded labels/text per pass; newline paragraph children bounded by submitted bytes | Layout jobs/rows/glyph meshes and Arc references coexist with output. `flush_cache` retains last-used generation, increments wrapping generation; an unused entry is removed on the next flush, long before counter wrap can accumulate history. Infallible allocation. | Current/previous pass text, finite high-water map capacity; no unbounded string history |
| `FullOutput`, `egui layers.rs::drain` | Caller until consumed/discarded | One output, one root viewport and one font texture namespace | Paint lists **move** Shapes into output; Vec append/realloc may overlap old capacity. Mesh and text payloads use Arc, not a mandatory deep clone. Caller cloning is excluded. `TexturesDelta` contains an owned full/partial CPU image; must be explicitly handled. | None under sequential ownership |
| CPU tessellator (`context.rs`, `epaint tessellator.rs`, `mesh.rs`) | One call, returned primitives owned by caller | Consumes `mem::take(output.shapes)`. New mesh at clip/texture boundary; finite primitive/text work. `Mesh::append_ref` copies curve vertices/offset indices into merged mesh using Vec growth. | Source Arc mesh and destination buffers overlap; destination Vec realloc can overlap old/new blocks. Text mesh reserve, scratch paths/discs and primitive Vec are infallible. All included in the native peak. | Context keeps paint **statistics**, not tessellated curve meshes; scratch drops with Tessellator |
| PlotState / Details / page transitions | Preview | One <=4096-byte borrowed PlotState, one hidden u32, one bounds/auto slot, one cursor/key; one bounded previous Page outside graph mode | No new Context during the output-returning navigation step. Next run drops old caches before constructing the new Context. Changed observations rebuild B1 each call; changed Details bytes retire glyph caches. | No occurrence, epoch, output or sample history |

`try_reserve_exact` guarantees room for the requested elements, **not** an exact
capacity on every implementation. FiniteTraceItem bounds requested counts and
emitted lengths. Native probes independently inspect actual vertex/index
capacities and verify B1's two live payload blocks. Spare capacities and
reallocation overlap are measured in addition to requested payload bounds.
The 1,703,936-byte curve payload bound is not the total renderer memory bound.

The audit found a second concrete lifetime/capacity defect: Rust's in-place
`into_iter().take(64).filter_map(...).collect()` reused the incoming event Vec's
capacity. One event in a 4096-capacity Vec therefore survived the event-count
filter with capacity 4096. A deterministic regression failed before the fix.
C3 uses a fresh Vec and bounded push loop, retaining at most 64 slots on the
pinned toolchain. Filtering rules and the first 64-event policy are unchanged;
tests cover 0, 1, 64 and 65 input events. Arbitrarily large *incoming* host buffers
cannot be made free by a callee: a future host must bound/charge those before
calling Preview. They are not an additional uncharged renderer allowance.

## Structural work result (A)

| Property | Bound and basis |
|---|---|
| Plot items / legend | <=32, directly from admitted traces; no per-segment PlotItem |
| Prepared source points | <=16,384, unchanged B1 water filling / min-max selection |
| Source scans | <=2 x 32 x 4096 point visits in B1; fixed-slot quota search <=13 iterations |
| Curve vertices / indices | <=65,536 / 98,304; 20-byte Vertex / 4-byte index; <=1,703,936 requested bytes |
| Drawing / closest hover | <=8 clipping iterations per source slot per scan; at most one extra slot for chosen hover; no length/pixel/magnitude-sized allocation loop |
| Bounds | Fixed 32 fault slots; preflight rejects unsafe numeric range before axes; no mutation of observations |
| Graph UI | <=112 labels, <=224 conservatively charged widgets, <=8192 text bytes; <=4 trace diagnostics/details selectors at once; <=1 aggregate allocation diagnostic |
| Details | <=24 labels, <=48 widgets, <=1024 submitted bytes; 16 Unicode scalars per fragment |
| Page | Existing A2 bounds: 4 cells, <=64 labels, <=128 widgets, <=12,288 text bytes, <=28 path-node resolutions |
| Output/Context lifetime | One of each; output/primitives/deltas discharged before next frame or external retirement; no retained curve history |

Exact numbers of heap calls may vary with clipping, ticks and text length. The
**bound** on allocation count/work does not scale with coordinate magnitude;
there is no arbitrary precision expansion or pixel-by-pixel curve generation.
Small Vec capacities can persist at the largest **bounded** current-view demand.
Literal absence of all persistent library caches would be false: font/glyph and
galley caches exist, but their owners/alphabet/generations are contained above.
Paint-shape and total tessellated vertex/index maxima are numerical checks,
separate from the exact own-curve bounds.

## Allocation failures

The test-only native allocator's existing one-shot size fault is used at the
fallible reservations. Consumption is now asserted, so a missed injection cannot
silently pass. No production allocator hook or OOM `catch_unwind` was added.

Mesh tests reject T01 and T32 independently at both vertex and index reservations.
The last trace is outside the first Details-selector page. The tests require:
one reported allocation fault, zero geometry for that trace, exactly the other
31 complete, correctly colored meshes, exact segment counts, and a visible
bounded aggregate diagnostic. Failure of indices tests dropping already reserved
vertices. Source observations and canonical document bytes remain identical;
TraceIdentity and full binding targets are rechecked. One later explicit caller
frame succeeds; no in-frame retry or internal retry queue exists.
`guarded = false` only means the reservation gate admitted the frame; it is not a
render-success flag. `allocation_faults` and `geometry_failures` remain nonzero
on a failed curve. Normal qualification rows reject either, while injected rows
require the exact expected failure and separately measure the later explicit frame.

B1 trace/point allocation refusal is necessarily **frame-wide**, because all
traces share the projection arena. It returns an error with no FullOutput and no
RenderReport, not a partial view labeled successful. The owner and original
observations remain usable for a later explicit attempt. The C3 CSV's projection
fault count is harness evidence of that error, not a fabricated renderer report.

Infallible dependency allocations (`Box`, `Arc`, formatting, font/atlas/cache,
library Vec growth, CPU tessellation) still follow Rust's allocation-error path,
which can abort the process. This stage does not certify interactive survival of
arbitrary system OOM. Even a local reservation diagnostic needs small library
allocations to paint it; the structured fault exists independently of the text.
The small Shape-slot reservation has a source-audited fallible return, but the
size-only hook cannot safely distinguish all equal-sized infallible library
allocations, so it is not indiscriminately injected. A future host must not turn
an error/fault report into successful presentation or schedule unbounded retries.
Existing research panic capture records library numeric panics as failed tests;
it is not used to catch allocator OOM or assert OOM recovery.

## Native numerical result (B)

Both profiles pass all **125 C3 rows**, all **119 geometry rows**, and the full
**113-phase widget probe**. The corresponding dev/release evidence files are
byte-identical. Historical C2/B2B/C1 CSVs and all existing geometry scenarios are
preserved; the new geometry CSVs add allocation/capacity columns without
replacing the old evidence. C3 adds deterministic combined sequences at
widths 0.9, 1 and 32: all 32 traces/16,384 points; sharp turns, coincident/near
points, extreme finite x/y and mixed rejected/ordinary traces; both orders of
unequal lengths; 340 zoom-out then 340 zoom-in frames with changing observations;
64-event bursts; all bytes of all long Unicode Details fields; metadata changes
while Details is open; hover, legend switches, manual/auto resets, 22 owner/revision
epochs per width, occurrence and actual Page/Plot button transitions; three extra
complete lifetimes per width; injected allocation failures and B1 capacities.
It also exercises oversized input spare capacity and explicit output discard
both before and after CPU tessellation, followed by Context retirement/recreation.

| Suite, same in dev/release | Rows | Retained at maximum charge | Phase peak | Realloc overlap peak | Peak blocks | Maximum accounted |
|---|---:|---:|---:|---:|---:|---:|
| Geometry, `mixed-2-large-reverse-true` | 119 | 616,264 | 3,723,744 | 4,439,264 | 831 | **4,610,337** |
| C3 combined, `epoch-actions` 6/14, each width | 125 | 645,262 | 3,591,234 | 4,230,938 | 1,277 | **4,458,653** |
| Full widget, Unicode repertoire 2 hover/updates | 113 | 880,636 | 3,671,460 | 4,251,124 | 1,311 | **4,483,157** |

Evidence: C3 [dev](PLOT_CPU_MEMORY_DEV.csv) / [release](PLOT_CPU_MEMORY_RELEASE.csv);
geometry [dev](PLOT_CPU_MEMORY_GEOMETRY_DEV.csv) /
[release](PLOT_CPU_MEMORY_GEOMETRY_RELEASE.csv); full widget
[dev](PLOT_CPU_MEMORY_WIDGET_DEV.txt) / [release](PLOT_CPU_MEMORY_WIDGET_RELEASE.txt).

Every complete enclosing lifetime has **zero** post-drop requested bytes/blocks.
All 119 geometry rows have zero allocation faults, policy failures, panics,
nonfinite positions/UVs, invalid meshes and unsupported shapes. The six C3
injected rows require exactly one expected allocation failure each; the four
explicit subsequent mesh frames pass with zero faults. No over-budget case was
found in this suite; neither the total allowance nor the trace/point limits moved.

The C3 input-capacity phase charges 4,185,650 bytes; output-discard/retirement
charges 4,388,351. Published own-curve capacities peak at 65,520 vertices / 98,280
indices (below the structural limits); coincident/offscreen cases may reserve
buffers and then discard them without publication, still counted by the meter.
B1 has exactly two retained native payload blocks: 24,832 trace bytes plus 262,144
point bytes = 286,976, with a 353,401 accounted projection peak inside 512 KiB.
Across the two CSV suites the observed total CPU maxima are 210 Shapes, 71,636
vertices and 110,520 indices. These include text/UI and are separate from curve
payload bounds; column maxima need not occur in the same frame.

Each CSV row is a phase envelope: maxima of shapes/geometry columns need not occur
in the frame with the heap peak. Retained bytes are after frame output consumption;
phase peak includes starting retained state. Post-drop is the complete enclosing
Preview lifetime, and is zero for every row. All normal rows require zero
allocation faults; injected rows require exactly one expected fault. Numeric
geometry refusal is explicit; ordinary/restored scenes must still emit curves.
Diagnosing all traces cannot satisfy the tests.
`geometry_faults` is the maximum per-frame unavailable trace/viewport count;
`allocation_faults` counts actual failed reservations observed in that phase.
Capacity columns describe published curve Shape meshes; transient unpublished
buffers are included in heap peaks. Old/new Contexts never coexist, but the old
Context can overlap the next B1 projection before text preflight retires it;
this overlap is also measured.

## CPU frame ownership and GPU/host handoff

This is a required **caller protocol**, not an implemented GPU host or an opaque
generation-checked output API. Current crate-private methods cannot prevent a
misbehaving caller from keeping FullOutput while running again. Such use is
outside this qualification. Future integration must enforce this protocol.

1. The single Preview/presentation owner creates Context lazily through
   `new_context`, and alone retires it. No Context handle escapes to another UI,
   thread or renderer. The same owner grants the single 8 MiB CPU allowance;
   another view/renderer cannot spend it simultaneously.
   It must also bound incoming RawInput storage before transferring ownership;
   a 64-event processing limit cannot pay for arbitrary caller allocations.
2. `run` borrows observations and returns one owned FullOutput and an honest
   report. A projection error returns neither. B1 is already dropped on return.
   Inspect/consume this output before another run, replacement, navigation API,
   allowance change, explicit Context retirement or Preview destruction.
3. FullOutput's Rust-owned bytes remain memory-safe until dropped. Its **semantic
   validity for tessellation/presentation** is restricted to its generating
   Context and owner/revision/occurrence/epoch. It expires when consumed,
   explicitly discarded, or that Context is retired; do not retessellate against
   a new Context even if numeric texture IDs or document IDs coincide.
4. CPU tessellation is permitted once, while the generating Context/atlas is
   alive. `Preview::tessellate` moves the Shapes out; it does not clone output.
   Own and drop the resulting primitives before the next frame. Do not clone
   Shapes/Arc meshes/galleys for a later frame. No output/primitives queue exists.
5. The caller must discharge TexturesDelta even when output is skipped. The
   current CPU-only probes call `drop_without_applying_deltas`. That explicitly
   frees the CPU images and clears obligations; it is not evidence of an upload.
   A later host must take/apply every `set` before drawing, and execute `free`
   after the last dependent draw. Dropping an unapplied delta is a debug error in
   epaint; disabling debug assertions does not transfer this responsibility.
6. A UI button may select the next Plot/Page during run. The transition frame
   still owns the old view's Context for tessellation. C3 fixes the earlier path
   that retired Context before returning output and silently produced no CPU
   meshes. Next run retires old caches before constructing the next Context.
   External `open_plot`/`close_plot`/`navigate` require the prior output to be
   discharged first; they reset the one slot, not append occurrence history.
7. Changed graph observations reproject into frame-local buffers. ASCII graph
   caches stay in their finite alphabet. Changed Details fragment/Runtime unit
   bytes retire Context before painting the new text. Only current bounds/auto,
   hidden mask and text cursor/key survive a Context-only retirement.
8. Changed admission reference, owner or revision/epoch retires Context and
   current Plot/navigation state. Old output must be consumed/discarded first;
   no stale output is re-labeled for the new epoch. Even equal content/IDs in a
   new admission does not authorize reuse. No implicit recovery or Runtime work.
9. On an output-processing error: stop that frame, release CPU primitives,
   explicitly discard remaining deltas/output, and retire Context before a later
   explicit attempt. No successful render acknowledgment. A future GPU host must
   additionally reconcile already applied sets/draws, drain/cancel its bounded
   in-flight ownership and release the retired namespace. No blind retry of
   partially applied deltas, and no reuse of numeric texture IDs across Contexts.
10. Destroying Preview requires no outstanding frame; then drop Context, caches,
    page and PlotState. A future host must separately release **all** textures of
    that Context, including those for which no final `free` delta was emitted,
    after their final use. Context drop alone does not free host/GPU resources.

Output retirement and Unicode/lifecycle regressions plus the native zero-retained
checks exercise the CPU portion. Texture upload/fences, asynchronous GPU completion,
host error recovery and memory accounting remain future work; no claim about GPU
memory, multiple Contexts, custom fonts/DPI or another dependency version follows.
The future host also needs an explicit Context generation/retirement signal:
current FullOutput is not generation-tagged, and internal text/page retirement
cannot be inferred reliably from numeric texture IDs or a full atlas delta.
Designing/enforcing that host boundary is an integration blocker, not implemented
by these CPU ownership rules.

## Reproduction and remaining blockers

```powershell
cargo test -p lab-workbench --locked -- gui::readonly_v2:: presentation:: gui::controls:: gui::app:: external::tests:: dispatcher::tests:: operator_boundary_tests:: model::read_binding:: --test-threads=1
cargo test -p lab-workbench --release --locked -- gui::readonly_v2:: presentation:: gui::controls:: gui::app:: external::tests:: dispatcher::tests:: operator_boundary_tests:: model::read_binding:: --test-threads=1
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
./scripts/test-v2-allocation.ps1
cargo fmt --all -- --check
cargo clippy -p lab-workbench --all-targets --locked -- -D warnings
git diff --check
```

`-MemoryOnly`, `-GeometryOnly`, `-PlotOnly` and `-ProjectionOnly` select bounded
subsets for diagnosis; the final gates run the full renderer launcher. Native
output and Cargo artifact evidence go to `target/v2-renderer-allocation-probe`.
Standalone support Rust files also receive rustfmt checks. No new dependency,
toolchain or license surface is introduced.

Final gates: **283 targeted tests in dev and 283 in release**, including finite
geometry, input capacity, lifecycle/Unicode, B1, v1 GUI/Plot/hover/Confirm, v2
admission/conversion/export, API/Dispatcher and operator architecture guards.
Full native renderer launchers pass A1/A2/B1/B2/B2A plus geometry and C3 in both
profiles. The independent admission/conversion/export allocation probe passes,
including its three fallible reservation failures. Formatting (workspace and
standalone probes), warning-denied all-target Workbench Clippy, and diff checks
pass. Exact execution/profile evidence is summarized in
[the gate record](PLOT_CPU_MEMORY_GATES.txt). This is not a full-workspace or
physical-hardware gate.

There is no remaining blocker for this scoped CPU qualification. The numerical
dependence on the pinned System allocator/accounting policy is an explicit scope
limit, not an endlessly deferred universal proof. Arbitrary-OOM recovery remains
unsupported. GPU/host generation and frame-ownership enforcement, texture cleanup,
CPU/GPU budgets and redraw scheduling, integration approval, and other activation
requirements remain blockers to activation. C3 is one Draft PR for independent
review, with no C3 merge or next-stage implementation authorized by this report.

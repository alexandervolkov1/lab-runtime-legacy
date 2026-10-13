# DGUI-4C7B2C1: current-frame geometry seam — review blocker

**No finite-geometry repair is installed in this PR. CPU qualification remains
BLOCKED and `ACTIVATION_QUALIFIED` remains false.** The requested integration
investigation found a usable public seam, but a reliable implementation would
need a substantial custom PlotItem covering bounds, drawing and hover. Section 4
of the C1 request requires separate review before introducing that change. This
PR supplies executable evidence and a concrete interface proposal for that review.
It does not claim that a dependency fork is inherently necessary.

The scope judgment is specific: changing auto-range contribution, current-frame
path generation, closest-hit selection and tooltip mapping, together with new
arena/fault ownership, is a substantive replacement. Merely wrapping one Line
method would leave the demonstrated bounds/hover failures. This is why the
request's review boundary applies; custom PlotItem is not categorically forbidden.

The production Preview, B1 arena/water filling/decimation, Runtime observations,
schemas, APIs, v1 renderer and pinned dependencies/profiles are unchanged. No
new production buffers, point cache or geometry state are introduced. The small
custom item below exists **only in headless tests**: it records transforms and
delegates stock Line behavior without changing geometry or hover.

## Actual frame order in egui_plot 0.37.0

Audited exact Cargo registry sources: `plot.rs`, `axis.rs`, `bounds.rs`,
`items/mod.rs`, `items/series.rs`, `math.rs`; epaint 0.36.2
`tessellator.rs`, `shapes/shape.rs`, and emath 0.36.2 vector arithmetic.

`Plot::show_dyn` (`plot.rs:1617`) performs this sequence:

1. Load PlotMemory; copy its transform into PlotUi's `last_plot_transform`.
2. Call the user's build closure. `PlotUi::transform`, `plot_bounds`,
   `screen_from_plot` and `plot_from_screen` use that previous transform.
   `set_plot_bounds` merely queues modifications; another immediate read is old.
3. Prepare legend/filter hidden items; compute auto/explicit bounds.
4. Apply drag, axis drag, boxed zoom, wheel zoom and scroll to the transform.
5. Render axes using the updated transform.
6. Initialize items, collect their shapes, run closest-item/hover handling.
7. Submit shapes, render legend/overlays, store memory and return PlotResponse.

`PlotItem::shapes`, `find_closest` and `on_hover` receive the current transform.
`PlotResponse::transform` also contains it, but returns **after** the shapes and
hover have already been generated. Stock Line has no user-supplied callback
between its transformation of source points and `Shape::line`. Painter clipping
does not clip unsafe world-coordinate arithmetic or the path's join normals.

The public boundary is therefore **an item operating at shapes/hover time**,
with separately checked bounds contribution. It is not a preprocessor using
PlotUi's transform and is not a filter after CPU tessellation.

## Reproducible evidence

Four tests in [plot_seam_tests.rs](plot_seam_tests.rs) use the pinned public API:

- First-frame auto bounds, queued manual restoration and Reset each demonstrate
  that the build transform differs from the transform received by `shapes`.
- Actual pointer/zoom and primary-button pan events demonstrate that draw and
  closest-item lookup receive the same updated transform, equal to the returned
  PlotResponse transform; the build closure still sees the previous one.
- Three points `(0.5, 0.5)`, `(0.5 + 1e-7, 0.5)`, `(0.5 + 2e-7, 0.5)` in bounds
  `[0,1] × [0,1]` are distinct under the build transform. A **single accepted
  Zoom(0.1)** makes all three screen positions equal in that frame. The test
  locates those exact three equal positions in the actual FullOutput Path.
  Dedupe based on the previous transform cannot establish the required invariant.
- With finite bounds `[-1,1]²`, endpoints `(-1e30,0)` and `(1e30,0)` transform to
  finite f32 positions, but stock `Line::find_closest` returns a nonfinite squared
  distance. `math.rs:dist_sq_to_segment` can overflow its squared length/dot
  arithmetic. Repairing `shapes()` while delegating stock hover is insufficient.

The witness does not supply safe drawing, suppress a trace, normalize coordinates,
catch a production exception, or create a false green geometry gate. Its two
Cell slots only record the callback arguments; it owns no copied point history.
Test outputs are explicitly discarded on unwind too, avoiding a secondary
TexturesDelta Drop panic that could obscure an assertion failure.

## Why the apparent small fixes are insufficient

| Candidate | Missing invariant or contract |
|---|---|
| Source-only dedupe | Distinct source points coalesce after current-frame f32 conversion. |
| Dedupe/clip with PlotUi::transform | Auto bounds, queued restoration and same-frame interaction invalidate it. |
| Check PlotResponse, FullOutput or mesh | Too late to contain item/hover work; checking tessellation output is also after the known dev panic. |
| Replace the clipping rectangle | Painter clipping does not bound the inputs to world-to-screen remap, distance arithmetic or epaint path normals. |
| Drop nonfinite vertices only | Finite positions can still produce zero-length joins or overflow differences/dot products. It can also reconnect unrelated pieces if invalid runs are simply removed. |
| Delegate Line hover after replacing shapes | Hover transforms the original source independently and can fail even for finite screen positions. |
| One stock Line per source segment | May avoid a multi-point join, but does not fix extreme transforms/hover; expands item/legend/ID/state work from 32 to up to 16,383 and changes trace semantics. |
| Temporarily disable all interactions or numeric traces | Violates the requested interaction and ordinary-curve availability behavior. |

`PlotItem::bounds` is evaluated before drawing and contributes to the shared auto
viewport. Simply omitting an unsafe trace from `shapes` leaves its extreme bounds
affecting every other trace. Explicit axis bounds and subsequent interactions
also require policy. A wrapper overriding one method is not a complete repair.

## Proposed interface for a separate implementation review

Prefer the existing public PlotItem extension over a fork. The following is a
design contract, **not implemented or memory-qualified code**:

```rust,ignore
// One per frame, preflighted before calling Plot::show.
fn prepare_frame_geometry(
    view: &PlotView<'_>,
    allowance: &mut GeometryAllowance,
) -> Result<FrameGeometry, GeometryLimit>;

// At most 32, borrowing the existing B1 TraceView/point slice and frame storage.
struct FiniteTraceItem<'frame, 'document> { /* borrowed fields only */ }

// Work with the supplied current-frame transform, never PlotUi::transform().
fn prepare_trace_geometry(
    source: &[PlotPoint],
    transform: &PlotTransform,
    output: &mut BoundedSegments,
) -> Result<SegmentRange, GeometryFault>;
```

The review should specify these obligations together:

1. **Identity/lifetime.** One item retains the complete existing TraceIdentity and
   ASCII alias. Segment ordinals are display bookkeeping only. Legend grouping,
   hidden mask, title/details, PlotState owner/revision/occurrence retirement and
   manual bounds restoration remain unchanged. A frame owns any geometry arena;
   dropping output/frame frees it, including on failure. No persistent points.
2. **Bounds policy.** Before Plot::show, validate explicit axis contributions and
   derive checked auto-range contributions from eligible traces. Do not substitute
   arbitrary smaller values for ±MAX. The review must choose how an unrepresentable
   contribution is diagnosed while ordinary traces remain accessible. Never
   silently discard the requested axes or reinterpret saved PlotState units.
3. **Current transform.** Shapes and hover use a common prepared result keyed to
   the transform actually passed to them. Validate finite viewport endpoints,
   positive dimensions, and the arithmetic required for mapping. Plot axes are
   drawn **before** item shapes; an item-level guard cannot by itself certify
   those earlier library paths. Prove them safe under the chosen bounds/input
   policy or retain an explicit blocker.
4. **Clipping/arithmetic.** Clip source segments to the finite viewport using
   checked, scaled arithmetic before f32 conversion. Require every intermediate
   used in remap, normalization and closest-segment distance to be finite. Reject
   a numerically unresolved segment/trace explicitly, without bridging across a
   rejected interval. Preserve source traversal order, including decreasing
   timestamps. Do not sort, alter B1 points or fabricate continuity.
5. **Degenerate joins.** Coalesce or segment only screen geometry; do not assert
   that measurements in one pixel are physically identical. Handle singleton,
   two-point, long coincident and near-coincident runs, signed zero and direction
   reversals. A safe-segment strategy avoids multi-point joins but changes caps
   and primitive/memory work; those effects need explicit qualification.
6. **Hover.** Search the same finite segments that are drawn, with checked distance
   arithmetic and the original trace identity. Clipped intersections are marked
   visual constructs, never samples. Keep `Curve position`, source Runtime unit,
   separate cosmetic unit and freshness. If inverse mapping is unresolved, show
   an explicit bounded diagnostic rather than a fabricated coordinate/provenance.
7. **Failure.** Fixed per-trace fault codes/masks communicate geometry unavailable,
   clipping/coalescing and allocation refusal to the four-row details UI. Other
   usable curves continue. Do not delegate a failed item to stock Line as fallback.
   Significant reservations must be fallible, checked, and completed before any
   primitive from that trace is published; rollback on failure is atomic.

A concrete arena bound to evaluate is one `ScreenSegment` per original adjacent
pair, with two f32 positions, a u16 index into the B1 trace slice and small visual
flags. At a reviewed/asserted 20-byte representation, a conservative 16,384-slot
arena needs **327,680 bytes**. Rectangle clipping gives at most one visible
interval/two endpoints per source segment; discontinuities must not duplicate
arbitrary output runs. Singleton treatment is separately bounded by 32 traces.
All counts/byte products and ranges use checked arithmetic and `try_reserve_exact`.
These are proposed ceilings, not allocations added by C1.

Do not allocate one boxed item per segment. Even one epaint Shape per segment
would add up to **1,048,576 payload bytes** at pinned `size_of::<Shape>() == 64`,
before Vec slack, clipped output, meshes and realloc overlap. The existing worst
fixture has only 496,734 accounted bytes of headroom; that strategy cannot simply
be declared covered by the old reservation. Evaluate bounded path runs versus
one finite mesh per trace, preserving stroke/color/highlight semantics and charging
all temporary/output storage. If the complete measured frame exceeds 8 MiB,
retain the memory blocker and separately review visible trace-work navigation.
B1's 16,384 points and all 32 definitions remain intact.

If public item-level work cannot contain the earlier axis/viewport work, propose
a separately reviewed dependency hook **after compute_bounds/handle_interactions
and before render_axis_widgets/collect_shapes**. Its contract would validate the
actual transform and either permit the frame or return a bounded local geometry
failure without publishing invalid bounds. It must not replace coordinates with
arbitrary values. No such dependency/API change or fork is included here.

## Geometry gate and historical evidence

The [B2B dev](PLOT_GEOMETRY_DEV.csv) and [B2B release](PLOT_GEOMETRY_RELEASE.csv)
tables remain byte-for-byte **historical pre-fix** evidence for approved commit
`f6131c68618e9f7b19983f55994d6e05fad9daac`.

C1 strengthens the research inspector: every Shape variant is considered,
including rectangle/ellipse/Bezier coordinates, stroke widths and transforms,
pre-existing Mesh positions/UVs, and final mesh positions/UVs/indices. Invalid
indices are counted rather than asserted during output teardown; unsupported
backend callbacks explicitly block CPU qualification. CSV adds `invalid_meshes`
and `unsupported_shapes`. This is diagnostic inspection, **not a runtime guard**
and not protection before library geometry work.

New evidence: [C1 dev](PLOT_FINITE_BOUNDARY_DEV.csv) and
[C1 release](PLOT_FINITE_BOUNDARY_RELEASE.csv). All 81 configuration and 38
interaction/lifetime rows are rerun with the corrected executable profiles.
The gate returns 0 only if every row is finite, has valid indices, no unsupported
callbacks, no panic, fits accounting and fully drops. Known or new failures remain
in the CSV with `BLOCKED` and native exit **2**. There is no allow-list converting
known NaN cases into passing gate rows. Because no repair was authorized at the
required custom-item boundary, the old counterexample tests remain evidence;
their expected-failure assertions must be replaced by positive finite-geometry
assertions when the reviewed repair is installed.

| Pre-fix fixture, still unresolved | Dev | Release |
|---|---|---|
| Coincident, widths 0.9 / 1 / 32 | Nonfinite mesh | Nonfinite mesh |
| SignedZero | Nonfinite mesh | Nonfinite mesh |
| HugeY / HugeX | Four recorded tessellation panics per fixture | Nonfinite shape and mesh positions |
| Zoom-out, 40-event regression / 340-event native sequence | Nonfinite mesh | Nonfinite mesh |
| Reset then zoom-in after zoom-out | Includes invalid preceding viewport frames | Same |

No B1 point is removed or altered. Hover production behavior is unchanged and
still says `Curve position`; the new overflow evidence is an additional reason
not to activate it. Safe ordinary, horizontal, vertical, step, zigzag, irregular,
nonmonotonic, wide-line, Unicode, retirement and lifetime fixtures retain their
prior outcomes. None of these partial successes qualifies the unresolved cases.

## Accounting and verification

The accounting remains `realloc_overlap_peak + 127 * peak_blocks + 65,536`, with
the B1 512 KiB already inside 8,388,608 bytes. No font/Context category is removed.
The independently attributed 24-byte ahash singleton is prewarmed; all Runtime
and admitted input objects predate the window. GPU/backend memory is unqualified.
There are **zero new production geometry buffers**; proposed arena sizes above
are not a measured implementation. Requested allocations and finite data bounds
are distinct from a universal numerical heap guarantee.

Both profiles preserve every corresponding B2B measurement. New invalid-mesh
and unsupported-shape counts are zero throughout the current 119-row matrix;
the original eight geometry failures remain. In the worst accounted phase:

| Profile / scenario | Shapes | Vertices | Indices | Retained | Phase peak | Realloc overlap | Peak blocks | Accounted |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Dev, zoom/pan/reset | 210 | 137,040 | 600,864 | 693,756 | 6,350,838 | 7,654,634 | 1352 | 7,891,874 |
| Release, zoom/pan/reset | 210 | 137,040 | 600,864 | 693,756 | 6,350,838 | 7,654,634 | 1352 | 7,891,874 |

The 21 combined epochs settle at 645,226 requested bytes; complete drops leave
zero measured bytes/blocks above the finite singleton. No SOURCE_MAP accumulation
is observed. These remain measurements of the **unrepaired** production widget.

Verification: **276 targeted dev tests**, **60 readonly-v2 release tests**, all
preserved native A1/A2/B1/B2/B2A scenarios in both profiles, and the full 81 + 38
geometry matrix (explicitly BLOCKED in both profiles). The dev selection includes
v1 GUI/Plot/hover/Confirm, external API/Dispatcher, typed reads, v2 DTO/graph/
canonical/admission/conversion/export, navigation and architecture guards.
`cargo fmt --all -- --check`, standalone probe rustfmt,
`cargo clippy -p lab-workbench --all-targets --locked -- -D warnings`, and
`git diff --check` pass. Native toolchain: rustc 1.95.0, x86_64-pc-windows-msvc.
No WSL or full-workspace gate is used.

Reproduce:

```powershell
cargo test -p lab-workbench --locked -- gui::readonly_v2::plot_seam_tests:: --test-threads=1
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
```

The full launchers run all preserved A1/A2/B1/B2/B2A scenarios before geometry;
their final geometry failure is reported explicitly. This stage does not turn
activation on, start a later implementation phase, merge its own PR or publish
a release. A separate review must first accept the custom-item boundary and its
numeric, hover and allocation contract; CPU/GPU/host qualification remains open.

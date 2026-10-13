# DGUI-4C7B2C2: finite current-frame PlotItem

This installs the reviewed public custom-item boundary in the isolated v2 CPU
Preview. The known coincident/zoom-out NaN joins and HugeX/HugeY tessellation
panics no longer reach epaint. This is not activation: `ACTIVATION_QUALIFIED`
remains false. No Runtime, v1 renderer, API, schema, B1 projection algorithm,
Cargo profile or dependency changes are included.

The [B2B](PLOT_GEOMETRY.md) and [C1](PLOT_FINITE_BOUNDARY.md) reports/CSVs remain
historical **pre-fix** evidence. C1's stock-Line witness tests deliberately still
demonstrate why a last-frame preprocessor and delegated Line hover are unsafe.
Production qualification tests now require finite output, not known failures.

## Integration and identity

`FiniteTraceItem` implements only public egui_plot 0.37.0 `PlotItem` APIs. Each
item borrows one complete B1 TraceView (including TraceIdentity and full binding
RuntimeRef), its existing point slice, and one frame-local status Cell. There
are at most 32 items/legend entries. `id()` retains the exact previous trace ID;
T01–T32 remain independent of user labels and sources. Color and numeric width
are retained; legend highlighting doubles width, capped by the admitted 32.
Metadata, freshness, generation fencing, units and B1 selection are unchanged.

Both drawing and hover call the same bounded segment mapping with the actual
`PlotTransform` supplied to `shapes`, `find_closest` and `on_hover`. No production
screen geometry uses `PlotUi::transform()`. There is no sample cache, persistent
screen cache, unsafe code, private dependency API or fork.

## Pre-axis boundary

egui_plot computes axes **before** calling item shapes. The item cannot guard
that earlier work. `plot_bounds::Preflight` therefore inspects at most 16,384 B1
points before `Plot::show`. It keeps 32 fixed diagnostic slots, admits safe trace
bounds independently, and supplies validated default bounds. A rejected trace
does not contaminate the ordinary traces' auto range. Empty/fully diagnosed plots
have explicit empty axes; no extreme point is replaced with a smaller sample.

The display arithmetic envelope requires, on each axis:

- finite endpoints, magnitude at most `f64::MAX / 4096`;
- a finite positive span between `4096 * f64::MIN_POSITIVE` and
  `f64::MAX / 4096`;
- a fixed CPU surface, with the actual plot rectangle at least one UI point.

The envelope leaves headroom for the pinned axis remap, grid spacing, derivatives
and reciprocal on the <=4096-point surface. This is a conservative implementation
policy, not a new v2 format restriction or a claim that every excluded range is
mathematically impossible to display by some other renderer. ±MAX, overflowing
differences and unresolved tiny spans yield `GeometryUnavailable: Bounds`.
Constant ranges get a nonzero viewport around the unchanged point. Nonconstant
trace ranges get a 5% margin. Explicit y_min/y_max retain their include semantics;
an unsafe explicit axis range refuses the plot with `AxisBounds`, not a silently
different axis request. All title/trace/unit Details remain accessible.

All stock post-build bounds mutators are disabled. Primary-button pan, wheel
pan and pointer-centered zoom instead compute a candidate using the current
`PlotUi::response()` rectangle, check it, then queue only approved bounds. This
happens in the build closure **before axes**; it is not screen geometry prepared
using the previous transform. A rejected candidate leaves the previous viewport
and auto flags intact and emits a bounded local diagnostic. Reset/double-click
select the checked auto range. Library default bounds are already valid and
stock auto expansion is disabled, so it cannot reintroduce unsafe contributions.
Axis-drag and boxed zoom are not provided by this preview policy.

An invalid saved manual viewport is retired before show. Owner/revision/document
and occurrence retirement rules remain unchanged. The one PlotState still owns
only finite bounds, auto flags, hidden mask and text navigation; it gains no
history or samples. Context retirement retains only that current slot.

Finite f32 pointer events can themselves overflow egui delta arithmetic. The
existing input boundary now excludes positions/wheel deltas outside ±16,384 UI
points before egui consumes them. This limits input to the fixed preview surface
and its finite surrounding area, never Runtime samples. Zoom-event policy
(0.1..=10, at most 64 events) is unchanged.

## Geometry and hover

Each original adjacent pair is processed independently, in original order:

1. Check the actual transform/bounds and compute normalized f64 coordinates.
2. Clip to the unit viewport with at most eight Cohen–Sutherland iterations.
   Face coordinates are pinned exactly; scaled interpolation avoids overflowing
   endpoint differences. Nonfinite or unresolved arithmetic refuses that segment.
   Opposing large ordinates whose cancellation uncertainty exceeds 1/4 UI point
   on a 4096-point surface are explicitly unavailable, not an invented zero.
3. Convert only clipped coordinates to f32. They lie in the actual frame.
4. Emit an independent square-capped rectangular segment directly into one Mesh.
   Normals use f64 hypot on bounded screen deltas. There are no open-path joins,
   path-normal divisions or zero-vector normalization in epaint.

Coincident/signed-zero/f32-coalesced positions emit a square marker; consecutive
identical markers are coalesced only in screen output. A singleton uses the same
marker. This does **not** assert physical equality of measurements. Widths that
underflow to zero in f32 get a `Stroke` diagnostic. The preview uses hard-edged
square caps rather than stock Line antialiasing/miter joins; it does not claim
pixel-identical stock-Line styling. Full numeric width/color identity is retained.

Clipping produces at most one interval per source segment. A refused or fully
offscreen segment does not connect its neighbors. No history is sorted, B1 point
is removed, quota changed, or intersection inserted into PlotView/observations.
Ordinary usable segments/traces remain available beside diagnosed geometry.

Closest-hit search recomputes the same clipped segments with the current
transform. Its f64 dot/distance arithmetic is bounded; all returned f32 distances
are finite. Ties use the first source segment. Hover retains the original item
identity and maps the closest screen position back through the same transform,
checking finite results before formatting. It never calls stock Line distance
code, claims a clipped intersection is a sample, or labels a segment position as
`Measured point`. The text remains `Curve position`, monotonic T+, exact Runtime
unit, separate cosmetic unit, quality/freshness and bounded ASCII output.
Crosshairs are drawn directly in the finite screen frame.

## Work, buffers and ownership

Let N be B1 prepared points, N <= 16,384. Across all nonempty traces, the number
of segment slots (including singleton slots) is at most N. Each slot contributes
at most four vertices and six indices. Thus curve payload lengths/capacities are
bounded by 65,536 vertices and 98,304 indices, or **1,703,936 requested payload
bytes** at the pinned 20-byte Vertex/4-byte index layouts. There are at most 32
Mesh objects, not 16,384 heap objects/items. Highlight does not increase counts.

Significant reservations use checked count/byte arithmetic and
`try_reserve_exact` before a trace publishes any mesh. Reservation failure drops
both buffers atomically for that trace, diagnoses Allocation and blocks native
qualification. Numeric failure affects only that source segment; no partial
failed allocation is published. The output has no segment arena: each clipping
call uses fixed local scalars/two endpoints. The 32 status Cells and bounds
preflight are stack-local. No new state persists in PlotState.

Mapping is O(N), with <=8 clip iterations per segment for drawing and closest-hit
search, plus one segment for hover. No loop/allocation depends on coordinate
magnitude, visible pixel length, number of document occurrences or frame history.
The small status and exact-count preflight run inside the existing single 8 MiB
allowance check, before allocating the first B1 projection buffer. The 512 KiB B1
reservation is included, never added on top. Output meshes live until FullOutput
and CPU tessellation are consumed/dropped before the next frame; B1/item scratch
is dropped on return from `run_plot`. No second persistent observation cache is
introduced. Complete retirement still frees Context and all per-frame geometry.

These are structural data/work bounds, not an allocator-wide numerical proof.
egui's output copies, spare capacity, font atlas, galleys and tessellator merging
still require combined measurement. The meter is not a production post-allocation
guard. No extra 8 MiB allowance or arbitrary font/Context subtraction is used.

## Qualification evidence

New tables: [C2 dev](PLOT_FINITE_ITEM_DEV.csv) and
[C2 release](PLOT_FINITE_ITEM_RELEASE.csv). Historical pre-fix CSVs are unchanged.
The original 81 configuration + 38 interaction/lifetime scenarios remain intact.
Columns add rendered segment count, explicit geometry diagnostics and policy
failures. Configuration expectations independently distinguish numeric envelope/
stroke/axis refusals from ordinary data: ordinary fixtures must draw geometry
without numeric diagnostics, and refused fixtures must diagnose without fake
curves. Diagnosing everything cannot pass. Allocation/transform errors, unknown invalid geometry, panic,
invalid indices/UVs, excess structural work, over-budget accounting and retained
drop bytes all block; the native process exits 0 only when every row passes.

The accounting remains:

`realloc_overlap_peak + 127 * peak_blocks + 65_536 <= 8_388_608`.

It includes projection, PlotState, Context, fonts/galleys/CPU atlas, egui_plot,
mesh construction, FullOutput, CPU tessellation, temporary/reallocation overlap
and allocator allowance. Only the independently attributed 24-byte ahash singleton
is prewarmed. Inputs predate the window. GPU/backend memory is not measured.

Both final profiles pass **all 119 rows**, with identical CSVs. Every row has
zero panics, nonfinite shape/mesh positions or UVs, invalid indices, unsupported
callbacks, policy failures and post-drop bytes. The rows report phase maxima;
one frame need not attain all column maxima simultaneously.

| Scenario (both profiles) | Points / traces | Shapes | Vertices / indices | Retained | Phase peak | Realloc overlap | Peak blocks | Accounted |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Two long + sixteen short, reversed (maximum charge) | 16,384 / 18 | 146 | 69,540 / 106,596 | 616,264 | 3,723,744 | 4,439,264 | 831 | **4,610,337** |
| 32 wide zigzags + Unicode | 16,384 / 32 | 202 | 69,764 / 106,932 | 644,386 | 3,590,642 | 4,170,306 | 1041 | 4,368,049 |
| Zoom/pan/reset alternation | 16,384 / 32 | 210 | 71,604 / 110,472 | 702,972 | 3,517,255 | 4,096,919 | 1428 | 4,343,811 |
| Coincident, width 32 | 16,384 / 32 | 203 | 4,536 / 9,090 | 647,448 | 2,906,876 | 2,906,876 | 1050 | 3,105,762 |
| HugeX / HugeY (diagnosed, no curve) | 1,024 / 2 | 71 | 3,000 / 6,558 | 528,692 | 941,592 | 954,264 | 526 | 1,086,602 |

The worst charge is **3,778,271 bytes below** 8,388,608. All earlier Plot probe
scenarios also fit: the largest B2/B2A Unicode-hover/update charge is 4,483,157.
The 21 combined epochs retain 645,226 requested bytes each; all complete
Context/Preview lifetimes drop to zero above the finite singleton. This is an
architecturally finite owner with measured values, not an inference from 21
epochs that arbitrary hidden history would be safe.

| Former counterexample | C2 result in dev and release |
|---|---|
| Coincident 0.9 / 1 / 32 | 32 finite markers, unchanged 16,384 B1 points, no joins |
| SignedZero | Finite markers, unchanged signed-zero source values |
| HugeX / HugeY | Explicit Bounds diagnostics, no panic/nonfinite primitives |
| One-frame and 40/340-event zoom-out | Finite clipped/coalesced geometry; unsafe final bounds requests are rejected |
| Subsequent 340 zoom-in / Reset | Finite geometry or explicit unresolved intersection diagnostics; Reset restores ordinary curves |
| Finite screen endpoints with stock-hover distance overflow | Finite clipped closest distance; no delegated stock hover |
| Extreme trace beside ordinary data | Extreme trace diagnosed; ordinary mesh and exact T02/unit hover remain visible |

Verification: **281 targeted dev tests**, **65 readonly-v2 release tests**,
**24 final affected dev Plot/text/geometry tests**, plus the strengthened seven
geometry-policy tests after tightening the gate. The broad selection includes
v1 GUI/Plot/hover/Confirm, API/Dispatcher, architecture boundaries, v2 admission/
DTO/graph/canonical/conversion/export, navigation, B1 spikes and water filling.
Native A1/A2/B1/B2/B2A suites and the full geometry matrix pass in both profiles;
affected dev geometry/Plot probes are repeated on the final production sources.
The independent admission/conversion/export allocation probe is unchanged.
Formatting, standalone probe rustfmt, warning-denied Workbench Clippy and diff
checks pass. Windows/MSVC rustc 1.95.0; no WSL or full-workspace gate.

Reproduce:

```powershell
cargo test -p lab-workbench --locked -- gui::readonly_v2:: presentation:: gui::controls:: gui::app:: external::tests:: dispatcher::tests:: operator_boundary_tests:: model::read_binding:: --test-threads=1
cargo test -p lab-workbench --release --locked -- gui::readonly_v2:: --test-threads=1
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
```

## Remaining limits

The finite geometry policy deliberately diagnoses unsupported numeric viewport,
axis, intersection and stroke requests. It is not a promise to display every
extreme f64 range simultaneously, or preserve stock Line's antialias appearance.
The bounded independent-segment representation keeps every B1 point/trace
available; it changes only frame-local display geometry. There is no trace-group
paging or reduced point quota hidden in this change.

Finite fixture success is not a universal 8 MiB allocator guarantee across every
legal interaction/data combination. GPU textures/backend, active shared-host
ownership, redraw scheduling, editors/drafts, ActionBinding execution, operator
Confirm and external v2 protocol/profile remain unqualified/out of scope.
Activation remains false; this PR must receive independent review before merge
or any subsequent phase. No release is created.

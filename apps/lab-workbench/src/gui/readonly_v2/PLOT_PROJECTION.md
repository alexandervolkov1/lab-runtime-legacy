# Bounded read-only Plot projection (DGUI-4C7B1)

`plot_projection::project` accepts an immutable `AdmittedV2Document`, immutable
`RuntimeObservations`, presentation owner/revision, a validated navigation page,
one cell index, and a byte allowance. It resolves that occurrence through the
admitted graph, then the Plot/Trace/DataBinding registries and structural summary.
It neither expands the graph nor constructs any egui widget. The current Preview
still paints the explicit inert Plot placeholder. There is no active Workbench
consumer, Runtime callback, query, mutation, metadata refresh or second cache.
`ACTIVATION_QUALIFIED` stays false.

## Frame-local API and bounds

`PlotView` borrows its Plot definition (title, axes, time window), carries exact
occurrence identity, owns at most 32 `TraceView`s and one `Vec<PlotPoint>` arena.
Each trace borrows its original definition and DataBinding, preserving the full
RuntimeRef, ID, display label, visibility, color, width and cosmetic display unit.
The trace identity contains owner/revision, document/window/tab/panel IDs,
root/child positions, Plot ID, Trace ID and ordinal. Repeated definitions, bindings
and sources do not merge identities. Arena ranges expose each curve separately.

The view additionally contains static availability, explicit diagnostic state,
actual scalar kind, source freshness/quality, bounded Runtime unit ID/symbol,
source-relative window, point counts, buffer evictions and decimation count.
Legend names use an injective ordinal prefix (`1: label`, `2: label`), including
when labels themselves resemble generated names. Original labels remain borrowed.

| Resource | Hard bound for one selected Plot |
| --- | ---: |
| Trace views | 32 |
| Prepared points, all curves combined | 16,384 |
| Point arena requested bytes | 262,144 (16 bytes per PlotPoint) |
| Source points inspected, including duplicate sources | 262,144 visits (two passes over 32 × 4096) |
| Legend text per trace | 520 UTF-8 bytes |
| Runtime unit ID / symbol per trace | 32 / 16 bytes |
| One on-demand hover text | 4096 UTF-8 bytes, inline |
| Projection reservation, including scratch and allocator slack | 524,288 bytes |

No storage scales with the document's 20,480 occurrences or 64 expanded Plots.
Registry lookup is bounded by existing definition limits; only the selected
bounded ancestor path is resolved. A 64-Plot fixture prepares exactly one view.
The result is intentionally not a persistent cache: consume/drop it before the
next projection. B2 must enforce that lifetime at its single view owner.

The byte guard runs before all allocations and checks point bytes + trace bytes
+ 64 KiB inline/scratch/slack with checked arithmetic. Insufficient allowance
returns `Budget`. The trace arena, point arena and temporary full Signal target
strings use fallible reservation and return `Allocation`; no partial view escapes.
The existing typed read layer's small unit/scalar allocations remain bounded:
oversized selected strings (>512 bytes) are refused before its cloning path.
This does not replace Rust's process-wide OOM behavior for infallible allocations
inside that existing helper. The native probe injects failure into both substantial
arenas and verifies complete release and immutable sources.

The 512 KiB reservation is **part of**, not additional to, the existing 8 MiB
presentation-owned visual ceiling. B1 qualifies only this isolated projection.
B2 must reserve it alongside Context/layout/paint/output and bounded view state;
it cannot simply add it to A2's full 8 MiB allowance. Existing admission
`VisualReservation` accounting and schemas remain unchanged.

## Reads, freshness and time

Only `signal_sample` and an exact Signal RuntimeRef can produce a curve. Static
Disabled/IncompatibleTarget is preserved. `resolve_read` supplies actual metadata,
typed sample, unit, quality and generation fencing. `expect_kind` can only reject
the actual type. Boolean, Integer (including i64 endpoints), Text and Enum remain
diagnostics, never f64 curves. Missing/unknown/incompatible metadata, absent or
unavailable/not-observed values, and generation mismatch produce no points.
Empty/missing live buffers have distinct diagnostics; no zero is fabricated.

Fresh and explicitly Stale Number sources may supply display history. Global
Unknown or Rebuilding overrides the typed read's stale descriptor floor and
suppresses curves. `begin_rebuild()` clears the existing live buffers; the new
projection cannot join a pre-rebuild curve to a later sample. Each call re-resolves
reads; it cannot retain a previously Fresh source result across epochs.

Freshness/quality and metadata describe the current cached source, not a proof of
each historical point's physical state. `LiveTraceBuffer` contains only bounded
display x/y pairs, without per-point generation, quality or Recorder provenance.
B1 neither manufactures that evidence nor upgrades the buffer to an archive.
Generation mismatch is fenced by the typed read until discovery confirms the
current source; the projection cannot independently attribute older buffer points
to generations that the existing buffer does not record.

The inclusive lower cutoff is the source's last buffered timestamp minus the
Plot time window, matching v1 semantics. Sources have independent latest times.
Subtraction underflow saturates only the cutoff to `-f64::MAX`; points are never
altered. Only finite x/y at or above the cutoff are eligible, in original order.
There are no interpolation, sorting, calendar conversion or continuity points.
Runtime/display units are both retained and no numerical conversion occurs.

Hover formatting says **Curve position**, using the same rounded millisecond
`T+HH:MM:SS.mmm` monotonic convention as v1. A future library-provided segment
position must not be called a measured sample. Nonfinite positions and invalid
monotonic durations produce no tooltip. Runtime units take precedence, and the
separate display unit is explicitly cosmetic. Text buffers fail on overflow;
there is no unbounded formatter output or silently truncated identity.

## Deterministic decimation

First count eligible window points for each visible, compatible, nonempty curve
without copying them. If their aggregate demand is at most 16,384, every eligible
point is copied exactly, including signed zero and subnormals. Empty, hidden,
missing or disabled/incompatible curves have zero demand and consume no quota.

Above that total, use integer water filling with two fixed 32-slot arrays. Find
the highest level L such that `sum(min(window_points, L)) <= 16,384` by at most
13 binary-search passes over the slots (each demand is at most 4096). Short traces
receive their full demand, leaving the remaining capacity available to longer
traces. Allocate the residual slots one apiece to still-unsatisfied traces in
original Trace order. Maximality of L guarantees fewer than 32 residual slots;
the final quota sum is exactly 16,384. Unsatisfied traces differ by at most one
slot. The water level is at least 512, so no nonempty trace is starved and every
decimated curve has room for both endpoints. Neither point buffers nor trace
order are modified. The bounded slot arrays fit within the existing 64 KiB
scratch allowance; no persistent cache or extra heap allocation is introduced.

For an over-quota curve, preserve its first and last eligible point and divide
the interior into `floor((quota - 2) / 2)` contiguous, equal-count buckets. Emit
each bucket's minimum and maximum y in original index order. First occurrence
wins ties; coincident min/max is emitted once. This preserves bucket extrema and
isolated positive/negative spikes with their original timestamps. It does not
promise retention of every extremum when several compete in the same bucket.
Flat buckets and odd quotas can leave slots unused. All omitted eligible points
are counted as `decimated_points`, separately from buffer eviction and windowing.
The second pass writes directly into the single fallibly reserved arena, with
constant scratch and no intermediate point vector or resampling.

## Native qualification

`scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev` and `-BuildProfile release`
link the actual Workbench dependency artifacts and fixed Cargo profiles. The new
`projection` scenario runs in its own native Windows/MSVC process, without Context
or worker threads. Fixtures/admission/shared observations exist before measurement;
all projection allocations and formatting are charged. It measures 32 independent
4096-point sources over 21 epochs, dropping every view, and injects arena failures.
It also measures unequal source lengths below, at and above the aggregate budget.
The finite lifetime proof is architectural (no global state or persistent owner),
not an inference from a finite number of successful epochs.

Native x86_64 Windows/MSVC dev and release gave identical results:

| Projection measurement (32 × 4096 source points) | dev | release |
| --- | ---: | ---: |
| Prepared points | 16,384 | 16,384 |
| Retained requested heap bytes, two arenas | 286,976 | 286,976 |
| Phase peak / realloc-overlap peak | 286,976 / 286,976 | 286,976 / 286,976 |
| Peak allocated blocks, including temporary typed reads | 7 | 7 |
| After each of 21 view/epoch drops | 0 bytes / 0 blocks | 0 bytes / 0 blocks |
| Insufficient allowance, before arena creation | 0 allocated bytes | 0 allocated bytes |
| Injected point-arena failure, peak / after error | 24,870 / 0 | 24,870 / 0 |

The trace arena is 24,832 bytes (32 × 776); points take 262,144 bytes. Using the
A2 native accounting policy, peak + 127 bytes per peak block + 64 KiB scratch is
**353,401 bytes**, below the 524,288-byte reservation. There is no Context baseline
subtraction here: this API creates no Context, font, GPU or ID provenance state.
Requested bytes are allocator measurements, not an RSS claim. All repeated epoch
readings are exactly equal. Budget refusal allocates nothing; injected trace-arena
failure allocates nothing; point-arena failure releases its earlier trace arena.

The non-wasteful quota correction was remeasured with `-ProjectionOnly` in both
profiles, with identical results (retained = phase peak = overlap peak):

| Window demands in Trace order | Prepared points | Requested bytes | Charged bytes |
| --- | ---: | ---: | ---: |
| 4096 + 31 × 1 | 4,127 (all) | 90,864 | 157,289 |
| 3 × 4096 + 4068 + 28 × 1 | 16,384 (all) | 286,976 | 353,401 |
| 3 × 4096 + 4069 + 28 × 1 | 16,383 | 286,976 | 353,401 |
| 5 × 4096 + 27 × 1 | 16,381 | 286,976 | 353,401 |

The budget-plus-one case assigns quotas 4096, 4096, 4095, 4069, then 28 singletons.
Only the third trace is decimated; the unchanged bucket algorithm emits 4094
points for its odd quota. In the mixed case the five long quotas are 3272, 3272,
3271, 3271, 3271; every short trace keeps its point. Odd quotas and coincident
min/max can still leave emission slots unused, without artificial duplicates.
All quota capacity is assigned before decimation. Each view drop releases every
allocation. The reservation remains 512 KiB, and the worst-case measurements and
allocation-failure behavior above are unchanged.

The original full native renderer suites also passed: all 5120 pages covering 20,480
occurrences, repeated frames, text stress, 21 epochs, Context recreation and
max-to-empty transitions, and zero SOURCE_MAP accumulation in the fixed profiles.
Their retained/peak evidence remains unchanged from A2. The existing admission,
conversion/export allocation probe is preserved. `-ProjectionOnly` reruns the new
scenario against freshly built executable dependency artifacts without repeating
the unrelated full-page traversal. Raw outputs are under
`target/v2-renderer-allocation-probe/{dev,release}/projection.txt`.

Validation after the quota correction: 253 selected dev regressions passed,
including 16 Plot projection tests and the live-owner/runtime-silence boundary
test. Coverage includes v1 Plot/hover/Confirm, v2
renderer/navigation, DTO/graph/admission/conversion/export, external API v1,
Dispatcher, typed reads and architecture guards. Native probes exercise actual
allocation and failure paths; deterministic unit tests cover irregular windows,
flat buckets/odd quotas/spikes, unsupported scalars, missing metadata/values,
generation/freshness, repeated identities, maximum Plot occurrences and byte
guards. `cargo fmt --all -- --check`,
`cargo clippy -p lab-workbench --all-targets --locked -- -D warnings`, standalone
probe formatting and `git diff --check` passed. No full-workspace gate was run.

## B2 view-state contract (design only)

The Preview retires Context whenever page text changes, so Context cannot be the
sole owner of plot interaction state. B2 should give the isolated preview owner
one fixed current-Plot state slot, keyed by the **exact** occurrence identity and
presentation owner/revision plus the borrowed admission lifetime. No map/history
per visited occurrence is proposed.

That slot needs at most: finite x/y viewport bounds (four f64), automatic-axis
flags and pan/zoom policy, a 32-bit legend visibility mask, and one optional hover
selection (trace ordinal plus bounded index or explicitly interpolated position).
Do not retain projected points, formatted hover strings, source snapshots, editor
drafts or egui-generated IDs as authority in it. Bound/validate values before
restoring into a newly created Context. Navigation to another Plot and any
admission/owner/revision replacement must discard the slot; same-page Context
retirement may preserve only the validated slot. Returning to another occurrence
does not resurrect its prior GUI state. Full trace identity, not a label or source
name, selects the legend/hover target.

B2 must qualify actual egui_plot allocations, layout/paint work, combined frame
and state budgets, Context retirement, hover honesty and host/GPU resource lifetime.
No slot, widget, pan/zoom, legend interaction, v2 Plot capability, GPU integration,
external v2 protocol/profile, drafts, action execution or Confirm is implemented
here. Independent review is required before beginning that work.

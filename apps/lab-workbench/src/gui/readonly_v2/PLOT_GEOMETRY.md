# DGUI-4C7B2B: CPU Plot geometry qualification — BLOCKED

**Full-schema CPU Plot qualification is blocked.** The finite native matrix stays
below 8,388,608 accounted bytes, but legal observations and ordinary zoom events
can produce NaN mesh vertices. Extreme finite observations also panic during
dev tessellation. `ACTIVATION_QUALIFIED` remains **false**. This change supplies
reproducible research fixtures, geometry inspection and allocation evidence;
it does not repair or activate the renderer, change the simultaneous-trace
policy, change dependencies/profiles, or reduce B1's point budget.

The approved [B2A text policy](PLOT_LABEL_MEMORY.md) remains in force. Unicode
access is contained, but that does not imply safe coordinate transformation,
valid tessellation, or a universal numerical heap bound.

## Reproduction and interpretation

Run native Windows/MSVC, using the actual locked executable dependencies:
the recorded toolchain is rustc 1.95.0 (59807616e), x86_64-pc-windows-msvc.

```powershell
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev -GeometryOnly
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release -GeometryOnly
```

The native geometry process deliberately exits **2**, and the launcher reports
`CPU Plot geometry qualification BLOCKED` after saving its complete CSV under
`target/v2-renderer-allocation-probe/{dev,release}/geometry.txt`. This is an
expected, unresolved qualification failure, not a passing memory gate. Omitting
`-GeometryOnly` preserves all earlier A1/A2/B1/B2/B2A scenarios before running
geometry. `-ProjectionOnly` and `-PlotOnly` retain their previous meaning.

Checked-in evidence: [dev CSV](PLOT_GEOMETRY_DEV.csv) and
[release CSV](PLOT_GEOMETRY_RELEASE.csv). Each row records independent maxima
over that named phase, not one frame simultaneously attaining every maximum.
`points` counts B1 prepared PlotPoints (including the arena retained while the
user hides a line); the library receives at most that many points. `traces`
counts TraceViews. Shapes include recursively inspected FullOutput shapes;
vertices/indices are actual CPU mesh lengths, including UI, axes and text.
`bad_shape` and `bad_mesh` count nonfinite positions (mesh UVs are checked too).
Mesh index validity is separately asserted. A dev panic produces no completed
mesh for that frame: zero vertices in that row is **not** evidence of cheap or
valid rendering. `panics` is a phase count. A row passes only when geometry is
finite, no panic occurred, accounted memory fits, and complete drop leaves zero.

The research harness catches dependency tessellation panics while keeping
FullOutput alive so it can explicitly discard texture deltas. Without that
cleanup, dev's unapplied-TexturesDelta Drop assertion can cause a second panic
and abort. No catch, check suppression, allocator instrumentation or new guard
is installed in production. The native hook suppresses repeated panic printing
inside allocation windows; each panic remains a BLOCKED result.

## Deterministic coverage

The 81 configuration fixtures and 38 lifecycle/interaction rows cover:

| Dimension | Cases |
|---|---|
| Cardinality | 0, 1, 2, 16, 32 traces crossed with 0, 1, 2, 512, 4096 source points; B1 maximum 16,384 |
| Width | Smallest positive f64, 0.9, 1, 32; legend hover also exercises the library's doubled stroke width |
| Shape | Horizontal, vertical, zigzag, alternating extrema, steps, coincident, nearly coincident, crossing, irregular and nonmonotonic |
| Numeric | Subnormal, signed zero, ±f64::MAX; tiny/overflowing spans; y bounds ±MAX, ±minimum subnormal, MAX/2..MAX |
| Time | Smallest positive admitted window and 604,800 seconds; duplicate, irregular and decreasing timestamps |
| Sharing | Independent sources, all traces sharing one source, hidden definitions, empty buffers |
| Unequal lengths | Exactly 16,384 points as 1..4 × 4096 plus the remaining 512-point traces, both trace orders |
| Combined text | 32 × 512-byte Unicode labels, 512-byte Unicode title and cosmetic units, maximum valid distinct Runtime units, width 32 and 16,384 zigzag points |
| Interaction | 340 zoom-out and 340 zoom-in events (attempts beyond the f64 exponent range), far pan both ways, 32 reset/zoom/wheel alternations, all 32 legend toggles, curve/legend hover, 24 alternating observations and Runtime units |
| Lifetime | All fragments of all fields of all 32 traces; 21 Context recreations; occurrence/page transitions; 21 owner/revisions; three complete lifetimes |

All documents pass ordinary admission. Every source buffer is the real bounded
LiveTraceBuffer, which admits finite x/y without requiring monotonic ordering
(`model/projections.rs`, `LiveTraceBuffer::push`). Nonmonotonic fixtures preserve
B1's existing lower-cutoff filtering and source order; this PR does not reinterpret
or sort their timestamps. The distinct-source 4096-point case exercises B1
decimation; the 32 × 512 case exercises the full arena **without** decimation.
The unequal-length cases show why total point count alone does not determine
Vec allocation capacity. All source/admission snapshots remain identical.

Actual pointer events exercise legend/navigation, and rendered UTF-8 hex text
is checked byte by byte against every full admitted label/unit/title. Existing
B2A probes preserve the three Unicode repertoires, skewed 4096 + 31 singletons,
pan/zoom restoration and other earlier measurement scenarios.

## Reproducible geometry blockers

1. **Coincident screen points.** Three or more identical positions can enter
   epaint's open-path join calculation. For the 32 × 512 coincident fixture,
   width 32, the returned mesh contains **130,560 nonfinite vertices**. Widths
   0.9 and 1 fail too. Signed-zero positions reproduce the same mechanism.
2. **Ordinary zoom-out.** Distinct, moderate zigzag data is not sufficient to
   avoid the defect: repeated accepted `Zoom(0.1)` events collapse adjacent
   transformed positions to the same f32 coordinate. The one-trace headless
   regression reaches nonfinite mesh positions within 40 events while saved
   PlotState bounds still have finite positive spans. The combined 32-line
   native phase reaches 130,560 nonfinite vertices. The subsequent reset/zoom-in
   phase includes frames painting the preceding failed viewport before reset;
   its BLOCKED row does not claim an independent zoom-in-only defect.
3. **Extreme finite data.** Alternating ±f64::MAX in X or Y creates nonfinite
   screen coordinates. Release returns 512 nonfinite shape positions and 4096
   nonfinite mesh vertices in each numeric fixture. Dev instead reaches emath's
   `Vec2::PartialEq` NaN comparison assertion during tessellation (four frames,
   four recorded panics). Finite source samples are therefore insufficient.

The existing PlotState capture rejects nonfinite spans and does not restore
them into a replacement Context. A regression verifies that behavior and old
Context-state retirement. It cannot prevent the first invalid FullOutput or
tessellation. Malformed pointer positions, wheel deltas and zoom factors are
already filtered; tests verify that they preserve valid manual bounds and the
hidden-line mask. Legitimate zoom events still expose the coalescing defect.

## Pinned implementation audit and structural bounds

Sources inspected are the exact Cargo registry copies of egui_plot **0.37.0**,
epaint/emath **0.36.2**, with no fork or dependency change:

| Source | Consequence in this Preview |
|---|---|
| egui_plot `items/series.rs:162,187`, `aesthetics.rs:35` | Each Line maps its borrowed points to one Vec of screen Pos2. No source copy persists. Solid lines create one path, singletons one circle; no fill/dash/dot expansion is configured. |
| egui_plot `axis.rs:382,487,499`, `bounds.rs:105` | f64 remap and f32 cast do not guarantee finite coordinates. Library bounds validity permits infinite width/height when both endpoints are finite. |
| epaint `tessellator.rs:385` | An open n-point line produces at most q = 2n−2 PathPoints. The duplicate-normal handling does not cover triplicated points; zero divided by zero can create NaN joins. |
| epaint `tessellator.rs:922,1141` | Thin open stroke: 3q vertices, 12(q−1) indices. Thick open stroke: 4q vertices, 18q−6 indices; reservation includes extra end-cap triangle capacity. |
| epaint `tessellator.rs:344,1486` | A singleton is a bounded circle, using finite precomputed rings (at most 65 path entries at this width, including highlight), or the finite circle-atlas fast path. At most 32 singletons exist. |
| epaint `tessellator.rs:1712,2220`, `mesh.rs` | Coarse bounding-box culling is not segment clipping. Mesh Vec reserve/append and reallocation depend on line order/capacity, in addition to length. `Mesh::is_valid` checks indices, not finite coordinates. |

For lines with at least two points, N ≤ 16,384 gives at most 131,072 curve
vertices and fewer than 589,824 curve indices. At 20 bytes per Vertex and
4 bytes per index their logical payload is below **4,980,736 bytes**. This is
**not** a heap bound: UI/text/singletons, Vec spare capacity, simultaneous old/new
realloc blocks, scratch paths, Context, atlas and output owners remain additional.
Transformed curve Pos2 lengths total at most N (131,072 payload bytes); one
tessellator scratch path has at most 8190 PathPoints for a 4096-point source.
No allocation count depends on physical segment length or unbounded timestamps
in this solid-line path. Grid generation remains exactly bounded by 12 marks
per axis. Highlight doubles stroke width, not path cardinality.

`cargo tree -p lab-workbench -e features --locked -i epaint` confirms **no rayon**
feature; tessellation is sequential, not an unmetered worker pool. The executable
profiles retain A1's egui-only provenance fix; emath/epaint dev assertions stay
enabled. Retained state is still one PlotState, one Context, fixed widget IDs,
bounded text fragments and current-generation galley caches. No point cache,
visited-plot history, expanded graph, Runtime worker or new persistent index
is introduced. Complete text/epoch retirement supplies finite history ownership;
21 sampled epochs alone would not establish it.

## Combined accounting and measurements

The existing accounting is unchanged:

`accounted = realloc_overlap_peak + 127 * peak_blocks + 65,536`

The **512 KiB B1 projection is included**, not added to 8 MiB. Windows requested
allocations cover PlotView, PlotState's heap context, fonts/galleys/CPU atlas,
egui_plot, axes/legend/hover/details, FullOutput, CPU meshes and temporary/realloc
buffers. The fixed 64 KiB credit covers stack-owned navigation/PlotState and
projection/fragment scratch. Baseline Context is included, not subtracted.
Only the independently attributed one-time 24-byte ahash singleton is prewarmed.
Admitted documents and immutable Runtime buffers exist before the measured
window. No Windows RSS or GPU memory claim is made. Every output/mesh is dropped
before the next frame, as required by the existing Preview lifetime contract.

| Scenario (dev and release unless indicated) | Points / traces | Shapes / vertices | Retained | Phase peak | Realloc overlap | Peak blocks | Accounted | Result |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| Combined width-32 zigzag, Unicode | 16,384 / 32 | 202 / 135,172 | 644,386 | 6,297,334 | 7,621,866 | 976 | 7,811,354 | PASS fixture |
| 32 × 4096, after B1 decimation | 16,384 / 32 | 202 / 131,764 | 645,634 | 6,167,254 | 7,457,226 | 976 | 7,646,714 | PASS fixture |
| Two long + sixteen short, reversed | 16,384 / 18 | 146 / 135,004 | 616,264 | 6,340,048 | 7,709,648 | 794 | 7,876,022 | PASS fixture |
| Zoom/pan/reset alternation | 16,384 / 32 | 210 / 137,040 | 693,756 | 6,350,838 | 7,654,634 | 1352 | **7,891,874** | PASS fixture |
| All Unicode fields + graph transitions | 16,384 / 32 | 210 / 137,036 | 627,354 | 6,298,198 | 7,622,314 | 1285 | 7,851,045 | PASS fixture |
| Coincident, width 32 | 16,384 / 32 | 203 / 135,224 | 647,448 | 6,300,476 | 7,625,008 | 985 | 7,815,639 | BLOCKED: NaN |
| 340 zoom-out events | 16,384 / 32 | 204 / 136,692 | 722,743 | 6,298,738 | 7,602,534 | 1250 | 7,826,820 | BLOCKED: NaN |

The maximum measured accounted total is **7,891,874**, with 496,734 bytes to the
ceiling; no over-budget fixture has been found in this matrix. This is empirical
evidence, **not a universal 8 MiB numerical guarantee** for the entire schema.
The separate logical-count bounds above cannot close that proof. All 21 combined
epochs settle at 645,226 requested bytes; all complete drops leave **zero**
measured bytes/blocks above the finite singleton. No SOURCE_MAP accumulation is
observed. Debug/release agree on completed ordinary cases; their extreme-numeric
results differ because only dev asserts on NaN comparisons.

Both complete 119-row outputs reproduce byte for byte in independent native
processes. The only dev/release row differences are `numeric-HugeY` and
`numeric-HugeX`. Their dev accounted charges are 957,915 and 944,609 respectively,
with no completed tessellation; release charges are 1,157,539 and 1,144,233 with
invalid mesh coordinates. These lower charges do not qualify either case.

## Regression verification

The targeted native dev selection passes **272 tests**:

```powershell
cargo test -p lab-workbench --locked -- gui::readonly_v2:: presentation:: gui::controls:: gui::app:: external::tests:: dispatcher::tests:: operator_boundary_tests:: model::read_binding:: --test-threads=1
cargo test -p lab-workbench --release --locked -- gui::readonly_v2:: --test-threads=1
```

The second command passes **56 release tests**. Four new headless regressions
inspect actual output/meshes, preserve the exact known failure set, exercise
invalid input without corrupting manual state, reject invalid restored bounds,
and reproduce zoom coalescing. A passing counterexample regression means the
blocker remains reproducible, not that the broken renderer is qualified. The
new tests were rerun after the final fixture-only Clippy correction.

Prior regressions retain RuntimeRef/TraceIdentity, all scalar/freshness/generation
fences, spikes and water filling, honest hover, all 64 Plot occurrences, full
Unicode text, state retirement and Runtime/intent silence. The selection also
covers v1 GUI/Plot/hover/Confirm, external API/Dispatcher, v2 DTO/graph/canonical/
admission/conversion/export and architecture guards. The existing admission/
conversion/export allocation probe is unchanged.

All preceding native renderer scenarios pass in both profiles: hash/empty Context,
numeric/object IDs, small/v1/maximum, bounded A2 navigation, B1 projection and
B2/B2A Plot lifetimes. The new geometry probe is a separate process and does not
replace their budgets or expected passing results. Formatting, standalone probe
rustfmt, `cargo clippy -p lab-workbench --all-targets --locked -- -D warnings`
and `git diff --check` pass alongside the explicitly BLOCKED geometry
gate. No WSL or full-workspace gate is used.

## Guard assessment and proposed separate review

The existing preflight bounds source visits, projected points, submitted text,
trace count, selected occurrence and fixed allowance before substantial widget
allocations. It cannot bound a library Vec's actual capacity from current data
counts alone, reject f32 coalescing using only source finiteness, or make
post-allocation accounting into a runtime budget guard. No new guard is claimed
here, and the research meter is not a production allocator limit.

A separate proposal should first establish a bounded, viewport-aware geometry
boundary before library tessellation: validate finite positive transform spans,
clip offscreen segments with overflow-safe arithmetic, and handle coincident
screen points/degenerate joins without changing source order or inventing sample
provenance. It needs explicit endpoint/extremum/hover semantics, capped temporary
buffers charged to the same 8 MiB, and a preflight work/capacity policy. It must
preserve B1's arena/water filling and access to every trace. Simply rejecting all
large numbers, or ignoring NaN meshes after allocation, is not a full-schema
solution. Dependency repair/fork/profile changes require separate review.

If a subsequent finite-geometry implementation still exceeds memory, an explicit
bounded group-of-traces navigator is a candidate for separate review; all 32
definitions and B1 data must remain reachable. No such simultaneous-visibility
change is made here, and trace paging alone would not fix coincident joins.

GPU textures/backend lifecycle, shared-host budget ownership and redraw cost are
unqualified. Editors/drafts, actions, operator Confirm and external v2 protocol/
profile remain outside scope. No release or next implementation stage is begun.

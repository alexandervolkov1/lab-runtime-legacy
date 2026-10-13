# DGUI-4C7B2: isolated interactive read-only Plot

Historical B2 report and measurements. The current title/legend/hover text policy,
Context retirement key, preflight and memory evidence are superseded by
[DGUI-4C7B2A full-label containment](PLOT_LABEL_MEMORY.md). In particular, the
1024-byte refusal and diagnostic guard-bypass probe described below are removed.
The B1 projection and B2 Plot identity/interaction contracts remain in effect.

`ACTIVATION_QUALIFIED` remains false. The active v1 Workbench, its 16 API
operations, Dispatcher, Runtime, recovery and v1 plotting are unchanged.
This is an isolated CPU preview, with a real egui_plot 0.37.0 widget. **Full-schema
Plot memory qualification is blocked** by the reproducible text counterexample
below. No GPU/backend or external v2 host is qualified.

## Navigation, projection and rendering

The four-cell page contains Plot cards and `Open Plot` buttons. Opening a card
retires the page Context and creates one current-Plot slot. Only that occurrence
is projected/rendered. Back drops the slot and returns to the same bounded
location/page and page scroll coordinates. Group/window/tab/panel navigation
remains the A2 policy; no expanded graph, 64-view array or visit history exists.
The headless test visits all 64 Plot occurrences through two repeated groups,
checks distinct IDs and actual Plot/legend output for each.

Each frame calls the unchanged B1 `plot_projection::project` with its 512 KiB
reservation **inside the shared 8 MiB allowance**. The frame-local arena is
borrowed by `Line`; there is no second point cache. Projection and widget
temporaries are released before CPU tessellation. B1 retains its 32 traces,
16,384 points, water filling, min/max buckets, exact timestamps, independent
Trace identities, window filtering and metadata/freshness fences. Its two-pass
source-read bound remains 262,144 visits. This is the existing display cache,
not Recorder history or evidence of a physical instrument state.

The widget has a 1200 by 540 logical-point plot inside the existing 1280 by 960
preview, one pass, default fonts/style and scale. Axis grids use at most 12 marks
each, with no range-sized iteration. Number formatting is bounded, elapsed time
is monotonic `T+`, and y_min/y_max use the existing v1 `include_y` semantics.
There is no unit conversion. Explicit colors/widths come from TraceStyle. Legend
entries retain B1's ordinal-prefixed unique names and document order. Missing,
disabled, unsupported, unconfirmed and empty curves have diagnostics and empty
series, never fabricated points. Up to 32 diagnostic rows are scrollable.

Plot work is bounded by one occurrence, 32 line items, 16,384 projected points,
24 grid marks, one hover label (4096 bytes), 128 conservatively charged labels,
256 widgets/containers and 16 KiB submitted text including diagnostic/axis/hover
text. The current pinned fixture reaches 205 FullOutput shapes and 119,496 CPU
vertices. These last two are measurements, not universal schema guarantees.
Text-layout preflight below runs before creating any plot/legend widgets.

Pan, wheel translation, finite positive zoom events, boxed zoom, legend clicks,
double-click reset and the explicit Reset / auto bounds button are local visual
interactions. Input filtering keeps the existing maximum 64 pointer/wheel
events, accepts zoom only in Plot mode, and rejects nonfinite pointer/wheel
values. No keyboard editor, Confirm, intent or laboratory operation is created.

## One state slot and the public dependency API

The slot borrows the exact occurrence identity and Plot definition from the
admitted document. That identity includes document/window/tab/panel IDs and
root/child positions plus owner/revision. The borrowed Plot includes its ordered
Trace definitions; actual line IDs hash the full B1 TraceIdentity (occurrence,
Plot ID, Trace ID and ordinal), not a label or Runtime source.

Only finite, positive, finite-span x/y bounds, two auto flags, a 32-bit hidden
mask, the selected child index, and fixed unit identity buffers are retained.
The 32 pairs of fixed 32/16-byte unit buffers permit exact detection of changing
font text. They are not an observation cache and contain no values/samples.
No owned strings, point arrays, arbitrary PlotMemory, hover history, drafts or
authority are stored. Hover is entirely frame-local.

The pinned public APIs are sufficient:

- `PlotMemory::load` supplies bounds, auto flags and hidden IDs. Its temporary
  clone is dropped in the same frame. The Context has exactly one Plot memory,
  at most 32 hidden IDs and one axis configuration per direction.
- A new Context receives `PlotUi::set_plot_bounds` and `set_auto_bounds` once.
  Ordinary frames leave interaction authority with egui_plot, including reset.
- `Legend::hidden_items`, `LegendGrouping::ById` and insertion order restore the
  legend. In 0.37.0 legend overrides do not filter first-frame lines. Hidden
  lines therefore use empty borrowed data until memory is restored. `PlotUi::add`
  is essential: `line()` discards empty lines and would forget their legend IDs.

Measurement updates preserve manual bounds/hidden lines. Auto axes continue
following eligible data. Context retirement preserves only this current slot;
the library's transient pointer gesture itself is not replayed. Switching Plot,
Back, navigation, admission replacement, owner or revision drops the slot.
Reopening a forgotten occurrence starts fresh, even for identical source IDs.

egui/epaint 0.36.2 `Fonts::begin_pass` calls `GalleyCache::flush_cache`, retaining
only the immediately used generation. Changing numeric text cannot produce an
unbounded galley history. Glyphs have a fixed repertoire: admitted title/legend/
display units, bounded current Runtime units, and fixed ASCII diagnostics/numbers.
Changing Runtime unit text retires the Context before submitting the new text.
Navigation/replacement also retires the Context, not just the Plot ID. Fonts,
scale, plot/legend IDs and axis configurations cannot grow with visit history.
This finite ownership argument is independent of the 21-epoch measurements.
The A1 package profile continues to disable process-global ID provenance.

## Hover identity and honesty

The formatter maps the injective B1 curve name back to its TraceView. It never
uses display_label alone. It delegates to B1's bounded hover formatter, showing
Runtime unit, a separate cosmetic display_unit, freshness/current quality and
separate decimation/buffer-eviction counts. Near-segment positions are labelled
`Curve position`, including positions not present in the source buffer. No
sample generation, quality or acquisition provenance is invented for a point.
There is no tooltip in empty space or for nonfinite positions. Equal labels and
equal Runtime sources retain independent trace and occurrence identities.

## Combined memory accounting and remaining blocker

No 512 KiB addition to 8 MiB is permitted. The native window includes actual
projection, one slot, all Context/font/galley/CPU atlas allocations, axes, legend,
hover, FullOutput, CPU tessellation, scratch and realloc overlap. All output and
meshes must be consumed/dropped before another frame; there is no output history.
`Preview::tessellate` consumes shapes using the current bounded Context. A
navigation transition retires that Context; its old output is discarded and the
new view is painted on the next frame. This is a CPU preview lifecycle, not a
GPU texture retirement protocol.

Source admission/observations were allocated by their existing owners before
measurement. No Context/font category is subtracted. The only excluded global
initialization is the independently measured, finite 24-byte ahash singleton.
The previous warm empty Context baseline is 326,161 bytes retained / 593,405 peak;
it remains INCLUDED in the totals below, not subtracted from each chart.

The conservative total is `realloc_overlap_peak + 127 * peak_blocks + 65,536`.
The 64 KiB covers fixed navigation/page/PlotState/projection scratch; the 127-byte
per-block charge retains the existing rounding/slack policy. Checked meter
arithmetic and the existing allowance guard remain in force. These are requested
allocation bytes, not an RSS or Windows heap fragmentation guarantee.

Native x86_64 Windows/MSVC results in the corrected dev and release profiles
agree exactly for these fixtures:

| Lifecycle phase | Retained | Phase peak | Realloc overlap peak | Conservative total |
|---|---:|---:|---:|---:|
| One trace, 4096 points | 493,004 | 1,801,032 | 1,806,152 | 1,934,807 |
| 32 traces / 16,384 points | 1,232,906 | 6,329,646 | 7,244,774 | 7,459,789 |
| 24 alternating observation updates | 1,873,388 | 6,730,212 | 7,645,340 | 7,896,423 |
| Pan / zoom / legend interactions | 1,242,960 | 6,160,780 | 7,075,908 | 7,322,546 |
| 21 Context recreations in one Plot | 1,205,748 | 6,325,104 | 7,236,136 | 7,438,959 |
| Plot → page → other occurrence | 434,329 | 6,329,646 | 7,244,774 | 7,446,454 |
| Skewed 4096 + 31 singletons (4127 retained) | 1,193,882 | 3,484,726 | 3,676,486 | 3,874,737 |
| 1024-byte text boundary | 1,262,689 | 6,359,429 | 7,274,557 | 7,489,572 |
| Width 32 boundary | 1,232,906 | 6,329,646 | 7,244,774 | 7,459,789 |
| Maximum → empty page | 400,206 | 1,232,906 | 1,232,906 | 1,428,490 |

All 21 owner revisions retain exactly 1,232,906 bytes after the same frames.
Three complete lifetimes leave zero measured bytes/blocks after drop, above the
one-time singleton. No SOURCE_MAP accumulation remains. The projection arena
still independently passes the B1 512 KiB suite, including allocation refusal.

**Counterexample:** 32 sources with 4096 points each, normal B1 decimation to
16,384, and 32 legal diverse Unicode display labels of up to 512 bytes. The
identical low-level widget plus CPU tessellation retains **3,581,760**, peaks at
**10,321,764**, has realloc-overlap peak **10,447,612**, and charges
**10,664,532 bytes**. This exceeds 8,388,608. The probe deliberately bypasses the
Preview guard only in this clearly labelled diagnostic reproduction; it is
NOT qualification of a different shim or executable configuration.

The production Preview checks the complete potential text repertoire before
creating plot/legend widgets: title plus all unique legend names, Runtime unit
IDs/symbols and cosmetic display units must total at most 1024 UTF-8 bytes.
Titles/legend names must be single-line, and each legend name at most 80 bytes,
because the built-in overlay has no scrolling API. Rejection shows a bounded
notice and Back; it never truncates a name or silently changes point quotas.
The production guarded Unicode case stays near 1 MiB charged. This is an
explicit **preview limitation**, not a schema/admission change or proof that
every legal chart is qualified. Arbitrary geometry/text combinations beyond the
measured envelope still require review; `ACTIVATION_QUALIFIED` stays false.

Minimal proposed next review: a bounded scrollable/paged legend/details surface,
with exact full labels accessible separately and a bounded set of text laid out
at once, while preserving TraceIdentity and all B1 curve data. If combined
geometry still needs restriction, separately review a deterministic visible
trace selection policy with explicit continuation. Do not silently change B1
water filling, raise the budget, or claim hidden traces are absent from the
document. This proposal is not implemented here.

## Reproduction and tests

`scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev -PlotOnly` and the same
with `release` build/link the actual executable dependency profiles. Each
scenario is a separate native process, with no Runtime workers or connections.
Raw evidence goes to `target/v2-renderer-allocation-probe/{dev,release}/plot.txt`.
The full launcher retains all A1/A2 CPU preview and B1 projection scenarios. The
admission/conversion/export allocation probe is unchanged.

Nine widget tests inspect actual egui text, path geometry and CPU meshes, and use
pointer presses/releases, movement, wheel and zoom events. They cover all 64
occurrences, 32 lines and 16,384 points, skewed input, positive/negative spikes,
metadata/generation/expect_kind/freshness, hidden and absent curves, duplicate
names and sources, units, segment hover, nonfinite input, legend/pan/zoom/reset,
observation updates, Context recreation and retirement on replacement. The live
owner boundary test also renders/tessellates a Plot while asserting zero client
calls and unchanged model, workflow, recovery and presentation revision.

262 selected native dev regressions passed, covering v1 GUI/Plot/hover/Confirm,
external API/Dispatcher, typed reads, v2 DTO/graph/admission/conversion/export,
all 20,480 navigable occurrences, and architecture guards. The final Plot text
preflight and last (32nd) legend interaction were rechecked with all nine widget
tests plus the live-owner boundary test (10 passed). Native A2 CPU and B1
projection suites pass in dev/release; the full release launcher also repeats
the empty Context, provenance and v1 scenarios. Formatting, standalone probe
formatting, Clippy with warnings denied and diff whitespace checks pass. No
full-workspace gate, dependency/profile change, release or activation is made.

Remaining activation blockers: full-schema Plot memory/legend layout, GPU/host
resource lifecycle, bounded editor drafts, ActionBinding execution, operator
Confirm, and the external v2 protocol/profile. No later stage is started.

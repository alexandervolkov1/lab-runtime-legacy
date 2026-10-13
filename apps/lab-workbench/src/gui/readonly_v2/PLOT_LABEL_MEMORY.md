# DGUI-4C7B2A: full-label access and text memory containment

This is the current Plot text/lifetime policy. It supersedes the text refusal,
full-label legend and unit-buffer retirement policy in the historical
[B2 report](PLOT_WIDGET.md). `ACTIVATION_QUALIFIED` remains **false**. The active
v1 Workbench, APIs, admission, dependencies and Cargo profiles are unchanged.

## Aliases, full text and identity

The built-in legend now uses fixed ASCII `T01` through `T32` in definition order.
An alias is independent of display_label and Runtime source. Actual line IDs
still hash the full B1 TraceIdentity (owner/revision, exact occurrence, Plot ID,
Trace ID, ordinal). Hover selects the ordinal within the current PlotView and
checks that complete identity. Labels, including labels equal to `T02`, never
select a source or become authority. B1 TraceView/DataBinding are unchanged.

All eligible curves are still submitted: 32 TraceViews, at most 16,384 points,
the original common frame arena, water filling and min/max decimation. Points,
timestamps, freshness/metadata fences, colors/widths, axes and interactions keep
their B1/B2 semantics. No persistent points or second observation cache is added.

Four trace selectors/ASCII diagnostics appear below the graph. `Previous traces`
and `Next traces` expose all eight ordered slices, including T32. This pages
text only, not curves. Legend visibility remains independent of label length.
`Title details` opens the complete title. `Details Txx` opens that trace's label,
with selectors for Runtime unit ID, Runtime unit symbol and cosmetic display
unit. Details replace the graph; `Back to Plot` restores its viewport/visibility.

Each fragment contains at most **16 Unicode scalars / 64 original UTF-8 bytes**.
Previous/next buttons, fragment numbers and byte offsets expose the entire field
in order. `char_indices` supplies code-point boundaries. At most 32 fragments
cover a legal 512-byte field; no text index or full-label GUI copy is retained.
Each frame borrows admitted strings or current validated typed metadata. Runtime
units retain the existing 32-byte ID / 16-byte symbol bounds. Oversized or
incompatible metadata is explicitly unavailable, not read through a raw-value
backdoor. Missing samples may still have valid descriptor units. Absent metadata
and an empty admitted string are distinguished.

Rust's Unicode-aware `char::escape_debug` preserves printable fragment text and
exposes controls/newlines/invisible format/combining characters as escapes.
They cannot create unbounded layout rows or hide navigation. A parallel UTF-8
hex line exposes every original byte, including unsupported font glyphs and
ambiguous escapes. Fixed display/hex buffers are 192 bytes each (at most 160
escaped bytes and 191 hex bytes). Canonical text is never rewritten. Tests
reconstruct full fields from the actual rendered output, not a second text API.

Graph mode supplies **only ASCII** to egui, including hover. Runtime units are
escaped exactly, without conversion. A cosmetic unit is a marked 16-scalar
preview with an explicit continuation to Details. The 2048-byte hover formatter
still says `Curve position`, shows freshness/current quality and separate
decimation/eviction counts, and rejects nonfinite positions. It does not claim
sample provenance for a position on a segment. Full Runtime/cosmetic text remains
independently accessible. No user label is silently discarded.

## Context retirement and bounded state

One PlotState borrows its occurrence and Plot definition. It stores finite
bounds, auto flags, a 32-bit hidden mask, the four-row trace-list page, an optional
field/trace/fragment cursor and one fixed prior fragment key. The key holds at
most 64 exact source bytes solely to decide Context retirement; it supplies no
readings. The complete slot is statically asserted to fit the existing 4096-byte
viewport credit. There are no owned strings, point arrays, arbitrary PlotMemory,
history, drafts or authority.

Before font/widget allocation, `prepare` constructs the fixed text page. Changed
field/trace/fragment, raw bytes, length or availability, or graph/details mode,
retires the complete Context before new glyphs are submitted. Only PlotState
survives. Changed validated unit fragments are reread and compared exactly;
there is no hash shortcut. A details Context sees at most 16 user Unicode scalars
plus fixed ASCII. A graph Context sees only a finite ASCII repertoire.

Pinned epaint 0.36.2 `Fonts::begin_pass` calls `GalleyCache::flush_cache`, retaining
only the immediately used generation. Numeric changes cannot accumulate galleys
indefinitely. Fixed font/scale, finite widget IDs and complete Context retirement
bound glyph-atlas/widget-map lifetime independently of visits and epochs. A1's
package profile still disables global SOURCE_MAP. This ownership argument,
not 21 sampled epochs alone, establishes finite history containment.

B2's public PlotMemory/PlotUi/Legend APIs still restore only validated bounds,
auto flags and the hidden mask. No old PlotMemory crosses Context lifetimes.
Pan/zoom/hidden lines survive text paging and observation updates; Reset works
in both modes. Another occurrence, Back to the component page, replacement
admission, owner or revision drops the entire slot even with identical IDs.
Ordinary four-component navigation and access to all 64 Plot occurrences remain
unchanged. No expanded graph or view history is built.

## Joint budget and preflight

The existing **8,388,608-byte** allowance guard precedes B1 projection. Its
512 KiB is inside that allowance, never added to it. Insufficient allowance
retains A2's bounded notice/static-error paths. Fixed text planning happens
before substantial egui allocations; full labels are not laid out and then
rejected post-allocation. Graph charges are 112 labels, 224 widgets/containers
and 8192 submitted text bytes, including axes/legend/hover/diagnostics. Details
charge 24 labels, 48 widgets/containers and 1024 text bytes. These are conservative
work limits, not heap measurements. Details prepare the bounded B1 projection
too, and its allocations remain included.

Measurement includes PlotView, PlotState, Context, fonts/galleys/CPU atlas,
axes/legend/details/hover, FullOutput, CPU tessellation, temporary buffers and
realloc overlap. Source admission/observations are owned before the window.
No font/Context category is subtracted; the empty Context baseline is included.
Only the independently measured one-time 24-byte ahash singleton is prewarmed.
Each output/mesh must be dropped before the next frame. No GPU host is involved.

The A2/B2 conservative formula is unchanged:

`accounted = realloc_overlap_peak + 127 * peak_blocks + 65,536 <= 8,388,608`

64 KiB covers fixed navigation/PlotState/fragment/projection scratch, with no
extra VisualReservation index. Per-block charge includes rounding/slack. This
is requested-allocation accounting, not Windows RSS/heap fragmentation. A future
shared owner must grant the allowance once, without duplicating ledger credits.

## Native evidence

The former PR #25 Unicode counterexample charged **10,664,532 bytes** when full
names entered the legend. The new probe uses the production Preview without a
guard bypass: 32 exact 512-byte labels, a 512-byte title, 32 exact 512-byte cosmetic
units, maximum valid Runtime units and all 16,384 points. Real pointer navigation
reads every byte of every field of all 32 traces. Three repertoires cover diverse
BMP glyphs, controls/combining/emoji and supplementary CJK. They include the
former counterexample; the graph opens rather than showing a refusal notice.

Windows/MSVC, corrected dev/release profiles, requested bytes; both profiles
produce exactly the same Plot probe output:

| Phase | Retained | Phase peak | Realloc overlap | Accounted |
|---|---:|---:|---:|---:|
| One trace / 4096 points | 505,500 | 1,818,864 | 1,826,648 | 1,958,732 |
| 32 traces / 16,384, ASCII or Unicode fields | 645,530 | 5,539,694 | 6,664,546 | 6,854,034 |
| All Unicode title fragments and return | 628,082 | 5,540,142 | 6,664,994 | 6,855,625 |
| All fields of all 32 Unicode traces | 628,498 | 5,540,558 | 6,664,994 | 6,855,752 |
| Unicode hover + 24 alternating observations, worst repertoire | 865,566 | 5,715,202 | 6,842,950 | 7,064,188 |
| Independent frequent curve turns, 32 sources / 16,384 | 645,634 | 6,167,254 | 7,457,226 | 7,646,714 |
| Skewed 4096 + 31 singletons, all 4127 retained | 625,866 | 2,839,126 | 3,110,818 | 3,284,050 |
| 21 recreations within one Plot | 630,852 | 5,542,832 | 6,663,588 | 6,842,027 |
| Plot to page to another occurrence | 434,329 | 5,539,694 | 6,664,546 | 6,841,715 |
| Maximum to empty page | 400,206 | 714,562 | 729,922 | 899,979 |

Maximum measured combined charge is **7,646,714**, below 8,388,608. All 21 ASCII
epochs settle at 645,530 bytes. Each of three Unicode repertoires also performs
21 owner revisions with details/graph/hover transitions: all settle at exactly
655,038 bytes. Every complete lifetime drops to **zero** measured bytes/blocks
above the finite singleton. No SOURCE_MAP accumulation is observed. The short
ASCII interaction suite still covers pan/zoom/legend, repeated Contexts, width
32 and ordinary pages. There is no point-budget or simultaneous-trace reduction.

Reproduce with `scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev -PlotOnly`
and `-BuildProfile release -PlotOnly`; omit `-PlotOnly` for preserved A1/A2/B1
suites. Each scenario is a separate native process without Runtime or unrelated
worker threads. Raw evidence and exact executable profile artifacts are under
`target/v2-renderer-allocation-probe/{dev,release}`. The admission/conversion/
export allocation probe is unchanged.

## Verification scope and remaining blockers

Six new headless tests inspect real text and pointer-driven navigation, UTF-8
boundary reconstruction, controls/combining/missing glyphs, alias-shaped labels,
distinct Signal/ConfigurationProperty identities, validated/missing/incompatible
units, Unicode hover, and viewport/visibility/reset across retirement. The nine
B2 widget tests and A2/B1/v1 regressions retain source immutability, Runtime
silence, no intents/Confirm, repeated identities and all reachable occurrences.

Verification: **268** targeted dev tests and all **52** readonly-v2 tests in
release pass. The dev selection includes v1 GUI/Plot/hover/Confirm, external API/
Dispatcher, typed reads, operator/renderer architecture guards, v2 DTO/graph/
canonical/admission/conversion/export and navigation. Plot native probes pass
in both profiles; A2 bounded-preview and B1 projection suites are preserved.
The full release launcher also repeats empty Context, ID provenance and v1
scenarios. `cargo fmt --all -- --check`, standalone probe rustfmt,
`cargo clippy -p lab-workbench --all-targets --locked -- -D warnings`, and
`git diff --check` pass. No WSL or full-workspace gate is used.

Architecturally guaranteed: full text reachability, fixed fragment/alias work,
finite state lifetime and no glyph history across fragments/epochs. Measured:
the table's combined CPU budget in the pinned configuration. Not proved by these
fixtures: a universal numerical bound for every geometry/interaction/allocator
combination, arbitrary fonts/DPI/host contexts or GPU/backend resource lifetime.
The text counterexample is addressed; **full Plot memory qualification remains
open for independent review**, and activation remains prohibited.

If remaining geometry needs restriction, a separately reviewed B2B must define
finite simultaneous trace work with explicit navigation to all other traces.
This PR does not implement it, lower B1 quotas, raise the ceiling or hide traces.
GPU texture retirement, redraw cost, active ownership/shared-budget integration,
bounded editors/drafts, actions, operator Confirm and v2 protocol/profile remain
separate blockers. No release or later stage is started.

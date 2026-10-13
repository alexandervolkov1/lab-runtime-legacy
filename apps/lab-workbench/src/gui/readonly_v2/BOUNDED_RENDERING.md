# DGUI-4C7A2: bounded CPU preview qualification

The isolated preview now separates admitted document size from submitted visual
work. No v1/v2 schema, Workbench operation, Dispatcher ownership, Runtime API,
recovery, OperatorWorkflow or persistence changes. Activation remains false.

DGUI-4C7B2 adds a separate one-Plot mode documented in [PLOT_WIDGET.md](PLOT_WIDGET.md).
The four-cell page limits below still apply to ordinary pages; Plot uses its own
bounded work within the SAME 8 MiB, including projection and CPU tessellation.
[B2A](PLOT_LABEL_MEMORY.md) adds ASCII curve aliases and lossless text-fragment
navigation with Context retirement, superseding B2's full-label refusal.
Full-schema Plot memory qualification remains open for independent review. Zoom
events are accepted only in Plot mode and never change the preview's font scale.

## Work limits and reachability

The production `Preview` owns its Context; arbitrary existing GUI Contexts cannot
be passed in. Its fixed default-font viewport is 1280 by 960 logical points at
default scale. One egui pass is permitted, including when a widget requests a
discard/re-layout. Up to 64 pointer/wheel events are accepted; text/key/zoom,
custom DPI/fonts, secondary viewports and host styling are outside this preview.

| Work/state | Limit |
|---|---:|
| Selected windows / tabs / panels | 1 / 1 / 1 |
| Immediate component cells submitted | 4 |
| Additional selected Group header | 1 |
| Occurrence-path node resolutions, including navigation validation | 28 |
| Group address slots | 4 |
| Text labels including navigation/button text | 64 |
| Submitted UTF-8 text, including navigation | 12,288 bytes |
| Bytes in any prepared text line | 1,152 |
| Widgets, conservatively including internal layout/scroll containers | 128 |
| FullOutput paint shapes, qualified pinned configuration | 192 |
| Prepared component allowance | 10 labels, 2,304 bytes |
| Additional fixed stack/inline accounting | 65,536 bytes |

`Page::prepare` runs before egui. A fixed text buffer and four cell descriptors
reserve/check complete cells; no partial value enters egui if the budget fails.
The constant bounds cover the existing read layer's caption/help, exact and
rounded scalar, bounded unit, quality, absence and freshness lines. Entity
projections are bounded before copying. Fixed navigation adds at most 256 text
bytes. Text/label/widget charges are checked again before any page is painted.
The widget charge includes 64 conservative container slots in addition to all
labels/buttons. The pinned egui output shape ceiling is independently asserted
by headless tests and the native probe, not used as a post-allocation guard.

A page visits at most four immediate references. A selected Group header walks
at most four nodes; each cell at most five, plus four for validating a new group
address: `4 + 4*5 + 4 = 28`. Registry lookups remain bounded by admission's
definition limits. No per-frame expanded-occurrence scan or subtree index exists.

Each admitted list has at most 64 members. Previous/next page exposes every
ordered slice of four. Previous/next window/tab/panel exposes every arrangement.
Entering a Group appends its occurrence position; Parent recovers the containing
page from that position. By induction on depth (at most four), every occurrence
is reachable, including repeated roots, siblings and shared/diamond subtrees.
No definition is deduplicated in the view. The maximum fixture is verified over
5,120 pages containing exactly 20,480 distinct occurrence IDs and real painted
component captions. Tests also address window 64/tab 64 in an admitted 127-tab document.
The independent lexical admission ceiling also applies; nominal arrangement
count maxima need not all be simultaneously representable in one admission.

Row/column is preserved within a page's immediate children. The entire nested
composition is not shown simultaneously. Long pages/text can be scrolled in both
axes; paging never hides a component permanently. This is explicitly a preview
layout policy, not a claim of full simultaneous v2 rendering compatibility.

## Finite ownership mechanism

Navigation retains selected indices, four group positions, a page index and two
scroll coordinates. There is no visit history. The admitted document is borrowed.
`replace` compares the immutable admission reference and owner/revision; all
navigation and Context state is retired on replacement, even for identical IDs.
Occurrence hashing remains the DGUI-4C7A algorithm. Navigation IDs additionally
contain document identity, owner, revision and the addressed page.

One fixed previous paint page is retained. Every frame resolves current readings
again and compares the complete prepared page, including exact text bytes, with
that page. Changed text, page, admission or epoch drops the old Context before
the next one is created. The previous page supplies no Runtime values to reads.
Unchanged pages use the same finite set of widget IDs and strings. egui's
per-ID animation/scroll maps therefore cannot grow across page/epoch histories;
glyph/font caches cannot grow across changing Runtime strings either. Default
fonts, sizes, widths and scale remain fixed; no input can create new zoom/font
variants. This storage/lifetime argument, rather than 21 sampled epochs alone,
establishes containment. The package profile from DGUI-4C7A1 separately removes
process-global SOURCE_MAP storage.

Changing text preserves only the page's local scroll coordinates; no egui state
is copied. Moving to another page resets them. Navigation uses primary press
inside its current clipped rectangle, independently of previous-frame focus or
click state. Continuously changing observations cannot prevent navigation.

## Accounting and guard

The read-only preview requests a uniform **8,388,608-byte visual allowance**.
This is the existing presentation visual ceiling, not a new document limit.
The offline `VisualReservation` still admits and accounts for the unchanged
document (5,284,672 bytes for the maximum fixture). Its 256-byte-per-occurrence
view credits and editor/plot credits are not allocated by this preview. They
must not be added as imaginary allocated objects, or used to pretend an empty
document's 4096-byte ledger header covers egui. This implementation requires the
complete uniform allowance for its actual read-only Context/page instead.
A future combined owner must grant this allowance explicitly and must not spend
the same credits simultaneously on v1, drafts, plots or another Context.

Before preparing values or allocating a Context, insufficient allowance selects
a fixed budget notice if at least 1,048,576 bytes are available. That path creates
no component cells. Below the notice allowance it returns a static explicit
error without any Context; the host must surface that reason using its own
already-reserved status surface. The native probe checks both pre-work paths.

All Context, font, glyph, CPU atlas, galley, widget map, paint output and temporary
read/layout heap requests are included. No font/cache category is subtracted.
Source admission, observations and fixture construction belong to their existing
owners and remain outside the window. Each output is consumed/dropped before the
next frame; retaining an output history is not a supported ownership contract.
The only excluded global initialization is the separately reproduced one-time
24-byte/two-block ahash singleton; subsequent Context drop must leave zero bytes.

The conservative accounting charge is:

`realloc_overlap_peak + 127 * peak_blocks + 65,536 <= 8,388,608`

127 per block bounds admission's existing policy of rounding a request up to 64
bytes and adding 64 bytes of allocator slack. The block peak includes a possible
extra realloc overlap block. The fixed 64 KiB includes navigation/epoch headers,
the retained and preparing text pages and bounded read scratch. Arithmetic in
the allocation meter and charge checks is checked. Requested-byte and block
peaks are measured independently, making the combined charge conservative.
This is capacity accounting, not a claim about Windows RSS or heap fragmentation.

## Native evidence

Windows x86_64 MSVC, Rust 1.95.0, locked egui/eframe 0.36.2. The launcher verifies
the actual executable artifacts: egui dev debug assertions off/overflow checks
on; application and other dev crates retain normal checks. Release is measured
in separate native processes. No dependency, feature, profile or lock change.
No Runtime connection, GUI worker or unrelated worker thread is started.

Results below are requested heap bytes. Peak includes transient work and old
state at the start of a phase; realloc-overlap peak equals phase peak in these
fixtures. Both profiles reproduce the same byte figures.

| Phase | Retained | Phase / realloc-overlap peak |
|---|---:|---:|
| Warm empty Context baseline | 326,161 | 593,405 |
| Budget notice | 407,677 | 698,469 |
| One Indicator, settled | 446,639 | 729,295 |
| Maximum document, initial page settled | 435,094 | 712,434 |
| All 5,120 pages, first-frame maxima | 446,003 | 732,955 |
| Last page settled; 32 more identical frames | 457,003 | 480,759 |
| Depth-four shared/diamond graph | 445,208 | 734,176 |
| Long Unicode captions/help/entity text | 1,709,235 | 2,252,043 |
| 512-newline fields and long Group headers | 1,252,322 | 1,709,235 |
| Maximum document then empty preview | 406,054 | 712,434 |
| Drop; four subsequent Context lifetimes dropped | 0 | — |

The worst stress phase has 1,071 peak blocks: its conservative charge is
**2,453,596 bytes**, below 8 MiB. All 21 new epochs settle at 446,639 bytes/362
blocks for the one-Indicator page; none retains its predecessor's capacities.
The maximum fixture's measured paint-shape count is at most 43 per frame.
Numeric and object ID probes still allocate zero provenance bytes in both fixed
profiles. The old 66,153,808-byte all-occurrences Context path is gone.

Reproduce with `scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev` and
`-BuildProfile release`. Results and exact Cargo artifact profile evidence are
written under `target/v2-renderer-allocation-probe/{dev,release}`. The independent
admission/conversion/export probe remains `scripts/test-v2-allocation.ps1`.

Verification: 237 distinct targeted dev tests (the 236-test regression run plus
the final arrangement/scroll case, with changed semantic/lifecycle tests rerun),
all 21 renderer tests in release, both native renderer profile probes, the native
admission/conversion/export probe, `cargo fmt --all -- --check`, standalone probe
rustfmt, `cargo clippy -p lab-workbench --all-targets --locked -- -D warnings`, and
`git diff --check`. Targeted regressions include v2 DTO/graph/canonical/admission/
conversion/export, v1 presentation/GUI/Plot/hover/Confirm, controls, application,
external API, Dispatcher, typed reads and operator architecture boundary guards.
No WSL, full-workspace gate or release operation was used.

## Qualification boundary

The isolated fixed-configuration CPU preview's bounded submission, navigation
and Context-lifetime memory blocker is addressed. This does **not** qualify an
arbitrary host Context or an activated Workbench renderer. A host must consume
each output before replacement, retire GPU texture resources with the Context,
and qualify tessellation/GPU/backend allocations and redraw cost. Recreating a
Context on changing values is intentionally conservative and can be expensive;
the preview is not a frame-rate or backend-integration qualification.

No shared active Workbench budget/owner integration is implemented. Full nested
layout compatibility, v2 Plot, bounded drafts/editors, actions, operator Confirm
and v2 protocol/profile still require independent reviewed work. Activation
remains false, and DGUI-4C7B is not started.

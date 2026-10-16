# EFRAME-3 Windows/MSVC validation

Qualification date: 2026-10-16. Scope: experimental isolated **standard eframe**
read-only Workbench, with an external Babashka client configuring the existing
Parent. **ACTIVATION_QUALIFIED=false. MP-1 remains OPEN.** EFRAME-3 requires
independent review and must not be merged on this report alone.

## Base and exact inputs

PR #48's remote HEAD was exactly the reviewed
`2ac7971f79f3a5aded0c6282b0f09a8e204bac02`; its remote main base was
`6205d6c1751420ecce4ae85c6d52a21b2d6c8264`. All 80 EFRAME-2 canonical Git-byte
checksums passed. The post-implementation Rust changes were confined to the
already documented test observer and absolute CLI test fixture; the remaining
followups were fixture/evidence/documentation changes.

The authorized ordinary merge commit is
`fcf340c74efac9e422635d5b736c606b8f4a674d`, with those base/head parents and a tree
identical to the reviewed head. Main was fast-forward synchronized and clean
before the EFRAME-3 branch was created. PR #43 remains the historical OPEN
Draft/BLOCKED at `04c0e5180a4bc45ae12c7b62fdb3b86ad8f10e7c`.
See [merge provenance](eframe-3-evidence/eframe-2-merge.json).

Production stock-host changes are in
`165524aa1909ae7c1b3e74741f820aeafff0b489` and the large-document correction
`f3a8445b054efa4190cfd847275fb5964b6561d4`. The initial external fixture is
`2a2af37e6d9d99b079408a8921a12649164527bf`; final stock-host gates and the complete
Clojure experiment use `f3a8445b054efa4190cfd847275fb5964b6561d4`. The root Workbench,
IPC, Supervisor, fmt/Clippy and dependency gates at `2a2af37` are reused: all apps,
crates and root manifests are byte-identical at `f3a8445`. The
[root source reuse proof](eframe-3-evidence/root-source-reuse.json) lists the only
three changed executable paths (stock IPC service and external fixture/client).
Subsequent changes are documentation and evidence only. The [476-file canonical source inventory](eframe-3-evidence/source-inputs.json)
covers apps, crates, scripts, clients, examples, root manifests and the neutral
contract include_str fixture. Runtime, Recorder, OutputAuthority, public APIs,
Parent, Supervisor, private protocol, canonical admission, Custom Glow and the
shared stock plotting function have **no source changes** relative to the merge.

[Binary identities](eframe-3-evidence/binary-identities.json) retain the actual
six-artifact set used by every final Clojure/normal-close run; all final case
hashes match that same current set. Previous-stage identities remain in the nested
archive, rather than being substituted into final receipts.
Native qualification scripts also build Workbench alone; selecting Runtime and
Workbench together changes shared Cargo feature unification, recorded in the
[root feature comparison](eframe-3-evidence/root-build-feature-comparison.json).
Later joint rebuilds also yielded different Workbench file hashes. Overwritten
intermediate executable bytes were not retained, so their byte differences are
not diagnosed. Bit-for-bit build reproducibility is not claimed. Each run keeps
its original source and exact executable hashes; four final normal-close repeats cover
the current joint-build artifacts and the corrected stock child. No identity was
silently substituted in a prior receipt.

The independent host still uses exactly the same 168 active third-party packages,
versions, features, archive hashes, extracted source checksums and licenses as
EFRAME-2. Root and independent lockfiles are unchanged. The independent working
lock's raw checksum changed only with checkout CRLF: normalized working bytes
equal the unchanged canonical Git bytes. See the [exact dependency comparison](eframe-3-evidence/dependency-comparison.json),
[inventory](eframe-3-evidence/dependencies.json) and
[feature list](eframe-3-evidence/features.txt). No new dependency/toolchain review
is being substituted for the existing exact-version license evidence.

## Real external virtual furnace experiment

Four successful experiments used real `lab-runtime.exe`, `lab-workbench.exe` and
`lab-eframe-host.exe`, in dev/release with explicit stock WGPU/Glow selection.
The [reproduction guide](eframe-3-readonly-workbench.md) and committed
`clients/babashka-smoke/eframe3.clj` describe the exact commands. The fixture uses
only the virtual thermal plant: 80-degree initial temperature, 20-degree ambient,
120-second time constant and required run-local Recorder. Passive cooling changes
real Runtime observations; neither client nor child fabricates measurements.
No controller/output commands are needed.

The external Babashka 1.13.220 process opens only the existing Workbench endpoint.
It uses hello, client_status, presentation_get and ui_replace_document, verifies
exact readback, and never opens a Runtime socket. Initial revision 3 contains two
windows, three tabs, Scalar/Plot, an absent-source probe and six disabled control
groups. Revision 4 changes the heading, plot time window from 120 to 20 seconds,
y bounds and line color/width. Revision 5 adds 30 inactive windows with 52
long-title tabs each, making a 979,038-byte canonical document without changing
measurement sources. Both replacements run while minimized, through the same
Clojure client and unchanged public API. The Parent is the sole presentation owner.

The native input sequence exercises stock pan, secondary-button boxed zoom,
clickable legend hiding, focused/all-panel views, tab/window navigation, disabled
controls and unchanged API document readback. Two traces deliberately share the
same real furnace source to test distinct legend names; this is **one independent
measurement**, not qualification of two independent sensors. Selected curve
units remain degrees C; incompatible/absent sources are explicitly unavailable.
Plot bounds, hidden legend count and occurrence identity are read from standard
egui_plot memory for diagnostics, not calculated by a replacement plot algorithm.
Panel filtering preserves the same occurrence's pan/zoom/legend state.

Large-document admission was observed after 1.788 / 2.356 / 1.845 / 1.961 seconds
for dev WGPU/Glow and release WGPU/Glow respectively. These observations include
external client/API work and a once-per-second diagnostic sample; they are not
precise IPC transfer-latency measurements. The original two-second transport
completion deadlines remained enforced and none fired in these final cases.

All four cases show changing Scalar values and Plot endpoints, original units,
Fresh and Unavailable states. The screenshot pairs in the evidence directory
show the real window: for example [dev WGPU live](eframe-3-evidence/dev-wgpu-live.png),
[dev Glow replaced/restored](eframe-3-evidence/dev-glow-restored.png),
[release WGPU disabled controls](eframe-3-evidence/release-wgpu-controls.png) and
[release Glow Stale](eframe-3-evidence/release-glow-stale.png). Each PNG has its
original adjacent JSON receipt. Controls below the viewport are still traversed;
the screenshot represents the current scroll viewport, not every admitted root.
The CPU regressions separately prove complete traversal and addressability at
the documented bounds rather than inferring it from a single screenshot.

## Minimize, crash and continuity

Actual Windows minimize lasted at least 120 seconds in every case, well beyond
the existing eight-second Supervisor observation deadline. The HWND/PID was
checked throughout. The Babashka replacement ran while minimized. Before restore,
the latest diagnostics showed advancing logic and selected snapshots, unchanged
UI call count, and admitted revision 5. Each large replacement has a separate
admission observation and canonical-byte receipt. The original child/incarnation
survived.

| Profile/backend | Minimized seconds | Logic calls before → after | UI calls before → after | Selected snapshot before → after |
|---|---:|---:|---:|---:|
| dev WGPU | 121.958 | 1613 → 2704 | 1592 → 1592 | 350 → 1579 |
| dev Glow | 122.524 | 1531 → 2633 | 1514 → 1514 | 289 → 1526 |
| release WGPU | 121.983 | 570 → 1670 | 549 → 549 | 126 → 1350 |
| release Glow | 122.114 | 572 → 1671 | 550 → 550 | 125 → 1349 |

The original one-step IPC service reproduced HeartbeatDeadline during exploratory
minimize. A further legal 818,711-byte v1 replacement reproduced GUI shutdown (OsFailure)
with the first correction's shared 65,536-byte budget. It had already passed small
document tests; those earlier results are retained separately and are not the
final large-document qualification. A focused probe of the final correction
admitted a 989,633-byte canonical document while hidden in about 1.12 seconds.

The final correction drives at most eight nonblocking framing steps per actual
logic callback, with four completed-message credits and separate allowances of
4 KiB control and 128 KiB bulk (two existing chunks). No buffer, queue or snapshot
credit is added. This keeps legal large documents within the unchanged two-second
transfer deadline while eframe throttles hidden logic. No
Supervisor deadline, repaint requirement, background progress counter, GPU
completion or second owner was introduced. The actual no-UI-pass counters above
are evidence for this correction, not a claim about GPU work.

After restore, each experiment kills only its verified original child PID.
Parent reports ExitedContained with original Job/process/thread objects all zero,
IPC retired, zero IPC handles and pending I/O, before incarnation 2 restores the
latest Parent-approved document using the same explicit renderer. The existing
Supervisor remains the authority for that retirement; test taskkill is merely
the fault injection.

Across navigation, minimize, document replacements and GUI crash/restart:

- The relay observes **one Runtime connection** and the same boot/scope.
- The nine original Parent requests remain hello, discover, measurements_current,
  reference, controller, recording_status, configuration_properties, discover,
  subscribe. No additional request is caused by display or GUI recovery.
- Runtime next_seq remains 1. There are zero Runtime mutations, operation_status
  requests or Exact Retry operations. Recovery summaries remain unchanged at
  active/quarantined/reconciliation counts zero and generation 1.
- All 12 configured editor/action widgets in the six groups are disabled. Native
  input on a disabled action changes neither document nor Runtime requests.

Only **after** those continuity assertions does a separate explicit termination
of the virtual Runtime exercise the Stale display. That independent Runtime loss
is not attributed to the GUI crash. The replacement child then closes normally
through Closed/process exit/original IPC retirement.

Four separate final normal-close cases keep Runtime running while closing the first
GUI incarnation, verify Parent exit 0 and complete process/IPC retirement, and
confirm Runtime remains alive. Earlier normal-close passes on intermediate
build artifacts are retained in the previous-stage archive as supplementary evidence. See
[main experiments](eframe-3-evidence/e3-results.json),
[normal-final results](eframe-3-evidence/normal-results.json),
[continuity assertions](eframe-3-evidence/runtime-continuity.json) and the archived
external API/wire transcripts. The final canonical matrix comprises 12 child processes
across eight runs. Earlier smaller-document experiments and normal-close artifacts
are separately identified in the previous-stage archive.

## Gates and memory

All [46 recorded gates](eframe-3-evidence/gates.json) passed with identical source
SHA at each command start and completion: 14 final-source gates and 32 reused or
previous-stage gates. The root source reuse is explicit above. The record includes
builds/rebuilds,
experiments and supplementary normal-close passes, not 46 distinct test suites.

| Check | dev | release |
|---|---:|---:|
| Complete Workbench unit/regression suite | 543 passed, 58 ignored | 543 passed, 58 ignored |
| New stock adapter CPU regressions | 5 passed | 5 passed |
| Native private IPC qualification | 22 passed | 22 passed |
| Native Windows Job/Supervisor qualification | 7 passed | 7 passed |
| Real Babashka furnace, WGPU and Glow | 2 passed | 2 passed |
| Normal-final close with live Runtime, WGPU and Glow | 2 passed | 2 passed |

Workspace/independent-host fmt and warning-denied Clippy passed; Workbench Clippy
includes all targets/features. The adapter tests cover 64 roots with 384 nested
occurrences, all 64 panels/windows/tabs, revision reset, contract examples,
independent occurrence IDs and inert Button/Toggle/Integer/Number/Text/Select.
The unchanged-source Custom Glow native allocation/geometry/retirement and
compile-fail evidence remains in EFRAME-2 at its original provenance; it is not
represented as a newly executed EFRAME-3 GPU qualification. Existing Custom Glow
CPU tests are included in the repeated full Workbench suite.

The measured maximum Windows process and Job peak commit across the two furnace
incarnations and corresponding normal-final run is below. These are OS readings,
not GPU allocation estimates. See [individual samples](eframe-3-evidence/memory.json).

| Profile/backend | Process peak bytes | Job peak bytes |
|---|---:|---:|
| dev WGPU | 443,961,344 | 450,445,312 |
| dev Glow | 176,553,984 | 183,123,968 |
| release WGPU | 432,533,504 | 439,095,296 |
| release Glow | 140,931,072 | 147,476,480 |

Every launch explicitly requested a 1,073,741,824-byte process/Job **test** cap.
No cap was raised on failure or installed as a deployment default. MP-1 remains
OPEN. Hardware observed here is AMD Radeon(TM) Graphics: stock WGPU/Vulkan and
stock Glow/OpenGL 3.3 Core, driver 25.10.30.02. Toolchain is Rust 1.95.0 MSVC,
Windows 10 build 19045, Python 3.14.6. Environment and original framework logs
are retained. No portable GPU/VRAM budget, hardware allowlist, monitor/IME/DPI
transition, multi-day lifecycle or laboratory hardware acceptance is claimed.

## Evidence integrity and development failures

The [verification receipt](eframe-3-evidence/verification.json) records 60 selected
E3 PNGs with complete-file, CRC, decode, SHA and pixel checks; the four normal-final
cases add 16 verified PNGs. Sixteen representative PNG/JSON pairs are directly
versioned. Stock eframe Screenshot is the capture API; no graphics API readback
implementation was added.

The [raw archive](eframe-3-evidence/raw-evidence.zip) is 32,033,039 bytes with 636
outer members, SHA256
`f987972877e4a609b2ccc8c73ea683ced2734bafe375defd8dbeeeba89f3c92e`.
Every member's CRC, length and SHA256 was checked against the
[manifest](eframe-3-evidence/raw-manifest.json). It retains actual run configuration,
API/wire transcripts, Parent/child lifecycle and framework logs, all final
qualification child captures (including unpublished temporary captures at crash), gate logs/receipts,
and virtual Recorder databases together with their retained WAL companions.
The previous 32-gate stage remains nested as `previous/raw-evidence.zip`
(17,042,328 bytes, 1,000 members, original SHA256
`09fb86da69d148f05e857c79d25c6c0024507bf2faf4b8b472c2cdeb4eda8170`), with its original
manifest and verification receipt. Its small-document passes are distinct from
the final large-document matrix. Original run directories and databases were not
removed. A readable archived database is not asserted to be a Recorder flush/seal receipt.

Exploratory dirty-tree runs are separated from the committed-source qualification:
ordinary wheel versus zoom input, click pacing, pre-transition navigation
rectangles, foreground/keyboard-state handling, the actual minimized heartbeat
failure, and a 30-second exploratory pass. Their raw logs, exact binary hashes and
dirty flags remain in the archive. Exact intermediate uncommitted source
snapshots were not retained; those runs are diagnostic evidence, not qualified
source claims. The [provenance notes](eframe-3-evidence/provenance-notes.json)
identify each case and the Workbench artifact rebuilds explicitly.

No release, production activation, automatic renderer fallback, new Runtime API,
GUI instrument control, Custom Glow change, Linux/macOS process host or M-CLEAN
was performed. Independent EFRAME-3 review remains required.

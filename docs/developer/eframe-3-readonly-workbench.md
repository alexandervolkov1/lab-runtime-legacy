# EFRAME-3: usable configurable read-only Workbench

This experimental Windows/MSVC mode uses the standard `eframe::App`, `egui`
and `egui_plot` connected in EFRAME-2. Ordinary Workbench v1 remains the default.
`ACTIVATION_QUALIFIED=false`; MP-1 is OPEN. No release, production activation,
CONTROL, M-CLEAN or laboratory hardware acceptance is part of this change.

## Ownership and scope

The existing Parent remains the only PresentationDocument owner and Runtime
client/scope owner. External configuration uses the existing loopback Workbench
API and dispatcher. Canonical conversion, document admission, selected-read
provenance/freshness/generation checks, four IPC credits, Windows Job supervision,
exit observation and original IPC retirement are retained.

Only the independent stock host's drawing adapter and bounded IPC servicing
change. Runtime, Recorder, OutputAuthority, public APIs, Parent, Supervisor,
private protocol, shared admission code and Custom Glow sources are unchanged.
The child has no Runtime connection, command dispatcher, editor candidates or
action callbacks. No GPU Device, Queue, textures, fences, FrameBackend,
HostSession or Preview custody is added.

## Usable views

- Named window/tab selectors and previous/next buttons address the whole admitted
  document. A panel selector, Next panel and All panels support focused plots or
  a scrolling overview. Long selector labels are truncated in a fixed-width
  selector rather than moving adjacent navigation buttons.
- Every root and admitted group occurrence is visited. Row groups use standard
  wrapping layout; column groups stack. Status, Indicator, Plot, labels and help
  are drawn directly from the admitted DTO. The former first-four-roots cutoff
  and two-panel page restriction are removed. No document limits are raised.
- Button, Toggle, IntegerInput, NumberInput, TextInput and Select are displayed
  disabled. Their temporary display values never become retained candidates.
  Missing or incompatible selected data remains explicitly unavailable.
- Indicators respect admitted numeric precision. Scalars preserve original units
  and selected origin generation; freshness may be downgraded, never promoted by
  drawing. Unsupported entities and absent sources do not acquire fake values.
- Plots use the existing shared stock `egui_plot` drawing function. Line colors
  include hexadecimal document colors; widths, trace visibility, original units,
  requested compatible units, document time window and y-axis bounds are honored.
  Time is Runtime monotonic seconds, with original value units on the y axis and
  hover text. No unit conversion is invented. Several units on one axis are
  explicitly listed; this is not an independent calibrated multi-axis plot.
- Stock drag pan, Ctrl+wheel/pinch zoom, secondary-button boxed zoom,
  double-click fit, axes, hover and clickable legend remain standard library
  behavior. Duplicate trace names receive the shared helper's distinct names.
  Each graph occurrence has its own stock ID; panel filtering retains the same
  occurrence's plot memory. A replaced revision clears obsolete widget memory.

Rendering uses temporary points borrowed/derived from the current selected
snapshot, not a second retained measurement or presentation model. The existing
canonical admission, expanded graph bounds and 32 selected-source / 16,384 total
IPC point limits still apply. This adapter does not make an otherwise
unpublishable document admissible or expand the selected-read contract. The
public Workbench API remains v1; direct neutral-v2 adapter fixtures are offline
tests, not a new public configuration endpoint.

## Hidden-window lifecycle defect and correction

The actual minimized-window experiment reproduced `HeartbeatDeadline` with the
old child servicing. eframe 0.36.2 deliberately throttles hidden-window logic to
approximately 100 ms and does not run a UI pass. The original one-step framed
read needed separate header/payload completions across those ticks, delaying
responses beyond the Supervisor's exact current challenge.

A second boundary experiment then reproduced a bulk TX deadline with a legal
818,711-byte v1 document while minimized: control traffic could consume the byte
credit needed to deliver a full bulk chunk. A 989,633-byte canonical replacement
passed after the correction.

The stock child now performs at most eight nonblocking framing steps per logic
callback, sharing the original four completed-message credits. Separate finite
allowances are 4 KiB for control and 128 KiB (two existing chunks) for bulk. This
scheduling allowance adds no buffer, queue or snapshot credit. It lets the existing
1 MiB document fit within the unchanged two-second transfer deadline under
hidden-window throttling. The child still sends at most one control response per call.
Only an actual logic callback advances the owner counter; no paint or GPU
completion is fabricated. No thread, second IPC owner, deadline relaxation or
Supervisor policy change is needed. The original snapshot deadlines, challenge
validation, close handshake, process-exit and retirement fences remain intact.

The explicit capture fixture permits at most 128 stock screenshot requests per
child, at least 750 ms apart, bounded to 3200x2000 pixels. The existing capacity-one
CPU capture queue drops excess captures; one worker may encode one image while
one waits. Completed PNG publication is atomic. The latest lifecycle JSON is
overwritten at most once per second only with capture enabled. These are finite
diagnostics, not an Application API, GUI control channel, progress authority or
GPU memory policy. The historical `target/eframe-2-parent/` diagnostic directory
name is retained for compatibility with the existing fixture.

## Reproduce the external virtual furnace experiment

Prerequisites: an interactive Windows desktop, Rust/MSVC 1.95 and the existing
pinned dependency graph, Python 3 and Babashka (`bb`) on PATH. No new dependency,
toolchain or graphics backend is installed. Use a unique run name each time.

```powershell
cargo build --locked -p lab-runtime -p lab-workbench
cargo build --locked --manifest-path scripts/support/eframe-host/Cargo.toml
python scripts/support/eframe-host/e3.py dev wgpu my-dev-wgpu
python scripts/support/eframe-host/e3.py dev glow my-dev-glow

cargo build --locked --release -p lab-runtime -p lab-workbench
cargo build --locked --release --manifest-path scripts/support/eframe-host/Cargo.toml
python scripts/support/eframe-host/e3.py release wgpu my-release-wgpu
python scripts/support/eframe-host/e3.py release glow my-release-glow
```

The driver launches real executables with a run-local copy of
`examples/runtime.virtual.toml`: loopback port 0, initial virtual temperature
80 degrees C, 20-degree ambient and a 120-second thermal time constant. Passive
cooling supplies real changing Runtime measurements without controller/output
commands. Recording stays enabled with its required policy and a run-local path.
Keep the resulting database and any WAL/SHM together; the experiment does not
assert an orderly Recorder seal after its explicit Runtime-loss injection.

The driver invokes the external client, never constructing documents in Python:

```powershell
bb clients/babashka-smoke/eframe3.clj PORT initial NEW_INITIAL.json
bb clients/babashka-smoke/eframe3.clj PORT updated NEW_UPDATED.json
bb clients/babashka-smoke/eframe3.clj PORT boundary NEW_BOUNDARY.json
bb clients/babashka-smoke/eframe3.clj PORT verify NEW_READBACK.json
```

`PORT` is the existing Parent's `workbench_endpoint` port. The client uses only
`hello`, `client_status`, `presentation_get` and `ui_replace_document`. It reads
the Parent's existing signal selection, installs scalar/plots, two windows,
three tabs and six disabled control groups, and verifies exact API readback.
Updated configuration changes the title, time window, y bounds, line color and
width. `boundary` adds 30 inactive metadata windows with 52 long-title tabs each,
within the existing limits, to make the converted canonical document exceed
900,000 bytes. It changes no measurement source or instrument state. All calls pass through the original Parent dispatcher. The client creates
no Runtime socket and submits no instrument/recovery operation.

The native driver verifies live Scalar/Plot changes and original units, pans,
boxed zoom, legend hiding, panel/tab/window navigation, unavailable data and
disabled controls. During 120 seconds of actual Windows minimize it performs
the normal and large Babashka replacements, checks advancing logic/snapshots with no UI passes,
then restores the original HWND/incarnation. It crashes only the verified child
PID, checks original process/IPC retirement and same-renderer recovery of the
latest document, then verifies unchanged Runtime boot/scope/next sequence and
wire request inventory. An independent virtual Runtime termination occurs
**after** those continuity checks to exercise Stale. Finally the new child closes
through the original graceful handshake. Separate E2 normal-close runs verify
that an otherwise running Runtime survives ordinary GUI close.

Outputs are unique under `target/eframe-3/RUN/`; child receipts identify their
original Parent/PID/incarnation directory. The transparent relay records the
Parent's exact Runtime requests and never constructs a request or scope.
Native actions verify HWND/PID and use the actual foreground window and Windows
input. Screenshots come from eframe's stock Screenshot command, not GPU access.
Keep this desktop available to the test while native input is being exercised.

The driver explicitly supplies 1 GiB **only as a test fixture limit**. Actual
experimental launch still requires the caller to choose a positive process/Job
commit limit. MP-1 remains OPEN; the observed peaks and this test value are not
a deployment capacity policy or an internal GPU memory manager.

See [validation and evidence](eframe-3-validation.md) for exact source/binary
identities, four profile/backend experiments, regressions and limitations.

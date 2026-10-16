# EFRAME-1 test-only standard host

This independent Windows/MSVC test workspace selects stock
`eframe::Renderer::Wgpu` or `eframe::Renderer::Glow` explicitly. One `eframe::App`
paints the same deterministic read-only fixtures. eframe owns native graphics,
tessellation, submission and screenshot readback. The shared `gui/plot_ui.rs`
function also paints the existing Workbench v1 plots with `egui_plot`; it owns
neither a document nor a client. Custom Glow and its Preview remain separate.

```
python scripts/support/eframe-host/audit.py
python scripts/support/eframe-host/qualify.py dev UNIQUE_RUN_NAME
python scripts/support/eframe-host/qualify.py release ANOTHER_UNIQUE_RUN_NAME
```

Commit executable inputs before qualification. Every run uses a new directory;
failed first attempts must be retained. The executable's `--launch wgpu|glow DIR`
creates one suspended Job child with arguments `--renderer wgpu|glow --receipts
DIR`. There is no automatic renderer selection, fallback or restart. Direct
child invocation is for the test harness only; use the launcher for capped tests.

The test ceiling is **1,073,741,824 bytes** for both process and single-child Job,
chosen before the first EFRAME-1 graphics trial to cover a standard windowed
backend, font atlas, swapchain, bounded fixture and stock screenshots at changing
sizes. This is a finite experimental ceiling, not a proven portable memory policy.
It is not derived from the historical 224/768 MiB qualifications and is never
raised on failure. Original membership and limits are verified before resume;
original process exit is observed before all capsule handles retire. The runner
has a 90-second containment deadline and 95-second exit-confirmation deadline.

Fixtures contain at most two 128-point curves, six screenshots, a 64-character
local input field and a one-byte stage control file. Capture serialization rejects
images above 3200×2000; this bounds test output after stock readback, not eframe
internal allocations. Logs retain at most 4096 records of 4096 characters each.
No custom GPU object accounting or allocation guarantee is asserted. Native
limits and process isolation are containment, not proof of VRAM reclamation.

The Python harness checks the original HWND's PID before every native operation.
It sends Windows mouse/key/text messages, resizes the window, minimizes/restores
it and posts WM_CLOSE. These are OS-injected integration tests, not physical
keyboard, hardware IME or monitor-switch qualification. Stock screenshots test
1100×720, 800×600 and 1440×900 physical client areas, egui zoom 1 and 1.5, and the
actual native DPI reported by eframe/Windows. Changing egui zoom does not prove
an OS DPI transition. Fault cases abort the process or deliberately stall its UI
after the first frame; the original Job contains the stalled child.

The EFRAME-1 fixture commands above create no Runtime, Workbench Parent, recovery
journal, private IPC endpoint, apparatus,
network connection or public API is created. The one-byte file is test-driver
control only, not a proposed replacement for private IPC. EFRAME-2 requires a
separate review of the real Parent/child snapshot adapter. MP-1 remains open;
SUP-1's original failure remains in RENDER-2A evidence. `ACTIVATION_QUALIFIED=false`.


## EFRAME-2 experimental Parent integration

The same executable also accepts only the original Parent-generated private IPC
launch arguments for the stock child. Start that path through the real
`lab-workbench.exe` opt-in, with an explicit executable path, renderer and process/
Job commit limit. It creates no Runtime client in the child. Parent retains its
one client/scope, dispatcher, document and recovery journal. Default Workbench v1
startup is unchanged; no renderer fallback or deployment memory default is added.

See [EFRAME-2 design and launch](../../../docs/developer/eframe-2-standard-integration.md)
and [validation](../../../docs/developer/eframe-2-validation.md). Finite E2E fixtures
start a real, exclusively virtual Runtime and real Workbench, then use the existing
Workbench API to replace the document. Each normal/fault case uses a unique output
directory and checks complete PNG CRC/pixels, client traffic and original process/
IPC retirement. `ACTIVATION_QUALIFIED=false`; MP-1 remains open.

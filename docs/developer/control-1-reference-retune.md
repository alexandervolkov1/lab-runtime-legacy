# CONTROL-1: explicit Reference retune in the stock isolated Workbench

Scope: virtual Runtime, stock eframe WGPU/Glow, existing Workbench v1 document
and operator semantics. `ACTIVATION_QUALIFIED=false`. Ordinary GUI v1 remains
the default. No physical instrument acceptance, release or activation is implied.
The [current desktop memory requirements](project-status.md#current-desktop-memory-requirements)
apply; MP-1 is OPEN and deferred outside this plan.

## User workflow

An external Babashka client configures a v1 `controls` panel with a
`set_reference` control targeting an existing **ramp Reference** through
`presentation_get` / `ui_replace_document`. No document or public API fields are
added. The stock adapter uses the existing lossless `v1_control` origin on the
canonical Group to display the current Reference and bounded target/rate drafts.
It does not activate or reinterpret the v2 `reference_fixed` action family;
the ordinary GUI v1 fixed-value control is unchanged. All other stock GUI control
kinds remain inactive.

Target is finite in the Reference's Runtime unit; rate is finite and positive in
units/second. Editing sends nothing. **Apply retune** asks Parent to prepare an
existing `OperatorIntent::RetuneReference`. Parent returns its frozen target,
rate and Runtime revision for a separate **Confirm retune** action. Confirmation
is mandatory even if the document's `confirm` flag is false, matching the existing
operator workflow. Only that confirmation can submit `reference_retune`.

The window separately displays local draft, confirmation, submitted/unknown
admission, accepted/in-progress, completed/failed/unknown outcome and the live
observed Runtime Reference. A terminal operation result includes the returned
Reference revision/value/target/rate; it does not overwrite the observed value.
Terminal acknowledgement clears the current workflow, while the existing bounded
Parent action history/recovery journal remain. Uncertain work cannot be cleared
or retried through CONTROL-1; use the existing explicit recovery workflow in v1.

## Ownership and bounds

Parent retains the only document, Runtime client/scope, dispatcher, mutation
sequencing, operator workflow and recovery. Child owns at most 64 local drafts
with two 65-character inputs (valid candidates <= 64 UTF-8 bytes each). Drafts,
unsent intentions and egui widget state retire on document revision/incarnation
change. No edit is a measurement or an API mutation.

Private framing **version 2** adds two closed control-frame variants. Old version
1 peers fail explicitly at header validation. One capacity-one typed intention
mailbox carries only proposal/confirmation/cancel/terminal-acknowledgement with a
document revision, nonce and Parent ticket. No operation string, arbitrary
Runtime arguments, instrument command, status or retry surface enters child.
Frames retain the existing 4096-byte control bound. Feedback is latest-only;
close/challenge/fence traffic keeps priority, and snapshot/IPC retirement credits
remain unchanged. Nonce/transport sequencing prevents replay; no automatic send
retry occurs after an I/O error or GUI replacement.

Before admission Parent checks the original IPC Workbench/incarnation, active
episode, current presentation revision, referenced `set_reference` binding and
Reference target, explicit control permission, Ready/Fresh projections, ramp kind,
Runtime revision/boot and recovery/action-history capacity. The existing workflow
captures `expected_revision` and rechecks before its existing client submission.
The Runtime remains the final authority. A GUI restart cancels only an **unsent**
confirmation from the retired incarnation. Submitted/accepted/ambiguous work stays
in Parent and is projected to the new GUI without mutation, status or Exact Retry.
Original process exit and IPC retirement still gate the next incarnation.

## Launch and reproducible virtual experiment

Build matching Parent/child binaries in dev or release:

```powershell
cargo build --locked -p lab-runtime -p lab-workbench
cargo build --locked --manifest-path scripts/support/eframe-host/Cargo.toml
target/debug/lab-runtime.exe --serve --config examples/runtime.virtual.toml
target/debug/lab-workbench.exe --connect 127.0.0.1:7420 --workspace target/control-1/manual --workbench-listen 127.0.0.1:0 --experimental-eframe wgpu --experimental-eframe-child D:/rust_projects/lab-runtime/scripts/support/eframe-host/target/debug/lab-eframe-host.exe --experimental-eframe-commit-limit 1073741824 --experimental-eframe-controls
bb clients/babashka-smoke/control1.clj WORKBENCH_PORT configure target/control-1/manual-configured.json
```

Use a new workspace/output path; choose the printed Workbench API port. The
explicit 1 GiB Job value is a fixture choice, not a universal memory guarantee.
Omit `--experimental-eframe-controls` or add `--observe` to disable GUI submissions.
Renderer selection is explicit; no fallback is added.

Finite native-window driver (committed executable inputs and an interactive
Windows/MSVC desktop required):

```powershell
python scripts/support/eframe-host/control1.py dev wgpu UNIQUE_NAME fault
python scripts/support/eframe-host/control1.py release wgpu ANOTHER_NAME fault
python scripts/support/eframe-host/control1.py release glow GLOW_NAME smoke
python scripts/support/eframe-host/control1.py dev wgpu OBSERVE_NAME observe
```

The driver uses the existing virtual-furnace example with a unique loopback port
and Recorder DB. Babashka uses only the existing Workbench API to configure and
replace the document. Native input is checked against the original HWND/PID.
Fault mode preserves original byte order while holding terminal-response delivery
for at most four seconds to expose Accepted separately, then kills only GUI and
releases the response to the still-running Parent. The relay creates no request
or Runtime scope. It records both wire directions, original exit/IPC retirement,
native screenshots, unchanged attachment and exactly one confirmed operation.
All failed attempts, run-local config and Recorder DB/WAL/SHM are retained.

Actual qualification results and limitations are recorded separately after the
new runs; unchanged heavy renderer/resource tests are reused with source proof.

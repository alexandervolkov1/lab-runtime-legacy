# Minimal Babashka boundary smoke

[`distributed.clj`](distributed.clj) is an explicit bounded virtual-demo
WS/WSS acceptance example. It uses Java WebSocket with normal TLS verification,
performs one direct Runtime mutation and confirms its authoritative result, then
changes a GUI plot through the existing local Workbench API. It never relays
measurements or replays mutations. Pass `--runtime-only` instead of the Workbench
port to operate with Workbench closed. See the
[transport reference](../../docs/reference/transports.md) for access-key options
and deployment security boundaries. Physical two-computer qualification remains
incomplete. These are developer examples, separate from the user walkthrough.

For a virtual Runtime already exposing local WS at port 8766 and a local Workbench
API at port 8767, the distributed smoke is:

```powershell
bb clients/babashka-smoke/distributed.clj ws://127.0.0.1:8766/application/v1 8767
```

For an administrator-provided WSS endpoint, substitute its hostname and provide
the key through the named environment variable. `--runtime-only` skips the local
presentation call:

```powershell
bb clients/babashka-smoke/distributed.clj wss://runtime.example.org/application/v1 --runtime-only --token-env LAB_RUNTIME_ACCESS_KEY
```

The example changes a virtual Reference; do not point it at physical equipment.
No service-specific tunnel setup or credentials are supplied here.

These two standalone scripts are wire examples and boundary-smoke evidence, not a
client SDK or a supported Clojure client architecture. Babashka is optional tooling;
Cargo builds and production executables do not depend on it. Tested with Babashka
1.13.220.
The scripts use its included Cheshire JSON parser and Java TCP streams, with no Rust
crate access, dependencies to install, recovery, mutation retry, or subscriptions.

Run these tools from the source checkout, with `bb` on PATH and PowerShell 7 on
Windows. Preview.5 user packages do not contain `clients/`. From the repository root:

```powershell
cargo build -p lab-runtime -p lab-workbench --locked
./clients/babashka-smoke/run-smoke.ps1 -Repeat 3
```

To test packaged executables instead, stay in the source checkout and pass their
paths explicitly. Replace the example package directory with your extracted folder:

```powershell
./clients/babashka-smoke/run-smoke.ps1 -Repeat 3 `
  -RuntimeExe 'C:\lab-runtime-package\lab-runtime.exe' `
  -WorkbenchExe 'C:\lab-runtime-package\lab-workbench.exe'
```

The runner never downloads or builds either product executable.

The runner starts the actual `lab-runtime.exe --serve --profile virtual-demo --port 0`,
reads its bound port, and starts the native Workbench with a temporary workspace and
`--workbench-listen 127.0.0.1:0`. It reads Workbench's readiness address rather than
guessing a port. A desktop capable of running the native Glow Workbench is required.
Only the scripts exchange Application/Workbench protocol messages; PowerShell manages
processes and reads readiness/PASS output. Each script has a 15-second process deadline;
connect/read deadlines also apply. Cleanup terminates only fixture processes, waits
at most five seconds each, and never sends a Runtime shutdown operation. Temporary
workspace/logs remain under the printed system-temp path for diagnosis.

To exercise already running **virtual-demo/test** instances separately:

```powershell
bb clients/babashka-smoke/runtime.clj 7420
bb clients/babashka-smoke/workbench.clj 7421
```

`runtime.clj` sends exactly:

```json
{"v":1,"msg_id":"hello-1","op":"hello","args":{"scope":null}}
{"v":1,"msg_id":"reference-1","op":"reference","args":{"reference":"1"}}
```

Babashka parses both responses and checks version, correlation, result type,
protocol identity, Runtime-issued boot/scope/next sequence, advertised `reference`,
and the Reference ID, decimal revision, and numeric target. It prints the observed
values, closes the socket, and exits nonzero on errors. No Runtime mutation is sent;
hello plus a read-only query is the deliberately safe direct Runtime example.

`workbench.clj` separately exercises Workbench hello, bounded `client_status` readiness
observation, a mediated `reference` query and its correlated `lab_update`, presentation
read, one UI-only add-plot, a deliberately stale remove-plot (`revision_conflict`),
the exact final document/revision, and `presentation_changed`. It then disconnects.
It leaves its plot in the temporary presentation; it does not alter Runtime experiment
state. Unique call IDs are used throughout each connection. Readiness observation is
not mutation retry or recovery-status polling.

The surfaces remain distinct:

```text
runtime.clj
  -> Runtime Application TCP API

workbench.clj
  -> Workbench TCP API
  -> existing single ClientHandle
  -> Runtime
```

## Declarative presentation acceptance (DGUI-3)

Use the same runner and Workbench wire client for the complete declarative path:

```powershell
cargo build -p lab-runtime -p lab-workbench --locked
pwsh -NoProfile -File clients/babashka-smoke/run-smoke.ps1 -Presentation -Repeat 3
```

This mode starts the same real `virtual-demo` Runtime and native Glow Workbench,
with an isolated temporary workspace and ephemeral `127.0.0.1` Workbench endpoint.
It skips `runtime.clj`: Babashka opens only Workbench connections. Discovery and
`latest` queries travel through Workbench's existing single Application worker.
It creates no new Runtime client/session infrastructure.

`workbench.clj PORT --presentation` performs hello, waits for Ready/Fresh, reads
the presentation revision, follows bounded frozen discovery pages, and chooses up
to two floating-point measurement sources. Transform instruments are excluded:
in this fixture the moving mean is derived from the same temperature, not an independent
second source. The document contains one logical window, `Live signals` and
`Missing data` tabs, Status panels, and one Plot panel. All traces refer to discovered
sources. A separate, explicitly labelled absent-reference Status probe exercises
Unresolved; it never supplies a fake observation or a second plotted source.

The script checks advancing authoritative measurement timestamps, one successful
`ui_replace_document`, exactly one revision increment and `presentation_changed`,
exact document readback, and `revision_conflict` on stale replacement with unchanged
readback. Each replacement is submitted once; the stale request is an explicit
negative check, not an automatic retry. The allowed Workbench requests exclude
`lab_mutation`, status/retry recovery actions, and connection controls.
The Runtime attachment/hello and recovery projections must remain unchanged.

PowerShell samples the OS TCP table during each Babashka run and optional inspection
interval. After initial attachment it requires exactly one established server-side
Runtime connection, without opening a probe socket. It also checks the
recovery-journal fingerprint, no new
`presentation-v1.json`, the native window handle, and Runtime survival when Workbench
is terminated. These checks complement the existing dispatcher/renderer source
guards and no-Runtime-work regressions. TCP sampling is not continuous packet
tracing, and `hello.next_seq` is a cached attachment observation, not a new
authoritative sequence query.

Every readiness wait is at most ten seconds; each Babashka process has a fifteen
second deadline, with a twelve second protocol budget and two second socket
connect/read timeouts. Discovery has a sixteen-page budget. Cleanup waits at most
five seconds per owned fixture and terminates only processes started by this run.
The runner retains JSON summaries (including the exact document and observations),
stderr logs, and `native-process-evidence.json` under its printed temporary path.

### Native visual inspection

A window handle and passing protocol exchange do **not** prove visual rendering.
For a separate bounded desktop inspection, keep the fixtures alive for up to 120
seconds after the protocol checks:

```powershell
pwsh -NoProfile -File clients/babashka-smoke/run-smoke.ps1 `
  -Presentation -Repeat 3 -GuiHoldSeconds 60
```

During the interval, inspect the logical window and the `Live signals` Status/Plot
panels, switch to `Missing data`, and return. The absent source must show
`Unresolved — no Runtime observation`. The missing-data tab must not show a fallback
legacy plot; the live tab must show one configured plot. The stationary safe plant
normally shows a flat temperature near 20 °C; advancing sample timestamps and the
live trace, rather than a changing temperature, establish observation progress.
Do not click operator controls or disconnect during this read-only check.

After the interval, the runner opens another **Workbench** caller and invokes
`--verify-presentation EVIDENCE.json` using the last summary. It checks the exact
document/revision and Runtime attachment again, with no Runtime query or mutation.
This makes tab-navigation inspection reviewable without adding a production
testing endpoint or changing the renderer. The runner itself does not assert that
any visual inspection occurred; `visual_assertions` remains false in its evidence.
If no interactive desktop is available, report the visual check as unperformed.

For a plot-hover inspection with two separated curves, use the same runner with
an isolated, generated virtual deployment instead of the default virtual-demo:

```powershell
pwsh -NoProfile -File clients/babashka-smoke/run-smoke.ps1 `
  -Presentation -TwoCurveHover -Repeat 1 -GuiHoldSeconds 120
```

This fixture has two independent virtual thermal plants, `1/1` at 20 °C and `2/1`
at 40 °C, with no controller or Recorder. Both sources come from authoritative
discovery and real Runtime observations; no display offset or conversion separates
the lines. The runner requires two discovered sources and retains its existing
deadlines, connection-count checks, exact readback and owned-process cleanup.
Move near each line in turn: only the selected curve should have a tooltip, with
`T+HH:mm:ss.SSS`, its Y value/unit and freshness. Moving between the lines or far
from both, inside the plot, must remove the tooltip. Hide a line through its legend
entry and verify it no longer responds to hover. The library chooses the line in
screen coordinates, including after zoom or pan; it does not summarize all curves
at the cursor's X. The readout is the position returned by egui_plot, not a new
measurement. These remain manual visual assertions, separate from protocol PASS.

On 2026-10-12, a native Windows/MSVC corrective DGUI-4A run with this two-plant
fixture passed. Client-area captures after mouse movement showed only
`temperature` at 20 °C near the green line and only `temperature (2)` at 40 °C near
the blue line, with their own relative times. Between the lines and after hiding
the blue curve through its legend entry, no tooltip appeared. Post-interaction
readback preserved the exact document and revision 3, with zero new presentation
events and unchanged attachment/recovery journal. All 569 TCP samples in that run
observed one Runtime connection; owned-process cleanup completed. These visual
observations are separate from the runner's protocol PASS; native Stale/Rebuilding
was not exercised (deterministic egui tests cover those states).

### Observed Windows result (DGUI-3 virtual-demo)

On 2026-10-12, using the native `x86_64-pc-windows-msvc` toolchain and Babashka
1.13.220, three consecutive replacements and post-inspection readback passed against
the DGUI-2A merge `c4f541d0f66069dc32f9c495d945f039e2dd9a90` binaries. Revisions advanced
from 2 to 3, 4, and 5; each success emitted exactly one replacement event and each
stale request was rejected. Authoritative discovery reported temperature `1/1` and
moving-mean output `202/1`. Only `1/1` was plotted; the absent probe was `1/2`.
Samples remained Good/Available at 20 °C with advancing observation timestamps.
All 586 OS TCP samples observed one Runtime connection, and recovery state/journal
and the attachment were unchanged. No presentation file was created.

Separately, native Win32 mouse input and captured Workbench client-area pixels were
visually inspected: both tab transitions worked, Fresh status/value and the green
temperature trace appeared, the missing tab showed Unresolved, and no duplicate
plot appeared. Post-inspection readback still returned revision 5 and the exact
document with zero presentation events on that readback connection. Fixture cleanup
completed, and Runtime survived Workbench termination before its own cleanup.

This is a virtual, read-only acceptance check. There is only one independent
measurement source in this fixture, so this run does not qualify two independent
temperature curves. Stale/Unknown/Rebuilding behavior is covered by the existing
deterministic renderer tests; this desktop run did not interrupt Runtime. Controls,
disk persistence, Arduino integration, and physical-device qualification remain
outside this check. The older operator GUI smoke deliberately submits a virtual
Runtime mutation and is a different scenario, not part of this read-only run.

### EFRAME-3 isolated standard GUI experiment

[`eframe3.clj`](eframe3.clj) configures the existing Workbench Parent through its
loopback API. `initial` installs the virtual furnace document, `updated` changes
its title/plot configuration, `boundary` adds admitted large-document metadata,
and `verify` reads it back without replacement:

```powershell
bb clients/babashka-smoke/eframe3.clj PORT initial NEW_INITIAL.json
bb clients/babashka-smoke/eframe3.clj PORT updated NEW_UPDATED.json
bb clients/babashka-smoke/eframe3.clj PORT boundary NEW_BOUNDARY.json
bb clients/babashka-smoke/eframe3.clj PORT verify NEW_READBACK.json
```

The client uses only hello, client_status, presentation_get and
ui_replace_document. It opens no Runtime connection and submits no instrument
command or recovery operation. The complete [Windows experiment driver and
reproduction guide](../../docs/developer/eframe-3-readonly-workbench.md) launch a
passively cooling virtual furnace, exercise standard WGPU/Glow windows with
native input, apply a 979,038-byte canonical replacement while minimized for two
minutes, and check
GUI crash/recovery against the same Parent/scope. The six configured control
groups remain disabled. The explicit fixture memory cap is not deployment policy;
MP-1 is OPEN and `ACTIVATION_QUALIFIED=false`.

The separate [browser smoke](../clojurescript-smoke/README.md) remains the real-browser
Runtime WebSocket regression. See the canonical
[Runtime Application API](../../docs/api/README.md) and
[Workbench API](../../docs/workbench-api.md) for the two surfaces; this README and
the scripts are not competing protocol specifications.


For TIME-1, `bb time1.clj PORT MODE NEW_RESULT.json` configures the existing
Workbench Parent with live virtual Scalar/Plot and a time-axis mode. MODE is
`default`, `local_calendar`, `utc`, `runtime_elapsed`, `experiment_elapsed`, or
`verify`. The script uses only hello/status/presentation-get/document-replacement
on the loopback Workbench API and checks exact readback. It opens no Runtime
socket. Experiment elapsed currently explains its fallback to Runtime elapsed.
See [TIME-1 reproduction and evidence](../../docs/developer/time-1-validation.md).

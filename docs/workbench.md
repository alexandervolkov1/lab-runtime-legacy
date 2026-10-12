# Workbench guide

[User manual](README.md) · [First experiment](getting-started.md)

Workbench is the Windows window onto a running experiment. Runtime must be started
separately. Closing Workbench does not stop instruments, controllers or recording.

## Connect to Runtime

From the extracted Windows package folder:

```powershell
.\lab-workbench.exe --connect 127.0.0.1:7420 --workspace .\my-workspace
```

Use the port reported by Runtime. Keep one Workbench per workspace folder.
For another computer, use the [local-network guide](distributed-workbench.md).

Workbench connects at startup. **Disconnect** leaves Runtime running and does not
reconnect automatically. Click **Connect** when you want to connect again.

Add `--observe` if you only want to watch. This disables this Workbench's experiment
commands, including recording start/stop, but still lets you view signals and
inspect command status. It does not prevent another connected operator from acting.

## Read the status

| Display | What to do |
|---|---|
| **Fresh** | The displayed data has caught up; available controls can be used |
| **Rebuilding** | Wait while Workbench refreshes its view |
| **Stale** or **Cached last observation (stale)** | Treat displayed values as old; do not use them to judge current equipment state |
| **Unknown** | No value is available yet |
| **Unresolved in the current Runtime view** | That item is not present in the current experiment |

The connection may say **Ready** before the display becomes Fresh. Wait for Fresh.

## Find instruments and measurements

The **Discovery** pane lists instruments, signals, References, controllers,
resources, configuration properties and Recorder.

Select **Instrument 1** to inspect its details. Select **Signal 1/1** to see
parameter 1 from instrument 1. Other configurations use their own numbers.
The details area currently displays structured text, including names, values and
units; it is not a separate instrument dashboard.

## View and adjust plots

Select a **Signal** in Discovery to display its live graph. The standard view also
creates a default live plot when signals become available. No add-plot action is
needed for this workflow.

When the current `PresentationDocument` defines windows, Workbench displays them
as titled groups inside the existing window, with a tab strip for each group.
Only the selected tab's panels are visible. Tab selection, plot panning and legend
interaction are local view state: they do not edit the document or contact Runtime.
These are logical windows, not additional native OS windows.

Status panels display the referenced Runtime entity's cached value and freshness.
Plot panels display their referenced plot with all configured visible traces,
using the existing bounded live buffers, labels, colors, widths and display time
windows. A plot referenced by a panel is not also rendered by the standard plot
list or the selected-signal graph. If `windows` is empty, the standard plot list
and selected-signal workflow remain available.

Panel and trace states distinguish **Fresh**, **Stale**, **Unknown**,
**Rebuilding**, and **Unresolved**. Stale observations may remain visible as cached
values and curves; Unknown/Rebuilding never claim completed freshness. Unresolved
means there is no cached observation for that Runtime reference. A known entity
can still be waiting for usable finite live samples. Layout references do not
initiate queries or manufacture missing data.

The [Workbench API](workbench-api.md#ui_replace_document) can atomically replace
the current in-memory layout. The renderer uses the committed replacement on its
next owner turn, reconciles removed windows/tabs, and discards old plot zoom/legend
state when the presentation revision changes. Explicit replacements and
declarative layouts do not implicitly acquire a default plot. The API does not
save the layout to disk.

Declarative Controls panels currently display an inactive placeholder. Existing
operator controls, confirmation, recovery and reconnect workflows remain separate
from the read-only layout.

Drag inside a plot to pan. Double-click the plot to restore automatic bounds.
These changes affect only your view, not measurement or recording.

Hover inside any live plot to see one **Nearest observed samples** tooltip. For
each visible curve it shows the nearest retained sample by distance along the
time axis, its name, raw numeric value, unit and its own sample time. Equal time
distances choose the earlier sample. It does not interpolate between samples.
Traces hidden by configuration or by the legend are omitted, as are curves without
usable live samples. The lookup uses the same trailing window as the plotted line.

Times use **T+HH:mm:ss.SSS**: elapsed monotonic time from the Runtime process clock
origin, not local calendar time or time since opening Workbench. Hours continue
past 24. Times normally round to milliseconds; nearby asynchronous samples use
six or nine fractional digits when needed to keep their timestamps distinct.
Each curve retains its own timestamp rather than implying a simultaneous reading.
The underlying live time is stored as floating-point seconds; extra display digits
do not recover precision already lost there.

The tooltip prefers the unit in the cached Runtime observation. If only the
presentation's display-unit metadata is available, it explicitly says **display
label; no conversion**. An unknown unit stays **unit unknown**. Values are never
converted to another unit by the renderer. **Fresh view** describes the source
cache/attachment, not a claim that a historical sample was measured just now or
is valid for physical control. Stale, Unknown, Rebuilding and Unresolved remain
explicit. Hovering neither contacts Runtime nor edits the document, revision,
recovery information or recording, and adds no retained sample history.

This GUI has no arbitrary plot/trace editor or **Save Layout** button. You cannot
use GUI menus to compose dashboards or edit trace colors and stored axis settings;
declarative dashboards are supplied through the document/API instead.

**Waiting for good finite signal events.** means no usable live samples have
arrived for that signal. A flat line can simply mean a steady value. Old display
points eventually leave the plot; **Display window truncated** is not a Recorder
data-loss message. Reconnecting can clear the live graph.

Live graphs are not saved recordings. Workbench has no archive browser or CSV
export button; see [Recording](recording.md).

## Reference and operator controls

A **Reference** is a setpoint. Select one to see **Reference controls**:

- **Configure fixed** uses the **Fixed value** field.
- **Configure ramp** uses **Target** and a positive **Rate**.
- **Retune** changes a Reference that is already a ramp.

For every action, read **Confirmation required**, press **Confirm** once, and
wait. **Accepted / in progress** is not completion. After **Operation completed**,
check the updated item details, then press **Acknowledge** to return to the controls.
**Cancel** discards an unconfirmed draft.

A running physical controller may use the Reference immediately. Review the
equipment's safe limits before changing it. Command completion is not independent
proof that physical equipment reached the requested state.

### Other available actions

| Selected item | Available controls |
|---|---|
| Controller | **Start**, **Pause**, **Resume**, **Reset failed**, **Configure PID** when applicable |
| Writable Integer or Text property | **Configure property** |
| Reconnect-capable resource | **Reconnect** |

Use controller and PID controls only with a reviewed equipment procedure. Pause,
reconnect and reset are not substitutes for an emergency stop, and reconnect does
not automatically rearm a controller. Some displayed properties are read-only.
With required recording, start a recording run before **Start** or **Resume**.
Pause the controller through its reviewed procedure before ending that recording.

A disabled button can mean data is stale, the current state does not allow the
action, observation mode is on, or a previous command is unresolved. Do not work
around it by opening another workspace to repeat the command.

## Recorder

The **Recorder** section shows `authoritative state`. Enter a label, use
**Start recording**, and confirm. After completion, **Acknowledge** restores the
controls. Use **Stop recording** and confirm when finished.

Check the completed stop result and the return to `idle` as described in
[Recording](recording.md). Closing or disconnecting Workbench does not stop a run.

## Workspace and saving

`--workspace` selects the folder Workbench uses for its local settings and command
recovery information. Reopen the same folder to retain that information. Without
the option, Windows normally uses `%LOCALAPPDATA%\lab-runtime\workbench`.

The GUI does not provide **Save**, **Save Layout** or **Save As** for a workspace.
It loads saved presentation settings when available, but does not automatically
save every graph adjustment or persist live plot samples. Keep this distinction
in mind before closing it.

To back up a workspace, close Workbench normally and copy the whole workspace
folder. Do not edit or delete its recovery files to bypass an unresolved command.
The Recorder database is separate and stays on the Runtime computer.

## Loss of connection and recovery

Workbench may briefly show **Reattaching** after an unexpected connection loss.
Wait for Fresh if it reconnects. If it remains **Disconnected**, check Runtime and
the network, then use **Connect**. Reconnecting never repeats a command automatically.

If you see **Unknown / reconciliation required** or **Ambiguous**, a command may
have run even though its answer was lost. Do not press its ordinary action button
again. Open **Recovery / reconciliation** and use **Check Status** when available.
It checks once; it does not keep polling.

`outcome_unknown` means the result is still unknown, not that nothing happened.
**Exact Retry…** is a separate, manual recovery action. It may execute a command
that did not previously run. Do not use it for uncertain physical work without
reviewing the equipment state and the retained command with a responsible operator.

After Runtime restarts, **Connect new scope** may be offered. It does not erase
old uncertainty. **Quarantined recovery evidence** means old commands cannot be
resolved against this running instance, and controls may remain blocked. There is
no **Discard** or **Forget** button. Preserve the workspace and recording and get
help; do not delete files or start a fresh workspace to bypass the block.

## Common problems

For a failed command, read **Operation failed** and its details before acknowledging
it. If a draft became stale, cancel it and review current values. Do not repeat
an operation whose result is unknown.

For startup failures, missing signals, locked workspaces and recording errors,
use [Troubleshooting](troubleshooting.md). Stop Runtime from its own terminal with
Ctrl+C; the Workbench window has no Runtime shutdown button.

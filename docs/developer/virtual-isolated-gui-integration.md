# DGUI-4C7B3B5: virtual isolated GUI integration admission

**BLOCKED — qualified hardware/driver profile unavailable.**
`ACTIVATION_QUALIFIED=false`. This Draft PR records the verified B3B4C merge,
the failed environment preflight and the remaining integration work. It contains
documentation/evidence only; no B3B5 host selection or startup implementation is
claimed. The existing executable continues to select GUI v1.

## B3B4C merge and source identity

[PR #42](https://github.com/alexandervolkov1/lab-runtime/pull/42) was checked at
reviewed HEAD `2dd4af1edc7f6ec0bb09ca9a4d28fc3c56670a1c` and base
`1890fafb15a53c4517a3dc7295d74d1a1756b5ea`, made Ready and merged with an ordinary
merge commit:

| Identity | SHA |
|---|---|
| Merge | `d96111f488d8535c294879e767832d537219108a` |
| First parent | `1890fafb15a53c4517a3dc7295d74d1a1756b5ea` |
| Second parent / reviewed HEAD | `2dd4af1edc7f6ec0bb09ca9a4d28fc3c56670a1c` |
| Qualified B3B4C source | `f444da9322e9db75d9bcb7e6ae908833a6a17b67` |
| B3B5 branch | `feature/dgui-4c7b3b5-virtual-isolated-host` |
| B3B5 executable source examined | `d96111f488d8535c294879e767832d537219108a` |

Remote HEAD/base were checked again immediately before merging, with the reviewed
HEAD supplied to GitHub's merge endpoint as the required SHA. The delta from the
qualified source consisted of 42 added documentation/evidence paths; all 400
canonical Git-blob SHA256 entries in its inventory matched. Both merge parents
were checked after fetching. Local main was fast-forwarded and its worktree was
clean before the B3B5 branch was created. No release was created.

[Merge receipts](virtual-isolated-gui-evidence/merge-verified.json) and
[source equivalence](virtual-isolated-gui-evidence/source-equivalence.json) retain
the exact identities. The B3B5 review HEAD is the enclosing Draft PR commit;
there is no newly qualified B3B5 binary/source pair.

## Environment denial

The user accepted **C = 234,881,024 bytes (224 MiB)** exclusively for the reviewed
fixed Windows/MSVC/AMD experimental profile. That acceptance does not cover this
machine. The read-only CIM/toolchain preflight found:

| Property | B3B4C qualified profile | Current machine |
|---|---|---|
| OS | Windows 10 Pro 10.0.19045 x64 | Windows 10 Pro 10.0.19045 x64 |
| Toolchain | Rust 1.95.0, x86_64-pc-windows-msvc | Same compiler commit and host |
| CPU | AMD Ryzen 5 6600H with Radeon Graphics | AMD Ryzen 5 9600X 6-Core Processor |
| GPU / driver | AMD Radeon(TM) Graphics / 32.0.21030.2001 | AMD Radeon(TM) Graphics / 32.0.21042.62 |
| Other installed GPU | No qualification transferred | NVIDIA GeForce RTX 3060 Ti / 32.0.16.1692 |
| Actual experimental GL drawable / scale | 1280 x 960 / 1 | Not created; preflight denied |

Neither installed adapter has the qualified GPU/driver pair. Installed-adapter
enumeration also does not prove which adapter a future WGL context would select.
No experimental child, Runtime or Workbench process was launched by this task.
No Job cap was installed or raised. There is no framebuffer, process/Job peak,
GPU cleanup receipt or connection/scope count from a B3B5 experiment.

[Preflight decision](virtual-isolated-gui-evidence/preflight.json),
[actual profile](virtual-isolated-gui-evidence/actual-windows-profile.json) and
[raw evidence](virtual-isolated-gui-evidence/raw-evidence.zip) preserve the inputs
and diagnostic. This is a pre-implementation experiment denial, not evidence of
an executable admission gate or a successful negative integration test.

## Minimal startup and virtual-only analysis

The existing ownership seams support a prospective single-parent composition:

- [GUI startup](../../apps/lab-workbench/src/gui/mod.rs) acquires workspace
  ownership and prepares the existing presentation/journal/endpoint.
  [ParentOwner::connect and ParentPump](../../apps/lab-workbench/src/parent.rs)
  already provide one client/dispatcher, one Application worker and a coalesced
  independent scheduler. A future explicit experimental branch must bypass
  `eframe::run_native` and `WorkbenchApp` creation entirely.
- [ParentBridge](../../apps/lab-workbench/src/private_ipc/parent_bridge.rs)
  derives canonical documents from the existing dispatcher revision and publishes
  selected observations. It must be installed before moving the same owner into
  ParentPump. Its bounded selected-reference list is supplied at construction;
  document replacement with changed sources still needs an explicit selection
  policy and regression coverage.
- Its `Bridge::worker_factory` reserves the sole existing
  [IPC/supervisor worker](../../apps/lab-workbench/src/private_ipc/backend.rs).
  [Job::prepare/resume](../../crates/lab-windows-job/src/windows.rs) already
  verifies limits and membership before ResumeThread. Each future incarnation
  must use the reviewed fixed C, with original process and IPC retirement gates.
- The current [child entry](../../apps/lab-native-child/src/entry.rs) remains a
  qualification harness: it has a 30-second process deadline, bounded evidence
  captures/admissions and automatic test navigation after drawing. Its existing
  `normal` mode is insufficient evidence of an application lifecycle. Any new
  application mode must preserve the original qualification modes and gates.

No substantial ownership incompatibility has been established by this inspection.
Implementation and qualification stopped at the environment gate. There is no
new CLI flag, fallback, worker, protocol, document owner or Runtime API change in
this PR.

An arbitrary `--connect 127.0.0.1:PORT` cannot prove virtual-only identity. The
bounded next option is a separate fixture-owned launcher that starts a pinned
Runtime binary with the existing `--serve --profile virtual-demo --port 0`, owns
its original process/readiness channel, retains the emitted boot/port identity
and verifies Workbench attachment to that boot. It must own Runtime shutdown
independently so closing/crashing Workbench can be tested while Runtime/Recorder
continue. The future Workbench opt-in must require that verified fixture, reject
arbitrary endpoints and verify the qualified renderer profile. No such launcher
or fixture-to-Workbench identity handoff is implemented here; a proposed manifest
or loopback address alone is not accepted evidence.

## Qualification status

| Required B3B5 evidence | Result in this task |
|---|---|
| Explicit opt-in and executable fail-closed admission gates | BLOCKED / not implemented |
| One real Runtime connection/scope and Parent authority | BLOCKED / no process run |
| Virtual Runtime to Parent to private IPC to native child | BLOCKED / no process run |
| External API `presentation_get`, atomic `ui_replace_document`, conflict | BLOCKED / no process run |
| Boot/source/event revision, quality, value/unit, Plot update and stale/rebuilding | BLOCKED / no observations |
| Actual GL framebuffer | BLOCKED / no GL context created |
| Normal startup/close, native startup failure | BLOCKED / not exercised |
| Child crash, owner stall, snapshot IPC stall | BLOCKED / not exercised |
| GPU Pending during document replacement, explicit restart | BLOCKED / not exercised |
| Parent shutdown/crash, original Job ownership, unconfirmed exit/IPC retirement | BLOCKED / not exercised |
| Workbench close preserves Runtime and Recorder progress | BLOCKED / not exercised |
| Focus, mouse, Page/Plot/legend/viewport, keyboard/IME, minimize/restore/resize/DPI | BLOCKED / not exercised |
| Process/Job peaks, GPU ledger, CPU Preview, parsed document, selected view, IPC credits | No B3B5 measurements |
| Display-fault-only Runtime query/operation delta against real baseline | Unknown / no baseline collected |
| Full B3B5 dev/release regression qualification | BLOCKED / not run |

The [B3B4C report](isolated-renderer-lifecycle-memory-admission.md) and all its raw
evidence remain unchanged. Its 48 passing gate receipts, including dev/release
full Workbench, native Job/IPC, B3B4A/B/C, fixed-C lifecycle, CPU Preview,
borrowing and strict v1 results, remain historical evidence for their exact
qualified inputs/profile. They are not new B3B5 results. Its largest fixed-C Job
peak was 148,611,072 bytes; it cannot be transferred to this machine or presented
as an end-to-end B3B5 measurement.

All 359 canonical source/build/fixture hashes from B3B4C match the merge source.
For this documentation/evidence change, formatting, inventory and diff checks are
recorded in [verification.json](virtual-isolated-gui-evidence/verification.json).
Heavy builds, Clippy and native gates were not repeated. No new binaries were
built or executed; hashes of existing local executables are explicitly marked
inspection-only with unknown build-source identity in
[binary-identities.json](virtual-isolated-gui-evidence/binary-identities.json).
The [canonical SHA256 inventory](virtual-isolated-gui-sha256.txt) seals this report
and its evidence; the raw ZIP contains the original preflight/merge records and
command transcripts.

## Unblocking and preserved limits

Continue on the exact qualified Windows/MSVC/AMD machine/profile, keeping C fixed,
then implement the fixture-only opt-in and execute the complete requested dev and
release matrix. Alternatively, qualifying this machine requires a separately
authorized and independently reviewed hardware/driver profile; this task does
not authorize that expansion or system/driver changes.

Historical hello-only remains **UNQUALIFIED**. M17.4 Recorder overlap, general
terminal GPU lifecycle and whole-host Rust budget remain **BLOCKED**. Final WGL
destruction remains **LIMITED**. Other GPU/DPI/driver configurations remain
**UNQUALIFIED**. `ACTIVATION_QUALIFIED=false`.
No production activation, hardware control, release or B3B5 merge is authorized
by this evidence. **Final verdict: BLOCKED.**

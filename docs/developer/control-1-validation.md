# CONTROL-1 Windows/MSVC validation

`ACTIVATION_QUALIFIED=false`.
[Draft PR #52](https://github.com/alexandervolkov1/lab-runtime/pull/52) requires
independent review. Ordinary GUI v1
remains the default; control in the stock isolated GUI requires an explicit flag.
No release, physical instrument acceptance or production activation was performed.
MP-1 is OPEN, deferred research; the current
[desktop memory requirements](project-status.md#current-desktop-memory-requirements)
apply. No new memory probes, kernel traces or historical memory reruns were made.

## Provenance

PR [#51](https://github.com/alexandervolkov1/lab-runtime/pull/51) was merged by the
authorized ordinary merge commit **`2426fc0823e1d6a5e28425cc8ebc2e5f079953cb`**.
Before merging, remote HEAD `ec4818aa86da48f4a790b954fdbd3c78e2144142` and base
`30a20e3867e1bb08e2aff3405a7015266add70ed` matched the independent review,
the worktree was clean, and 32 canonical MP-1 checksums matched. The merge has
those two parents and the reviewed tree. Main was synchronized clean before
creating `feature/control-1-reference-retune` at the merge. The
[merge receipt](control-1-evidence/merge-51.json) independently rechecks these facts.

The memory policy is a separate logical commit,
`60af34b2ada16d6376234b8b911842b8ffa17405`. CONTROL-1 implementation starts at
`a6c91a72dd026abea20cb0b462d3948e8610857c`; subsequent small commits retain the
fixture and qualification fixes without rewriting history. Final E2E executable
inputs are **`0d3f3a4e6bb187cb0e9523dd697d7ac0d98fa7cd`**. Reports/evidence added
after that source do not change executable inputs.

[Source inputs](control-1-evidence/source-inputs.json) hash raw canonical Git blobs
with `git cat-file`, independently of Windows newline conversion. Per-run binary
SHA256 identities are in [results](control-1-evidence/gui-results.json); the final
six on-disk executables also have a [binary inventory](control-1-evidence/binary-identities.json).
Root dev/release builds and gates finished at `000a6548f6bd88ef72c682347a2643de4383b48c`;
stock hosts were built at `35fbfc986fb73a7d636594e1714a7bb8fb0034e2`. The only later
host-external Rust changes are explicit Parent module paths and the malformed
version fixture. The final E2E source changes only the Babashka unit spelling and
driver Plot assertions after the Rust gates. See
[qualification source comparison](control-1-evidence/qualification-source.json).

Environment: Windows 10 Pro 22H2, build **19045.6691**, x64/MSVC, Rust/Cargo 1.95.0,
Python 3.14.6, Babashka 1.13.220. The original child receipts identify AMD Radeon
Graphics, stock WGPU/Vulkan and stock Glow/OpenGL. No renderer fallback occurs.
See [environment](control-1-evidence/environment.json) and raw `ready.json` files.

## Real application scenario

The [launch and reproduction commands](control-1-reference-retune.md#launch-and-reproducible-virtual-experiment)
build real Runtime, Parent and stock eframe child binaries. The finite driver
uses a unique copy of `examples/runtime.virtual.toml`, loopback ports and a
Required Recorder database. The virtual furnace starts at 80 degrees C and cools
with no heater/controller start. The separate ramp Reference advances from the
example's initial value and is retuned by the confirmed operation. This is a
Reference retune qualification, not a furnace-control-loop acceptance claim.

The external `clients/babashka-smoke/control1.clj` opens only Workbench API sockets.
It uses existing `hello`, `client_status`, `presentation_get` and
`ui_replace_document`, verifies exact readback, and replaces the document at the
same Parent. The panel contains current Reference, target/rate and the existing
`set_reference` binding; a stock plot displays actual virtual measurements.
No public API, Presentation schema or Runtime operation was added.

Native input targets the original HWND/PID. Invalid `NaN` input leaves Apply
disabled. Valid editing sends nothing. **Apply retune** prepares Parent's frozen
confirmation and still sends no Runtime request. **Confirm retune** sends exactly
one existing `reference_retune`, target **65**, positive rate **3**, Reference **1**,
`expected_revision="1"`, mutation sequence **1** in the original scope.

| Final run | Native control and live display | GUI failure / close | Runtime clients / mutations |
|---|---|---|---|
| `dev-wgpu-q2` | PASS: draft, confirmation, Accepted, Completed, live Reference/Plot | crash, controlled restart, normal close PASS | 1 / 1 |
| `release-wgpu-q2` | PASS: same complete scenario | crash, controlled restart, normal close PASS | 1 / 1 |
| `release-glow-q2` | PASS: functional smoke, Completed, live Reference/Plot | normal close PASS | 1 / 1 |
| `dev-observe-q2` | PASS: editing stays local, disabled Apply cannot submit, live Reference/Plot | normal close PASS | 1 / 0 |

In fault mode a transparent byte relay holds terminal delivery for at most four
seconds, preserving original response order, so Accepted is visible before the
original GUI is killed. The relay originates no request or scope. Parent receives
the terminal reply and restores Completed to incarnation 2 without resubmission.
Accepted has no terminal result; Completed displays Runtime's typed returned
Reference revision/value/target/rate separately from the live observation.
Representative [native screenshots](control-1-evidence/screenshots/) preserve both
states. Glow can complete between frames; its smoke does not claim a separately
captured Accepted frame.

Each run captures the same nine startup/rebuild requests plus exactly the one
confirmed mutation, or only nine in observation mode. No editing, Apply,
acknowledgement, repaint, document replacement or GUI restart creates a query,
mutation, `operation_status` or Exact Retry. Attachment checks preserve the boot
and scope. The hello snapshot's `next_seq` is not used to infer later submission;
the original wire `request_id` is the mutation evidence.

After a crash the original process reaches `ExitedContained`, with zero owned
Job/process/thread objects, zero IPC handles/pending I/O and `ipc_retired=true`
before incarnation 2. Normal native close reaches `ExitedHealthy`, original exit
code 0, the same retirement conditions, completed Closed write and Parent exit 0.
Runtime remains alive after GUI close. Accepted/submitted work stays Parent-owned.

All four final runs show Fresh Reference and a Fresh Plot with changing original
measurement coordinates, at least two points and exact `degree C` display unit
(`U+00B0` followed by `C`). [Live projections](control-1-evidence/live-projections.json)
record first/last points and stages. This does not substitute an operation result
for an observed measurement or claim that the requested target was already reached.

## Gates and reused evidence

All final gates below pass. Complete logs, including earlier failures, are in the
[raw archive](control-1-evidence/raw-evidence.zip) and indexed by
[gate receipts](control-1-evidence/gates.json).

| New gate | Dev | Release |
|---|---|---|
| Parent subset, including CONTROL-1 fence/replay regressions | 20 passed, 14 native ignored | 20 passed, 14 native ignored |
| Private IPC unit subset | 28 passed, 22 native ignored | 28 passed, 22 native ignored |
| Existing operator workflow | 22 passed | 22 passed |
| Explicit isolated options | 1 passed | 1 passed |
| Stock eframe host | 14 passed | 14 passed |
| Native Windows private IPC | 22 passed | 22 passed |
| Workbench and stock host Clippy, all targets, `-D warnings` | PASS | PASS |
| Root/host fmt and diff check | PASS | same source |

Owner regressions cover disabled control, invalid finite/rate validation, absent
binding, stale document/incarnation, invalidated confirmation after replacement,
loss of Fresh, duplicate confirmation and repeated turns. Unsent confirmation is
canceled on restart; submitted and ambiguous work survives without queries or
replay. The child regression drives actual egui pointer press/release and checks
one outgoing intention; editing alone and pending response produce no second send.
Closed codecs reject arbitrary Runtime operations/fields and oversized input.
Private v1 headers fail explicitly rather than enabling control accidentally.

Reproduction gates:

```powershell
cargo test --locked -p lab-workbench --profile dev -- parent:: --test-threads=1
cargo test --locked -p lab-workbench --profile dev -- private_ipc:: --test-threads=1
cargo test --locked -p lab-workbench --profile dev -- model::operator:: --test-threads=1
cargo test --locked -p lab-workbench --profile dev -- isolated_options::
./scripts/test-private-ipc.ps1 -BuildProfile dev
cargo test --locked --manifest-path scripts/support/eframe-host/Cargo.toml
cargo clippy --locked -p lab-workbench --all-targets -- -D warnings
cargo clippy --locked --manifest-path scripts/support/eframe-host/Cargo.toml --all-targets -- -D warnings
cargo fmt --all -- --check
cargo fmt --manifest-path scripts/support/eframe-host/Cargo.toml --all -- --check
git diff --check
```

Repeat the affected test/Clippy commands with `--profile release` / `--release`,
and the native script with `-BuildProfile release`. Heavy unchanged Runtime,
Custom Glow rendering/resource and Supervisor matrices were **not rerun**.
[Raw source equality](control-1-evidence/source-reuse.json) confirms unchanged
Runtime/Core/Recorder/API, Supervisor/Job, Custom Glow rendering, operator and
dependency inputs from merged main. Existing [TIME-1 gates](time-1-validation.md#gates)
and [MP-1 gates](mp-1-job-accounting.md) remain historical evidence, including seven
native Supervisor cases per profile. Custom Glow includes the changed shared
private protocol; reuse concerns its unchanged rendering, not an assertion that
its entire dependency closure or old binary is identical. New native IPC and
stock backend runs cover the changed integration.

## Evidence and limits

[Verification](control-1-evidence/verification.json) records ZIP/member SHA256,
length/CRC verification and PNG decoding/pixel checks. Every archived member has
a [manifest](control-1-evidence/raw-manifest.json) entry. All run-local Recorder
databases and every existing WAL/SHM companion are retained together exactly.
No original data was removed. A readable database or terminated fixture is not
asserted to be a Recorder flush/seal receipt.

The archive contains **740 members**, **22,856,862 bytes**, SHA256
`6e1bee075e102b19091058ba02a8aceb6c46ee852881f09d83033a4ea2cb8606`.
Sixty selected PNGs passed full-file/CRC/decode/pixel checks. It retains 14 native
attempts, including eight successful runs and six failed attempts, all 18 original
stock child directories, API/wire/lifecycle logs and the gate logs. The
[canonical changed-file checksums](control-1-sha256.txt) cover source and evidence
using raw Git blob bytes and exclude only the checksum manifest itself.

Development failures remain visible: missing required nullable axes in the first
Babashka document; native foreground/input and minimized-window driver failures;
a successful control/fault flow rejected by the driver's wrong top-level mutation
counter; headless test texture-delta cleanup and warning-denied Clippy issues;
standalone Parent module paths; and a negative version fixture that accidentally
used the new valid version 2. These were fixed and rerun; failed logs are not
passing qualification. Earlier `*-qualified` native runs also passed CONTROL-1;
the final `*-q2` runs add explicit Plot/unit assertions. A packaging helper first
read UTF-8 frame JSON with the Windows default encoding and falsely reported a
unit problem. Reading original bytes disproved that diagnosis: both old and final
fixtures display the correct degree unit. The final client keeps an explicit
Unicode escape, and the final live-projection receipt uses byte-decoded JSON.

Only ramp Reference retune is enabled. Other control kinds remain inactive;
uncertain action reconciliation remains the existing explicit v1 workflow.
Private IPC is version 2 and requires matching Parent/child binaries. No new
Runtime client, public API, arbitrary child command channel, automatic retry,
renderer fallback or GUI-default switch is introduced. The explicit 1 GiB Job
setting is a fixture value. These finite runs do not establish a universal
memory bound, long-term accumulation qualification or physical safety acceptance.

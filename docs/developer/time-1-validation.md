# TIME-1 Windows/MSVC validation

`ACTIVATION_QUALIFIED=false`. Independent TIME-1 review is required. No release,
production activation, hardware acceptance, GUI Controls or M-CLEAN was performed.
MP-1 remains OPEN; the explicit 1 GiB Job limit below is a test setting.

## Source and provenance

Base is the ordinary PR #49 merge
`5c0730c8f827de9b326cabaf4176ae21d2e3308b`. Reviewed head/base, 66 raw Git-blob
checksums, absence of post-qualification executable changes, two merge parents
and reviewed/merged tree equality were verified before synchronizing clean main.
[Merge evidence](time-1-evidence/merge-49.json) preserves those identities.

TIME-1 implementation is `806d6b6286a3305f2a56322751d616ef46e06cef`.
`85f1464631338d892efdf50d614e1a8c7e898440` fixes only a native shutdown test's
one-shot observation; `5eaef186b6a7cf5582e77dafd90b3cd5351e7b5f` corrects only
the virtual experiment driver's disabled-Recorder policy. The latter is the
qualified source. Subsequent report/evidence changes are documentation only.

[481 source inputs](time-1-evidence/source-inputs.json) hash **raw Git blobs**
with `git cat-file --batch`, not archive/export or working-tree newline bytes.
[Binary identities](time-1-evidence/binary-identities.json) separately identify
the actual six current dev/release executables and two preserved EFRAME-3 dev
compatibility executables. Root dev binaries were built at `85f1464`, root release
at `5eaef18`, and stock hosts from implementation-equivalent inputs at `806d6b6`.
[Source reuse](time-1-evidence/source-reuse.json) enumerates the intervening
test-only differences; host and Runtime/Workbench production inputs are unchanged.
Actual matrix binary hashes were rechecked after all runs. Relinking is not
claimed to reproduce identical executable bytes.

Environment: Windows 10 build 19045, x86_64 MSVC, Rust/Cargo 1.95.0, Babashka
1.13.220, Python 3.14.6. Local zone is Windows `Russian Standard Time` (UTC+03
in these actual runs). Stock WGPU and stock Glow each have original child
`ready.json` renderer/adapter receipts in the raw archive. Native screenshots
are 1375x900 physical pixels for a 1100x720 logical viewport on this desktop.
See [environment](time-1-evidence/environment.json).

## Actual Runtime to API to window

All six runs use real `lab-runtime.exe`, `lab-workbench.exe`, the standard
isolated `lab-eframe-host.exe`, and the virtual thermal plant from
`examples/runtime.virtual.toml`. The run-local configuration starts at 80 °C,
ambient 20 °C, time constant 120 s, and passively cools with no instrument
commands. Recorder is disabled with the existing `best_effort` policy. No
scientific database is produced; UTC binding is available independently of it.

`clients/babashka-smoke/time1.clj` connects only to the existing Workbench API.
It calls `hello`, `client_status`, `presentation_get`, and `ui_replace_document`,
checks exact document readback/revisions, and confirms unchanged Runtime
attachment. It never opens a Runtime socket or calls control/recovery operations.
The original Parent remains the sole client and presentation owner.

| Run | Modes / API replacement | Live Scalar/Plot | Crash/restart and normal close | Runtime connections / display requests |
|---|---|---|---|---|
| dev WGPU, `time1-dev-wgpu-q2` | default, UTC, Runtime elapsed, local, experiment fallback | PASS | PASS | 1 / 0 |
| dev Glow, `time1-dev-glow-q1` | all five configurations | PASS | PASS | 1 / 0 |
| release WGPU, `time1-release-wgpu-q1` | all five configurations | PASS | PASS | 1 / 0 |
| release Glow, `time1-release-glow-q1` | all five configurations | PASS | PASS | 1 / 0 |
| old Runtime / new Parent, dev WGPU | all modes; calendar falls back | PASS | PASS | 1 / 0 |
| new Runtime / old Parent, dev Glow | default; new field omitted | PASS | PASS | 1 / 0 |

[Full results](time-1-evidence/gui-results.json) contain actual mode labels,
original X/Y coordinates, units, freshness, revisions, child paths and process
retirement receipts. Every requested UTC label was independently recomputed
from the captured Runtime hello anchor and original monotonic X. Example dev
WGPU labels were `09:27:31` UTC and `12:27:39` local at different successive
measurement instants, and `T+00:00:09` in Runtime elapsed mode. These are
display estimates with an explicit OS-clock accuracy qualification.

Each original transparent relay captured exactly these nine Parent requests:
`hello`, `discover`, `measurements_current`, `reference`, `controller`,
`recording_status`, `configuration_properties`, `discover`, `subscribe`.
No request was added by a document/mode change, display refresh, child crash or
restart. No mutation identity, `operation_status` or Exact Retry was sent.
All Workbench configuration sockets addressed the same Parent. The Runtime
boot/scope/next sequence remained unchanged through the GUI-only fault.

Each child was explicitly killed by its original PID. Logs show incarnation 1
`ExitedContained`, zero process/Job objects, zero IPC handles/pending I/O and
`ipc_retired=true` **before** incarnation 2 starts. The same explicit renderer
restores the current Parent-approved mode and live data. Normal native window
close then exits the Parent successfully with `ExitedHealthy` and the same
retirement proof, while Runtime remains alive. No automatic renderer fallback
was exercised or added.

The two compatibility runs use the exact previously qualified EFRAME-3 dev
Runtime/Workbench hashes, checked against its existing binary inventory. Old
Runtime sends no anchor; the new window visibly explains its relative fallback.
Old Parent ignores the additive hello field and continues forwarding LRD1 to the
new child. Older Parents require omission of the new presentation field.
This verifies repository clients; third-party strict closed response DTOs must
allow the documented optional hello result field.

## Time and contract tests

- Known monotonic/UTC pairs, including a fact before the anchor endpoint and
  signed pre-epoch UTC; relative origin is never treated as Unix time.
- UTC+03 crosses a date boundary; midnight and a synthetic two-day interval
  include the date where needed. Runtime elapsed includes an explicit day count.
- Synthetic Europe/Berlin spring/autumn 2025 transitions use a per-instant
  injected offset resolver through the real mapping path. The skipped and
  repeated local hours are checked; tooltip offsets distinguish repeated times.
- Missing, malformed, inconsistent, overflowing, unknown-source and wrong-boot
  handshake bindings cannot provide calendar time. A sampling bracket exceeding
  one second falls back. Unknown experiment start is explicitly unavailable.
- Service restart produces a different boot and binding; model hello replaces
  the binding and legacy hello clears it. A separate pair of **real executable**
  boots, with one hello-only client each and Recorder disabled, confirms new
  boot IDs and same-boot anchor identities. See [reboot evidence](time-1-evidence/runtime-reboot.json).
- Every optional mode survives v1 to canonical v2 admission/export. Absence
  remains omitted; unknown tokens and object-shaped enum values are rejected.
- LRD1/LRD2 round-trip compares the original selected scalar/trace record bytes
  exactly. Same-boot rebinding is rejected; a new clock source requires source
  progress. Existing selected-read freshness/revision/generation checks remain.

DST is a deterministic synthetic test, not a change to the user's Windows zone
or a qualification of every historical OS zone rule. Multi-day display is
synthetic, not an actual overnight acquisition. Absolute OS UTC accuracy/NTP/PTP
synchronization and a real experiment-start contract are not established.

## Gates

[Gate records](time-1-evidence/gates.json) link complete logs in the raw archive.
All final gates exit zero:

| Gate | Result |
|---|---|
| Full Runtime dev | 491 passed, 2 ignored, across 62 suites including empty harness/doc suites |
| Runtime release time/API/framing subset | 21 passed |
| Full Workbench dev and release | 547 passed, 58 ignored in each profile |
| Stock host dev and release | 9 passed in each profile |
| Native private IPC dev and release | 22 passed in each profile |
| Native Windows Supervisor dev and release | 7 passed in each profile |
| Separate Custom Glow child dev and release | 13 passed in each profile |
| Existing allocation/ownership probe release | PASS, including 10 ownership compile-fail cases |
| Workspace + stock host Clippy, both profiles | `--all-targets --locked -- -D warnings`, PASS |
| Workspace + stock host fmt and diff checks | PASS |

The allocation dev stress suite was not repeated; release probes and both
profiles' complete CPU/native suites cover the affected shared contracts. No
custom renderer/GPU algorithm was changed. [Protected boundaries](time-1-evidence/protected-boundaries.json)
confirm unchanged Core/OutputAuthority, Recorder sources, root Cargo files and
Custom Glow backend/device. Existing GUI control code changes are typed hello
test-fixture initialization only; enablement/actions are unchanged.

Preliminary failures are retained, not counted as passing qualification:

1. The first handshake test constructed a second trusted service before dropping
   the first, receiving `Component(Busy)`. Sequential shutdown/restart fixes the
   fixture; both final handshake tests pass in both profiles.
2. One initial native IPC suite passed 21/22, failing a one-shot shutdown status
   assertion. The original case passed in isolation. `Bridge::status` uses
   `try_lock`, so that assertion could interpret metadata contention as failed
   retirement. Only the test now waits for a readable bounded status and asserts
   the same handle/I/O conditions, with diagnostics. Both complete native suites
   subsequently pass. Production shutdown/retirement logic is unchanged.
3. The first GUI attempt used `enabled=false` with `policy=required`; existing
   validation correctly rejected it before any window/client was launched.
   The fixture correction to disabled `best_effort` is the final driver-only
   commit. All six later GUI runs pass.

## Process and Job memory

These are measured Windows process/Job peak commit values over both child
incarnations, not internal GPU allocations or a deployment memory policy.

| Current pair | Process peak bytes | Job peak bytes |
|---|---:|---:|
| dev WGPU | 442,556,416 | 449,097,728 |
| dev Glow | 185,339,904 | 191,897,600 |
| release WGPU | 432,779,264 | 439,345,152 |
| release Glow | 140,120,064 | 146,690,048 |

See [memory records](time-1-evidence/memory.json) for compatibility runs too.
Every test explicitly supplied 1,073,741,824 bytes. It is not a default or
deployment recommendation; MP-1 stays OPEN. No GPU memory manager is introduced.

## Evidence and reproduction

Follow [the contract and reproduction commands](time-1-time-axis.md). Keep unique
run directories. The raw archive preserves run-local configs, API and original
two-direction Runtime NDJSON, Parent logs, original standard-child JSON/PNG and
exit receipts, gate logs, failed attempts, dependency metadata and the exact
real-process reboot probe. Existing EFRAME scientific data was not modified.

[Raw manifest](time-1-evidence/raw-manifest.json) verifies all 387 archive members,
lengths, CRCs and SHA256. `raw-evidence.zip` is 8,463,050 bytes with SHA256
`860243631f9fb333cadeca4624ad88c337a013d9bb4acf07b4473ce20a1e6fdb`.
All 58 selected screenshots were decoded/CRC/pixel/hash checked during the
actual runs; archive verification then rechecked their bytes. Eighteen original
PNG/JSON pairs are included outside the archive for review. UTC/local WGPU,
release Glow UTC and missing-anchor WGPU frames were also visually inspected.
No image was edited. A hovered native tooltip screenshot is not claimed;
formatter strings are checked directly and the standard `label_formatter`
callback remains the original shared plotting path.

[173-package dependency inventory](time-1-evidence/dependencies.json),
[graph comparison](time-1-evidence/dependency-comparison.json) and the added
license texts preserve exact third-party evidence. Registry archive/source and
license checksums refer to original bytes; audit lock-file hashes refer to
checked-out files. Canonical source identities are separately the raw Git-blob
inventory. Published release license/provenance materials remain untouched.

These results qualify the stated virtual Windows experiment for independent
review. They do not qualify physical equipment, all time-zone databases,
other-platform GUI hosting, wall-clock synchronization, production memory policy
or production activation.

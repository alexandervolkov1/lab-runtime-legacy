# PORT-1: capability admission and current-machine renderer evidence

**READY FOR INDEPENDENT REVIEW — PORT-1.** `ACTIVATION_QUALIFIED=false`.
The existing Glow renderer admits the available NVIDIA context by API and
numerical capabilities, without a CPU/GPU/driver whitelist. Normal Workbench
startup still selects GUI v1. This is Windows test-only qualification at the
existing 1280 x 960, scale-one boundary; PORT-2 and PORT-3 have not started.

## Source, policy and scope

Base main is the B3B4C merge `d96111f488d8535c294879e767832d537219108a`.
Branch: `feature/port-1-gl-capability-admission`.
Final executed source: **`18c1aeb77899e0c7736831fcc0e49b8161f54702`**.
Subsequent review commits add documentation/evidence only.
[Draft PR #43](https://github.com/alexandervolkov1/lab-runtime-legacy/pull/43) remains
unmerged at `04c0e5180a4bc45ae12c7b62fdb3b86ad8f10e7c`; its BLOCKED record
is preserved. No release or product activation occurred.

There was no Rust hardware allowlist in the inspected main. The former B3B5
denial was an experiment-admission policy documented in #43. The current
[capability contract](gl-capability-contract.md) supersedes that policy without
rewriting its historical record. Driver/model changes alone no longer deny an
explicit PORT-1 experiment. Missing capabilities produce UnsupportedCapability;
resource/unknown initialization failures retain InitializationFailure and their
original diagnostic. No capability error triggers a backend fallback or retry.

The shared [Device](../../scripts/support/native-glow-device.rs) now checks
required entry points before constructing Glow, then queries the real context
profile, framebuffer and numerical limits. It records the actual selected GL
identity on success or a returned initialization failure. Known WGL invalid
version/profile errors and an empty framebuffer-config selection retain the
UnsupportedCapability category. Constructor unwind capture occurs before own GPU
payload exists; runtime rendering/retirement never crosses that catch boundary.
The [pure validator](../../scripts/support/native-glow-capabilities.rs) accepts
no hardware-name input. CIM fingerprint collection in the
[explicit experiment runner](../../scripts/test-port-1-gl.ps1) is diagnostic only.

Glow Device, HostSession, ParentPump, Supervisor, private IPC and the original
B3B1-B3B4C tests remain. Runtime, OutputAuthority, Recorder, the dispatcher,
recovery, presentation and read-only widget/Plot implementations are unchanged.
[Source boundaries](port-1-evidence/source-boundaries.json) and
[source hashes](port-1-evidence/source-inputs.json) retain that distinction.
No dependency, toolchain, public API, Runtime client or worker topology changed.

## Actual selected graphics context

| Observation | Current experiment |
|---|---|
| OS / compiler | Windows 10 Pro 10.0.19045 x64 / Rust 1.95.0 MSVC |
| CPU, diagnostic only | AMD Ryzen 5 9600X |
| Platform API / renderer implementation | WGL / existing Glow Device |
| GL_VENDOR | NVIDIA Corporation |
| GL_RENDERER | NVIDIA GeForce RTX 3060 Ti/PCIe/SSE2 |
| GL_VERSION | 3.3.0 NVIDIA 616.92 |
| GL_SHADING_LANGUAGE_VERSION | 3.30 NVIDIA via Cg compiler |
| Installed active-adapter driver diagnostic | 32.0.16.1692 |
| Actual Core profile / context flags | 1 / 0 |
| Default framebuffer | Complete, double buffered, RGBA 8/8/8/8, depth/stencil 0, samples 0 |
| Maximum texture / viewport dimensions | 32768 / 32768 x 32768 |
| Required resolved entry points | 64 |
| Actual drawable / scale | 1280 x 960 / 1 |

All **306 final native context records** admitted those capabilities. This counts
initialization, including later intentional faults, not healthy shutdowns.
The installed AMD adapter/driver is recorded separately and was not selected for
these GL runs. No current AMD, other-driver, monitor or DPI execution is claimed.
Their names do not become launch-denial rules. See
[graphics-contexts.json](port-1-evidence/graphics-contexts.json) and the original
`native-profile.json` records in the raw archive.

The seven new deterministic tests cover exact minima/newer Core versions,
missing numerical/API capabilities, every missing required symbol, diagnostic
identity independence, original error retention, documented WGL rejection versus
resource errors, and empty-config/loader unwind classification. Negative API
inputs are modeled; no physical unsupported-GL machine was substituted for them.

## Real framebuffer, ownership and bounded experiments

The original isolated harness feeds parent-approved read-only snapshots through
the original private IPC to the real native child. Its canonical document
replacement, scalar, Plot, live update, absence/rebuilding and revision tests
run unchanged. **552 final GL framebuffer buffers** were rechecked against their
original curve samples. Maximum cases match 32/32 curve samples, with zero
geometry failures, 32 traces and 16,384 projected points. The repeated-source
case reaches the existing 262,144 point-visit boundary; the distinct-source case
uses 16,384 original selected points. Canonical maximum admission remains exactly
1,048,576 bytes, with one live parsed child document.

[Dev maximum framebuffer](port-1-evidence/dev-maximum-repeated.png) and
[release live update](port-1-evidence/release-live-plot.png) are lossless exports
of actual Device RGBA readbacks. PNG conversion reverses GL row order only.
[Pictures](port-1-evidence/pictures.json) link every PNG to its raw member/hash;
[framebuffers](port-1-evidence/framebuffers.json) retain all final frame metadata.
GDI capture belongs only to the separate legacy v1 smoke.

The native renderer input is the existing bounded virtual Application fixture.
This is not the deferred real-Runtime-to-isolated-child B3B5 integration.
The separate existing real virtual Runtime suites and v1 smoke remain regressions
in their original scopes. No instrument commands originate from the child.

| Original lifecycle/fault gate | Dev / release result |
|---|---|
| Healthy own-object shutdown, original process exit and IPC retirement | PASS; own objects zero, independent exit/IPC proof |
| Canonical maximum, maximum selected data, sequential document replacement | PASS; old GPU/document roots retired before new admission |
| Crash, owner stall, expected-redraw stall, snapshot stall | PASS; original containment and explicit restart; no display-caused submissions |
| GPU Pending handoff and parent shutdown | PASS; original obligations retained until their existing receipts |
| 17 isolated GPU injection cases | PASS; unresolved identities retained, no blind deletion retry |
| Unconfirmed original process/IPC observations | PASS in preserved supervisor/IPC controls; replacement remains fenced |
| Deliberate 16 MiB denial | PASS as failed admission/containment; no healthy GPU receipt |
| Resize/minimize/restore and artificial unsupported-scale policy input | PASS in original scope; no new DPI qualification |
| 101 fresh incarnations per profile | PASS; 51 healthy closes, 50 intentional crashes, 100 explicit restarts |

Each final cycle checks its original process identity/exit and original IPC
retirement, zero retained OS/IPC handles and pending I/O, and credit classes
`[2 free, 2 latest, 0 current, 0 writing, 0 Assembly]`. Driver/system cache state
was not reset. The existing standalone Pending/fault controls retain their
BLOCKED general-terminal-lifecycle verdict; external termination is not cleanup.
[Cycles](port-1-evidence/cycles.json) and [faults](port-1-evidence/faults.json)
preserve original details rather than inferring object deletion from exit.

## Memory receipts

The initial ceiling was fixed before measurement at **536,870,912 bytes
(512 MiB)** for these explicit PORT-1 experiments. The existing Job owner sets
and reads both process and Job limits and checks membership before ResumeThread
for every incarnation. No error raised the cap. The original cycle receipt's
legacy `candidate_C` field denotes this test argument, not a deployment proposal.
`deployment_C=null`; the historical 224 MiB qualification is not transferred.
This cap applies to the supervised isolated child. Parent and Runtime are never
put in that Job. Separate legacy v1 and standalone GL controls keep their original
test/watchdog scopes; no new Job memory receipt is claimed for those processes.

| Final source measurement | Dev bytes | Release bytes |
|---|---:|---:|
| Maximum process commit | 178,253,824 | 178,552,832 |
| Maximum Job commit | 184,823,808 | 185,126,912 |
| Largest Job peak across 101 fresh incarnations | 153,063,424 | 152,788,992 |

Both final maxima occur in the repeated-source canonical-maximum scenario with
document replacements. These are original kernel cumulative peaks, not additive
phase allocations. The original B3B4C maximum Job peaks were 148,611,072 / 147,947,520
bytes on its AMD profile. A preliminary first dev episode on source
`fe4539156cea139197b6152aa46fb4c302405312` measured 215,875,584 Job bytes and
209,301,504 process bytes. It remains separately attributed; cache conditions
were not controlled and no memory improvement is inferred from the lower final
measurements. All those experiments retained the same 512 MiB ceiling.

CPU/GPU accounting remains separate: CPU Preview allowance 8,388,608 bytes,
fresh frame/no-upload/retirement maximum accounted 4,367,945 bytes; maximum parsed
document 2,335,040 bytes; selected-view receipts 9,348/9,742 bytes; four encoded
slots and the existing 4,399,104-byte encoded/IO/diagnostic bound; 4,915,200-byte
readback; existing staging/payload limits and original own-object ledger.
There is no sum presented as a whole-host Rust, driver or VRAM bound.
[Memory readings](port-1-evidence/memory-summary.json),
[CPU controls](port-1-evidence/cpu-summary.json), frame counters and raw native
receipts preserve those scopes. PORT-4 deployment-memory policy is not implemented.

## Dev/release regression and provenance

All **37 final gate receipts PASS** at the final source. Commands, durations,
log hashes and executed Cargo test-binary hashes are in
[gates.json](port-1-evidence/gates.json). Group counts overlap and are not summed
as unique tests.

| Gate | Dev | Release |
|---|---:|---:|
| Full Workbench | 537 PASS / 58 ignored | 537 PASS / 58 ignored |
| Capability tests, included above | 7 PASS | 7 PASS |
| Original isolated renderer matrix | 13 PASS | 13 PASS |
| Original 101-incarnation lifecycle gate | PASS | PASS |
| Native Windows supervisor / native IPC | 7 / 22 PASS | 7 / 22 PASS |
| OS capsule / actual suspension unit controls | 1 / 2 PASS | 1 / 2 PASS |
| Native non-Send/protected-policy compile failures | E0277 / E0453 as expected | E0277 / E0453 as expected |
| Original standalone GL smoke / Pending / fault matrix | PASS in original scopes | PASS in original scopes |
| CPU frame / no-upload / retirement rows | 12 / 4 / 9 PASS | 12 / 4 / 9 PASS |
| Borrowing negative compiler controls | 10 expected rejections | 10 expected rejections |
| Existing real virtual Runtime suite / strict v1 smoke | 13 PASS / PASS | 13 PASS / PASS |
| All-target/all-feature Clippy `-D warnings` | PASS | PASS |

The [test inventories](port-1-evidence/test-inventories.json) retain every one of
the 588 historical test names and add seven, for 595 in each profile. Document
allocation controls are a separate dev-only command; they are not labelled a
release run. The native child's Cargo target remains `test=false`; executed
capability tests come from the Workbench test import, not a zero-test Cargo receipt.
Formatting, PowerShell AST, archive, SHA256, links and diff checks are recorded in
[verification.json](port-1-evidence/verification.json).

Full historical CPU geometry/memory allocation measurements were not rerun or
relabelled as current-machine results. Their compiled inputs remain unchanged;
[cpu-evidence-reuse.json](port-1-evidence/cpu-evidence-reuse.json) identifies the
four changed original files that are absent from that probe wrapper. Fresh
Workbench, borrowing and frame/no-upload/retirement controls ran at the final
source. All original B3B4C source tests and qualification records remain intact.

[Preliminary gates](port-1-evidence/preliminary-gates.json) retain 25 successful
receipts from the earlier source before the WGL/config-error refinement. They
are separate from final qualification. Its Workbench test executable was rebuilt
before its hash was captured; no exact missing preliminary test-binary identity
is reconstructed. Final executed test and child identities are captured.
See [binary identities](port-1-evidence/binary-identities.json),
[deviations](port-1-evidence/deviations.json), and the
[canonical SHA256 inventory](port-1-sha256.txt).

[raw-evidence.zip](port-1-evidence/raw-evidence.zip) contains 6,187 original
members: command logs, native JSON/RGBA, original cycle aggregates, execution
identities and reproducer wrappers. Its
[manifest](port-1-evidence/raw-manifest.json) records every member's raw SHA256.
Final frame and memory summaries exclude preliminary-source measurements.

## Remaining limits and next-stage boundary

Desktop GL 3.3 Core, fixed 1280 x 960 physical drawable and scale 1 remain required.
Other graphics contexts require their actual capabilities, not a hardware-name
allowlist. Current execution evidence covers the selected NVIDIA context;
other implementations remain untested here. No universal stock-eframe backend
equivalence, WGPU fallback, resolution/DPI portability or Linux/macOS process host
is claimed. B3B5, CONTROL and M-CLEAN remain future work.

Historical hello-only remains UNQUALIFIED. M17.4 Recorder overlap, general
terminal GPU lifecycle and whole-host Rust budget remain BLOCKED. Final WGL
destruction remains LIMITED. `ACTIVATION_QUALIFIED=false`.
Do not merge, release or activate PORT-1 before independent review.

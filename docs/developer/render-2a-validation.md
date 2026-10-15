# RENDER-2A validation and evidence

Verdict: **READY FOR INDEPENDENT REVIEW — RENDER-2A**, limited to the independent
stock egui-wgpu offscreen proof. Review must retain the low-cap Job accounting
anomaly and the existing supervisor test race described below. Neither WGPU
memory policy nor production ownership/activation is qualified.
`ACTIVATION_QUALIFIED=false`.

The [proof report](render-2a-wgpu-proof.md) describes the dependency contract,
fixtures, independent owners, queue lifecycle, fault controls, limitations and
proposed RENDER-2B review boundary. It does not authorize starting RENDER-2B.

## Source, machine and selected API

Branch: `feature/render-2a-wgpu-proof`. Qualified executable source:
`b05007fc64efdfc2e675fd485d5f5a9ef2a13b11`. Later commits contain documentation
and evidence only. RENDER-1 merge and both parents are recorded in the
[merge identity](render-2a-evidence/merge.json).

All 361 original PORT-1 source/build/fixture inputs retain their canonical Git
SHA256. Only the separate twelve-file `scripts/support/wgpu-proof` workspace is
new among executable/build inputs; none of the root Cargo files or existing
apps/crates/clients/scripts is changed. [Source inventory](render-2a-evidence/source-inputs.json)
records the old and new inputs and the historical gate hash. New dependencies
are isolated from the Workbench graph, not hidden behind a product feature flag.

Windows/MSVC, rustc 1.95.0, Ryzen 5 9600X. Actual adapter in both profiles:
**NVIDIA GeForce RTX 3060 Ti, DX12, driver 32.0.16.1692**, vendor 4318, device 9353,
PCI `0000:01:00.0`. No AMD adapter execution is claimed. Requested features are
empty; device limits use wgpu 30.0.1 defaults and MemoryUsage allocation hints.
Validation is explicitly enabled in dev and release. Adapter-reported features,
limits, downlevel capabilities and target format usages are stored in each
`adapter.json`, including the unmodified driver_info field (empty on this backend).
Hardware strings are diagnostics, not admission criteria.

Glow comparison records the original WGL GL 3.3 Core NVIDIA 616.92 context and its
unchanged capability admission. It uses its own process and independent GL owner,
not a WGPU emulation of GL fences or deletion receipts. Environment diagnostics
are in [environment.json](render-2a-evidence/environment.json).

## Final proof gates

All eight final proof command receipts pass: fmt, warning-denied Clippy, build and
controller run in each profile. Every controller has twelve sequential child
cases, verified suspended membership/exact Job limits, original exit and handle
retirement. The negative low-cap outcome is **refusal/containment**, not a passing
hard aggregate-memory-bound test. [Commands](render-2a-evidence/gates.json) and
[process receipts](render-2a-evidence/processes.json) retain that distinction.

| Final proof | Dev | Release |
|---|---:|---:|
| Standard egui-wgpu real rendered/read-back frames | 24 | 24 |
| Same fixture through unchanged Glow Device | 24 | 24 |
| Matching CPU geometry/content pairs | 24 | 24 |
| Context generations per renderer | 2 | 2 |
| WGPU frame submits / completion callbacks / successful maps | 24 / 24 / 24 | 24 / 24 / 24 |
| Uncaptured WGPU errors in successful frame case | 0 | 0 |
| Fault/negative child cases apart from two frame backends | 10 | 10 |
| Unexpected final controller outcomes | 0 | 0 |

The **96** final raw framebuffer buffers are actual GPU reads, not JSON statuses
or GDI screenshots. Each has 4,915,200 RGBA bytes. CPU receipts preserve vertex/
index counts and exact fixture geometry digests; both renderer paths match for
all frames. Pixel checks tolerate raster differences, checking text pixels,
absence of false no-data curve pixels, both curve colors, expected transformed
sample neighborhoods, and full/partial/replacement image colors. Raw frames also
show changed numbers, title and axes. These are deterministic fixture values,
not Runtime observations and not a B3B5 end-to-end claim.

Examples: [WGPU partial update](render-2a-evidence/dev-wgpu-f07.png),
[Glow partial update](render-2a-evidence/dev-glow-f07.png),
[release no-data](render-2a-evidence/release-wgpu-f00.png),
[release dense Plot](render-2a-evidence/release-wgpu-f11.png).
[Picture provenance](render-2a-evidence/pictures.json) identifies their source
readback and SHA256. PNGs are lossless encodings with no resampling; Glow's row
order was normalized by the proof before archival. The dev WGPU partial frame
was visually inspected: two curves, labels/units, changed axes and the green
partial rectangle within the red image are present.

## Memory receipts and open qualification

The fixed test policy is 805,306,368 bytes (768 MiB) for both process and single-child
Job. Normal-case measured peaks are below those independently read-back limits.
They are not a choice of deployment C. Historical 224 MiB remains historical;
neither its qualification nor PORT-1's 512 MiB cap is transferred to WGPU.

| Final fixed fixture, bytes | Dev | Release |
|---|---:|---:|
| WGPU process commit peak | 338,776,064 | 340,881,408 |
| WGPU Job commit peak | 345,321,472 | 347,451,392 |
| Same-fixture Glow process commit peak | 162,856,960 | 164,474,880 |
| Same-fixture Glow Job commit peak | 169,435,136 | 171,040,768 |
| Maximum sampled WGPU HAL buffer memory | 10,944,512 | 10,944,512 |
| Maximum sampled WGPU HAL texture memory | 6,553,600 | 6,553,600 |
| Maximum sampled allocator allocated bytes | 16,515,072 | 16,515,072 |
| Maximum sampled allocator reserved bytes | 34,275,328 | 34,275,328 |

Initial standard vertex/index requests are 20,480 / 12,288 bytes. Actual allocator
entries start at 65,536 / 65,536 bytes and grow to 1,376,256 / 1,245,184 bytes. The
largest fixture submits 1,337,040 vertex bytes and 1,197,096 index bytes, below the
unchanged 2 MiB per-kind test ceiling. Staging allocations observed include 65,536,
262,144, 1,048,576, 1,245,184 and 1,376,256 bytes. A new buffer can coexist with old
buffer and staging references before completion. The snapshots capture up to
twelve buffers, three textures (including target) and two samplers. They are
sampled observations, not exhaustive allocator events or intra-call maxima.

After frame completion, free/drop of renderer resources and a device poll, the
last sampled allocator still reports 589,824 allocated and 12,582,912 reserved
bytes for WGPU internal buffers. Application reference release, queue completion,
allocator counters and process exit are separately recorded and none is promoted
to native memory/VRAM reclamation evidence. Full snapshot details are in
[memory.json](render-2a-evidence/memory.json); [summary](render-2a-evidence/proof-summary.json)
keeps each named allocation size and separate peak category.

**Open issue MP-1: negative Job accounting.** Both profiles installed and read back
16,777,216 bytes for process and Job before ResumeThread. The negative child entered
the executable but DX12 returned no adapter; it then awaited controller containment.
The original exit was confirmed (code 92) and all original handles retired.

| 16 MiB negative control, bytes | Dev | Release |
|---|---:|---:|
| PeakProcessMemoryUsed | 15,032,320 | 14,954,496 |
| PeakJobMemoryUsed | 21,614,592 | 21,524,480 |

The Job peak exceeds its configured limit. The cause is **not established**;
do not explain it away as allocator overhead, sampling, failed reservations or
successful enforcement. The original capsule's ABI layout/flags were inspected
against the documented structure; it uses both process/job-memory flags and exact
readback. Microsoft documents these as commit limits and separately tracked peak
fields; that documentation does not resolve this observation.
[Microsoft Job information contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information).

Thus initialization refusal and containment were observed, but a strict aggregate
16 MiB peak guarantee is **UNQUALIFIED**. No cap was raised, no successful GPU frame
was admitted under 16 MiB, and no whole-host bound is claimed. Resolve the accounting
semantics with separate measurements before qualifying a portable WGPU memory
policy. This does not erase the actual below-cap normal 768 MiB readings.

Other unqualified costs remain native compiler/driver allocations, physical VRAM,
surface/swapchain storage, other adapters/backends, maximum legal documents,
arbitrary legal texture-option histories, prolonged cycles, realistic failed
allocation sequences and final native teardown. The proof is not a B3B4C-equivalent
maximum/lifecycle qualification.

## Existing regressions and the retained release failure

| Original source gate | Dev | Release |
|---|---|---|
| Full Workbench suite | 537 passed, 58 ignored | First run: 536 passed, 1 failed, 58 ignored; one full recheck: 537 passed, 58 ignored |
| Original native frames / presentation replacement | PASS | PASS |
| Original native GPU Pending / original-root retention | PASS | PASS |
| Original native GPU fault matrix | 17 scenarios PASS | 17 scenarios PASS |
| Workbench/native child/Job Clippy, all targets/features, `-D warnings` | PASS | PASS |
| Workspace fmt and diff checks | PASS | Same unchanged sources |

The failed release test was
`supervisor::tests::in_flight_handle_retirement_never_publishes_terminal_or_admits_replacement`,
at `supervisor/tests.rs:720`: `root.stop(Stop::Graceful).unwrap()` received Busy.
`wait_status(Ready)` releases the metadata mutex; `stop` immediately uses try_lock
and explicitly permits Busy. The test assumes no intervening worker lock
acquisition. No WGPU code is linked into this test binary; its SHA256 is identical
to the RENDER-1 executable. The focused test and one full suite recheck passed.
This establishes intermittent behavior, not a repaired test. No supervisor source,
test expectation, timing, ignored list or safety contract was changed. Test
hardening is an open follow-up; the first failure remains in gates/raw evidence.

The six fresh native Glow commands use the original private IPC/native child and
the original 512 MiB test cap. They produced 74 context records and 153 actual
GL framebuffer captures, including intentional fault/restart cases. Every recorded
curve sample was checked again against its raw bytes during evidence sealing.
These are not 74 healthy shutdown receipts. The complete older qualification is also preserved:
PORT-1's 37 gates, B3B4C maximum/fault/fixed-C lifecycle, native IPC/Job supervisor,
borrowing/unsafe compiler negatives, CPU Preview bounds, strict v1 and original
real-virtual-Runtime tests retain their exact original inputs and evidence.
They are **reused unchanged-source evidence**, not relabelled as new WGPU tests.
See [PORT-1 gates](port-1-evidence/gates.json) and [RENDER-1 validation](render-1-validation.md).
Fresh smoke checks do not replace those full historical gates.

## Development failures and artifact identities

The first dev attempt at `5e9aa0e7138d6f7186768d5445120533cbf52994` produced a real
no-data frame, then aborted with exit 3221226505. The controller confirmed original
exit and stopped. The harness had not cleared applied TexturesDelta before Drop;
epaint 0.36.2 asserts on unconsumed deltas. This source inspection explains the
observed failure, but no original child panic text was captured in that attempt,
so its precise panic cause is an inference. No renderer cleanup is claimed.

Commit `370cd01df390163c65845c63c6c1297c47290bf2` consumed applied deltas and added
bounded panic diagnostics. Its entire dev series passed. Final source additionally
distinguishes pre-root initialization failure from actual retained-root failure
and tests atomic normal partial-update refusal. Both profiles were then rerun on
the final source above. All earlier receipts and compile diagnostics are retained.

| Executed final artifact | SHA256 |
|---|---|
| Dev proof | `5b63f60a6fcfa757e7b3c958126658b3c58bb6c1c2a88481ea53506253403a29` |
| Release proof | `6c29ef787ae81581c334e4ccb4b69759728f6ffa37aa7eeefe7cecfad3f90e57` |
| Dev Workbench tests | `db0df146d179a9e1e3b1ea182a4d485b27b33bc84ac07b98d815be0c587c06b4` |
| Release Workbench tests | `c43c9c63094545c38444e3921678d79a9e34a27fc24fc40d7f0245c00944042f` |

[Binary identities](render-2a-evidence/binary-identities.json),
[Glow regressions](render-2a-evidence/glow-regressions.json),
[framebuffer receipts](render-2a-evidence/framebuffers.json),
[raw archive](render-2a-evidence/raw-evidence.zip) and
[per-member hashes](render-2a-evidence/raw-manifest.json) preserve exact evidence.
The archive includes all logs, original failed runs, metadata, raw GPU bytes,
controller receipts and collector/recheck scripts. It does not bundle executables.
It contains 1,493 members in 13,100,626 compressed bytes. Every archive member is
decompressed and checked against its length and SHA256.
[Canonical SHA256 inventory](render-2a-sha256.txt) seals the report and evidence.

Historical hello-only remains UNQUALIFIED; M17.4 Recorder overlap and general
terminal GPU lifecycle remain BLOCKED; whole-host Rust budget remains BLOCKED;
final WGL destruction remains LIMITED. Surface/other GPU/DPI/OS qualification,
real Runtime B3B5, actions/controls and product activation are not claimed.

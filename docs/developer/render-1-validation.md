# RENDER-1 validation and evidence

Verdict: **READY FOR INDEPENDENT DESIGN REVIEW — RENDER-1**.
The [architecture decision](render-1-backend-separation.md) is a documentation-only
deliverable. WGPU execution and the ownership extraction remain **UNQUALIFIED**
and require the next review. `ACTIVATION_QUALIFIED=false`.

## Source and environment

Fresh commands ran on source `ea5789c1077ed1c8ac7f0651599d7daea2a232ed`, the verified
PORT-1 merge. All later RENDER-1 changes are developer documentation/evidence.
The branch is `feature/render-1-backend-separation`. All 361 PORT-1 source/build/
fixture inputs have identical canonical Git SHA256; protected roots `apps`,
`crates`, `scripts`, `clients`, Cargo manifests and lockfile have no delta from
qualified source `18c1aeb77899e0c7736831fcc0e49b8161f54702`.
See [input identity proof](render-1-evidence/unchanged-inputs.json).

Windows/MSVC, rustc 1.95.0, Ryzen 5 9600X. Actual selected GL was WGL,
NVIDIA Corporation, `NVIDIA GeForce RTX 3060 Ti/PCIe/SSE2`,
`3.3.0 NVIDIA 616.92`, GLSL `3.30 NVIDIA via Cg compiler`. All 74 context records
passed the unchanged capability checks: Core profile, RGBA8, complete double
buffer, scale 1, 1280x960 drawable, 64 required entry points. Actual reported limits
were texture side 32768 and viewport 32768x32768. Hardware diagnostics are not
admission criteria. This run does not claim to have selected the installed AMD GPU.

## Fresh checks

All **11 command receipts PASS**. Commands, timestamps, durations, output SHA256
and test-executable SHA256 are retained in [gates.json](render-1-evidence/gates.json)
and the raw archive. Counts below overlap; they are not summed as unique tests.

| Gate | Dev | Release |
|---|---:|---:|
| Complete Workbench tests | 537 passed, 58 ignored | 537 passed, 58 ignored |
| Preview/frame, generation, ownership/deletion/retirement, parent/IPC/supervisor unit controls | Included in full suite | Included in full suite |
| Original native first frames, selected Scalar/Plot, live update and document replacement | PASS | PASS |
| Original native GPU Pending handoff/stop, old root retention and explicit restart | PASS | PASS |
| Original native GPU fault matrix | 17 scenarios PASS | 17 scenarios PASS |
| Clippy: Workbench, native child, Windows Job; all targets/features; `-D warnings` | PASS | PASS |
| `cargo fmt --all -- --check` | PASS (workspace-wide command) | Same sources |

Native commands use unchanged `test-port-1-gl.ps1` / `test-isolated-renderer.ps1`
with an explicit **536,870,912-byte test-only process/Job cap**, exactly the existing
PORT-1 experimental ceiling. The original launcher sets and verifies membership
and limits before ResumeThread. No error changed it. This is not a new deployment
cap or a WGPU cap; `deployment_C=null`. There are no source changes requiring a
new memory-policy qualification.

The first native test uses the real private IPC/native child and a socket client
of the test Parent's existing presentation API. It checks `presentation_get`,
`ui_replace_document`, scalar/Plot snapshots, new points and revision replacement.
Runtime observations come from the original bounded virtual Application fixture;
**this is not a real Runtime → production Workbench executable B3B5 trial**.
Its zero-extra-submissions checks are fixture checks, not a count of real Runtime
TCP connections. No new Runtime integration claim is made.

The 17 fault scenarios include later real fence proof after artificial timeout,
wait failure, device loss, native teardown failure, original execution error, and
before/after deletion errors for texture/buffer/shader/program/VAO/fence. They
check exact original-root retention, no blind retry, original process/IPC
retirement and explicit recovery from latest approved snapshots. Except for the
later fence-proof case, containment stays **BLOCKED as a GPU cleanup proof**.
Healthy new incarnations do not erase the old failure evidence.

## Actual GL pixels and readings

The six native commands produced 37 contexts per build profile, including
intentional fault cases and their explicit restarts. These are not 74 healthy
lifecycle receipts. The complete [parent receipts](render-1-evidence/parent-receipts.json)
and [context reports](render-1-evidence/graphics-contexts.json) retain that distinction.

| Fresh, limited regression set | Dev | Release |
|---|---:|---:|
| Actual `glReadPixels` frame buffers archived and sample-validated | 78 | 76 |
| Maximum recorded process commit, bytes | 153,235,456 | 152,145,920 |
| Maximum recorded Job commit, bytes | 159,801,344 | 158,720,000 |

All **154 raw frame buffers** are 1280x960 RGBA and have zero reported geometry
failures. Every recorded curve sample was checked again against the actual color
bytes. The native first-frame tests also check no-data pixels, changed observation
pixels and changed document-axis pixels. The sample/counter records and raw-byte
hashes are in [framebuffers.json](render-1-evidence/framebuffers.json).

[Dev replacement frame](render-1-evidence/dev-glow-replacement.png) and
[release replacement frame](render-1-evidence/release-glow-replacement.png) are
lossless encodings of original GL readback, reversing only GL row order. The
[picture provenance](render-1-evidence/pictures.json) identifies their exact source
buffers. The dev image was visually inspected: the 65-point trace and changed
axes are visible. Existing fixed-profile axis-label clipping/overlap is also
visible; these tests do not establish new usability, resolution or DPI acceptance.

Peaks are kernel process/Job readings from this smaller regression set. They do
not replace PORT-1 maximum-document or 101-cycle measurements and are not evidence
of a memory optimization; driver/cache states were not controlled. Own-object,
CPU Preview, parsed-document, selected-read and IPC counters remain in the raw
records. No sum is claimed as a whole-host Rust/VRAM bound, and historical 224 MiB
qualification is not transferred.

## Reused qualification, not rerun or relabelled

The [37 original PORT-1 gates](port-1-evidence/gates.json) retain their original
source, binary and command identities. Their canonical evidence SHA256 and
unchanged inputs were rechecked. Historical B3B4C/PORT-1 source tests and reports
are not removed or shortened. The following complete evidence is reused because
no executable input changed:

| Preserved gate | Original dev/release result |
|---|---|
| Full original isolated renderer matrix / 101-incarnation fixed-cap lifecycle | 13 tests / 101 incarnations per profile PASS in original scope |
| Native private IPC / native Windows supervisor | 22 / 7 PASS per profile |
| OS capsule / suspension controls | 1 / 2 PASS per profile |
| Native non-Send and protected unsafe-policy compiler negatives | E0277 / E0453 as expected |
| CPU frame / no-upload / retirement / borrowing negatives | 12 / 4 / 9 rows PASS; 10 expected compiler rejections per profile |
| Standalone original Glow smoke / Pending / fault controls | PASS in their original scopes; terminal lifecycle limitation retained |
| Strict legacy v1 / existing real virtual Runtime acceptance | PASS / 13 tests PASS per profile, M17.4 excluded as before |

The original complete qualification remains in
[PORT-1](port-1-gl-capability-admission.md), including its separate dev-only
document-allocation gate and historical CPU geometry evidence. These results are
not represented as new RENDER-1 test runs. The fresh native checks above are
additional regression evidence, not replacements for those gates.

## Binary and raw evidence identity

| Executed artifact | SHA256 |
|---|---|
| Dev native child | `fe4709968ee5d75f0efdc79bc72e353b2973a174174897e0e1a0a75413145797` |
| Release native child | `fb9e8d4813adb5e4a82175f5d0c065f4509c1dd3c31f3a9a8bdad5325857882e` |
| Dev Workbench test executable | `db0df146d179a9e1e3b1ea182a4d485b27b33bc84ac07b98d815be0c587c06b4` |
| Release Workbench test executable | `c43c9c63094545c38444e3921678d79a9e34a27fc24fc40d7f0245c00944042f` |

Child identities were recorded by the unchanged launcher before execution and
verified again during sealing; [binary-identities.json](render-1-evidence/binary-identities.json)
keeps all six invocations. Test-binary hashes were captured after each command
before subsequent profile work. [Experiments](render-1-evidence/experiments.json)
retain the test-only cap and source identity.

[Raw evidence](render-1-evidence/raw-evidence.zip) contains **907 members**,
5,215,323 compressed bytes: complete command logs, native JSON/RGBA, parent
receipts, hardware diagnostics, identities and local reproducer/collector scripts.
Every member's SHA256 and byte length was rechecked after compression against
[raw-manifest.json](render-1-evidence/raw-manifest.json). The scripts in the archive
are audit tooling; they are not linked into the workspace or a product host.

[Canonical inventory](render-1-sha256.txt) seals this report, design, source-input
identity and artifacts; it excludes itself. [Verification](render-1-evidence/verification.json)
records final documentation links, diff checks and protected-input checks.
No new dependency, toolchain, Runtime/OutputAuthority/Recorder/Clojure semantics,
public API or memory/geometry/IPC quota changed.

Remaining limits: desktop GL 3.3 Core, fixed 1280x960, scale 1; historical hello-only
UNQUALIFIED; M17.4 Recorder overlap, general terminal GPU lifecycle and whole-host
Rust budget BLOCKED; final WGL destruction LIMITED. WGPU pixels/lifecycle/memory,
new DPI/size, other-platform child ownership and B3B5 are unqualified here.
RENDER-1 is ready for design review, with no RENDER-2 implementation, merge,
release or production activation performed.

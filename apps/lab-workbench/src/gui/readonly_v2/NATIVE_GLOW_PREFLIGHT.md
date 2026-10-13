# DGUI-4C7B2C4B1: native Glow preflight and platform failure correction

**Status: BLOCKED at the required integration-seam gate.** No native GPU smoke
or GPU lifecycle qualification is claimed. `ACTIVATION_QUALIFIED = false`.

C4A PR #31 was checked at approved HEAD
`60ae02e859e762d4d1292a8a023d713174de2c83`, made Ready and merged normally as
`58035120aedaf0a2105ee0f0763aeca811d0eac3`. The approved commit is its second
parent. Main was synchronized and clean before branch
`feature/dgui-4c7b2c4b1-native-glow-smoke` was created. No release was created.

The task requires a separate review proposal before a substantial change to the
C4A interface. Its infallible `drain` contract needs such a change to represent
bounded GPU completion failure. This PR fixes an independently reproducible
platform cleanup gap and provides the [concrete retirement proposal](GLOW_DRAIN_PROPOSAL.md).
It does not install an unqualified backend behind the existing trait.

## Public API and source findings

Sources are the exact Cargo.lock versions: eframe/egui/epaint/egui_glow 0.36.2,
egui_plot 0.37.0, glow 0.17.0, glutin 0.32.3, glutin-winit 0.5.0 and winit 0.30.13.
Cargo.lock, features, dependency declarations and compilation profiles are
unchanged. `NATIVE_GLOW_SOURCES.txt` hashes the audited sources, including all
previous C4A dependency evidence.

| Boundary | Verified public surface | Limit / consequence |
|---|---|---|
| Document / native app lifetime | `eframe::AppCreator<'app>` returns `Box<dyn App + 'app>`; `run_native` accepts `AppCreator<'_>` | A borrowed admitted document can outlive the native run. No leak, owning clone or self-reference is needed merely to create an App |
| GL handle | `CreationContext::gl`, `Frame::gl`; `eframe::glow` reexport | Supplies an Arc to the API, not ownership of the OS context's current-thread lifetime |
| Independent meshes | `eframe::egui_glow::Painter::new`, `set_texture`, `paint_primitives`, `texture`, `free_texture`, `destroy` | A separate painter has its own texture map. Never forward Preview shapes into eframe's main Context/painter |
| Full / partial upload | Ordered `ImageDelta` calls to `set_texture` | Pinned implementation borrows pixel bytes; may panic on creation failure; no fallible upload receipt |
| Draw / staging | `paint_primitives` borrows the CPU mesh slice; streams vertices/indices through VBO/EBO | Does not supply a completion fence or fallible draw receipt; driver copies/storage are not measured by the Preview meter |
| GPU completion | Public raw Glow `fence_sync`, `client_wait_sync`, `delete_sync`, `get_error` | Unsafe current-context calls; timeout/error must preserve ownership. Existing production lint forbids unsafe code |
| Cleanup | C4A `drain() -> ()`, then unconditional free of all tracked keys | Cannot report pending/failed bounded drain; this is the primary integration blocker |

The native renderer launcher now compiles `scripts/support/glow-seam-check.rs`
against the real Workbench artifacts in both profiles. It checks the borrowed
Preview/document through `run_native` and the public painter signatures. The
witness is never invoked and creates no window, GL context or GPU resource.
It is explicitly not evidence for render scheduling, input, DPI or shutdown.

Pinned `Painter::set_texture` and `paint_primitives` return `()`. In dev,
`check_for_gl_error!` consumes errors and logs them; checking GL error only after
the call is not a reliable adapter failure receipt. `set_texture` also unwraps
texture creation. A backend returning `Ok(())` around these calls would need
separate error observation/creation handling. No private API workaround or
`catch_unwind` claim is introduced.

`client_wait_sync` can return a signaled result, timeout or wait failure;
the latter two are not completion. The pinned native binding takes `i32`
nanoseconds and casts to `u64`, so a negative timeout is not a bounded policy.
Its `fence_sync` wraps the raw pointer in `Ok` without checking for null; a Rust
`Ok` alone is not proof of a valid fence.
See the [Khronos wait reference](https://registry.khronos.org/OpenGL-Refpages/gl4/html/glClientWaitSync.xhtml).
`finish()` has no timeout argument or failure result; it waits for prior GL
effects to complete, so it does not supply the required finite failure path.
See the [Khronos finish reference](https://registry.khronos.org/OpenGL-Refpages/gl4/html/glFinish.xhtml).
This is a blocker for the present ownership contract, not a claim that every
possible OpenGL host design is impossible.

## Implemented platform failure correction

Previously `TextureOwner::process` armed `in_flight` before upload and submit,
but not before `platform`. With an empty set delta, platform processing could
start dependent work and fail; retirement then skipped drain and could free
the previously uploaded texture. Even an empty ledger could miss a platform
completion obligation. The fix arms `in_flight` **before** platform processing.
Every error after that point drains before namespace free, without new storage,
retry, interface change, or altered projection/geometry policy.

`errors_without_new_uploads_drain_before_retirement` failed before the fix:
expected `[Platform, Drain]`, observed `[Platform]`. It now checks platform,
submission and completion failure both with and without existing textures,
exact call order, no premature free, and idempotent retirement/Drop.
`warmed_frame_faults_without_uploads_never_acknowledge_or_retry` verifies the
actual Preview/Frame path: a completed initial frame, another frame with no sets,
each of the three errors, a failed outcome, drain/free and complete CPU cleanup.
The mock starts dependent work in `platform`, so an omitted drain is observable.
The before/after transcript is in `NATIVE_GLOW_PLATFORM_REGRESSION.txt`.

Four additional native **CPU/mock** cases use the production 32-Trace / 16,384-point
Plot: one completed initial frame followed by either success or platform/submit/
completion failure with no uploads. They measure every Preview allocation and
realloc overlap through shutdown. Mock counters and GPU counters are distinct;
no mock `HostCompleted` is presented as real GPU evidence.
The new CSV's `submitted` counter counts mock submit calls, including the
injected failed attempt; `completed` counts only successful mock completions.

## Qualification results

Final dev/release gate totals and CPU evidence are recorded in
`NATIVE_GLOW_GATES.txt` and `NATIVE_GLOW_NO_UPLOAD_{DEV,RELEASE}.csv`.
The fixed CPU scope remains Windows x86_64 MSVC, original profiles/default fonts,
1280x960 at DPI 1, bounded events/Unicode, one Plot/Context, 32 traces and
16,384 B1 points. B1's 512 KiB is already inside the 8,388,608-byte allowance.
The formula remains `accounted = realloc_overlap_peak + 127 * peak_blocks + 65_536`.
Historical C3/C4A CSVs and limits are preserved.

Both full native launchers exited successfully. The 299 previous targeted
regressions plus two new tests pass in **dev (301) and release (301)**. All three
borrowing compile-fail gates, public API/lifetime compile witness, v1 GUI/Plot/
hover/Confirm, widget/Unicode/navigation, admission/conversion/export and
architecture gates pass. Native admission allocation faults, formatting,
warning-denied all-target Clippy and diff checks also pass.

| Native CPU set | Cases/phases in each profile | Maximum accounted bytes, dev = release |
|---|---:|---:|
| Geometry | 119 | 4,610,337 |
| C3 combined memory | 125 | 4,458,653 |
| Plot widget | 113 | 4,483,157 |
| C4A frame owner | 12 | 4,355,499 |
| New no-upload host failures/control | 4 | 4,367,945 |
| A2 Page/tessellation | 5,120 pages plus stress/lifetimes | 3,013,664 |

The combined maximum remains **4,610,337 / 8,388,608 bytes**, leaving 3,778,271.
The five preserved evidence sets match C4A in both profiles; new dev/release
rows are byte-identical. `NATIVE_GLOW_EVIDENCE_SHA256.txt` records the comparison
and hashes instead of duplicating unchanged historical CSVs. Isolated **CPU
MEMORY QUALIFIED** remains applicable only to the original Windows/MSVC scope.

New cases have phase peak 3,590,538, realloc-overlap peak 4,170,202 and peak blocks
1,041; maximum retained 644,282 (failed cases retain zero after retirement).
Maximum shapes/vertices/indices are 202 / 69,760 / 106,926 including GUI geometry;
the separate own-curve bounds remain 65,536 / 98,304. All new cases have zero
geometry/allocation faults and post-drop **0 bytes / 0 blocks / 0 mock keys**.
The unchanged C3 suite still tests four curve and two shared B1 allocation faults.

| New CPU/mock sequence | Set / partial | Submit calls / successful completions | Drain / free | Outstanding after drop |
|---|---:|---:|---:|---:|
| Warm + success without uploads | 1 / 0 | 2 / 2 | 0 / 1 | 0 |
| Warm + platform error without uploads | 1 / 0 | 1 / 1 | 1 / 1 | 0 |
| Warm + submit error without uploads | 1 / 0 | 2 / 1 | 1 / 1 | 0 |
| Warm + completion error without uploads | 1 / 0 | 2 / 1 | 1 / 1 | 0 |

Each new sequence has one generation and one texture key. The sole completion
in an error row belongs to its initial successful frame. There is no automatic
retry or acknowledgement of the failed second frame. These are mock obligations,
not GPU objects or proof of actual GPU drain.

## Native GPU evidence and memory accounting

| Requested evidence | C4B1 result |
|---|---|
| Window/GL creation, first frame, real full/partial/multiple updates, draw | Not run: integration-seam gate blocked |
| Native navigation, hover/pan/zoom, Unicode/runtime changes, occurrence/epoch transitions | CPU regressions only; native behavior not measured |
| Real submitted/completed frames, generation changes, textures created/destroyed, outstanding GPU resources | Not measured; mock counters cannot fill these fields |
| Upload/partial/platform/submit/completion/device-loss fault injection on a real adapter | No real adapter; existing and new mock errors exercise CPU protocol only |
| Bounded GPU drain and shutdown without final free | Not qualified; infallible retirement cannot retain an unresolved obligation |
| Host CPU staging, upload copies, transient allocations | Not measured; no host is implemented, and driver allocation is outside the native CPU meter |
| Texture dimensions/payload, buffers/meshes and VRAM | Not measured; no VRAM claim from CPU measurements |
| Maximum simultaneously live Preview namespaces / frame payloads | Still structurally one / one; no new resource queue |

For the reviewed follow-up, account texture base-level payload as
`width * height * bytes_per_texel` per actual format, with mipmaps separately;
account VBO/EBO uploads from actual mesh lengths and driver reallocations
separately. Include native framebuffer/depth/swapchain storage, painter metadata,
OS input and any staging/copy storage. Formula payload is not measured VRAM or
a driver-allocation upper bound. The one-generation limit is not relaxed.

## Remaining blockers and review boundary

1. Review and implement fallible retirement, one pending namespace and an explicit
   terminal shutdown owner. The proposal specifies the required state transitions
   and bounded-wait failure policy; C4B1 does not silently change C4A's interface.
2. Review the narrow native raw-GL safety/error boundary, current-context lifetime
   and public event-loop/render scheduling. A separate painter API and App lifetime
   compile check alone do not discharge these obligations.
3. Implement and measure the real native smoke after that review: fixed viewport,
   input conversion, redraw, all requested faults and real resource counters.
   Device loss may remain a lifecycle blocker even after healthy-device smoke.
4. Budget actual host CPU/GPU storage and obtain independent GPU/host qualification
   and separate activation approval. v2 remains isolated; no smoke Runtime worker
   or physical equipment is used. No C4B2, release or active integration is started.

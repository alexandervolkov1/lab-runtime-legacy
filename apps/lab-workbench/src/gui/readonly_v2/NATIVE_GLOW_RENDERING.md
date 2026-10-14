# DGUI-4C7B2B1: native rendering and healthy GPU lifecycle

Qualification is limited to this isolated Windows/MSVC smoke. No Runtime,
physical instrument, active Workbench v2 integration or release is involved.
`ACTIVATION_QUALIFIED = false`. This is a Draft for independent review, not C4B2B2.

| Independent result | Status | Scope |
|---|---|---|
| GPU RENDER SMOKE | PASS | Actual desktop OpenGL and framebuffer verification, both profiles |
| HEALTHY GPU LIFECYCLE | PASS | Proven completion and checked resource release before context destruction |
| TERMINAL GPU LIFECYCLE | BLOCKED | Unresolved work, device loss and failed deletion have no qualified teardown |
| HOST BUDGET QUALIFICATION | BLOCKED | Rust/capacity/payload measurements do not establish a complete native/driver ceiling |

CPU MEMORY QUALIFIED retains the existing isolated Preview scope and its
**8,388,608 accounted-byte allowance**, including B1. Native host measurements
do not redefine that allowance or extend its qualification.

## Baseline and native integration

PR #33 was verified at `efc10ae53be1d4e10f356be0145c7cb4aae37dbb`, made Ready,
and merged normally as `f3fd5d00d6e8b964513e09dc13163230c3ffbe3f`. Its second
parent is the reviewed commit. Main was synchronized and verified clean before
creating `feature/dgui-4c7b2b1-native-glow-rendering`. No release changed.

`scripts/test-native-glow.ps1` compiles production Preview, PreviewFrame,
HostSession and TextureOwner into a separate test executable using exact locked
Workbench build artifacts. The sole production Rust change exposes the existing
generation allocator at crate scope for independent synthetic conformance frames.
Runtime/Workbench APIs, DTOs, B1, FiniteTraceItem and active v1 behavior are unchanged.

The public seam is **winit 0.30.13 ApplicationHandler + glutin-winit 0.5.0
DisplayBuilder + glutin 0.32.3**. The finite scripted sequence renders synchronously
inside RedrawRequested, using deterministic RawInput events. This does not qualify
arbitrary native input or resize handling. The Windows-only process disables DPI
awareness before event-loop creation and verifies a native scale of one and a
1280x960 drawable. Preview/readback uses one pixel per point. Desktop composition
may scale the displayed window; it does not change the GL coordinate domain.

The native root retains the device and external HostSession. Its scoped exercise
owns the admitted document, virtual observations and exclusively borrowed Preview.
No self-reference or forged lifetime is needed. A pending Preview Drop transfers
its CPU frame and Context into HostSession, while the native root remains alive.
There is no eframe App, main v1 painter, on_exit or adapter bypass.

Creation order is window/config, surface/context, make current, Glow API, session,
document/observations and Preview. Healthy shutdown requires Released, zero keys,
no retained frame, checked deletion of all owned GL objects and no GL errors.
It then explicitly makes the context non-current and verifies that transition.
Session/device/context/surface drop before the window. glutin's WGL destructor
calls DeleteContext without a returned receipt; its Drop is never GPU completion
evidence or a qualified terminal teardown.

## GL boundary and receipts

`scripts/support/native-glow-device.rs` is the only new unsafe module, compiled
only into the standalone test crate, whose root denies unsafe code. Production
Workbench retains workspace `unsafe_code = forbid`. The previously reviewed
allocation-meter module is reused only in the test executable. The device owns
one unshared context, checks its creating thread and exact current context, and
exposes no raw GL handle. No foreign painter consumes GL errors. Explicit byte
packing avoids casts, assumed egui vertex layouts and retained borrowed staging.

The pinned Painter audit remains in [the preflight](NATIVE_GLOW_PREFLIGHT.md).
Its void set_texture/paint_primitives, error-consuming internal checks and texture
creation unwrap are **not used** by this adapter. Direct Glow commands have local
error checks, shader compile/link status, buffer-size and texture size/format
queries. Unknown upload/draw outcomes return FrameError, never HostCompleted.
This is not a late glGetError around a library that consumed its own error state.

Keys are `(ContextGeneration, TextureId)`. Eight fixed texture slots and the
production sixteen-update cap remain. Full RGBA8 and bounded partial uploads,
including ordered updates of the same key, share one staging vector. There are
no mipmaps. Packed vertices are 20 bytes; indices are u32. One VBO/EBO pair per
populated generation grows only when needed and otherwise uses buffer_sub_data.
GLSL 330 performs premultiplied gamma-color multiplication and alpha blending,
without optional dithering. Viewport and scissor are explicit. Callbacks and
unregistered textures are rejected. The platform sink accepts root-only output,
its already-selected default theme and local cursor intent; other native work
such as clipboard, URLs and secondary windows fails closed.

Desktop GL 3.3 and a sufficient texture limit are checked before rendering. After
all dependent commands, including commands preceding an operation failure, one
fence is created and checked for non-null, is_sync and GL errors, then flushed.
Glow's Ok alone is insufficient because its native fence wrapper can contain null.
Only ALREADY_SIGNALED/CONDITION_SATISFIED can yield Quiesced. Timeout/failure retain
ownership; context loss is sticky. The signaled fence is deleted and checked.
No glFinish is used. See the [Khronos wait contract](https://registry.khronos.org/OpenGL-Refpages/gl4/html/glClientWaitSync.xhtml).

The unchanged WaitPolicy allows twenty calls, each requested for at most 5 ms,
under a monotonic 100 ms application deadline. The caps are checked **between
calls**, not absolute bounds on driver execution. An intermediate TIMEOUT_EXPIRED
can be followed by another wait within that budget; resources stay busy until
actual proof. An exhausted/failed observation leaves HostSession Pending.

Texture deletion follows Quiesced and is validated before clearing the device
ledger. Last-key deletion releases VAO, buffers, program and shaders for the
generation. FrameBackend free remains void: the native root additionally checks
the device ledger, sticky errors and zero objects. A HostSession Released value
alone is insufficient native evidence. Failed deletion retains the root and
blocks successful exit; it is not qualified as terminal cleanup.

## Actual rendering evidence

Recorded hardware: AMD Radeon(TM) Graphics, ATI Technologies Inc., OpenGL
`3.3.0 Core Profile Context 25.10.30.02.250923`. Native config: RGB8/alpha8,
depth/stencil zero, samples zero. Toolchain: rustc 1.95.0, Windows x86_64 MSVC,
existing pinned egui 0.36.2 / egui_plot 0.37.0 dev/release profiles.

The sequence renders Page; opens Plot; draws 32 traces/16,384 points; updates
observations; exercises hover/zoom; navigates Plot -> Unicode Details -> Plot;
explicitly retires the Context; draws the next generation with a full atlas
upload; and shuts down. It includes ordinary/coincident points and widths through
32. Five successive Preview generations and one independent conformance namespace
are exercised, with only one generation live at a time.

After each real completion fence, glReadPixels reads the actual back buffer.
Thirty-two separate curve meshes supply interior sample positions, each required
to have expected cyan nearby in readback. The same Plot with no observations is
a negative control. The full Plot has **32/32 matching samples and 85,972 cyan
pixels** in the tested region; the control has **zero**. Identical repeated Plot
frames have identical RGBA digests. Measurement updates and zoom change pixels;
hover text and exact Unicode UTF-8 details are also checked. After zoom, all
twenty sampled visible curves match their pixels. Digests are diagnostic, not
cross-driver golden-image promises.

A conformance frame performs full red -> partial green -> partial blue updates
of one 4x4 texture in a single ordered delta. Readback is `[0,0,255,255]` inside
the clipped quad and `[5,8,10,255]` outside its scissor. Its texture free follows
the real fence and leaves zero registered keys.
Two malformed-image cases (short and long pixel vectors) must fail before any
staging growth or GL allocation; they cannot bypass the dimension-based bound.

| Real counters | Dev | Release |
|---|---:|---:|
| Submitted/completed frames | 21/21 | 21/21 |
| Draws; submitted vertices/indices | 76; 596028/931788 | same |
| Textures created/deleted; full/partial uploads | 6/6; 6/6 | same |
| VBO/EBO objects created/deleted; reuse calls | 12/12; 124 | same |
| Programs; shaders; VAOs created/deleted | 6/6; 12/12; 6/6 | same |
| Fences created/deleted; flushes | 21/21; 21 | same |
| Actual waits / intermediate timeouts | 21/0 | 22/1 |
| GL errors; peak simultaneous own objects | 0; 8 | same |
| Remaining frames / keys / own GL objects | 0/0/0 | same |

## Pending root and terminal limit

`-Pending` uses a separate child process and suppresses completion observations
after real upload/draw. It injects Timeout at the backend seam, **not** a driver
timeout or a real wait. Twenty-five new-frame attempts are rejected. After
Preview Drop and shutdown, the root retains one key, Context, FrameData, seven
GL objects and the native context/window. Deletes and completion receipts are
zero. The event loop waits and ignores CloseRequested while pending. The script
observes that state and terminates only its child externally. That is explicitly
**not** a teardown receipt. Its monotonic 30-second watchdog likewise treats any
unexpected hang or unresolved healthy run as failure, never cleanup.

Exhausted real waits, wait failure, reset/device loss, failed GL deletion,
panic/forced OS destruction and confirmed terminal teardown remain BLOCKED for
C4B2B2. There is no automatic recovery, re-render or device recreation.

## Host and GPU accounting

The existing Rust allocation meter runs in this separate native process. The
before-Preview phase includes native Rust root/GL loader and virtual fixture.
The rendering phase includes **both host and Preview**; it is not host-only and
must not be compared directly with the isolated Preview allowance. OS/driver
heaps and VRAM are excluded. Both profiles record:

| Measured CPU scope | Bytes |
|---|---:|
| Before Preview: live / peak requested Rust heap | 1,778,355 / 3,987,578 |
| During rendering: peak requested Rust heap including host + Preview | 9,066,176 |
| After window destruction: retained Rust heap | 342,780 (31 platform/singleton blocks) |
| Fixed Device / Root (Root includes inline HostSession) | 6,696 / 1,256 |
| Host staging maximum capacity | 1,312,400 |
| Conservative old+new staging realloc capacity overlap | 2,624,400 |
| Single framebuffer readback allocation | 4,915,200 |

One staging vector and one readback coexist with bounded inspection text/sample
records; the backend retains no mesh clone. Source ImageDelta/mesh and packed
staging coexist during upload. Readback starts after FrameData consumption.
The retained Rust platform/singleton allocations are not registered GL objects.
Their allocation-site attribution remains unqualified; the meter is not a leak
classification or proof of complete native heap reclamation.

| GPU resource | Logical/queried payload |
|---|---:|
| VBO / EBO maximum queried storage | 1,312,400 / 395,592 bytes |
| Maximum simultaneous VBO+EBO storage | 1,707,992 bytes |
| Font atlas | 2048x32 RGBA8 = 262,144 bytes |
| Conformance texture | 4x4 RGBA8 = 64 bytes |
| Maximum simultaneous texture payload | 262,144 bytes |

These are **not exact driver VRAM consumption or its upper bound**. Native
front/back drawable storage, compositor resources, shader executables, driver
alignment, internal/transient copies and allocator metadata are unqualified.
Even RGBA8's 4,915,200-byte nominal drawable image is not a measured driver
allocation. A complete host ceiling and terminal-path accounting remain open.

## Verification and reproduction

```powershell
./scripts/test-native-glow.ps1 -BuildProfile dev
./scripts/test-native-glow.ps1 -BuildProfile release
./scripts/test-native-glow.ps1 -BuildProfile dev -Pending
./scripts/test-native-glow.ps1 -BuildProfile release -Pending
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile dev
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
./scripts/test-v2-allocation.ps1
cargo test -p lab-workbench --locked -- gui::readonly_v2:: presentation:: gui::controls:: gui::app:: external::tests:: dispatcher::tests:: operator_boundary_tests:: model::read_binding:: --test-threads=1
# Repeat targeted tests with --release before --locked.
cargo fmt --all -- --check
rustfmt --check --edition 2024 scripts/support/native-glow-device.rs scripts/support/native-glow-smoke.rs
cargo clippy -p lab-workbench --all-targets --locked -- -D warnings
git diff --check
```

The native launcher also runs warning-denied Clippy on its generated crate,
including the unsafe module. CPU probes remain separate executables. Historical
CSVs and allocation-failure tests are preserved. See `NATIVE_RENDER_GATES.txt`,
`NATIVE_RENDER_{DEV,RELEASE}.txt`, `NATIVE_RENDER_PENDING_{DEV,RELEASE}.txt` and
`NATIVE_RENDER_SHA256.txt` for recorded evidence. New direct probe inputs and
exact license/source evidence are in `third-party-licenses/native-glow/README.txt`.
Cargo.lock, workspace dependencies, features and profiles are unchanged.

Both profiles pass 308 targeted regressions, five borrowing compile-fail gates,
119 geometry, 125 C3 memory, 12 C4A frame, four C4B1 no-upload and nine C4B2A
pending cases. Widget/Page/Unicode/navigation, admission/conversion/export,
v1 GUI/Plot/Confirm, architecture, formatting and warning-denied Clippy pass.
All measured CPU CSV rows and Widget/Page bytes match the preserved qualification
evidence. The combined maximum remains 4,610,337 / 8,388,608 accounted bytes.

Remaining blockers: terminal GPU release, complete host/driver memory ceiling,
other GPUs/drivers/DPI/window configurations, arbitrary native interaction and
separate activation approval. Healthy smoke authorizes no active integration.

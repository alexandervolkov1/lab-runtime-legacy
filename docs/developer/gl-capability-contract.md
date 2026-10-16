# PORT-1: capability admission for the existing Glow renderer

This is the current admission policy for explicitly requested Windows test-only
renderer experiments. It supersedes the hardware-fingerprint restriction used
to stop B3B5 in [historical Draft PR #43](https://github.com/alexandervolkov1/lab-runtime-legacy/pull/43).
That PR and B3B1-B3B4C evidence remain historical records; they are not rewritten.
`ACTIVATION_QUALIFIED=false`. The ordinary Workbench still selects GUI v1.

CPU/GPU model names, installed-adapter lists, driver versions and monitor names
are diagnostics, never launch allowlists. A driver update alone cannot deny an
experiment. Admission checks the actual current context selected by glutin and
the capabilities used by the existing renderer. A successful experiment on one
adapter does not establish equivalence with every other adapter or eframe backend.

## Required API and numerical capabilities

The [original Device](../../scripts/support/native-glow-device.rs) requests an
unshared desktop OpenGL 3.3 Core context on its native event-loop thread. A newer
desktop Core version is accepted. OpenGL ES and compatibility-only contexts are
outside this contract. Shader compilation/linking, actual allocation/storage
validation, GL error checks and object/fence validation remain execution gates.
The API requirements follow the operations used by Device and the pinned Glow
loader, against the [OpenGL 3.3 Core specification](https://registry.khronos.org/OpenGL/specs/gl/glspec33.core.pdf).

| Capability | Required behavior |
|---|---|
| Context | Exact creating context current on its creating thread; desktop version >= 3.3 and actual Core profile bit |
| Entry points | All symbols in `REQUIRED_SYMBOLS` resolved before Glow construction; null and WGL failure sentinels rejected |
| GLSL | Existing `#version 330 core` vertex/fragment shaders compile and link; existing diagnostic failure/retirement behavior retained |
| Vertex input | VAO, vertex/index buffers, 32-bit element indices and the existing attribute layout |
| Texture input | RGBA8 level-zero 2D textures, full/subimage uploads, sampler uniform, existing filter/wrap support |
| Texture extent | Actual `GL_MAX_TEXTURE_SIZE >= 2048`; every subsequent image extent checked against that actual limit before staging |
| Framebuffer | Original default framebuffer complete, double buffered, each back-left RGBA channel >= 8 bits |
| Viewport | Actual `GL_MAX_VIEWPORT_DIMS` covers 1280 x 960 |
| Drawing/readback | Existing blend/scissor, default back-buffer RGBA/unsigned-byte readback and surface presentation |
| Completion | Sync creation/validation, flush, finite client wait and checked deletion of the original fence |

No extension/vendor string is used to grant an API that is missing. No maximum
GL limit is used to enlarge an application quota. The pure
[capability validator](../../scripts/support/native-glow-capabilities.rs)
contains numerical/API inputs only. It cannot acquire Runtime, GL ownership or
completion authority.

Missing required entry points, API version/profile or numerical capabilities are
reported as **UnsupportedCapability**, with the failed requirement. Other
initialization failures retain **InitializationFailure** and their original stage
and diagnostic, including glutin errors and a caught Glow loader panic before
own GPU objects exist. A glutin context error explicitly marked NotSupported is
classified UnsupportedCapability, as are documented WGL invalid-version/profile
codes and an absent matching framebuffer configuration. The constructor captures
Rust initialization unwinds before any own GPU payload exists; runtime GPU work
and retirement never cross that catch boundary. Unknown/resource initialization
failures are not relabelled as unsupported hardware. There is no
UnqualifiedHardware category.
The WGL code meanings follow
[WGL_ARB_create_context/profile](https://registry.khronos.org/OpenGL/extensions/ARB/WGL_ARB_create_context.txt).

## Diagnostics and ownership

One structured `GL_INITIALIZATION` record is emitted for success or a returned
initialization failure. The native child also retains it in the `graphics` field
of `native-profile.json`, including failed startup. It records the actual WGL/EGL
platform API, GL_VENDOR, GL_RENDERER, GL_VERSION, GLSL version, selected framebuffer
configuration, actual context flags/profile, limits, color bits, drawable/scale,
entry-point count, admission result and originating error stage. Unavailable
fields remain absent/empty on earlier failure. Retained GL strings are limited to
512 characters each; this is not a bound on all transient driver/library memory.

The external experiment runner separately saves CIM hardware diagnostics. Failure
to read those diagnostics does not deny capability admission. Installed hardware
is not evidence of which GPU renders a context; GL_RENDERER is the actual context
observation, not a CPU/GPU preference or a hardware-switching feature.

Glow Device, HostSession, ParentPump, Supervisor and private IPC retain their
owners and existing progress/failure rules. Capability checks happen before own
GPU payload creation; they do not prove future allocation success or GPU cleanup.
After submission, existing original fence/deletion and retained-root obligations
apply. Process exit and IPC retirement remain distinct from an own-object cleanup
receipt. No unresolved root is destroyed to try another backend.

## Test-only execution and memory

```powershell
./scripts/test-port-1-gl.ps1 -BuildProfile dev -TestCommitCap 536870912
./scripts/test-port-1-gl.ps1 -BuildProfile release -TestCommitCap 536870912
```

The runner requires committed source inputs and an explicit test commit cap; it
has no hardware-name comparison and launches only the original isolated-renderer
test harness. The existing Windows Job owner sets and reads both process and Job
commit limits and verifies membership before ResumeThread, for every incarnation.
No new worker, Runtime connection, host selector, fallback or restart loop exists.
The default harness excludes its separate 101-cycle gate; request that original
gate explicitly with `-TestFilter native_hundred_fixed_cap_cycles`.

**536,870,912 bytes (512 MiB)** is the initial finite PORT-1 experiment ceiling,
chosen before current-machine measurement. It is not a calibrated deployment cap
or an automatically increased retry value. The historical 224 MiB qualification
is preserved exclusively as historical evidence; it is not transferred to this
machine. New receipts must record process commit, Job commit and the original
CPU Preview/document/selected-view/IPC/GPU-object accounting independently.
VRAM and driver-global memory remain outside those accounting scopes. A portable
deployment memory policy remains PORT-4 work.

## Remaining boundaries

PORT-1 retains the fixed **1280 x 960 physical pixels, scale 1** profile, including
the existing shader constants, drawable suspension, staging/readback and CPU/GPU
budgets. Dynamic resize and actual DPI support belong to PORT-2. No WGPU backend,
backend fallback, Linux/macOS process host, public API, B3B5 virtual integration or
control implementation is introduced. Those require their own review and tests.

Historical hello-only remains UNQUALIFIED. M17.4 Recorder overlap, general terminal
GPU lifecycle and whole-host Rust budget remain BLOCKED. Final WGL destruction
remains LIMITED. `ACTIVATION_QUALIFIED=false`.

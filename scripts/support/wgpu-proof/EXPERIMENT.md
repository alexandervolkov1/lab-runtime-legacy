# RENDER-2A fixed experiment policy

This is an independent test workspace, excluded from the product Cargo graph by
its own workspace root. The root Cargo.lock and product feature selection stay
unchanged. No production host, paired document/session migration or fallback.

Before any WGPU execution, select **805,306,368 bytes (768 MiB)** as the finite
test-only process AND single-child Windows Job commit ceiling. This is an initial
exploratory ceiling for driver/shader initialization with separately bounded test
payloads, not inherited Glow qualification or a deployment proposal. Never raise
it following an error. A separate **16,777,216-byte (16 MiB)** negative experiment
tests inadequate commit admission. Each child is created suspended through the
unchanged lab-windows-job capsule; membership and exact limits are checked before
ResumeThread. The controller runs one child at a time, verifies original exit and
retires original handles before the next case. A 30-second controller deadline
contains a stuck case; it is not an OS or GPU termination-time guarantee.

Fixed target: 1280x960 RGBA8, scale 1, one offscreen target and one 4,915,200-byte
readback buffer (5120-byte aligned row pitch). WGPU uses the actual DX12 adapter
selected by its capability-based request, with no vendor/model whitelist. Glow
comparison uses the original Device and the exact same deterministic test UI.

Application fixture bounds: at most 32 frames per case, 192 mesh primitives,
2 MiB vertex bytes and 2 MiB index bytes per frame, eight textures, sixteen texture
updates per frame, no paint callbacks, no user/native textures, no mipmaps. Images
are at most 4 MiB each; validate shape, byte length and partial bounds before
normal uploads. At most one submitted frame/map operation is admitted at a time.
The standard renderer owns all WGPU pipelines, textures and buffers. Its internal
capacity growth, caches, staging and driver allocations are measured separately;
these input ceilings alone are not a whole-host or VRAM proof.

Only deliberate isolated negative controls bypass normal validation. Label every
injection, retain original diagnostics and distinguish API-forced destruction,
withheld completion observation, real API validation errors and physical loss.
Pending work and ambiguous errors are never success. GPU timeout keeps the same
root alive until original-process containment; no GL deletion receipts are made.

Native surface/presentation, resize/DPI, real device-reset/TDR, general cleanup,
whole-host Rust/VRAM bounds and RENDER-2B ownership remain unqualified.
ACTIVATION_QUALIFIED=false.

# EFRAME-1 dependency and unsafe-boundary review

The test-only [Cargo.toml](../../scripts/support/eframe-host/Cargo.toml) has an
independent workspace and lock, seeded from the reviewed RENDER-2A lock and
resolved offline. No unrelated package update or root Cargo change is needed.
`audit.py` records Cargo metadata for x86_64-pc-windows-msvc, feature edges,
exact package licenses and license-file hashes. Registry archive SHA256 must
match Cargo.lock, and every extracted file must match the archive. This is an
integrity/license inventory, not an exhaustive vulnerability or driver audit.

There are 168 active Windows packages and 338 package/version entries shared
with the product lock, with identical checksums. egui/epaint/eframe/egui-wgpu each
resolve once at 0.36.2; egui_plot is 0.37.0; wgpu/core/hal/types/naga are 30.0.1.
winit is 0.30.13, glutin 0.32.3, glutin-winit 0.5.0, raw-window-handle 0.6.2,
image 0.25.10, log 0.4.33. The full graph and additions/removals are in the
[inventory](eframe-1-evidence/dependencies.json) and
[feature tree](eframe-1-evidence/features.txt).

The host enables standard eframe `wgpu`, `glow`, `default_fonts`, `accesskit`;
upstream egui-wgpu includes its normal winit and capture integration. WGPU enables
its standard backend set, with target-specific Vulkan/DX12/GL implementations on
Windows, rather than an application-written backend selector. Default feature
names for other platforms in Cargo resolution are not platform qualification.
There is no direct wgpu dependency or application GPU resource construction.

New application Rust uses `forbid(unsafe_code)`. Trusted native unsafe operations
remain in upstream eframe/egui-wgpu/egui_glow, wgpu-core/hal, Naga/compiler paths,
ash, gpu-allocator, glutin, winit, Windows bindings and graphics drivers. The
unchanged lab-windows-job capsule retains its narrow audited native ownership.
Python ctypes is a separate test-only OS input boundary: Windows x64 INPUT ABI,
original HWND/PID checks, temporary input-queue attachment with guaranteed
detachment, foreground verification for mouse/Unicode injection and finite
operations; it performs no graphics, Runtime or apparatus calls.

WGPU initialization, shader compilation, swapchain, texture/font caches,
submission and screenshot staging may allocate inside upstream libraries and
drivers. No finite vertex count is represented as a bound on all those objects.
Standard eframe cleanup and OS process containment replace neither production
memory-policy qualification nor a proof of native GPU allocation reclamation.

Licenses include existing MIT/Apache alternatives, BSD, ISC, Zlib, BSL-1.0,
Unicode and font terms. Select the Apache-2.0 option for `Apache-2.0 OR GPL-2.0-only`;
no GPL-only dependency is adopted. Preserve the exact compound expressions and
license files recorded in the inventory. This source experiment does not update
published product license evidence or authorize binary redistribution. A later
shipping decision must extend the exact-version product notices and license gate.

# Exact test dependency and safety review

This proof is not a product dependency or release license change. Its independent
Cargo.lock is seeded from the root lock and resolves offline. The shared package
versions and registry checksums remain identical; root Cargo.toml/Cargo.lock are
unchanged. `audit.py` records the full Windows target graph, feature tree, package
licenses and license-file hashes, verifies registry archive SHA256 against the
lock, and compares every extracted source file with its archive bytes.

egui/epaint/eframe/egui-wgpu are exactly 0.36.2, egui_plot exactly 0.37.0;
wgpu/core/hal/types/naga are exactly 30.0.1. egui-wgpu defaults are disabled.
WGPU enables only std, dx12, wgsl and diagnostic counters. No Vulkan, Metal, GLES,
WebGPU, WebGL, noop, winit integration feature or egui-wgpu custom renderer is
enabled. Existing winit 0.30.13/glutin 0.32.3/glutin-winit 0.5.0 are used for the
unchanged Glow comparison only. Eframe enables its existing Glow/default-fonts/
accesskit graph; it does not run a second host or render WGPU in this executable.

New renderer/compiler/allocator code is trusted third-party native code, not a
sandbox. The Rust proof uses safe wgpu/egui-wgpu APIs and denies unsafe code.
The original, byte-unchanged native-glow-device and lab-windows-job capsules retain
their existing explicit unsafe boundaries. Wgpu-core validates API inputs;
wgpu-hal, gpu-allocator, presser, Windows COM/DX12 bindings and native graphics
drivers contain unsafe operations and allocations outside the application's CPU
budgets. Bytemuck vertex casts are in upstream egui-wgpu/epaint; no new local cast,
shader, allocator, native handle escape or custom WGPU resource manager is added.
WGSL translation/pipeline creation may allocate before the first frame. Queue
writes allocate staging that can survive until a later submit/completion. Native
driver calls may overrun a software deadline. The preselected Job limit and
original-process containment are required for every GPU experiment.

The graph uses permissive MIT/Apache alternatives, BSD, ISC, Zlib, BSL-1.0 and
the already present Unicode/font terms. Choose Apache-2.0 for the alternative
Apache-2.0 OR GPL-2.0-only declaration; this is not an adoption of GPL-only code.
The inventory retains exact expressions instead of rewriting compound licenses.
This is a source experiment, not a redistribution authorization: any future
product/backend shipping decision must extend the product's exact-version notice
inventory and license gate. Do not replace published release license evidence.

Review anchors are the exact registry sources and upstream API documentation:

- [egui-wgpu Renderer 0.36.2](https://docs.rs/egui-wgpu/0.36.2/egui_wgpu/struct.Renderer.html)
- [wgpu Queue 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html)
- [wgpu Device 30.0.1](https://docs.rs/wgpu/30.0.1/wgpu/struct.Device.html)

No exhaustive dependency vulnerability audit or native-driver correctness proof
is claimed. These limitations prevent promoting the experiment to production.

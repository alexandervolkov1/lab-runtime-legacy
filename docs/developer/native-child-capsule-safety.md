# B3B4A native child capsule safety and license review

The unpublished `lab-native-child` Cargo binary is an inert packaging proof.
It does not launch an event loop, window, GL context, IPC episode or Runtime
connection. It is not the isolated renderer host and grants no integration
admission. `ACTIVATION_QUALIFIED=false`.

Its sole unsafe permission is the existing checked
`scripts/support/native-glow-device.rs` module, included by path. All of that
implementation's thread/current-context checks, fixed ledger, bounds, completion,
checked deletion, failure latches and glutin RAII ownership remain unchanged.
The binary crate denies unsafe everywhere else. Workbench, Runtime and lab-core
continue inheriting workspace `unsafe_code=forbid`. No public library interface
is exported from this binary; the Device remains private and non-Send.

The shared safe source contains only ContextGeneration's existing monotonic
allocator, FrameError, TextureKey, Completion/WaitFailure/WaitPurpose/WaitRequest.
It also shares the unchanged 192-primitive MAX_SHAPES bound.
The existing deletion diagnostics source is included unchanged. Private aliases
preserve Device's original paths without copying its implementation or importing
Preview, FrameBackend/TextureOwner, model, client, dispatcher, recovery, Recorder
or OutputAuthority. Workbench reexports these same ownership values at the old
internal paths, so its renderer and the B2B2 probe use the same type definitions.

The new package promotes the exact previously qualified transitive versions:
eframe 0.36.2, glutin 0.32.3, glutin-winit 0.5.0, winit 0.30.13 and
raw-window-handle 0.6.2. Glow 0.17.0 remains eframe's reexport. Explicit feature
sets match the existing Windows B2B2 artifacts; defaults that would add GLX or
window-decoration dependencies are disabled. Cargo.lock adds only the local
package entry: no third-party version/checksum/lock record changes are accepted.
An initial default-feature experiment was stopped and its lock changes removed;
it is not the reviewed configuration.

Exact-version archives, source/license hashes and source audit are retained in
[native Glow license evidence](../../third-party-licenses/native-glow/README.txt)
and the existing release license inventory. No vendor license bytes, toolchain,
SDK or released provenance are modified. This review covers the newly direct
dependency/feature relationship and reuse of the existing audited unsafe module;
it does not certify arbitrary GL drivers or declare whole-host Rust storage,
driver/VRAM allocation, failed cleanup or final WGL destruction qualified.

`scripts/test-native-child-capsule.ps1` builds the real Cargo target and runs
Clippy with denied warnings in each profile. A separate metadata-only negative
control imports the same private sources and requires Device: Send; E0277 is
required. A second negative control applies Workbench's protected forbid policy
to the same source and requires E0453. The script never executes the binary or native operations. Packaging
this compatibility target into a product release is outside this stage.

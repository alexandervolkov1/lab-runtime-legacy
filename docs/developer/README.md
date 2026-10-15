# Developer and API documentation

This section is for integrators, driver authors and maintainers. It is separate
from the [User manual](../README.md); ordinary users need no source checkout.

## Interfaces and configuration

- [Application API](../api/README.md): requests, discovery, operations and events.
- [Application overview](../application-api.md): the experiment/client boundary.
- [Workbench API](../workbench-api.md): the local presentation and client endpoint.
- [Generic Controls Architecture](../generic-controls-architecture.md): DGUI-4C
  approved direction with DGUI-4C1 internal view normalization, DGUI-4C2 bounded
  scalar drafts and DGUI-4C3 typed read bindings; v2 contracts remain proposals.
- [Generic Presentation v2 contract proposal](../generic-controls-v2-contract.md):
  DGUI-4C4 design-only DTOs, bindings, version selection, bounded composition and
  lossless conversion rules for independent architectural review; not implemented.
- [Native GUI host decision](native-gui-host-adr.md): DGUI-4C7B3A preflight comparing
  an embedded host with a supervised GUI child, including ownership, containment,
  memory limits and read-only recovery; GUI v2 remains inactive.
- [Independent parent pump](independent-parent-pump.md): DGUI-4C7B3B1 serial model
  owner and independently scheduled renderer-free harness; legacy v1 keeps its
  existing host and GUI v2 remains inactive.
- [Windows Job supervisor](windows-job-supervisor.md): DGUI-4C7B3B2 inactive
  production seam, original-process exit proof and no-GPU containment fixtures.
  [Independent review follow-up](windows-job-supervisor-review-followup.md)
  records launch admission and first-containment fixes and their qualification.
- [Private read-only IPC](private-readonly-ipc.md): DGUI-4C7B3B3 owner-only
  overlapped pipes, closed snapshot codec, shared credits and original I/O
  retirement receipts; qualified no-GPU fixtures, GUI v2 remains inactive.
- [Isolated renderer compatibility seams](isolated-native-renderer-child.md):
  DGUI-4C7B3B4A explicit admission handoff, selected borrowed provider and inert
  Cargo native capsule; full B3B4 renderer integration remains BLOCKED.
- [First isolated renderer frame gate](first-isolated-renderer-frame.md):
  DGUI-4C7B3B4B shared read-only consumer and first real isolated GL frame,
  native fault/restart evidence and preliminary memory measurements; full B3B4
  qualification and activation remain blocked.
- [Configuration and CLI reference](../reference/configuration.md): full field
  tables, argument grammar, limits, path resolution and live configuration.
- [Transport reference](../reference/transports.md): TCP, WS and verified WSS.
- [Babashka examples](../../clients/babashka-smoke/README.md) and
  [ClojureScript example](../../clients/clojurescript-smoke/README.md): optional
  programmable clients, not user installation prerequisites.

## Drivers and contracts

- [Architecture](../architecture.md) and [safety/failure contracts](../safety-and-failures.md).
- [Recorder/SQLite reference](../recorder-sqlite.md) and
  [recovery/fault reference](../recovery-and-faults.md).
- [Extension overview](../extending-runtime.md).
- [SimpleDevice schema](../simple-device.md) and [tutorial](simple-device-tutorial.md).
- [Native driver tutorial](full-driver-tutorial.md).
- [Project status](project-status.md): published baseline and qualification limits.

## Build and package from source

Use the Git source repository, not the portable package. The reviewed toolchain is
Rust 1.95.0; changing it requires a new standard-library license-evidence review.

On Windows, use an existing Rust/MSVC build environment:

```powershell
cargo build --workspace --locked
cargo fmt --all -- --check
.\scripts\test-release-license-evidence.ps1
.\scripts\package-developer-preview.ps1 -PreviewVersion v0.1.0-preview.5
```

The packager builds binaries and writes `dist`; do not use it to check documents
or overwrite archived release evidence. Run it only in a deliberate packaging
workspace. A version argument is not permission to replace an existing release.

For Linux packaging, use an existing Linux x86_64 Rust environment with a C
compiler/linker, binutils, Git and Python 3.11.4 or newer:

```sh
python3 scripts/test_release_license_evidence.py
python3 scripts/package-linux-runtime.py --preview-version v0.1.0-preview.5
```

The Linux packager builds Runtime, checks the extracted files, and writes a Runtime
archive, mandatory license archive, provenance JSON and SHA-256 companions. Existing
outputs are protected against accidental overwrite. Windows packaging includes
Runtime and Workbench. Neither script publishes a release.

Both user packages take their manual and safe examples from
`scripts/user-package-files.json`. Integration clients and engineering/API
references remain in Git; the manual links to versioned online references.
The Linux license companion must be extracted into the executable directory.
Compare binary hashes with the previous release: unchanged Rust inputs alone do
not prove byte-for-byte identical binaries.

Use [release license evidence](../release-license-evidence.txt) for legal materials,
exact-version checks and notice scope. Keep the original `.crate`, Rust library
inventory and nested license texts; Cargo application dependencies alone do not
cover every distributed component.

For documentation-only changes, check Markdown paths and fragments in both the
source tree and the package's explicit public-file inventory. Run the packager's
Markdown checks against a documentation projection; do not rebuild binaries just
to validate links. For example, with a new task-owned output directory:

```powershell
$auditDirectory = Join-Path ([IO.Path]::GetTempPath()) ('lab-runtime-doc-audit-' + [guid]::NewGuid().ToString('N'))
.\scripts\test-package-documentation.ps1 -OutputDirectory $auditDirectory
```

This copies only public documents/examples, checks their hashes and links, validates
PowerShell examples and checks audience separation. It leaves the projection for
inspection and refuses to overwrite an existing directory. Changing the inventory
must also update extracted-package presence checks. Choose targeted tests for
affected behavior instead of repeating unrelated full workspace gates.

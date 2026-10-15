# DGUI-4C7B3B4: renderer-child compatibility stop

**BLOCKED.** This branch records the requested architectural stop before renderer
integration. It does not implement or qualify a renderer child. The stop concerns
direct composition of the existing seams, not an impossibility result for the
isolated-host design. `ACTIVATION_QUALIFIED=false`.

## B3B3 merge and starting point

PR [#39](https://github.com/alexandervolkov1/lab-runtime/pull/39) was checked open,
Draft and mergeable, with reviewed head
`0230c89045802ee78ed1872cee3697137eaabdad` and unchanged main base
`5190760603ddbef4e567dc6b08a52001e046fe78`. Its qualified implementation source is
`82d8ad12e3cf71f7b389a21e94e456b214da9e0d`. After another identity check, it was
made Ready and merged normally on 2026-10-15:

| Merge fact | SHA |
|---|---|
| Merge | `76c2ae718c7c1f368ccf7b9bb90dbb7540ab19a2` |
| First parent: unchanged base | `5190760603ddbef4e567dc6b08a52001e046fe78` |
| Second parent: reviewed head | `0230c89045802ee78ed1872cee3697137eaabdad` |

Local main was fast-forwarded to origin/main, both parent identities were checked,
and `git status --porcelain` was empty before creating
`feature/dgui-4c7b3b4-isolated-native-renderer-child`. No release was created.

## Concrete ownership/admission conflict

The [host contract](native-gui-host-contract.md#memory-scopes-and-admission-gates)
requires one admitted child document, with the old parsed document retired and
dropped before admitting its replacement. The
[B3B3 admission contract](private-readonly-ipc.md#admission-atomic-replacement-and-freshness)
qualifies zero persistent child parsed documents and one temporary admission.

The unchanged sources expose the following incompatible composition:

1. [`Preview`](../../apps/lab-workbench/src/gui/readonly_v2/mod.rs) borrows an
   `AdmittedV2Document` and, for a GPU host, an external mutable `HostSession`.
   Its Plot state also borrows the document. It cannot render encoded slot bytes.
2. [`Replica::chunk()`](../../apps/lab-workbench/src/private_ipc/replica.rs) calls
   `v2::admit()` on a complete document inside the receive operation, compares its
   canonical bytes, drops that temporary parsed candidate, then publishes the
   encoded Current slot. It supplies no renderer retirement/admission handoff.
3. Keeping the old Preview/document alive while calling this operation on a new
   complete document therefore overlaps two parsed admissions. Each admission's
   existing <=4 MiB check only covers itself; it does not authorize the overlap.
   Four encoded credits do not account for an additional parsed document.
4. Re-admitting the accepted document on each frame does not eliminate this
   issue without an explicit lifetime/retirement boundary. Dropping a Preview
   can transfer unresolved Context/FrameData to HostSession; it does not prove
   completion, deletion, or permission to drop that session or start a generation.

This is a source-level control-flow/lifetime finding, not a measured memory-cap
overrun or a failing existing B3B3 regression. Merely spawning the current smoke
fixture under a Job would leave it unresolved. No overlapping-document allowance,
second renderer, hidden v1 pump, or unresolved-root Drop is used on this branch.

## Other boundaries the proposed adapter must resolve

The checked Glow implementation is
[`native-glow-device.rs`](../../scripts/support/native-glow-device.rs), explicitly
standalone-only. It uses `allow(unsafe_code)` and crate-private renderer types.
Workbench inherits workspace `unsafe_code=forbid`; it cannot directly include that
module. This is a packaging/safety boundary, not a reason to lower the Workbench
lint. A separate reviewed child/native boundary can reuse the implementation;
the current repository has no such Cargo target. The compile-only experiment
below verifies this narrow incompatibility without executing GL.

The [B3B3 codec](../../apps/lab-workbench/src/private_ipc/reads.rs) retains selected
typed values, absences, units, source identities, quality/freshness and points in
encoded bytes. B2B2's scalar reader and Plot projection instead take the concrete
`RuntimeObservations` cache and inspect its entity/discovery/live storage. A
consumer adapter is still needed. Building synthetic discovery or Runtime
messages to satisfy that interface would not establish original source evidence.
A bounded read-only provider should expose the validated selected semantics and
points directly, including absence and the independent non-current latch.

The existing private Status/Closed packet describes a no-GPU owner counter, read
snapshot and current flag. It has no native-loop/render-loop/checked-frame receipt.
Closed currently feeds `healthy_close`. An integrated child must emit its healthy
receipt only after real own-object release, with the final WGL limitation stated;
neither a helper pong nor that existing fixture packet establishes GPU completion.

These are integration gaps. They do not invalidate B3B1-B3B3 or B2B2's historical
qualification scopes.

## Minimal options for independent architectural review

Both options retain the existing ParentPump, single supervisor/Job episode,
protocol identities, four shared slots and original IPC retirement. Both need a
bounded selected-read adapter and a separately reviewed native child boundary
that reuses the same Device and checked deletion/completion implementation.
Neither widens public Application/Workbench DTOs or moves authority into the child.

| Option | Required narrow ownership change | Evidence required before implementation admission |
|---|---|---|
| A: scoped epoch owner | Keep document, HostSession and Device in stable outer scopes; borrow Preview during native event pumping. Before delivering the first replacement-document chunk to Replica, suspend rendering, retire Preview, resolve its HostSession obligations and drop the old parsed document. Preserve that chunk in its original I/O buffer until the handoff completes. | Demonstrate safe callback lifetimes, continued native/control progress while admission is suspended, no buffer reuse before receipt, one parsed admission, and no replacement/new generation on Pending. |
| B: explicit admission handoff | Extend the internal Replica consumer seam so complete Assembly bytes stay in one existing credit until the renderer provides retirement/drop permission. Only then admit and commit the replacement; retain old accepted encoded bytes on rejection. | Demonstrate atomic accepted identity, rejected-candidate rollback, bounded pending admission, exact credit retention, old-read invalidation and unchanged B3B3 cancellation/retirement behavior. |

These are unimplemented alternatives, not qualifications or a request to restart
B3B1-B3B3 design. For either, document replacement uses Preview retirement, not
`HostSession::shutdown()` as a reusable transition: shutdown closes admission
permanently. An unresolved session retains its exact root/ledger until valid proof
or OS containment. A failed candidate may resume the old encoded document only
after the candidate's parsed scratch is gone and all epoch/freshness fences apply.

Review should choose the lifetime/admission seam and native packaging boundary
before integration resumes **within B3B4**. This stop does not start B3B5.

## Verification, provenance and unrun gates

[`test_renderer_child_compatibility.py`](../../scripts/test_renderer_child_compatibility.py)
builds the existing Workbench artifacts at locked versions, then type-checks the
unchanged B2B2 source twice. The standalone `deny` control must succeed; applying
Workbench's `forbid` policy must fail with E0453 at the native module's `allow`.
It emits metadata only: it creates no native window, GL context, renderer child,
Runtime connection or laboratory effect. Reproduce in fresh task-owned directories:

```powershell
python scripts/test_renderer_child_compatibility.py --profile dev --output-dir target/b3b4-compatibility/dev-new
python scripts/test_renderer_child_compatibility.py --profile release --output-dir target/b3b4-compatibility/release-new
cargo fmt --all -- --check
git diff --check
```

See [compatibility evidence](isolated-native-renderer-child-evidence/verification.json)
for actual control outcomes, unchanged-source counts and archive digests. The raw
archive preserves generated wrappers, commands, compiler diagnostics, artifact
records and per-input hashes. Git-blob and Windows worktree SHA256 are distinct;
CRLF/LF checkout differences are reported explicitly. Metadata compile results
are not native framebuffer or process-memory measurements. No new child binary
exists, so no child binary hash or deployment-cap receipt is asserted.
The [normalized SHA256 inventory](isolated-native-renderer-child-sha256.txt)
covers the final assessment, audit script and archived evidence.

| Area | Verdict on this branch |
|---|---|
| B3B3 normal merge and exact reviewed second parent | PASS |
| Reproduction of direct native-module lint conflict | PASS in dev/release: standalone control exits 0; Workbench-policy control exits 1 with E0453 |
| Existing B2B2 own-resource/pixel/CPU and B3B1-B3B3 gates | Historical scope preserved for unchanged inputs; no isolated-renderer upgrade |
| New isolated native window, GL lifecycle, framebuffer and observation/document replacement | BLOCKED: integration stopped before implementation |
| New renderer heartbeat, GPU faults, process/IPC retirement and restart/no-replay qualification | BLOCKED: no renderer child |
| Actual child process/Job commit peaks, dev/release | Not measured |
| Candidate deployment cap C | BLOCKED/unselected: no true child measurements or justified headroom; fixture 32/128 MiB caps are inapplicable |
| Full final-source integration regression list | Not run for an unimplemented child; unchanged-source evidence is retained, with targeted compatibility checks only |
| >=100 launch/close/crash/restart cycles | BLOCKED/unrun; integration admission remains BLOCKED |
| Historical hello-only session | UNQUALIFIED |
| M17.4 Recorder overlap, terminal GPU lifecycle, whole-host Rust budget | BLOCKED, unchanged |
| Production GUI v2 activation | `ACTIVATION_QUALIFIED=false` |

Parent remains the sole presentation/Runtime-client/dispatcher/recovery owner.
No Runtime, OutputAuthority, Recorder, public API/DTO, Clojure, active v1 GUI or
cooperative scheduling source changed. Preview/B1/FiniteTraceItem limits and all
historical evidence remain intact. This branch is a **BLOCKED Draft PR** for
review of the compatibility stop, not READY FOR INDEPENDENT REVIEW of a completed
B3B4 implementation. It must not be merged as a completed renderer milestone.

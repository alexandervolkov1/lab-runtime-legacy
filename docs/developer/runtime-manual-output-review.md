# Runtime review and manual output work

Baseline: `2426fc0823e1d6a5e28425cc8ebc2e5f079953cb` (`origin/main`).
The initial clean checkout was `feature/control-1-reference-retune` at
`7e532202c5b2b405fc34a5132b262afe5243c557`; that branch and Draft PR #52 were
preserved separately. No previous PR was merged for this task. Runtime work uses
`feature/runtime-manual-output-api`. `ACTIVATION_QUALIFIED=false` remains unchanged.

## Review coverage

Read the production implementation throughout `crates/lab-core/src` and
`apps/lab-runtime/src`, including the transport executor, configuration lifecycle,
serial worker, Application/session delivery, managed executor, diagnostics, and
Recorder/SQLite modules. Read workspace/Core/Runtime Cargo manifests and the active
architecture, safety, Application API, recovery, SimpleDevice, extension, transport,
Recorder and project-status documentation. Tests are selected by the affected
contracts; this is not a claim to have individually reviewed every test, dependency,
platform implementation or historical report. Workbench, graphics and Windows Job
implementations are outside this review. Reading and passing tests do not establish
that the Runtime is free of defects.

## Module and execution map

| Responsibility | Implementation and actual path |
|---|---|
| Startup and configuration | Runtime `main`, `service`, `configuration`, `deployment`, `definition`, `simple_device`: parse CLI/TOML, freeze and validate artifacts/cross-references, compose `HostCore`, prepare resources and safe evidence, commit enabled Recorder provenance, advertise listeners. Staged changes reuse the existing owner and publication fences. |
| Domain ownership | Core `runtime`, `runtime/dispatch`: one mutable `Runtime`; commands change state and queries return bounded owned projections. Instrument descriptors, signals, References, controllers and outputs remain in this owner. |
| Acquisition | Host `scheduler` admits due reads without replaying missed ticks. Core `runtime/physical_io` and `transport` correlate bounded transactions, validate the codec result and commit signal observations/history. `metakon` and `simple_device` define protocol-specific evidence; `serial` isolates finite OS calls on the existing resource worker. |
| API | `server` TCP/WebSocket adapters decode the same `wire` contract; `application` checks advertised `protocol` operations and `sessions` admits/deduplicates exact typed mutations before dispatch. Outcomes are retained before delivery; a socket is not an experiment owner. |
| Control | Core `runtime/controllers`, `control`, `processing`, `reference` consume fresh committed inputs. `output/authority` owns leases, epochs and safe obligations. Physical output reserves a typed transaction and rechecks authority immediately before possible bytes; ACK/readback remain separate. |
| Virtual and managed sources | Core `plant`, `virtual_instrument`, `managed`, `runtime/managed_components`; Runtime `managed_executor`: native plant/generator or finite registered invocation/result paths. Only Runtime commits returned observations. Managed workers have no output authority. |
| Recording | Core `recording` emits semantic facts; Host `recording` transfers bounded groups; Recorder `worker` owns ingress/receipts and one SQLite worker. `sqlite` commits facts/projections/checkpoints atomically. Admission and durability are separate. |
| Failure, reconnect, shutdown | Core transport and authority fence stale/ambiguous work; `service/reconnect` retires the old adapter before a new generation and safe validation. `service/shutdown` drives finite safety and Recorder barriers. Client loss preserves admitted work; reconnect never restores ordinary output authority. Diagnostics remain lossy and non-authoritative. |

The dependency direction is useful: Core has no wire, serial OS, SQL or filesystem
dependency. Host orchestration and storage/transport ownership are real boundaries,
not redundant alternative Runtime owners.

## Concrete findings

| Code | Consequence | Disposition |
|---|---|---|
| Core `output/authority::complete_transport_readback` and `_exact` | Duplicate dispatch/finite-value checks and terminal bookkeeping could diverge between native and declarative protocols. | Share the existing exact-comparison completion path; preserve protocol-specific comparison and all evidence stages. |
| Core `simple_device::SimpleResponsePlan::validate` and `validate_response_evidence` | READ and ACK repeat match/checksum bounds and overlap validation. | Reuse the existing validator; scalar extraction, scale and extract overlap checks remain explicit. Both parser and Core validation remain necessary because trusted Rust callers can construct compiled plans directly. |
| Core `runtime/dispatch` discovery and `Runtime::activation_instrument_descriptors` | Two lists must independently enumerate every supported instrument family. | Build one sorted catalog and filter prepared identities for ordinary discovery. Activation still includes unpublished topology for provenance. |
| Runtime `host/configuration::enter_configuration_safe_barrier` | The physical-device test recognizes only Metakon. A SimpleDevice can enter simulated completion without transport evidence. | Requires a focused correction and regression before exposing manual output. |
| Runtime `host/configuration::apply_configuration` safe-profile replacement | The replacement path unconditionally uses simulated dispatch/completion, including physical bindings. | Requires the same physical/simulated evidence separation. Configuration success must not fabricate safe evidence. |
| Core `runtime/managed_components::capture_component_input` source identity | The fallback identity does not distinguish SimpleDevice rebinds or native model restarts as explicitly as the corresponding acquisition path. | Retain as a follow-up to verify with focused lineage tests; do not reconstruct managed components for manual output. |
| Runtime `service` and `simple_device` inline test modules | Large test bodies make the primary execution path harder to navigate. | Keep existing tests and module identities in this change. Moving tests is optional navigation work, not a safety or API requirement. |

Separate representations for deployment DTOs, validated Core plans, wire DTOs,
Recorder facts and SQLite rows have distinct validation/ownership/versioning duties.
They should not be merged into a universal model. Likewise, transport uncertainty,
safe obligations, finite leases, publication phases and durable receipts represent
observable failure boundaries; removing their states would remove guarantees.

## Existing control and required extension

At the baseline, `protocol.rs` registers 43 operations. `output` is a query;
`reference_retune` changes a Reference, and configuration properties do not set
heater power. The public direct manual-power scenario is therefore incomplete.
Core already has `OutputOwner::Manual`, finite leases/proposals, safe release,
physical WRITE/ACK/readback and immutable Recorder attempt correlation. Reuse those
mechanisms; add only typed Application operations and bounded completion correlation.
Do not expose arbitrary bytes/registers or turn API admission into device success.

An output operation must retain the result of its own attempt even if a later safe
transition changes the current output snapshot. Recorder facts cannot be used as the
operation's completion channel: recording may be disabled. Lease expiry must remain
effective after client loss, with no automatic renewal or rearm on reattachment.

No identified Arduino configuration or physical port was supplied. The checked-in
SimpleDevice writable example is a generic fixture with placeholder `COM256`, not
evidence identifying a connected Arduino. Physical acceptance must be reported
separately from virtual and byte-transport fixture results.

# Isolated v2 syntax, local and structural validation

This module implements the DTO vocabulary in the
[v2 contract](../../../../../docs/generic-controls-v2-contract.md), sections 2–4
and the applicable definition limits in section 8. It has no active consumer.
The running Workbench, its 16 operations and its v1 document remain unchanged.

`parse(&[u8]) -> ParsedDocument` first checks the 2 MiB input-body ceiling,
encoded/decoded string bounds, 16 container levels, 16,384 values/members and
duplicate keys. The streaming preflight builds no JSON value tree. Typed decoding
then requires closed object shapes (including rejecting positional arrays),
required fields, explicit nullable fields, closed tags, exact decimal strings and
bounded registry/nested arrays. The input API does no I/O; a future stream caller
must enforce the body ceiling before buffering the input.

`ParsedDocument::check_local() -> LocallyCheckedDocument` checks numeric ranges,
editor purpose/kind/constraint compatibility, enum choices, literal action kinds,
inert-button shape and aggregate panel/group definition counts. Local success
provides only immutable DTO access. It is **not full admission** and cannot publish
a document.

`LocallyCheckedDocument::check_structure() -> StructurallyCheckedDocument` consumes
the local stage and proves global ID uniqueness (including document ID), typed
registry references, acyclic composition with at most four Group levels, static
widget/editor/data/action compatibility, and expanded view budgets. All definitions,
including unused ones, undergo reference, type, cycle and depth checks. An unused
definition creates no visual occurrences. A used `property_scalar` editor requires
both IntegerInput and TextInput definitions with `matching_type`; an unused editor
is allowed. Only one `recording_label` editor definition is allowed.

After bounded ID indexing, graph analysis uses a leaf-first Kahn traversal with
one cached cost per definition and explicit incoming-edge multiplicity. Reachability
visits definitions once. Checked additions reject before exceeding 20,480 component
or 64 Plot occurrences; all roots in all windows/tabs/panels count. No recursive
traversal or expanded view tree is allocated. Temporary graph storage is bounded
by 512 definitions, 8,192 child edges and 4,096 panel roots. Unused definitions do
not consume the occurrence budget, but cannot conceal cycles or excess depth.

`occurrence()` resolves one address with at most four child positions. Its identity
contains document/window/tab/panel IDs, root position and the bounded child-position
path, including its length. Repeated siblings, roots and diamonds therefore have
distinct view identities while editor IDs stay shared. This is a document-local
static address; a future renderer must additionally scope it to the owner epoch.
No GUI state is created.

Structural errors reject the candidate: duplicate IDs, missing/wrong-registry
references, cycles/depth/instance limits, widget/candidate mismatches, editor-purpose
and action-family/source mismatches, or a non-SignalSample trace. A known selector
or action family with the wrong RuntimeRef category instead retains that exact
target with `Disabled(IncompatibleTarget)`. Different full editor/action targets
yield `Disabled(ParameterTargetMismatch)`, including differences in property owner
kind, owner ID or property key. Other bindings remain `NeedsRuntimeResolution`;
this does not assert existence, compatible metadata or action eligibility.
`expect_kind` remains a restricting assertion for future Runtime resolution.

Migration annotations are restrictions, not authority. `v1_kind` requires marked
control-group roots, one Plot root, or one Status(entity) root respectively.
Unsupported `v1_control` kind/target pairs impose an inert ceiling on descendant
button occurrences, including shared and nested groups. Supported origins also
restrict descendant actions to their original family and exact target. These
ceilings are occurrence-local and supplement the binding summary; inspecting an
action definition alone cannot establish applicability. `confirm:false` remains
provenance. Full converter/exporter normal-form and round-trip proofs (including
labels, helper sharing, ordering and defaults) belong to the future migration stage.

The checked stage has a private payload, no deserializer/public constructor and
no mutable DTO accessor. Static success grants no publication or execution rights.

Intentionally pending admission checks:

- Runtime metadata applicability and trusted adapter guards; no authority is
  established by locally valid literal values or presentation constraints.
- Canonical owner serialization and its 1 MiB bound, envelope round-trip headroom,
  golden bytes and the measured 4 MiB retained-allocation budget. Input/element
  ceilings bound decoding work but do not claim that measured admission budget.

There is no admitted-document constructor, owner conversion, serializer, migration,
v2 protocol/profile, renderer, observation lookup or execution path in this module.
Tests load the three complete section-11 examples directly from the contract and
exercise invalid forms at every nested object level. Fixtures with dangling IDs,
duplicate definition IDs and cycles remain only locally checked and are rejected
by the structural stage. All three complete examples also pass structural checks.

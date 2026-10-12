# Isolated v2 syntax and local validation

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
provides only immutable DTO access. It is **not full admission**, does not resolve
Runtime targets and cannot publish a document. Known target-category mismatches
remain syntactically representable for later disabled resolution.

Intentionally pending admission checks:

- Global definition-ID uniqueness and typed cross-registry reference resolution.
- Component cycles, graph depth, expanded component and Plot-instance budgets.
- Widget/editor/data/action compatibility across references, exact target equality,
  the shared Recorder editor rule and migration-origin restrictions.
- Runtime metadata applicability and trusted adapter guards; no authority is
  established by locally valid literal values or presentation constraints.
- Canonical owner serialization and its 1 MiB bound, envelope round-trip headroom,
  golden bytes and the measured 4 MiB retained-allocation budget. Input/element
  ceilings bound decoding work but do not claim that measured admission budget.

There is no admitted-document constructor, owner conversion, serializer, migration,
v2 protocol/profile, renderer, observation lookup or execution path in this module.
Tests load the three complete section-11 examples directly from the contract and
exercise invalid forms at every nested object level. Fixtures with dangling IDs,
duplicate definition IDs and cycles deliberately remain only locally checked.

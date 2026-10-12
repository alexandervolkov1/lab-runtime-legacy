# Isolated v2 validation, canonical bytes and wire capacity

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

## Canonical serialization (DGUI-4C5C1)

`StructurallyCheckedDocument::check_canonical_wire()` returns a private-constructed
`CanonicalWireCandidate` borrowing that immutable source. The bytes are derived
data, not another document owner or a `FullyAdmittedDocument`. A failed size,
encoding or envelope check returns no candidate and changes no source/summary.

The serializer visits typed fields directly, never the input JSON bytes or an
intermediate `Value` tree. Object fields follow the contract tables. Components
start `id,kind,label`, then present `help`, then their row fields; actions start
`id,family,target`, then their parameter. Optional fields follow required fields
in table order. Arrays and all identity strings retain their original order/bytes.
Pretty JSON uses two-space indentation, LF separators and no trailing newline.
UTF-8 remains literal except JSON-required escaping: quotes/backslash, short
escapes for backspace/form-feed/newline/CR/tab, and lowercase `\u00xx` for other
U+0000..U+001F controls. There is no Unicode normalization or slash/HTML escaping.

Canonical defaults are explicit: `settings:{"density":"comfortable"}`,
`applicability:"always"`, `constraints:{}`, and `expect_kind:null`. Fields whose
default is absence (`help`, `precision`, `v1_kind`, `v1_control`, constraint members,
`disabled_reason`) are omitted when absent. Required nullable fields stay present.
Only these presentation defaults normalize; targets, arrays, literal values,
disabled reasons and origin annotations are never rewritten.

Number encoding operates on the already-decoded finite binary64. Its reference
formatter is the existing locked `serde_json 1.0.151` / `zmij 1.0.23` pair in
[Cargo.lock](../../../../../Cargo.lock): shortest correctly rounded decimal
significand, fixed notation for decimal exponents -5 through 15 inclusive,
otherwise scientific notation with lowercase `e`, explicit exponent sign and no
exponent leading zeros. Integral fixed-notation values have `.0`. Signed zero
is preserved (`0.0` / `-0.0`); nonfinite values fail instead of becoming null.
The locked formatter defines shortest-significand selection and tie handling;
dependency updates must preserve these bytes or receive contract review.
This rule is locale-independent, round-trips to the same binary64 and does not
promise more precision than binary64. Integer/unsigned decimal types instead use
exact i64/u64 decimal JSON strings without any float intermediate.

Representative literal golden bytes (the tests contain exact byte expectations):

| Typed value | JSON bytes |
|---|---|
| Number +0 / -0 / integral 42 | `0.0` / `-0.0` / `42.0` |
| Number 10^-5 / 10^-6 | `0.00001` / `1e-6` |
| Number 10^15 / 10^16 / 10^30 | `1000000000000000.0` / `1e+16` / `1e+30` |
| Smallest positive subnormal / normal | `5e-324` / `2.2250738585072014e-308` |
| Maximum finite Number | `1.7976931348623157e+308` |
| Number 2^53-1 / 2^53 / next representable | `9007199254740991.0` / `9007199254740992.0` / `9007199254740994.0` |
| Integer 2^53+1 | `"9007199254740993"` |
| i64 MIN / MAX | `"-9223372036854775808"` / `"9223372036854775807"` |
| u64 MAX | `"18446744073709551615"` |

## Full wire headroom

Private serialization-only probes use the proposed v2 replacement-request and
presentation-result shapes in contract section 6. They do not implement an
endpoint, protocol negotiation or operation dispatch. Their document object is
encoded compactly from the same normalized DTO: inserting raw pretty bytes would
introduce physical LF inside the line-framed message. The existing endpoint
constants are imported read-only; no active v1 behavior or limits change.

Both probes use a legal worst-case 64-byte call_id consisting of NUL characters
(384 encoded bytes), a 32-character lowercase-hex workbench_id, and exact
`"18446744073709551615"` revision. This is capacity evidence, not permission to
increment a saturated revision. Every emitted body is scanned for depth,
values/members, key/string byte lengths and LF/CRLF physical-frame sizes.

| Checked resource | Bound / exact overhead |
|---|---|
| Canonical pretty document | 1,048,576 bytes, inclusive |
| Body / LF or CRLF physical frame | 2,097,152 / 2,097,153 bytes |
| Body fitting both LF and CRLF | At most 2,097,151 bytes |
| JSON containers / values-members / key or string | 16 levels / 16,384 / 512 decoded UTF-8 bytes |
| Replacement request | Compact document + 566 bytes, +18 values/members, +2 document depth |
| Presentation result | Compact document + 540 bytes, +14 values/members, +2 document depth |
| Effective canonical document value ceiling | 16,366 to fit both exchanges |

Thus the byte ceiling is comfortably below the wire ceiling (request body at
most 1,049,142 bytes), but a bare document can still fail envelope value headroom.
Request overhead dominates response overhead: a response-only production failure
after a successful request check is unreachable under these fixed shapes. Tests
exercise each probe independently at its exact value boundary and one over.
The current flat typed schema has maximum container depth 8 bare / 10 enclosed;
depth-16/17 boundary probes use explicitly scanner-only synthetic JSON fixtures,
not allegedly valid DTOs. Neither documents nor labels are truncated to fit.

## Buffer ownership and remaining admission

One check reserves a canonical `Vec<u8>` payload capacity of exactly 1 MiB and a
single reusable wire-body `Vec<u8>` payload capacity of exactly 2 MiB. Reservation
uses `try_reserve_exact`; writers reject before crossing their ceilings and never
grow these buffers. The wire buffer is cleared and reused for response after
request scanning, then dropped before returning. The successful candidate retains
only the canonical buffer (including its reserved capacity), source borrow and
small usage records. Failure drops both buffers. There is no retained input-byte
copy or document clone. These are bounded requested capacities, not a claim about
allocator bookkeeping/rounding or the final 4 MiB admission budget.

Additional temporary storage is one 64-byte call-ID string per probe, fixed-size
scalar-formatting scratch, and the existing streaming scan's bounded object-key
sets/escape scratch (at most 16 nested containers, 512-byte decoded strings with
at most 3,072 encoded bytes). It builds no JSON value tree. The DTO and structural
summary remain borrowed for the candidate's lifetime. Final retained allocation,
staging overlap, GUI-owned state and combined admission accounting belong to C2.

Intentionally pending admission checks:

- Runtime metadata applicability and trusted adapter guards; no authority is
  established by locally valid literal values or presentation constraints.
- Measured 4 MiB retained-allocation budget, staging/GUI allocation accounting and
  final combined admission constraints. Canonical/wire success is not admission.

There is no admitted-document constructor, owner conversion, migration,
v2 protocol/profile, renderer, observation lookup or execution path in this module.
Tests load the three complete section-11 examples directly from the contract and
exercise invalid forms at every nested object level. Fixtures with dangling IDs,
duplicate definition IDs and cycles remain only locally checked and are rejected
by the structural stage. All three complete examples also pass structural checks.

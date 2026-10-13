# Isolated v2 document admission

This module implements the DTO vocabulary in the
[v2 contract](../../../../../docs/generic-controls-v2-contract.md), sections 2–4
and the applicable definition limits in section 8, plus the pure section-7
v1 conversion and lossless export. It has no active consumer. The separate
[DGUI-4C7A read-only preview](../../gui/readonly_v2/README.md) borrows admitted
documents in isolation; its egui memory activation blockers remain explicit.
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
provenance. The normal-form converter, exporter and round-trip proofs are
described below.

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

## Buffer ownership

One check reserves a canonical `Vec<u8>` payload capacity of exactly 1 MiB and a
single reusable wire-body `Vec<u8>` payload capacity of exactly 2 MiB. Reservation
uses `try_reserve_exact`; writers reject before crossing their ceilings and never
grow these buffers. The wire buffer is cleared and reused for response after
request scanning, then dropped before returning. The successful candidate retains
only the canonical buffer (including its reserved capacity), source borrow and
small usage records. Failure drops both buffers. There is no retained input-byte
copy or document clone. Final admission charges the actual retained capacity and
the allocator allowance described below, including unused canonical capacity.

Additional temporary storage is one 64-byte call-ID string per probe, fixed-size
scalar-formatting scratch, and the existing streaming scan's bounded object-key
sets/escape scratch (at most 16 nested containers, 512-byte decoded strings with
at most 3,072 encoded bytes). It builds no JSON value tree. The DTO and structural
summary remain borrowed for the candidate's lifetime.

## Offline admission (DGUI-4C5C2)

```text
parse -> ParsedDocument -> LocallyCheckedDocument -> StructurallyCheckedDocument
  -> source/visual reservation checks -> CanonicalWireCandidate (borrowed proof)
  -> actual retained-capacity check -> AdmittedV2Document (one moved owner)
```

`admit(&[u8])` runs the complete chain. `check_admission(self)` on the private-payload
structural stage consumes that stage, performs all remaining checks, and moves
the source plus its canonical buffer into the result without cloning either.
The admitted type has no deserializer, unchecked constructor, mutable access or
conversion into the active v1 owner. A failure returns no partially admitted
result. The source/summary, array order, identities, disabled bindings and
occurrence-local v1 origin restrictions remain unchanged.

This is **offline document admission for a future bounded owner**, not publication,
Runtime applicability, or operational qualification of a renderer. There is no
dispatcher consumer, protocol/profile, automatic migration, observation lookup or
execution path. Actual metadata, readiness, Confirm and trusted adapters remain separate.

### Retained allocation accounting

`allocation.rs` exhaustively visits all DTO fields/variants and the structural
summary. Adding a field/variant requires updating its exhaustive accounting
pattern. Each String contributes its **capacity**, and each Vec contributes
`capacity * size_of(element)`, including unused elements' storage. Nested owned
allocations are visited separately; their headers already reside in the parent
Vec or root and are not counted twice. Numeric/enum values have no hidden heap
payload. There is no deduplication assumption for repeated equal strings.

For every nonempty heap block, the conservative charge is
`round_up(requested_capacity_bytes, 64) + 64`. The entire inline admitted root is
charged once using the same rule, as though boxed even when kept on the stack.
Empty allocations cost zero. All products, additions and rounding are checked.
Reports distinguish measured capacity bytes, live allocation count, and the
larger accounted total. The 64-byte rounding plus 64-byte per-block allowance is
an explicit accounting policy for unmeasured allocator bookkeeping/rounding,
**not a measurement or universal guarantee about process RSS or fragmentation**.
No claim covers a different allocator, page-heap verifier or library version
without repeating qualification. No dependency or workspace unsafe-code policy
was changed.

Each admitted active document and each admitted staged candidate must separately
fit **4,194,304 accounted bytes**, including its canonical capacity, DTO, summary
and proof records. Before serialization, admission checks that the source and
known canonical reservation fit; after serialization it checks actual capacity.
Active plus staged may therefore retain up to **8,388,608 accounted bytes**.
This is deliberately not a shared 4 MiB quota. Moving staged ownership performs
no new document allocation. These quotas exclude temporary validation scratch,
the caller's separately bounded input, and the separate GUI reservation.

| Lifetime | Ownership and bound |
|---|---|
| Input | Borrowed; caller must cap buffering at the existing 2 MiB body limit |
| Parse | Preflight key sets/escape scratch; then serde decoding and internally tagged-enum buffers, bounded by the existing 16-level, 16,384-value/member, string and input caps; no retained JSON tree |
| Local validation | Temporary enum-choice uniqueness sets; dropped on return |
| Structural validation | Borrowed-ID BTreeMap and graph Vecs; at most 512 definitions, 8,192 child edges, 4,096 roots; all temporary indices drop before the structural result |
| Structural result | One DTO and three availability Vecs, plus two counts; no expanded occurrence tree |
| Canonical/wire check | 1 MiB canonical capacity plus one reused 2 MiB wire capacity and bounded scanner scratch; wire capacity drops before returning |
| Admitted candidate | Source, structural proof, canonical capacity and small inline accounting/wire records; no input copy or JSON tree |
| Atomic replacement model | Old admitted owner remains alive during staging; after moving staged, dropping old releases exactly its retained allocations; no third document copy |

Temporary validation peaks are separate from the 4 MiB retained quota. Existing
input/count limits bound their algorithms; the measurements below are fixtures,
not a universal maximum peak for every legal or rejected input. Ordinary serde,
Vec and BTree allocations still use Rust's infallible allocation behavior (process
abort on exhaustion); this stage does not promise process-wide OOM recovery.
The two large serialization reserves are fallible and tested with injected real
allocation failures. Neither failure path can yield an admitted result.

Exact 4 MiB tests inject spare capacity into a private parsed-stage String while
preserving every DTO value, then run all remaining stages. A tiny canonical
document consequently passes exactly at 4 MiB and fails after one more capacity
byte. This is an allocator-capacity fixture, **not** a claimed wire-reachable
document. The budget comparison also tests exactly 4 MiB + 1 accounted byte;
the capacity fixture crosses a 64-byte accounting quantum. No test-only unchecked
constructor is exposed to consumers.

### GUI reservation and remaining renderer qualification

`VisualReservation` is an allocation-free model. No view, editor or plot is
created. On 64-bit Windows its worst simultaneous reservation is:

| Resource | Accounted bytes |
|---|---:|
| 20,480 small view states, 256 each | 5,242,880 |
| 64 Plot viewport states, 4,096 each | 262,144 |
| 65 drafts, 37,696 each | 2,450,240 |
| Arena/ledger headers and owner epoch reserve | 4,096 |
| **Total / remaining within 8 MiB** | **7,959,360 / 429,248** |

Each draft reserves 17 text blocks (current plus **16 total undo/redo snapshots**),
each with capacity 2,052 and a 2,176-byte block charge; a 448-byte charged Vec of
16 String headers; and 256 accounted bytes for inline headers, cursor/selection,
focus, ring indices and interned identity. This intentionally uses the maximum
input bound even for Reference (260) and Recorder (516). Each editor definition
gets one reservation, plus the shared Recorder slot; repeated views add no draft
history. Recorder aliasing can consume less but cannot enlarge the reservation.
The view/viewport charges already include their allocation overhead. They require
bounded arenas/indices, not copied arbitrary-length identity strings or unbounded
per-view containers. The simultaneous-maxima graph test has only four component
definitions, 20,480 occurrences, 64 plots and 64 editor bindings.

**Mandatory future renderer guard:** debit actual capacities and all library-owned
focus/history before retaining them; enforce the individual reserves, 65 slots,
16 combined history entries and total 8 MiB; never silently truncate a candidate
into validity. Overflow disables the affected interaction with a bounded reason.
Retire old occurrence state, focus and history on document retirement. Staging
reserves a number, not another GUI-state copy; a future replacement must retire
old state before allocating the new epoch's state. The shared Recorder slot must
be counted once. Global fonts/GPU caches and existing observation/sample buffers
remain outside this presentation-owned quota.

The reservation fits without a contract-limit change. Actual egui undo/focus
allocations, arena implementation, post-publication failures and replacement
retirement behavior are **not qualified** here. Those are blockers to connecting
a renderer/owner, not evidence of a working v2 GUI.

### Native allocation experiment

Run [test-v2-allocation.ps1](../../../../../scripts/test-v2-allocation.ps1).
It obtains the exact serde/serde_json rlibs from `cargo test --locked --no-run`,
then compiles the **same v2 source files** into an isolated single-thread native
MSVC executable. Wire constants are extracted verbatim from their existing owners.
The probe also imports the unchanged v1 document/reference source files for the
offline conversion experiment; it never imports persistence or a running owner.
There is no second dependency manifest/lockfile or altered production allocator.
The tiny test-only unsafe GlobalAlloc shim in
[the probe](../../../../../scripts/support/v2-allocation-probe.rs) only delegates
to `System`, counts allocation requests and injects the two fallible failures.
It is not compiled into any workspace binary; workspace `unsafe_code = forbid`
remains intact. The probe itself denies unsafe outside that shim.

Fixture creation, input allocation, warm-up and reporting occur outside each
measurement. No GUI, runtime worker, test harness or background thread runs in
the process. Each fixture is measured twice and must give identical per-phase
live/peak/block counts; all tracked allocations must return to zero after drop.
Retained allocator request bytes and block count must **exactly equal** the DTO/
summary/canonical capacity walker, not merely fall below the conservative charge.
The probe also reports a conservative realloc overlap estimate (old + new), since
System's in-place behavior is opaque. It measures requested heap payload, not
allocator metadata, reserved pages, stack, input or operating-system allocations.

Native x86_64 Windows/MSVC, Rust 1.95.0, locked dependencies; bytes observed:

| Fixture | Retained requests | Accounted retained | Structural phase peak | Canonical/wire peak | Active + replacement peak |
|---|---:|---:|---:|---:|---:|
| Contract example 1 | 1,050,851 | 1,054,976 | 2,990 | 3,149,958 | 4,200,809 |
| Contract example 2 | 1,052,058 | 1,057,280 | 4,311 | 3,151,165 | 4,203,223 |
| Contract example 3 | 1,053,167 | 1,059,712 | 5,867 | 3,152,371 | 4,205,538 |
| 1,152 data definitions | 1,234,605 | 1,378,240 | 237,893 | 3,333,434 | 4,568,039 |
| 128 groups / 8,192 edges / 32 traces | 1,392,606 | 2,458,752 | 493,863 | 3,491,810 | 4,884,416 |
| 20,480 occurrences / 64 plots / 65 slots | 1,189,495 | 1,771,264 | 183,083 | 3,288,699 | 4,478,194 |
| Exactly 1 MiB canonical document | 2,052,617 | 2,207,552 | 1,036,097 | 4,151,465 | 6,204,082 |

For the 1 MiB fixture, parse retained 1,003,341 bytes and peaked at 1,003,594;
structural retained 1,004,041; active plus completed staged retained 4,105,234;
retiring old returned to 2,052,617. The borrowed input has length 993,223 and
measured Vec capacity 1,255,424 bytes, so the observed replacement payload peak
**including that one caller input capacity** is 7,459,506 bytes. No GUI memory is
allocated by this experiment. Realloc overlap
exceeds ordinary peak in some parse/graph phases (e.g. 274,351 vs 184,877 for
1,152 definitions' parse), but not the reported maximum replacement peaks.

All three contract examples, maximum individual collections, repeated long-label
definitions/edges, worst visual reservation, previous numeric golden bytes and
v1/graph/API guards remain covered. The independent ceilings intersect: this is
not a promise that every registry maximum can occur simultaneously within JSON,
wire, canonical and allocation limits.

## Deterministic v1 conversion (DGUI-4C6A)

`ValidatedV1Document::check(&source)` borrows a source only after the unchanged
v1 `validate()` succeeds and the existing pretty serializer fits 1,048,576 bytes.
A checked counting writer performs the size check without retaining a serialized
v1 copy. It rejects at the byte ceiling. The proof holds an immutable borrow; it
has no deserializer, mutable accessor or unchecked constructor.

`convert_v1(proof)` implements contract section 7.1 and returns only an
`AdmittedV2Document` or an error. `InvalidV1` identifies invalid structure/version,
strings/numbers/references or source canonical size. A valid source whose expanded
form cannot satisfy **any** v2 bound fails with `conversion_limit`. Nothing is
truncated, renamed, dropped, published or saved.

### Normal form and identities

Every original document, window, tab, panel, control, plot and trace ID is reserved
before allocation. Source IDs are borrowed in a `BTreeSet<Cow<str>>`; generated IDs
are owned there temporarily. Four monotonic checked counters implement the first
free `v2:data:n`, `v2:editor:n`, `v2:action:n`, `v2:component:n`, starting at zero
and checking the common namespace. Occupied lower indices are never removed, so
counter advancement chooses the first free index, including gaps. There is no
hash iteration, UUID, time, observation or hardware lookup. Runtime identity
strings are preserved targets, not presentation definitions in this namespace.

Walk order is controls, plots/traces, windows/tabs/panels in original array order.
Within each control allocate data, editor, action, then children. An absent
definition consumes no ID. Append the original-ID group before its children.
At the first supported Recorder control allocate the shared data/editor, even
if that definition is unused; subsequent Recorder controls reuse only those two
definitions. Each action remains independent. Trace bindings are always distinct.

| Source control pair | Group children in order | Binding definitions |
|---|---|---|
| RecordingLifecycle / Recorder | Status `Recorder state`, TextInput `Recording label`, Button `Action` | RecorderState; shared recording_label/Text; RecorderLifecycle using that editor |
| SetReference / Reference | Indicator `Current`, NumberInput `New value`, Button `Action` | ReferenceValue; reference_fixed/Number; ReferenceFixed |
| ControllerLifecycle / Controller | Status `Controller state`, Button `Action` | ControllerState; no editor; ControllerLifecycle |
| ApplyConfiguration / ConfigurationProperty | Indicator `Current`, IntegerInput `Integer candidate`, TextInput `Text candidate`, Button `Action` | PropertyCurrent; one property_value/property_scalar; both inputs matching_type; PropertyConfigure |
| ReconnectResource or incompatible pair | Status(entity) `Target`, Button `Unavailable` with null action and reason `Unsupported v1 control kind/target` | No data, editor or action definition |

All groups have column layout and preserve the control's ID, label and complete
`v1_control {kind,target,confirm}`. Generated help/expect_kind/constraints are
absent at construction and undergo the existing canonical default normalization.
The action has no confirm override: false survives only in the origin annotation.
The converter never selects Start/Pause/Resume/Stop from a state snapshot.

Controls panels preserve ordered roots, including repetition and empty arrays;
Plot/Status panels each receive one generated leaf, their `v1_kind`, and original
panel identity/title. Plot leaf labels use the referenced plot title; Status
leaf labels use the panel title. Windows/tabs and unused definitions retain their
order. Plots preserve time window, axes, trace style, visibility and display_unit.
Every trace has its own SignalSample binding with its exact full RuntimeRef,
including category-incompatible references that remain statically disabled.

### Validation and memory

The converter builds the **closed typed DTO directly**. Required fields and tags
come from compiled variants; source string/numeric forms were already checked;
fixed labels/IDs are bounded; every List insertion checks its type's count limit.
Thus the syntax invariant of `ParsedDocument` is established by construction,
not by an unchecked caller-provided DTO or raw JSON entry. It then runs the
existing local, reference/graph, static/origin, canonical/wire, retained allocation
and visual reservation stages. Both full wire probes run the existing bounded
JSON scanner, including duplicate keys, depth, string and value/member limits.
No serializer/parser round-trip is used to reconstruct f64: the exact source bits,
including signed zero, subnormals and maximum finite axes, survive in the DTO.

There is no intermediate `serde_json::Value`, full serialized v1 buffer or second
decoded v2 document. One bounded builder owns the output; the ID index and shared
Recorder helper IDs drop before final admission. At most four children await
insertion per control. Checked List insertion uses fallible reservation; ordinary
String/BTree allocations retain the existing process-OOM behavior.

The v1 structural ceilings allow at most 5,345 original definition IDs and 1,536
generated IDs (6,881 index entries), with at most 384 component definitions, 1,088
data bindings, 64 editors/actions, 32 plots and 1,024 traces. These are conservative
construction ceilings, not promises that they fit all intersecting wire/canonical
budgets. Repeated views are never expanded during construction. The 20,480-instance
fixture constructs only five component definitions. The existing full admission
still enforces 4 MiB retained and the separate visual reservation.

The same single-thread native probe measures conversion twice per fixture:

| v1 fixture (canonical bytes) | Source validation peak | Conversion retained requests | Accounted retained | Additional conversion peak |
|---|---:|---:|---:|---:|
| Empty (101) | 285 | 1,048,581 | 1,049,280 | 3,147,160 |
| Six controls / mixed panels (3,992) | 1,575 | 1,060,769 | 1,082,432 | 3,160,192 |
| 20,480 expanded instances (144,088) | 6,861 | 1,209,572 | 1,746,048 | 3,308,995 |
| 1,024 traces, long labels (896,571) | 58,545 | 0 (conversion_limit) | — | 3,954,958 |

These are **additional requested heap bytes above the existing borrowed v1
owner**, not total process memory, allocator overhead or a universal peak bound.
Source/fixture construction and reporting are outside the measurement window.
Validation retains no allocations; successful output equals the existing capacity
walker exactly; failure/drop returns to zero. Converter indices are never retained.
Tests also admit v1 at exactly 1 MiB while rejecting its amplified v2 result;
v1 at 1 MiB + 1 fails source validation instead. Large valid v1 does not imply
that v2 can fit its wire/canonical limits.

### Conversion safety evidence

Tests cover all five kinds crossed with all target categories, including all
three property-owner kinds, independent owners and repeated equal targets.
Inert templates have no action; shared/nested edited groups retain their inherited
origin ceiling. A pure occurrence predicate verifies that only referenced,
structurally supported actions contribute to ordinary-button suppression,
including hidden tabs; unused/inert definitions do not. This specifies the future
consumer's behavior without modifying the existing GUI suppression implementation.

A deterministic model checks `eligible_v2 => eligible_v1` over 18,432 combinations
of freshness, eight blocker flags, property metadata/access and lifecycle
availability, with twelve candidate fixtures for all six example controls.
Positive witnesses prevent a vacuous always-disabled result. This proves the
modeled structural/guard floor only: it is **not** an end-to-end proof of actual
Runtime readiness, constraints, recovery or the future v2 operator adapter.
The existing separate Confirm floor and production GUI regressions remain intact.

The offline exporter below completes the two inverse checks. No hidden v1 payload
is kept to fake those identities. No public protocol/profile, live migration, new
widgets, metadata refresh, physical control or persistence is introduced here.

## Lossless v1 export (DGUI-4C6B)

`export_v1(&AdmittedV2Document)` is the sole export entry. It borrows immutable
admitted syntax and returns either a complete `PresentationDocument`,
`unrepresentable_v1` with a bounded first offending schema/registry path, or
`conversion_limit`. No partial document escapes any failure. It neither reads
observations nor has access to the running presentation owner, persistence,
Dispatcher, ClientHandle or operator workflow. The v1 schema/API and the approved
converter are unchanged.

The exportable subset is exactly the image of the approved converter C, modulo
the existing v2 canonical normalization of object field order and specified
optional defaults, intersected with the validation/allocation limits. It is not
all valid admitted v2. Merely having `v1_control`/`v1_kind` is insufficient.

The candidate inverse reconstructs all groups in component-registry order as v1
Controls, using their ID/label and complete origin kind/target/confirm. Windows,
tabs, panel identities/titles, ordered repeated Controls roots and single
Plot/Status leaves restore the arrangement. Every plot and trace is restored in
registry order, with the complete trace binding target inlined as its v1 source.
All display numbers are direct binary64 copies; no numeric JSON parsing occurs.
Unused Controls/Plots, empty arrays, all RuntimeRef/property-owner variants,
display_unit null, visibility and confirm false retain their original meaning.

The candidate passes the unchanged v1 format/structural validator and the same
canonical pretty-serialization size check via `ValidatedV1Document::check`'s
counting writer. Count/size/allocation failures become `conversion_limit`; any
other invalid inverse becomes `unrepresentable_v1`. A private final-fence test
deliberately supplies invalid reconstructed v1; there is no unchecked export API.

### Two round-trip proofs

1. **E(C(v1)) = v1.** C copies every v1 field into the above definitions and
   annotations without reordering its original arrays. Reconstruction reads
   precisely those copies, including full targets and provenance. Generated
   helpers are the only fields excluded from reconstruction, and the second
   proof verifies every one of them. Tests compare the entire serialized v1
   byte sequence as well as DTO equality and explicit float `to_bits()` values;
   floating-point `PartialEq` alone would miss signed-zero loss.
2. **C(E(v2)) = normalize(v2).** Before returning any candidate, export invokes
   the unchanged C and requires equality of its complete admitted canonical bytes
   with the source's complete admitted canonical bytes. No hash, approximate DTO
   equality, projection or subset comparison grants success. Admission has
   already applied the permitted field-order/default normalization. Neither
   side drops unknown fields, definitions or arrays during this comparison.

This recognizes the exact child composition/order/labels, data/editor/action
families and targets, defaults, generated-ID allocation and collision handling,
shared Recorder data/editor, independent per-control bindings and independent
trace bindings using one authoritative template implementation. An extra unused
v2 definition cannot vanish: C must reproduce it byte-for-byte. Canonical
comparison also retains the inert origin ceiling and separate Confirm policy;
no new Boolean/Number/Enum property mutation or Reconnect action is introduced.

Edits with a complete v1 inverse are permitted: for example a Control label,
confirm annotation, trace visibility or the full target of its own trace data
definition. Retargeting the trace's **binding ID**, merging bindings or leaving
obsolete definitions is not that edit and fails exact reconstruction.

### Rejection and limit evidence

Deterministic tests require full admission before testing export refusal for
Toggle/Select, multi-source groups, literal action parameters, custom constraints,
nondefault settings, missing origin/panel annotations, help/precision/label/layout/
applicability edits, child and registry reordering, noncanonical helper IDs, extra
unused component/data/editor/action definitions, shared independent control
bindings, coupled/swapped trace bindings, separate Recorder input/data topology,
an incomplete PropertyScalar group, changed action family and inert-button action
injection. Duplicate recording_label editors or malformed matching_type pairs
are already refused by admission; they cannot be export inputs.

Positive fixtures cover all five kinds crossed with all ten RuntimeRef/owner
variants and both confirm values, every PanelKind, multiple windows/tabs, unused
definitions, repeated references, shared Recorder, two traces on one source,
collisions in every source namespace, exact UTF-8/escaping and array order.
Numeric cases include signed zero, positive/negative minimum subnormal, minimum
normal, both finite extrema, the 2^53 boundary and time-window/width endpoints.

Limit tests include 65 annotated Controls, a compact admitted document with 480
shared long-source traces whose v1 serialization exceeds 1 MiB, and 600 shared
traces whose independent normal-form expansion fails repeat-C wire admission.
Every failed export leaves source bytes, summary and allocation accounting intact.
The 4 MiB reconstruction quota is checked at its exact boundary, the next block
and checked-multiplication overflow. Prior DTO/graph/canonical/admission/conversion
regressions, v1 presentation/GUI, external 16-operation API, Dispatcher and
architectural guards remain part of the targeted gates.

### Memory ownership and native qualification

Reconstruction owns one candidate with at most **4 MiB accounted allocations**.
Every String/Vec uses fallible exact reservation, charges before allocation and
then charges actual capacity with the existing rounding/allocator allowance.
Arrays reserve their known final length once; no incremental growth or expanded
view tree is needed. Count bounds come from admission, with the additional v1
64-Control fence. All capacity multiplication/addition is checked. Reference
lookups are bounded linear scans over at most 512 components / 1,152 data bindings.

The borrowed admitted source (at most 4 MiB), candidate (at most 4 MiB), and one
repeat-C admitted output (at most 4 MiB including its 1 MiB canonical buffer) can
coexist. This **12 MiB retained accounting sum is not a process peak limit**:
repeat C also uses its existing finite construction/index/validation scratch and
one transient 2 MiB wire buffer. Its pre-admission builder retains the documented
construction ceilings above. The exporter builds no JSON Value tree, v1 byte
buffer, cloned source document or second hidden original. Repeat-C and its
scratch are dropped before return; only the reconstructed v1 remains.

The native probe measures additional requested heap bytes above the already
borrowed admitted v2, including reconstruction, v1 validation and complete repeat
conversion. It repeats each fixture twice, requires identical readings and zero
remaining allocations after output drop or failure. Three injected failures
(first reconstructed String, 1 MiB canonical reserve, 2 MiB wire reserve) return
`conversion_limit` and leave the source unchanged. Inherited String/BTree
allocations inside v1 validation/C retain their existing process-OOM behavior;
this is bounded allocation evidence, not universal graceful process-OOM handling.

Native x86_64 Windows/MSVC, Rust 1.95.0, locked dependencies; requested bytes:

| Export fixture | Borrowed v2 retained | Additional v1 retained | Additional peak |
|---|---:|---:|---:|
| Empty | 1,048,581 | 5 | 3,147,165 |
| Mixed controls/panels | 1,060,769 | 2,482 | 3,162,674 |
| 20,480 expanded occurrences | 1,209,572 | 143,879 | 3,452,874 |
| 320 traces, long labels/colors/full sources | 1,805,720 | 712,806 | 4,617,730 |
| v1 serialized-size refusal | 1,615,334 | 0 | 1,096,258 |
| Repeat-C amplification refusal | 1,144,916 | 0 | 3,458,434 |
| Edited settings refusal | 1,060,658 | 0 | 3,162,674 |

These are fixture observations, not universal maxima or RSS/allocator-overhead
measurements. Export remains an offline library operation; no endpoint, profile,
renderer, autosave, metadata refresh or Runtime execution is enabled. DGUI-4C6B
requires independent review before merge or any subsequent stage.

Native targeted gates for this implementation passed: `presentation::v2::`
(77 tests, including 13 export tests), `presentation::tests::` (47 v1/GUI tests),
`external::tests::` (39), `dispatcher::tests::` (19), `operator_boundary_tests::`
(5), the native allocation probe, `cargo fmt --all -- --check`, standalone probe
rustfmt, `cargo clippy -p lab-workbench --all-targets --locked -- -D warnings`, and
`git diff --check`. No full workspace gate was needed for this offline-only change.

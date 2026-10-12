# Isolated v2 document admission

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
dispatcher consumer, protocol/profile, migration, observation lookup or execution
path. Actual metadata, readiness, Confirm and trusted adapters remain separate.

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

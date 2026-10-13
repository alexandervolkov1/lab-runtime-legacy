# Isolated read-only v2 preview (DGUI-4C7A)

`show(Ui, &AdmittedV2Document, &RuntimeObservations, PresentationEpoch)` is an
internal, isolated renderer. It has no active application/Dispatcher consumer,
protocol capability, presentation ownership, persistence or Runtime call path.
The existing v1 GUI and all 16 external operations continue to use their current
owner. This module is not activation-qualified (`ACTIVATION_QUALIFIED = false`).

## Rendering and identity

The preview visits logical windows, tabs, panels and roots in document order.
All tabs are shown; navigation/viewport policy belongs to a future caller.
Group supports row/column layout, ordered repeated references and four nested
Group levels. Each visit resolves the admitted registry reference and its
occurrence-local availability through `source().occurrence()`. The traversal
keeps four child-position slots and bounded recursion, not a materialized tree,
document copy or per-view model. Checked admission proves at most 20,480 visits.

Semantic identity combines the borrowed document/window/tab/panel IDs, root and
child positions with an explicit owner identity and presentation revision.
An owner must advance revision on every replacement, including a replacement
with the same document ID. Repeated views have different identities while their
DataBinding definition remains shared. The egui hash is a widget address, not a
replacement for the semantic identity. Numeric salts avoid recursively embedding
egui's debug ID representation in descendant salts.

Indicator and Status(data) use the existing DGUI-4C3 typed read layer, with a
closed selector mapping and full RuntimeRef, including ConfigurationProperty
owner kind, identity and property. `expect_kind` is an additional restriction:
it cannot invent metadata, parse an Enum as Number, convert Integer to Number,
or turn missing data into a scalar. Static Disabled remains Disabled; static
NeedsRuntimeResolution still requires a compatible cached observation.

Boolean, exact i64, Number, Text and Enum are distinct. False, zero and empty
Text are displayed explicitly; missing values retain their absence reason.
Numbers use the round-trippable Rust display spelling, including signed zero,
and precision adds only a secondary rounded Number caption. Runtime units are
shown separately without conversion. Quality, metadata/generation failures and
freshness remain explicit. Global Rebuilding overrides a cached freshness label;
a missing entity is Unresolved even when the connection is Fresh.

Status(entity) uses those same typed meanings where available. Instrument,
Resource and Component retain v1's bounded cached JSON status meaning, with no
new field selectors. JSON is streamed into a 1,024-byte UTF-8 prefix, marked with
an ellipsis on truncation; there is no full serialized observation copy. Scalar
strings above 512 UTF-8 bytes are explicitly unavailable before typed decoding
can clone them. This display refusal never mutates the cached value.

Button, Toggle, Select, all Input components and Plot show explicit inert,
unsupported placeholders. Labels are nonselectable and no input/click response
is consumed. No drafts, undo, sample buffers, Confirm interaction, intent,
observation refresh or action submission exists. Inherited v1 inert/family/target
ceilings are read from each occurrence, including reused nested descendants.

## Memory evidence and activation blocker

The renderer retains **zero owned heap state between frames** and makes one
bounded typed read at a time. This is not a proof that egui retains zero state.
The offline `VisualReservation` also cannot prove egui's actual allocations.

`scripts/test-v2-renderer-allocation.ps1` links the production renderer and exact
locked Cargo artifacts into a separate native System-allocator probe. The
fixture has three component definitions and 20,480 occurrences; its reservation
is **5,284,672 accounted bytes**. Source/observation owners and fixture creation
are outside the measurement window. No expanded test-side text/shape list is
constructed. Outputs are dropped after every frame; IdTypeMap stays empty.

Native Windows x86_64 MSVC / Rust 1.95.0 / locked egui 0.36.2, requested heap bytes:

| Context phase (debug ID tables already warmed) | Live | Phase peak |
|---|---:|---:|
| Empty preview baseline | 325,653 | 593,821 |
| First maximum-graph frame | 33,386,868 | 45,054,960 |
| Three more frames, same epoch | 66,154,224 | 77,822,316 |
| New epoch | 66,154,224 | 66,310,100 |
| Three empty frames after retirement | 65,889,685 | 66,156,032 |
| Context drop | 0 | 65,889,685 |

These Context-only readings repeat identically in the probe. They include egui
frame buffers, retained capacities and font state; they are neither RSS nor an
allocator-overhead estimate or a universal maximum. Their scale already prevents
claiming compliance with the **8 MiB presentation-owned visual state** ceiling.
Retiring a document does not release these Context capacities.

A separate fresh-process mode includes previously cold debug ID tables. After
the first epoch settles it retains 149,015,916 requested bytes; a second epoch
raises this to 231,872,525. After Context drop, **165,719,485 bytes remain** in
egui's process-global debug provenance tables. In the pinned egui sources,
`id.rs::id_source` and `id_salt.rs::id_salt_source` insert into static SOURCE_MAPs
without an eviction path. Distinct future epochs therefore have no finite
lifetime bound in this build. Empty IdTypeMap and zero renderer drafts do not
qualify those tables. The probe exposes this retention instead of misreporting
it as released Context memory.

Activation stays blocked. A future owner needs a measured runtime limit on
submitted visible widgets/paint work and a lifecycle for retiring Context
capacities, all charged to the combined visual budget. It also needs an exact
build configuration with bounded or disabled debug ID provenance, and allocation
qualification for that configuration. Merely reducing the traversal stack,
clearing IdTypeMap, changing epoch or dropping Context cannot satisfy both
problems. No dependency/profile change or partial live-render fallback is part
of this foundation; the isolated preview still visits the whole admitted graph.

## Regression evidence

Twelve deterministic headless egui tests exercise actual painted text/layout,
all five scalar types and selectors, i64 endpoints, Number extremes/signed zero,
all three property owners, absence/quality/metadata/generation/expect_kind,
freshness/rebuild, static blockers, repeated references, multiple arrangements,
four Group levels, epoch isolation and every inert placeholder. The maximum
fixture renders twice without renderer state or a materialized expansion.
Boundary tests verify immutable document/observations/owner/recovery/workflow,
zero client query/mutation/status calls and no active application consumer.

Targeted gates also cover the 77 v2 DTO/graph/canonical/admission/conversion/export
tests, 47 v1 presentation/GUI/Plot/hover/Confirm tests, seven GUI control tests,
12 GUI application tests, 39 external tests, 19 Dispatcher tests, five operator
architecture guards and ten typed-read tests.
The existing native admission/conversion/export probe uses the same extracted
test-only allocation meter. Formatting, warning-denied scoped Clippy and diff
checks remain required. No full workspace gate or release is needed here.

Plot v2, actions, editors, Toggle/Select interaction, protocol/profile and
activation require separately reviewed work. DGUI-4C7B is not started here.

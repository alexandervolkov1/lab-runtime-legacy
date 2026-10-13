# Isolated bounded read-only v2 preview

`Preview` borrows an `AdmittedV2Document`, accepts immutable
`RuntimeObservations` and an explicit presentation owner/revision, and owns one
private egui Context. It has no active application/Dispatcher consumer,
protocol capability, Runtime callback, persistence or laboratory action path.
The v1 Workbench and its 16 external operations retain their existing owner.
`ACTIVATION_QUALIFIED` remains **false**.

DGUI-4C7A2 replaces the original all-occurrences `show(Ui, ...)` traversal with
hierarchical paging and an explicit Context lifetime. See
[bounded rendering and allocation qualification](BOUNDED_RENDERING.md) for the
limits, accounting policy, evidence and remaining host restrictions.
[ID_MEMORY.md](ID_MEMORY.md) retains the DGUI-4C7A1 provenance investigation and
package-specific Cargo profile fix. Neither Cargo profiles nor Cargo.lock change
in DGUI-4C7A2.

## Navigation and identity

Only one window/tab/panel and one page of four immediate components is submitted.
Previous/next selectors follow document order. Each Group has an explicit Open
button; Parent returns to the page containing that particular occurrence.
Four group-position slots suffice, including diamond/shared subtrees. A scroll
area exposes the whole current page, including long text. No expanded graph,
subtree-count index, per-occurrence GUI model or navigation history is retained.

Row/column applies to the immediate children of the selected Group on the
current page. Nested Groups are previews with explicit entry, not simultaneously
expanded compositions. This is a hierarchical preview policy, **not full v2
layout compatibility**. Every admitted occurrence remains reachable.

Occurrence identity still combines document/window/tab/panel IDs, root/child
positions, owner and revision. Returning to a page retains its semantic IDs but
creates a new Context, so it cannot revive stale focus, scroll or draft state.
Replacement, including a different admission with identical document IDs, resets
navigation. Changed rendered text retires Context caches but preserves the two
local scroll coordinates on the same page. Navigation activates on primary press
inside the current clipped button rectangle, so observation updates cannot
starve navigation between press and release. It creates no laboratory intent.

## Read-only meaning

Indicator and Status(data) use the existing DGUI-4C3 typed read layer and full
RuntimeRef, including ConfigurationProperty owner kind, identity and property.
`expect_kind` only restricts actual metadata. Exact i64 and Number values,
negative zero, false, empty Text, Enum without choices, units without conversion,
quality, absence and generation fencing retain their existing meaning.
Precision supplies a secondary rounded Number caption; it never replaces the
exact value. Rebuilding overrides stale observation presentation.

Status(entity) uses the existing typed meaning where available. Instrument,
Resource and Component show a bounded cached projection, without new selectors
or Runtime queries. Runtime scalar text over 512 UTF-8 bytes is explicitly
unavailable before a typed-read clone; entity text remains an explicitly marked
1024-byte prefix. This is read-only display, never a candidate value.

Button, Toggle, Select, IntegerInput, NumberInput, TextInput and Plot remain
explicit inert placeholders. All availability is resolved through admitted
occurrence paths, including inherited v1 origin ceilings. There are no drafts,
Confirm implementation, sample buffers, metadata refresh or action execution.
The fixed previous paint page is compared with freshly resolved text each frame;
it is not an independently usable observation cache.

## Verification

Headless tests inspect actual egui output and geometry, including all 20,480
occurrences, real navigation buttons, repeated/diamond paths, depth four,
64 windows and the 64th tab, long scrolled text, owner/revision/admission replacement,
scalar/freshness/generation semantics and inert controls. Native probes link the
real executable's locked dependencies in both corrected dev and release profiles.
The admission/conversion/export allocation probe is preserved; its shared meter
additionally reports peak block count for conservative per-block accounting.

The previous 66,153,808-byte retained Context was caused by submitting all
20,480 occurrences. The bounded preview removes that submission path. Full
activation still requires a reviewed host lifecycle/GPU integration, Plot v2,
bounded editors/drafts, actions, operator Confirm and the external v2 protocol/
profile. DGUI-4C7B is not implemented here.

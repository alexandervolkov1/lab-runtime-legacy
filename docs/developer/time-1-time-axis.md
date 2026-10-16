# TIME-1: measurement time in the stock Workbench

Experimental, read-only Windows/MSVC scope. `ACTIVATION_QUALIFIED=false`;
MP-1 remains OPEN. No release, hardware acceptance, GUI Controls or M-CLEAN.

## Contract and ownership

PR #49 was independently reviewed at
`d51792f216091037281a6b9a96a1608e241aa8b7`. Remote main was
`fcf340c74efac9e422635d5b736c606b8f4a674d`; all 66 canonical Git-blob
checksums matched and the post-qualification diff contained only documentation
and evidence. Normal merge commit `5c0730c8f827de9b326cabaf4176ae21d2e3308b`
has those two parents and exactly the reviewed tree. Main was synchronized with
a clean working tree before this branch. PR #43 remains historical Draft/BLOCKED.

`SystemClock` has one `Instant` origin. Measurements expose `observed_at_ns`
relative to that origin. Previously `recorder::TimeAnchor::capture` bracketed
an OS wall-clock read using copies of that clock only when recording started.
Recorder's boot, periodic and interval anchors retain their existing behavior
and schema. They are not queried by the GUI.

`SystemClock::new` now captures one additional display anchor immediately after
creating its origin, using the existing checked `TimeAnchor` implementation.
Every copy of that clock retains the same sample. Capture failure makes the
display anchor unavailable and does not fail Runtime startup. No timers,
leases, deadlines, acquisition times, output decisions or Recorder fields use
the display mapping. Later wall-clock corrections do not modify it.

The existing `hello.result` optionally contains `time_anchor` (or null):

```json
{"boot_id":"<same boot as hello>","source":"system_clock",
 "monotonic_before_ns":"1000","monotonic_after_ns":"2000",
 "utc_unix_us":"1735689599000000","bracket_uncertainty_ns":"1000",
 "utc_accuracy_ns":null}
```

All times are decimal strings. UTC is signed Unix microseconds. Monotonic
endpoints and bracket uncertainty are unsigned nanoseconds. Mapping follows
the existing Recorder convention:

```text
utc_estimate_ns = utc_unix_us * 1000 + measurement_ns - monotonic_after_ns
```

The actual wall read occurred within the bracket, rather than necessarily at
its later endpoint. The bracket width bounds sampling uncertainty only. The
OS wall clock's absolute UTC accuracy is **unverified** (`utc_accuracy_ns:null`);
this is an OS UTC estimate, never NTP/PTP synchronization evidence or a device
timestamp. Calendar labels and tooltips state that limitation. The GUI uses
this genuine boot-matched clock association for calendar estimates; it does
not infer UTC from message arrival or interpret monotonic seconds as Unix time.

This is an additive response extension under envelope v1 / API `0.1-pre`.
Requests, operation lists, scopes, sequencing, subscriptions and recovery stay
unchanged. Existing Workbench hello decoding consumes named fields and ignores
additional result fields. Updated decoding accepts old responses without the
extension. Missing/null, unknown source, boot mismatch, malformed/overflowing
numbers or inconsistent brackets provide no calendar authority. Future source
or accuracy semantics require an explicit decoder extension. Strict third-party
clients that close every response object must allow this optional result field.

The Parent retains the binding alongside its existing observations. Private
selected reads use LRD2 only when a binding exists: the original six-byte
count/freshness header is followed by a bounded boot string (u16 byte length,
1..128 UTF-8 bytes), before/after u64 nanoseconds and signed i64 UTC microseconds,
all little endian, then the exact original selected records. Extra encoded
size is at most 154 bytes. LRD1 remains accepted and is emitted without a
binding. This private extension requires matching Parent/child binaries;
old private children reject LRD2. All original message limits, four credits,
1 MiB slots, source/revision/generation/freshness fences and retirement remain.
Metadata cannot change within a boot; a new clock source requires source
progress. The child uses a mapping only for traces with matching source boots.
Its bounded boot allocation is included in selected-provider accounting.

## Presentation and display semantics

An optional `plots[].axes.time_mode` accepts these strings:

| Value | Display |
|---|---|
| absent / v1 null | Local calendar if a usable binding exists, otherwise Runtime elapsed |
| `local_calendar` | Workbench machine's configured local time zone at each measurement instant |
| `utc` | UTC, with date/offset in the tooltip |
| `runtime_elapsed` | `T+HH:MM:SS`, with a day count for intervals of at least one day |
| `experiment_elapsed` | Runtime elapsed with an explicit unavailable-start explanation |

There is currently no authoritative experiment-start identity/time contract.
Runtime start, Recorder run start, connection time and document replacement
are not substituted for it. Thus experiment elapsed is deliberately unavailable
in TIME-1; adding a real experiment-start contract requires its own review.

Both explicit calendar modes fall back to Runtime elapsed when the binding is
unavailable, has a sampling bracket exceeding one second, or cannot be represented
by the date/local-offset provider. The reason appears beside the plot. Absolute
OS clock accuracy remains unverified even for a narrow bracket. Short axis labels
show `HH:MM:SS`; a visible range spanning dates or at least 24 hours includes
`YYYY-MM-DD`. Tooltips always include the calendar date and numeric UTC offset,
which distinguishes repeated local times during autumn DST transitions. The
offset is resolved per instant, not cached at launch. Local time-zone rules
come from Windows through the pinned `time` crate; they follow the installed
OS rules rather than a bundled worldwide historical zone database.

The optional field passes through existing v1 validation, canonical conversion,
bounded v2 admission and v1 export. Absent mode is omitted from both encodings,
preserving old canonical bytes. Unknown modes and object-shaped enum values
are rejected. Existing document/allocation limits still apply. Older Workbench
versions reject documents containing the new field; omit it when configuring
an older Parent. The public Workbench operation/envelope API is unchanged.

The stock adapter sets `egui_plot::Plot::x_axis_formatter` and its existing
ordinary tooltip callback. Pan, zoom, legend, axes and plot geometry remain
library-owned. Original X coordinates, Y values, point order, history, units
and freshness are preserved. WGPU/Glow use the same adapter and formatting.
No GPU code, Custom Glow renderer logic, Runtime client, mutation or request
is added to the child.

## Dependency review

Only the independent stock host adds exact `time = 0.3.55`, with `local-offset`
and default std/alloc features. The root lock already contains that version;
the root Cargo manifests/lock do not change. The active Windows host graph adds
`time 0.3.55`, `time-core 0.1.9`, `num-conv 0.2.2`, `deranged 0.5.9`,
and `powerfmt 0.2.1`, all MIT OR Apache-2.0. Existing active packages/features
are unchanged. Lock-only optional/non-Windows entries are `time-macros 0.2.32`
and `num_threads 0.1.7`. The audit verifies exact registry archive checksums,
every extracted source byte and included license-file hashes for all 173
active packages. Preserve that inventory and the added license texts in the
qualification evidence. Published release license bundles are unchanged.

Safety review: date arithmetic and OS offset lookup are display-only, bounded
by the selected boot metadata; errors yield relative labels. The crate's
thread-safe Windows time-zone FFI is used through its safe API. No application
unsafe code, timezone mutation, scheduler, network synchronization, GPU owner,
or instrument command is introduced. No new toolchain is required.

## Reproduce the virtual experiment

Build the root Runtime/Workbench and independent stock host with the pinned
MSVC toolchain. Run unit/native gates before freezing executable identities.
With Babashka available:

```powershell
python scripts/support/eframe-host/t1.py dev wgpu UNIQUE-RUN
python scripts/support/eframe-host/t1.py dev glow UNIQUE-RUN
python scripts/support/eframe-host/t1.py release wgpu UNIQUE-RUN
python scripts/support/eframe-host/t1.py release glow UNIQUE-RUN
```

Each run uses the virtual thermal plant, passive cooling, Recorder disabled,
one Runtime connection/scope, one Parent and the existing external Workbench
API. `clients/babashka-smoke/time1.clj` replaces the document through each mode
and verifies exact readback. The transparent relay only forwards and records
original bytes. Screenshots/JSON capture live scalar/plot updates, calendar
labels, relative fallback and API replacement. A targeted child crash checks
retirement before the same-renderer restart and current Parent-approved mode
restoration. Normal window close must leave Runtime alive. Requests before and
after display configuration/failure must remain identical, with no mutation,
operation-status or retry. The explicit 1 GiB Job cap is test-only, not deployment
policy. Keep unique run directories and all process/Job memory evidence.

Compatibility runs additionally accept `old-runtime` or `old-parent`, using
the preserved qualified EFRAME-3 dev executables under
`target/time-1/compat-old/`. Old Runtime provides no binding, so the new Parent
falls back. Old Parent ignores the new Runtime hello field and forwards LRD1;
the test omits the new presentation field. Neither run creates a second client.

See the separate TIME-1 validation record for actual results and limitations.

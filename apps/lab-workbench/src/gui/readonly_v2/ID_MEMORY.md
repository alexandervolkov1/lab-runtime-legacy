# egui ID provenance containment (DGUI-4C7A1)

The default native dev/test build now compiles **only egui 0.36.2** with
`debug-assertions = false`, retaining `overflow-checks = true`. Release already
omits this cfg. The process-global ID provenance accumulation is eliminated in
both configurations without changing hashes, occurrence identity, dependencies,
features or Cargo.lock. **ACTIVATION_QUALIFIED remains false**: maximum-document
Context capacity is still about 63 MiB, independently of ID diagnostics.

## Exact cause in the locked sources

The inspected crates are egui/eframe 0.36.2. The packaged egui VCS identity is
`49682f8baa058bf49e011035cfbd6e825f88a5ef`, path `crates/egui`.

- `Id::new` hashes its input, then conditionally calls `id_source::insert_root`.
  `Id::with` hashes a parent and `IdSalt`, then calls `insert_child`. The latter
  stores a formatted copy of the parent's provenance plus the new salt.
- `IdSalt::new` hashes its input and conditionally calls
  `id_salt_source::maybe_insert`. Numeric sources also take this insertion path.
- Both private modules and their calls are gated solely by
  `cfg(debug_assertions)`. Each owns a static `LazyLock<RwLock<...<String>>>`
  SOURCE_MAP. An existing key avoids insertion; a new key retains another String.
  Neither module has removal, capacity, Context ownership or shutdown logic.
  Context drop and IdTypeMap clearing cannot reach them. See the pinned
  [Id source](https://docs.rs/egui/0.36.2/src/egui/id.rs.html) and
  [IdSalt source](https://docs.rs/egui/0.36.2/src/egui/id_salt.rs.html).

`Ui::push_id` enters a child Ui; `Ui::new_child`, `next_auto_id`,
`allocate_space` and `allocate_response` also create/salt IDs. Thus ordinary
labels/layout scopes contribute IDs even without persistent widget state.
These paths are present in the inspected `egui/src/ui.rs` (109, 209, 884, 1231,
1272, 2164). DGUI-4C7A adds owner/epoch and checked occurrence addresses to them.
The existing v1 `gui/presentation.rs` scopes by document/revision/window/tab/panel;
`gui/controls.rs` adds control identities. Its normal widgets therefore exhibit
the same diagnostic lifetime problem. The probe invokes those actual production
v1 modules through a test-only bridge, not an imitation of their ID scheme.

The resolved egui features are `bytemuck` and `default_fonts`; eframe uses
`accesskit`, `default_fonts`, `glow`. The feature tables contain no provenance
switch. `callstack`, inspection, selectable labels and persisted widget memory
are different mechanisms. Disabling them does not remove the two static maps.
An unsafe/raw ID constructor would not contain automatic Ui child/salt creation,
and reusing old epoch IDs would violate state isolation. Neither is used.

## Supported build change and its tradeoff

The workspace manifest contains this version-specific override:

```toml
[profile.dev.package."egui:0.36.2"]
debug-assertions = false
overflow-checks = true
```

Cargo supports package profile overrides, and the test profile inherits dev.
This applies to ordinary `cargo build/run/test`, not just the measurement binary.
The existing release defaults already disable egui debug assertions; this change
does not alter release settings. See [Cargo profiles and overrides](https://doc.rust-lang.org/cargo/reference/profiles.html#overrides).

This switch is **not provenance-only** inside egui. It also removes that crate's
`debug_assert*` checks (layout/NaN, widget rect, sense/focus, grid, slider, image,
resize, panel, hit-test and plugin diagnostics), debug painting/widget inspection,
debug text and hover hooks; its missing-viewport debug panic follows the existing
release fallback. The source audit included every `debug_assert*` and
`cfg(debug_assertions)` site in egui. Normal assertions and explicit overflow
checks remain. All other dev crates, including Workbench, Runtime dependencies,
eframe and epaint, retain their existing checks. No global optimization,
assertion, panic, feature or safety-policy setting is changed.

Disabling only the provenance branches while preserving every egui diagnostic
would require an upstream feature/eviction API or a dependency patch. That is not
available in the locked crate and would need separate review. This PR uses the
supported package profile mechanism allowed by the task, documents its wider
diagnostic effect, and changes no dependency source/license evidence.

## Native method and executed configuration

Reproduce in Windows PowerShell/MSVC:

```powershell
./scripts/test-v2-renderer-allocation.ps1 -DiagnosticProvenance
./scripts/test-v2-renderer-allocation.ps1
./scripts/test-v2-renderer-allocation.ps1 -BuildProfile release
```

The first command explicitly restores the former egui debug cfg via Cargo's
command-line package override for diagnosis only. The other two are the proposed
dev and ordinary release configurations. Each command first builds the **actual
Workbench executable**, inspects Cargo artifact profiles and links exactly those
rlibs/native library paths into the probe. It refuses an unexpected egui cfg,
changed application checks, or a dev crate other than egui losing its checks.
Artifact records and reports go under ignored
`target/v2-renderer-allocation-probe/{dev-provenance,dev,release}/`.

Every scenario runs in a separate process. The existing checked System allocator
meter is unchanged. Fixture/model construction and reporting are outside the
measurement window; the renderer/document are production code. No app startup,
worker, Runtime connection, submission or disk persistence runs in the probe.

The small graph has ten occurrences (two roots sharing one Group and its repeated
Indicator/Status children) and three definitions. It uses revision 0 for seven
frames, revisions 1..16 in the same Context, then four new Contexts with revisions
17..20, each rendered four times and dropped. Empty and v1 scenarios use the same
schedule. The maximum graph uses only revisions 0, 1 and 2 across two Contexts,
keeping diagnosis finite instead of extrapolating it into gigabytes.

The first sequence measures previously unseen renderer IDs. Two exact replays
then isolate Context-local allocations with the diagnostic tables already warm;
the replay readings must match and Context drop must return to zero. Settled
same-epoch frames must retain the same bytes/blocks. In the fixed configurations,
even the first sequence must return to zero above the library baseline after
every Context drop. The baseline scenario measures the first-ever empty Context
separately. Each epoch report includes live bytes, blocks, phase peak, conservative
realloc-overlap peak and retained delta. None is RSS, allocator overhead or a
universal process-memory bound.

## Results: Windows x86_64 MSVC, Rust 1.95.0

Requested bytes after Context drop (small/v1/max columns are **additional above
an empty-Context warmup**, not total process memory):

| Scenario | Diagnostic dev cfg | Fixed dev | Release |
|---|---:|---:|---:|
| First empty Context, then four more: finite baseline | 442 | 24 | 24 |
| Small v2: first Context, revisions 0..16 | 771,327 | 0 | 0 |
| Small v2: four further Contexts, through revision 20 | 947,224 | 0 | 0 |
| Actual v1 renderer: first Context, revisions 0..16 | 164,631 | 0 | 0 |
| Actual v1 renderer: four further Contexts, through revision 20 | 201,989 | 0 | 0 |
| Maximum v2: first Context, two epochs | 165,720,375 | 0 | 0 |
| Maximum v2: new Context, third epoch | 261,553,112 | 0 | 0 |

The 24-byte/two-block first-Context baseline is independently reproduced by
constructing `ahash::RandomState` alone in a fresh process. The locked aHash
0.8.12 `random_state.rs::get_src` initializes a single RAND_SOURCE: an 8-byte
AtomicUsize owner and a 16-byte boxed trait-object pointer. Thirty-two more
constructions retain exactly those same two blocks. It is independent of the
renderer, epochs and egui SOURCE_MAPs, and is present in both fixed profiles.
The probe links the already-built transitive aHash artifact solely for this
attribution; no workspace dependency is added.

| Scene | Fixed dev/release settled Context-local bytes | Observed phase peak |
|---|---:|---:|
| Empty Ui, after one-time baseline | 326,161 | 593,405 |
| Small v2 | 491,088 | 745,936 |
| v1 (including its local renderer state), after epoch changes | 396,259 | 660,763 |
| Maximum v2, 20,480 occurrences | 66,153,808 | 77,821,900 |

Dev and release fixture readings match. The diagnostic dev maximum Context-only
figure remains 66,154,224, matching DGUI-4C7A. Its second new epoch adds 82,862,188
live bytes; small-v2 epoch 16 adds 35,586, v1 adds 7,220 (other steps include table
growth). Those increases vanish from the fixed renderer sequences. Global
retention is structurally absent because neither SOURCE_MAP module nor any
insertion call is compiled, not merely because a bounded fixture happened to
plateau. The diagnostic-mode maximum phase peak is 339,375,428; this limited
reproduction never needs a gigabyte.

An additional ID-only experiment creates the same 1,024 root/salt/child triples
with numeric sources and verbose Debug objects implementing the same Hash input.
Diagnostic dev retains **253,664** versus **1,455,496** bytes, but **3,073 blocks
in both cases**, with growth at every step. Same hashes/insertion sites imply
the same map cardinality: numeric formatting reduces String sizes, not lifetime.
Fixed dev and release allocate **zero bytes**, including transient peak, for
either ID sequence. All variants produce digest `1fded109995dc2dc`; the sampled
real occurrence sequence produces `9af04dbec3c35359`. These fingerprints supplement
the unchanged production identity code; they are not a substitute for equality
or collision-proof semantics.

## Regression evidence and remaining activation conditions

The real Workbench test target has a generic ID source whose Debug implementation
panics: normal root/salt/child construction must never format it, in dev or
release. This catches a missing/overridden dependency profile, including generic
monomorphization in the application crate. Another headless test checks 128
document/owner/revision/repeated-view identities and three Context lifetimes,
seeding foreign focus/TextEdit/view state and proving new identities cannot find
it. Renderer identity/hash code and the twelve original headless tests are
unchanged; no editor state or Runtime call is introduced by rendering.

Native targeted gates: 230 dev tests (14 renderer, 77 v2 DTO/graph/canonical/
admission/conversion/export, 47 v1 presentation/GUI/Plot/hover/Confirm, seven GUI
controls, 12 GUI app, 39 external, 19 Dispatcher, five architecture guards, ten
typed-read); 14 release renderer tests; all three native renderer probe variants;
the unchanged admission/conversion/export allocation probe; cargo/standalone
format checks, scoped warning-denied Clippy and diff check. No workspace-wide
gates, WSL, dependency updates, release or live renderer activation are involved.

The independent Context/paint-work blocker remains. The 5,284,672-byte reservation
and 8 MiB visual-state ceiling are unchanged, as are all admitted component limits.
A later reviewed DGUI-4C7A2 must bound simultaneously submitted widgets, retained
Context capacities and paint work while keeping every component reachable through
bounded navigation/virtualization. It has not started. Arbitrary custom Cargo
profiles/flags and future egui upgrades need this cfg/artifact/memory qualification
again; re-enabling egui debug assertions restores the demonstrated growth.

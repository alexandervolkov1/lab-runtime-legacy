# Runtime manual-output validation

`ACTIVATION_QUALIFIED=false`. This is a Draft review artifact, not release or
physical-device qualification. No Workbench/GUI, Windows Job, GPU, memory audit or
hardware run was performed. No previous PR was merged for this task.

## Source and scope

- Initial clean checkout: `feature/control-1-reference-retune`,
  `7e532202c5b2b405fc34a5132b262afe5243c557`, retained with Draft PR #52.
- New branch base: `2426fc0823e1d6a5e28425cc8ebc2e5f079953cb` (the pre-existing
  PR #51 merge). This task did not perform that merge.
- Shared validation/catalog simplification: `26eb194dfa04dcb1bb15dd8dd166f6788a0e9879`.
- Physical configuration-safe correction: `2d95f683a5559e2deeb96af38ae8c6c574c4c9a2`.
- Final production/API/example source: `100a533e306edd8487e6b32f0af74fb33d791eeb`.
  Later qualification/document commits do not change those production inputs.

The [review](runtime-manual-output-review.md) gives the module map, reading coverage,
concrete simplifications, safety defect and deferred findings. The
[API contract](../api/manual-output.md) defines the four operations, scope ownership,
finite authority, PID conflict, safe request, truthful delivery and manual recovery.
Cargo manifests/lockfile and dependencies were not changed.

## Actual external experiment

Windows 10 Pro 10.0.19045, AMD64; Rust 1.95.0 (`59807616e`, MSVC), LLVM 22.1.2;
Babashka 1.13.220. Each run starts one real `lab-runtime.exe` with the native virtual
thermal plant and Required Recorder, then one external Babashka TCP client. The
client uses one server-issued scope, discovers/describes the instrument, reads
temperature/output, explicitly acquires authority, writes **10%, 20%, 15%** with
terminal delivery results and subsequent observations, explicitly renews between
steps, requests safe output, waits for evidence, seals recording and requests
fixture shutdown. Workbench is absent; no PID-reference mutation substitutes power.

| Final run | Temperature before → after | Client / Runtime exit | Safe / archive |
|---|---|---|---|
| `debug-e2e-20261016-163159` | 20 → 22.435475228974724 °C | 0 / 0 | confirmed; run and boot sealed, complete coverage, zero gaps |
| `release-e2e-20261016-163327` | 20 → 22.440787417499514 °C | 0 / 0 | confirmed; run and boot sealed, complete coverage, zero gaps |

Both SQLite archives pass `PRAGMA integrity_check`. Each contains exactly three
accepted and three completed `output_write` operations and one Requested,
SendStarted and ReadbackVerified fact for each selected power. Acquisition,
three explicit renewals, safe return and temperature measurements are also stored.
Simulation facts are labeled `virtual_simulation`; they are not physical evidence.
`recording_stop` confirms COMMIT/run seal, and shutdown separately confirms Recorder
flush and finite owner cleanup.

| Binary | SHA-256 |
|---|---|
| dev `lab-runtime.exe` | `d6e536a218672f7ccef5ac04f1f4bfc69dcf7522f5b4900239d79747e8cdf0d5` |
| release `lab-runtime.exe` | `0508c69e2b9abbb281dcf85c94195f5305944a536be92ea252eb2f0b7239d8a6` |

Each final `run.json` records the exact command, boot, binary hash and **100 canonical
build-input SHA-256 values**, checked against committed Git blobs before launch.
The native binaries were built from the final source commit above. No binary is
published or activated by storing these hashes.

## Checks

| Check | Result and evidence |
|---|---|
| Core complete dev suite | 173 passed; complete Core portion of `manual-full-dev-2.log` |
| Runtime complete dev suite | 499 passed, 2 existing opt-in soaks ignored; `manual-runtime-dev-final.log` |
| Final post-effect publication regression | 1 passed in dev; `manual-post-effect-dev.log` |
| Final public manual API dev suite | 5 passed, including physical SimpleDevice detach/reattach; `manual-api-dev-final.log` |
| Complete Core + Runtime release suite | 673 passed, 2 existing opt-in soaks ignored; `manual-full-release.log` |
| Scoped all-targets Clippy, warnings denied | PASS; `manual-clippy-final.log` |
| Final dev/release builds | PASS; `manual-build-dev-final.log`, `manual-build-release-final.log` |
| `cargo fmt --all --check`, `git diff --check` | PASS |

Tests ran on the working implementation before the source commit. The final dev
targeted runs cover the last change preserving a delivered/queued receipt after a
host publication error. The complete release suite includes that regression. Four
output-vocabulary doc comments were clarified after release test compilation;
executable behavior was unchanged, and both final binaries were built afterward.
No unchanged GUI matrices or memory probes were rerun or claimed as new evidence.

The new physical fixtures prove queued versus ACK versus separate readback,
readback mismatch, loss before/after possible send, no ordinary retry, retained
receipt through later safe output, PID/manual conflict, Required Recorder gating,
and safe revocation of queued work. Public API tests prove value/fence rejection,
foreign-scope exclusion, exact-ID deduplication, finite explicit renewal, client
loss without replay and scope reattachment without implicit rearm. Existing
transport/reconnect/Recorder failure and shutdown suites remain passing.

The configuration regression was reproduced before its fix: SimpleDevice safe
barrier and profile replacement could accept simulated completion without bytes.
The corrected code uses existing `ServiceSafety`; new tests require actual
WRITE/ACK/readback and retain uncertainty when an ACK is suppressed, including
Metakon replacement. Configuration commit is not physical safe confirmation.

## Reproduce

From a Windows/MSVC checkout, with the existing Rust toolchain, Python and `bb`:

```powershell
cargo test -p lab-core -p lab-runtime --locked
cargo test -p lab-core -p lab-runtime --release --locked
cargo clippy -p lab-core -p lab-runtime --all-targets --locked -- -D warnings
cargo fmt --all --check
git diff --check
cargo build -p lab-runtime --locked
cargo build -p lab-runtime --release --locked
python docs/developer/runtime-manual-output-evidence/run_e2e.py debug
python docs/developer/runtime-manual-output-evidence/run_e2e.py release
```

The runner prints a new retained directory under `target/runtime-manual-api`.
It checks committed build inputs, uses ephemeral loopback ports and finite
readiness/client/process deadlines, and terminates only its own fixture if cleanup
fails. Validate the printed archive with:

```powershell
python docs/developer/runtime-manual-output-evidence/check_archive.py <printed-directory>/history.sqlite
```

For interactive use and the explicit identified-Arduino invocation, see the
[Babashka instructions](../../clients/babashka-smoke/README.md#manual-heater-power-direct-runtime).
No verified Arduino firmware/identity/COM configuration was available. The existing
generic writable fixture's `COM256` is a placeholder and was never opened.
Physical acceptance, hardware safeguards, long-duration qualification and Linux
execution remain outside these results. ACK/register readback never proves actual
heat or another independent physical effect.

## Retained evidence

[Results](runtime-manual-output-evidence/results.json),
[raw archive](runtime-manual-output-evidence/raw-evidence.zip),
[entry hashes](runtime-manual-output-evidence/raw-manifest.json), and
[repository checksums](runtime-manual-output-sha256.txt) preserve the commands,
transcripts, databases, successful gates and intermediate failures. This is
experimental evidence, not a regression-input directory. Original local files and
historical project reports were retained.

The earlier `debug-e2e-20261016-161532` was a development run before the production
commit; its diff hash does not cover then-untracked new files and it is **not** the
final source-identity qualification. It remains preserved. Earlier configuration
test logs include initial fixture path/policy/assertion corrections; the valid
before-fix SimpleDevice assertions failed, and corrected regression tests pass.
`manual-full-dev.log` stopped at old prohibited-operation expectations;
`manual-full-dev-2.log` stopped at a new test's missing NDJSON newline. The corrected
wire test and subsequent full Runtime run pass. These failed attempts were not
deleted or presented as successful gates.

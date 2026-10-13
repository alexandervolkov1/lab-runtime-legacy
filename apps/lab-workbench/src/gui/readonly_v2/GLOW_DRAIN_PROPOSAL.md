# C4B1 review proposal: fallible GPU retirement

**Proposal only; not implemented or authorized as a follow-on stage.** The
[C4B1 preflight](NATIVE_GLOW_PREFLIGHT.md) found that the C4A cleanup contract
cannot represent a bounded GL fence wait that did not complete. This proposal
changes the internal C4A host interface, not Runtime or Workbench APIs.

## Concrete blocking path

1. Generation G uploads its font texture and submits a draw.
2. `complete()` observes a fence timeout or wait failure and returns an error.
3. `PreviewFrame::finish` retires G. `TextureOwner::retire` calls `drain() -> ()`.
4. On return, it unconditionally clears `in_flight`, removes the generation and
   calls `free` for every key. Preview then drops CPU output and Context.

Returning after another finite timeout violates step 4's quiescence premise.
Waiting until success has no finite failure policy. Panicking in Drop, aborting
the process, making `free` a no-op or silently leaking a painter does not fulfill
the existing contract. Changing the return type alone is insufficient: retirement,
new-generation admission, frame failure and shutdown must preserve the obligation.

## Proposed ownership change

Introduce one outer native `HostSession` that outlives its Preview and owns the
backend and **one fixed retirement slot**. GPU resource ownership must not depend
on successful execution of a Preview destructor. Preview Drop can revoke the
session's generation and mark that slot as pending without allocating, retrying,
or issuing a premature GL delete. Move the generating Context and any still-owned
FrameData into that same slot when output settlement requires them; do not clone
them or drop the generating Context early. This transfer must leave Preview empty
and blocked, with no second frame or Context admitted. The native runner must settle the session
before destroying its current OS/GL context. The document can remain borrowed
from the caller for the entire native run; an owning document adaptation is not
needed for `eframe::AppCreator<'app>` itself.

An illustrative internal protocol, to refine during independent review:

```text
complete(deadline) -> Completed | Pending(Timeout) | Failed(reason)
request_retirement(generation) -> Retired | Outstanding(reason)
shutdown(deadline) -> ReleasedReceipt | OutstandingSession
```

Only `Completed` discharges dependent GPU use. Only `Retired`/`ReleasedReceipt`
discharges ownership. `Failed`, timeout, missing fence, null fence and device
loss are never completion receipts. Error acknowledgement retains the original
frame error as well as the cleanup status; a cleanup error cannot turn the
frame into `HostCompleted`.

| Session state | Allowed work | Resource obligation |
|---|---|---|
| Idle | Prepare one frame for the current or a new generation | At most eight registered texture keys |
| Processing | Ordered set/partial, bounded platform output, submit, completion | One frame; attempted keys registered before each fallible operation |
| RetirementPending | Bounded completion observation or terminal shutdown only | Same generation, texture keys, buffers and at most one fence; no new frame or namespace |
| Retired | Free keys and destroy backend objects with the GL context current | No dependent work remains; a new generation may begin |
| DeviceLost/Unresolved | No upload, draw, new generation or render acknowledgement | Runner owns the terminal session until a reviewed context/device teardown discharges it |

The slot is not a queue. A pending retirement prevents replacement even when a
different Plot/document/owner/revision is requested. No numeric texture ID may
escape its generation. No second generation is introduced to hide slow cleanup.
Frame/mesh ownership remains serial; any retained CPU frame payload is charged
to Preview's existing 8 MiB allowance and measured again. Native session metadata,
GL wrapper storage, staging, framebuffer and driver allocations are separate.

## Bounded completion policy to review

Require desktop GL fence support before accepting a Preview frame. Insert one
fence after all dependent uploads/platform operations/draws, check fence creation
and GL errors, and permit a finite number of waits under a monotonic deadline.
An initial candidate for smoke review is at most 20 waits, each at most 5 ms,
and a 100 ms overall deadline; this is a proposed policy, not measured driver
latency or a universal wall-clock guarantee. Values must fit glow 0.17.0's
signed `i32` timeout parameter without negative-to-`u64` conversion.

Only `ALREADY_SIGNALED` or `CONDITION_SATISFIED` allows free. Timeout retains the
single obligation and disables further rendering. `WAIT_FAILED`, a GL error or
context loss enters terminal unresolved shutdown. No repeated upload/submit and
no automatic recreation is permitted. A bounded return with unresolved ownership
is an explicit failure, not a successful drain or GPU lifecycle qualification.

The native runner needs a reviewed policy for terminal GL-context destruction.
If the selected Windows driver/API cannot establish bounded terminal release,
that remains a GPU lifecycle blocker even if healthy-device smoke passes. This
proposal does not assert that a timeout, `glDeleteTextures`, window close, or
dropping `Arc<glow::Context>` proves that all GPU use has ended.

## Public Glow adapter and error reporting

Use a dedicated Preview painter/texture namespace, separate from eframe's main
painter. `Painter::set_texture` and `paint_primitives` return `()`; dev error
checks can consume GL errors into logs. Wrapping them in `Ok(())` or checking
only `glGetError` afterward can falsely acknowledge failed work. An adapter must
account for these error sites in both profiles before its acknowledgement is
trusted. A small explicitly audited raw-Glow upload/draw/error wrapper is a
candidate; its exact implementation and current-context/thread invariant need
review. No private dependency access or unsafe self-reference is proposed.

Raw Glow fence/error calls are public but unsafe. The workspace currently forbids
unsafe code. Review an isolated native support executable/module with a narrowly
documented boundary, as distinct from weakening production Workbench lints.
Any additional direct dependency must use reviewed pinned source/license evidence;
the existing eframe reexports already expose Glow and the painter APIs.

Before choosing an eframe callback or a dedicated native event loop, verify that
the GL context is current at every upload/draw/completion/free, that Preview draw
is not erased by the host, and that shutdown retains that context until ownership
is discharged. A successful borrowed `AppCreator` compile check does not prove
those render scheduling and shutdown properties. Do not queue copied FullOutputs
or borrow non-static frame guards into a static paint callback.

The preferred runner proposal is a separate native executable whose outer scope
owns the window, current GL context and HostSession in explicit shutdown order,
using the already resolved glutin/winit versions and public APIs. It renders
Preview directly through its own painter, without a main egui Context. This
requires review of the exact native adapter/dependency declarations. Eframe's
`App::on_exit` returns `()` and its GL Arc does not transfer the OS context, so
an unresolved session must not merely be returned from that callback and left
behind after eframe destroys the context.

## Acceptance after review

First test timeout, wait error, device loss, partial application, platform error
with no uploads, forgotten frame, explicit retirement and shutdown in the new
state machine. Assert one generation/one frame, no premature free, no successful
acknowledgement, no retries and no growth on repeated failure. Then implement the
isolated fixed 1280x960/DPI-1 native runner and run the requested dev/release GPU
sequence, counters and separate host memory accounting. Re-run CPU qualification
if any ownership storage/lifetime changes. Independent approval of this proposal
is required before changing the C4A contract under the C4B1 task's section 3.

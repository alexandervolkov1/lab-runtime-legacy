//! Persistent serial owner outside Preview; no GPU or native context implementation.
use super::{
    completion::{WaitObservation, WaitPolicy, WaitPurpose},
    deletion::DeletionFailure,
    frame::{ContextGeneration, FrameData, FrameError, FrameOutcome},
    texture_owner::{FrameBackend, TextureOwner},
};
use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetirementReason {
    ViewChanged,
    Explicit,
    FrameFailed,
    Discarded,
    Abandoned,
    PreviewDropped,
    Shutdown,
}

/// Fixed diagnostic record; frame failure and cleanup observation are separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RetirementStatus {
    pub(crate) generation: ContextGeneration,
    pub(crate) reason: RetirementReason,
    pub(crate) frame: Option<FrameOutcome>,
    /// Original observation at retirement request; later waits cannot erase it.
    pub(crate) initial_wait: WaitObservation,
    /// Most recent cleanup observation, distinct from the original frame error.
    pub(crate) wait: WaitObservation,
    /// First deletion failure; independent of execution and completion evidence.
    pub(crate) deletion: Option<DeletionFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CleanupOutcome {
    Idle,
    Active(ContextGeneration),
    Pending(RetirementStatus),
    Retired(RetirementStatus),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "Unresolved requires retaining this HostSession and its native context"]
pub(crate) enum ShutdownOutcome {
    Released,
    Unresolved(RetirementStatus),
}

/// Moves, never copies, the generating Context and the single CPU frame slot.
pub(super) struct RetirementPayload {
    pub(super) context: Option<egui::Context>,
    pub(super) frame: Option<FrameData>,
}

struct PendingRetirement {
    status: RetirementStatus,
    _payload: RetirementPayload,
}

/// Trusted native owner. Keep this owner alive on Unresolved shutdown, including
/// after Preview has gone. Final OS/device destruction is a separate host duty;
/// dropping the root owner is never a quiescence or successful shutdown receipt.
/// Rust borrowing enforces Preview's shorter lifetime, not a linear final root
/// destructor: dropping an unresolved session destroys its CPU diagnostic/payload
/// fields. That violates the native host contract and is not a recovery path.
#[must_use = "retain the session on unresolved shutdown"]
pub(crate) struct HostSession<B: FrameBackend> {
    pub(super) textures: TextureOwner<B>,
    pending: Option<PendingRetirement>,
    last: CleanupOutcome,
    closing: bool,
}
impl<B: FrameBackend> HostSession<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self {
            textures: TextureOwner::new(backend),
            pending: None,
            last: CleanupOutcome::Idle,
            closing: false,
        }
    }
    pub(crate) fn status(&self) -> CleanupOutcome {
        self.last
    }
    pub(crate) fn owned_keys(&self) -> usize {
        self.textures.owned_keys()
    }
    pub(crate) fn last_wait(&self) -> WaitObservation {
        self.textures.last_wait
    }
    pub(crate) fn retained_cpu_frame(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p._payload.frame.is_some())
    }
    pub(crate) fn retained_context(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p._payload.context.is_some())
    }
    pub(super) fn admit_work(&self) -> Result<(), FrameError> {
        if self.pending.is_some() {
            Err(FrameError::RetirementPending)
        } else if self.closing {
            Err(FrameError::HostClosed)
        } else {
            Ok(())
        }
    }
    pub(super) fn begin(&mut self, generation: ContextGeneration) -> Result<(), FrameError> {
        self.admit_work()?;
        self.textures.begin(generation)?;
        self.last = CleanupOutcome::Active(generation);
        Ok(())
    }
    pub(super) fn retire(
        &mut self,
        reason: RetirementReason,
        frame: Option<FrameOutcome>,
        payload: RetirementPayload,
    ) -> CleanupOutcome {
        if self.pending.is_some() {
            // Preview has already transferred its sole payload. Never replace
            // the first cause, original frame error, or pending generation.
            assert!(payload.frame.is_none() && payload.context.is_none());
            return self.last;
        }
        let Some(generation) = self.textures.generation() else {
            return self.last;
        };
        self.pending = Some(PendingRetirement {
            status: RetirementStatus {
                generation,
                reason,
                frame,
                initial_wait: self.textures.last_wait,
                wait: self.textures.last_wait,
                deletion: self.textures.deletion,
            },
            _payload: payload,
        });
        self.release_if_quiesced()
    }
    fn release_if_quiesced(&mut self) -> CleanupOutcome {
        if let Some(pending) = &mut self.pending {
            pending.status.wait = self.textures.last_wait;
            let released = self.textures.release();
            pending.status.deletion = self.textures.deletion;
            if released {
                self.last = CleanupOutcome::Retired(pending.status);
                // Free backend keys first, then CPU output, then its Context.
                let mut retired = self.pending.take().unwrap();
                retired._payload.frame = None;
                retired._payload.context = None;
            } else {
                self.last = CleanupOutcome::Pending(pending.status);
            }
        }
        self.last
    }
    /// Explicit bounded observation only: never resubmit an upload or draw.
    pub(crate) fn poll_retirement(&mut self, policy: WaitPolicy) -> CleanupOutcome {
        if self.pending.is_some() {
            self.textures.wait(WaitPurpose::Retirement, policy);
            self.release_if_quiesced();
        }
        self.last
    }
    /// A failed shutdown keeps its record and payload in this same owner. It
    /// permanently closes admission, but may be observed again with a new budget.
    pub(crate) fn shutdown(&mut self, policy: WaitPolicy) -> ShutdownOutcome {
        self.closing = true;
        self.retire(
            RetirementReason::Shutdown,
            None,
            RetirementPayload {
                context: None,
                frame: None,
            },
        );
        match self.poll_retirement(policy) {
            CleanupOutcome::Pending(status) => ShutdownOutcome::Unresolved(status),
            _ => ShutdownOutcome::Released,
        }
    }
}
impl<B: FrameBackend> Drop for HostSession<B> {
    fn drop(&mut self) {
        // No wait and no inferred completion in Drop. release refuses in-flight
        // work. The trusted native root must remain alive on Unresolved; a real
        // backend cannot implement destructor-based disposal of pending objects.
        self.textures.release();
    }
}

// The only inline owner is constructed for the explicit CpuOnly Preview. Hosts
// must borrow an external HostSession; no owning GPU-backend constructor exists.
#[allow(
    clippy::large_enum_variant,
    reason = "fixed inline storage avoids an extra allocation"
)]
pub(super) enum Session<'a, B: FrameBackend> {
    Cpu(HostSession<B>),
    Host(&'a mut HostSession<B>),
}
impl<B: FrameBackend> std::ops::Deref for Session<'_, B> {
    type Target = HostSession<B>;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Cpu(s) => s,
            Self::Host(s) => s,
        }
    }
}
impl<B: FrameBackend> std::ops::DerefMut for Session<'_, B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Cpu(s) => s,
            Self::Host(s) => s,
        }
    }
}

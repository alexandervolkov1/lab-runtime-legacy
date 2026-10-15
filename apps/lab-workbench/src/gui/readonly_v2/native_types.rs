//! Shared native ownership values, with no Preview, model, client or GPU calls.
use eframe::egui::TextureId;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Existing per-frame primitive bound, shared by Preview and native Device.
pub(crate) const MAX_SHAPES: usize = 192;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Unique across all Preview lifetimes in this process; never recycled/wrapped.
/// Not a persistent or cross-process GPU resource identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ContextGeneration(u64);
impl ContextGeneration {
    // Native backend conformance frames use the same never-reused namespace
    // allocator even when no egui Context is needed for their synthetic meshes.
    pub(crate) fn allocate() -> Result<Self, FrameError> {
        Self::from_counter(&NEXT_GENERATION)
    }
    fn from_counter(counter: &AtomicU64) -> Result<Self, FrameError> {
        let mut candidate = counter.load(Ordering::Relaxed);
        // Bound contention as well as exhaustion. No automatic caller retry.
        for _ in 0..8 {
            if candidate == 0 {
                return Err(FrameError::GenerationExhausted);
            }
            match counter.compare_exchange(
                candidate,
                candidate.checked_add(1).unwrap_or(0),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(Self(candidate)),
                Err(current) => candidate = current,
            }
        }
        Err(FrameError::GenerationContended)
    }
}

/// Fixed diagnostics; errors never carry unbounded host strings or imply retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameError {
    OutstandingFrame,
    Preparation(&'static str),
    GenerationExhausted,
    GenerationContended,
    AlreadyTessellated,
    NotTessellated,
    Tessellation,
    TextureCapacity,
    TextureProtocol,
    TextureUpdate,
    PlatformOutput,
    Submission,
    Completion,
    Deletion,
    RetirementPending,
    HostClosed,
}

/// A texture name is meaningful only within its creating Context generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureKey {
    pub(crate) generation: ContextGeneration,
    pub(crate) id: TextureId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaitFailure {
    WaitError,
    MissingFence,
    InvalidFence,
}

/// Only Quiesced proves that all uses covered by this generation have ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Completion {
    Quiesced,
    Unobserved,
    Timeout,
    Failed(WaitFailure),
    DeviceLost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaitPurpose {
    Frame,
    Retirement,
}

/// A requested API wait, not a promise about time spent inside a driver call.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WaitRequest {
    pub(crate) generation: ContextGeneration,
    pub(crate) purpose: WaitPurpose,
    pub(crate) timeout: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_exhaustion_never_wraps_or_restarts() {
        let counter = AtomicU64::new(u64::MAX - 1);
        assert_eq!(
            ContextGeneration::from_counter(&counter),
            Ok(ContextGeneration(u64::MAX - 1))
        );
        assert_eq!(
            ContextGeneration::from_counter(&counter),
            Ok(ContextGeneration(u64::MAX))
        );
        for _ in 0..3 {
            assert_eq!(
                ContextGeneration::from_counter(&counter),
                Err(FrameError::GenerationExhausted)
            );
        }
    }
}

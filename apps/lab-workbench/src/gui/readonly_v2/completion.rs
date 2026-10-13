//! Finite observation of dependent host work, distinct from frame success.
use super::{frame::ContextGeneration, texture_owner::FrameBackend};
use std::time::{Duration, Instant};

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

/// Independent caps for calls, each requested timeout and the application clock.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WaitPolicy {
    attempts: u8,
    per_call: Duration,
    elapsed: Duration,
}
impl WaitPolicy {
    pub(crate) fn new(attempts: u8, per_call: Duration, elapsed: Duration) -> Self {
        Self {
            attempts: attempts.min(20),
            per_call: per_call.min(Duration::from_millis(5)),
            elapsed: elapsed.min(Duration::from_millis(100)),
        }
    }
}
impl Default for WaitPolicy {
    fn default() -> Self {
        Self::new(20, Duration::from_millis(5), Duration::from_millis(100))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WaitObservation {
    pub(crate) completion: Completion,
    pub(crate) attempts: u8,
    pub(crate) elapsed: Duration,
    pub(crate) requested_wait: Duration,
    pub(crate) deadline_reached: bool,
    pub(crate) attempt_limit_reached: bool,
}
impl WaitObservation {
    // No dependent operation has started; this is a structural quiescence fact.
    pub(super) const IDLE: Self = Self {
        completion: Completion::Quiesced,
        attempts: 0,
        elapsed: Duration::ZERO,
        requested_wait: Duration::ZERO,
        deadline_reached: false,
        attempt_limit_reached: false,
    };
}

pub(super) fn observe<B: FrameBackend>(
    backend: &mut B,
    generation: ContextGeneration,
    purpose: WaitPurpose,
    policy: WaitPolicy,
) -> WaitObservation {
    observe_with_clock(backend, generation, purpose, policy, Instant::now)
}

// Injectable monotonic clock for deterministic deadline/driver-overrun tests.
pub(super) fn observe_with_clock<B: FrameBackend>(
    backend: &mut B,
    generation: ContextGeneration,
    purpose: WaitPurpose,
    policy: WaitPolicy,
    mut now: impl FnMut() -> Instant,
) -> WaitObservation {
    let start = now();
    let mut result = WaitObservation {
        completion: Completion::Timeout,
        ..WaitObservation::IDLE
    };
    for _ in 0..policy.attempts {
        let elapsed = now().saturating_duration_since(start);
        if elapsed >= policy.elapsed {
            break;
        }
        result.attempts += 1;
        let timeout = policy.per_call.min(policy.elapsed.saturating_sub(elapsed));
        result.requested_wait += timeout;
        result.completion = backend.wait(WaitRequest {
            generation,
            purpose,
            timeout,
        });
        if result.completion != Completion::Timeout {
            break;
        }
    }
    // A driver can overrun its requested timeout/deadline. Report elapsed time;
    // never manufacture Quiesced from reaching any software limit.
    result.elapsed = now().saturating_duration_since(start);
    result.deadline_reached = result.elapsed >= policy.elapsed;
    result.attempt_limit_reached =
        result.completion == Completion::Timeout && result.attempts == policy.attempts;
    result
}

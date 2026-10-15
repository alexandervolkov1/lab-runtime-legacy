//! One OS worker and one child incarnation, independent of the model owner.
//! No renderer, Runtime client, recovery state or application authority is stored.
#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "inactive production seam; product host selection is deferred"
    )
)]

use std::{
    io,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
mod windows;
pub(crate) use windows::{Launch, WindowsBackend};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Stopped,
    Starting,
    Ready,
    Stopping,
    TerminationRequested,
    ExitedHealthy,
    ExitedContained,
    ExitUnconfirmed,
}
impl Phase {
    fn replaceable(self) -> bool {
        matches!(
            self,
            Self::Stopped | Self::ExitedHealthy | Self::ExitedContained
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    ExplicitStop,
    ExplicitTermination,
    OwnerDropped,
    StartupFailure,
    StartupDeadline,
    HeartbeatDeadline,
    OwnerProgressDeadline,
    UnexpectedExit,
    OsFailure,
    WorkerPanic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Prepare,
    Resume,
    PollExit,
    Challenge,
    ReadProgress,
    Service,
    Close,
    Terminate,
    Retire,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FinalDiagnostic {
    pub(crate) incarnation: u64,
    pub(crate) reason: Option<Reason>,
    pub(crate) original_error: Option<lab_windows_job::Error>,
    pub(crate) cleanup_error: Option<lab_windows_job::Error>,
    pub(crate) exit_code: Option<u32>,
    pub(crate) reading: lab_windows_job::Reading,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Status {
    pub(crate) incarnation: u64,
    pub(crate) phase: Phase,
    pub(crate) alive: Option<bool>,
    pub(crate) reason: Option<Reason>,
    pub(crate) original_error: Option<lab_windows_job::Error>,
    pub(crate) cleanup_error: Option<lab_windows_job::Error>,
    pub(crate) exit_code: Option<u32>,
    pub(crate) owner_counter: u64,
    pub(crate) heartbeat_fresh: bool,
    pub(crate) owner_progress_fresh: bool,
    pub(crate) termination_requests: usize,
    pub(crate) objects: lab_windows_job::Objects,
    pub(crate) reading: lab_windows_job::Reading,
    pub(crate) last_final: Option<FinalDiagnostic>,
    pub(crate) call: Option<Call>,
    pub(crate) worker_progress: Instant,
    pub(crate) degraded: bool,
    pub(crate) worker_finished: bool,
}
impl Status {
    fn new() -> Self {
        Self {
            incarnation: 0,
            phase: Phase::Stopped,
            alive: None,
            reason: None,
            original_error: None,
            cleanup_error: None,
            exit_code: None,
            owner_counter: 0,
            heartbeat_fresh: false,
            owner_progress_fresh: false,
            termination_requests: 0,
            objects: Default::default(),
            reading: Default::default(),
            last_final: None,
            call: None,
            worker_progress: Instant::now(),
            degraded: false,
            worker_finished: false,
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Policy {
    pub(crate) challenge: Duration,
    pub(crate) observation: Duration,
    pub(crate) graceful: Duration,
    pub(crate) post_termination: Duration,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            challenge: Duration::from_millis(250),
            observation: Duration::from_secs(2),
            graceful: Duration::from_secs(2),
            post_termination: Duration::from_secs(2),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    Graceful,
    Terminate,
}
enum Request {
    Stop(Stop),
    Restart(Launch),
}
struct Shared {
    latest: Status,
    pending: Option<Request>,
    // Dequeue reserves admission until publication transfers it to the phase.
    // In particular, the initial retire call may block before Starting exists.
    launch_in_flight: bool,
}
struct Signal {
    state: Mutex<Shared>,
    changed: Condvar,
    owner_gone: AtomicBool,
}
/// Fixed protocol sample: an I/O/helper pong can refresh heartbeat but cannot
/// refresh owner progress unless the challenge is exact and counter advances.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Progress {
    pub(crate) incarnation: u64,
    pub(crate) challenge: u64,
    pub(crate) owner_counter: u64,
    pub(crate) healthy_close: bool,
}
/// All methods execute only on the sole OS worker; no model callback is accepted.
pub(crate) trait Backend {
    /// Bounded independent transport progress, including while closing.
    fn service(&mut self) -> Result<(), lab_windows_job::Error> {
        Ok(())
    }
    fn prepare(&mut self, launch: &Launch, incarnation: u64) -> Result<(), lab_windows_job::Error>;
    fn resume(&mut self) -> Result<(), lab_windows_job::Error>;
    fn has_process(&self) -> bool;
    fn poll_exit(&mut self) -> Result<Option<u32>, lab_windows_job::Error>;
    fn challenge(&mut self, incarnation: u64, sequence: u64) -> Result<(), lab_windows_job::Error>;
    fn progress(&mut self) -> Result<Option<Progress>, lab_windows_job::Error>;
    fn close(&mut self, incarnation: u64, sequence: u64) -> Result<(), lab_windows_job::Error>;
    fn terminate(&mut self, rollback: bool) -> Result<(), lab_windows_job::Error>;
    fn retire(&mut self) -> Result<lab_windows_job::Reading, lab_windows_job::Error>;
    /// True only for pending original I/O receipts, not a failed release call.
    /// Re-observe that same episode after process proof; never admit replacement.
    fn retirement_pending(&self) -> bool {
        false
    }
    fn objects(&self) -> lab_windows_job::Objects;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejected {
    Busy,
    StopPending,
    ExitUnconfirmed,
    Active,
    InvalidLaunch,
    Closed,
}

/// Nonblocking model-facing handle. The thread capture is the unique OS owner;
/// dropping this handle requests asynchronous stop and supplies no exit receipt.
pub(crate) struct Supervisor {
    signal: Arc<Signal>,
    join: Option<JoinHandle<()>>,
    policy: Policy,
}
impl Supervisor {
    pub(crate) fn spawn<B: Backend + 'static>(
        factory: impl FnOnce() -> B + Send + 'static,
        policy: Policy,
    ) -> io::Result<Self> {
        if [
            policy.challenge,
            policy.observation,
            policy.graceful,
            policy.post_termination,
        ]
        .iter()
        .any(Duration::is_zero)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "zero supervisor policy",
            ));
        }
        let signal = Arc::new(Signal {
            state: Mutex::new(Shared {
                latest: Status::new(),
                pending: None,
                launch_in_flight: false,
            }),
            changed: Condvar::new(),
            owner_gone: AtomicBool::new(false),
        });
        let worker = signal.clone();
        let join = thread::Builder::new()
            .name("lab-workbench-supervisor".into())
            .spawn(move || {
                let mut backend = factory(); // Native handles are created on this thread.
                Worker::new(worker, policy).run(&mut backend);
            })?;
        Ok(Self {
            signal,
            join: Some(join),
            policy,
        })
    }
    pub(crate) fn windows(policy: Policy) -> io::Result<Self> {
        Self::spawn(WindowsBackend::default, policy)
    }
    pub(crate) fn restart(&self, launch: Launch) -> Result<(), Rejected> {
        if self.signal.owner_gone.load(Ordering::Acquire) {
            return Err(Rejected::Closed);
        }
        launch.validate().map_err(|_| Rejected::InvalidLaunch)?;
        let mut state = self.signal.state.try_lock().map_err(|_| Rejected::Busy)?;
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) && !state.latest.worker_finished
        {
            return Err(Rejected::ExitUnconfirmed);
        }
        if state.latest.phase == Phase::ExitUnconfirmed {
            return Err(Rejected::ExitUnconfirmed);
        }
        if matches!(state.pending, Some(Request::Stop(_))) {
            return Err(Rejected::StopPending);
        }
        if state.pending.is_some() {
            return Err(Rejected::Busy);
        }
        if state.launch_in_flight || !state.latest.phase.replaceable() {
            return Err(Rejected::Active);
        }
        state.pending = Some(Request::Restart(launch));
        self.signal.changed.notify_all();
        Ok(())
    }
    /// Irreversible parent/root stop. Atomic latch cannot lose a stop when the
    /// short metadata lock is busy; it also prohibits a queued/future restart.
    pub(crate) fn shutdown(&self) {
        self.signal.owner_gone.store(true, Ordering::Release);
        self.signal.changed.notify_all();
    }
    pub(crate) fn stop(&self, mode: Stop) -> Result<(), Rejected> {
        let mut state = self.signal.state.try_lock().map_err(|_| Rejected::Busy)?;
        // Stop supersedes the sole pending restart. A pending immediate stop cannot
        // be downgraded by a repeated graceful request.
        let mode = match state.pending {
            Some(Request::Stop(Stop::Terminate)) => Stop::Terminate,
            _ => mode,
        };
        state.pending = Some(Request::Stop(mode));
        self.signal.changed.notify_all();
        Ok(())
    }
    pub(crate) fn status(&self) -> Result<Status, Rejected> {
        let state = self.signal.state.try_lock().map_err(|_| Rejected::Busy)?;
        let mut status = state.latest;
        if self.join.as_ref().is_some_and(JoinHandle::is_finished) && !status.worker_finished {
            status.phase = Phase::ExitUnconfirmed;
            status.reason.get_or_insert(Reason::WorkerPanic);
            status.degraded = true;
            return Ok(status);
        }
        status.degraded =
            !status.worker_finished && status.worker_progress.elapsed() >= self.policy.observation;
        Ok(status)
    }
    /// Observation only; deliberately no OS wait/join is offered to the model.
    pub(crate) fn worker_finished(&self) -> bool {
        self.join.as_ref().is_none_or(JoinHandle::is_finished)
    }
    #[cfg(test)]
    pub(crate) fn wait_status(&self, predicate: impl Fn(&Status) -> bool) -> Status {
        let (state, timeout) = self
            .signal
            .changed
            .wait_timeout_while(
                self.signal.state.lock().unwrap(),
                Duration::from_secs(8),
                |s| !predicate(&s.latest),
            )
            .unwrap();
        assert!(
            !timeout.timed_out() && predicate(&state.latest),
            "supervisor status barrier: {:?}",
            state.latest
        );
        state.latest
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.shutdown();
        // Unfinished join detaches, but its capture still owns the one obligation.
        // Never join or close a live Job on the model thread.
    }
}
struct Worker {
    signal: Arc<Signal>,
    policy: Policy,
    status: Status,
    started: Instant,
    last_heartbeat: Instant,
    last_progress: Instant,
    next_challenge: Instant,
    stopping: Instant,
    terminated: Instant,
    sequence: u64,
    accepted_challenge: u64,
    resumed: bool,
    healthy_close: bool,
    retire_failed: bool,
}
impl Worker {
    fn new(signal: Arc<Signal>, policy: Policy) -> Self {
        let now = Instant::now();
        Self {
            signal,
            policy,
            status: Status::new(),
            started: now,
            last_heartbeat: now,
            last_progress: now,
            next_challenge: now,
            stopping: now,
            terminated: now,
            sequence: 0,
            accepted_challenge: 0,
            resumed: false,
            healthy_close: false,
            retire_failed: false,
        }
    }
    fn publish(&mut self, call: Option<Call>) {
        self.status.call = call;
        self.status.worker_progress = Instant::now();
        let mut shared = self.signal.state.lock().unwrap();
        shared.latest = self.status;
        shared.launch_in_flight = false;
        self.signal.changed.notify_all();
    }
    fn take(&self) -> Option<Request> {
        let mut shared = self.signal.state.lock().unwrap();
        let request = shared.pending.take();
        if matches!(request, Some(Request::Restart(_))) {
            shared.launch_in_flight = true;
        }
        request
    }
    fn begin_stop<B: Backend>(&mut self, backend: &mut B, reason: Reason, immediate: bool) {
        if self.status.phase.replaceable() {
            return;
        }
        if self.status.phase == Phase::ExitUnconfirmed {
            // Observation failure is not a termination request. An explicit
            // Terminate or irreversible root stop may contain the original Job
            // once, without clearing uncertainty or retrying failed retirement.
            if (immediate || reason == Reason::OwnerDropped)
                && !self.retire_failed
                && self.status.alive != Some(false)
            {
                self.terminate(backend);
            }
            return;
        }
        if matches!(self.status.phase, Phase::Starting | Phase::Ready) {
            self.status.reason.get_or_insert(reason);
            self.status.phase = Phase::Stopping;
            self.stopping = Instant::now();
            self.publish(Some(Call::Close));
            if self.resumed
                && let Err(e) = backend.close(self.status.incarnation, self.sequence)
            {
                self.status.cleanup_error.get_or_insert(e);
            }
        }
        if immediate || !self.resumed {
            self.terminate(backend);
        }
    }
    fn terminate<B: Backend>(&mut self, backend: &mut B) {
        if self.status.termination_requests != 0 || !backend.has_process() {
            return;
        }
        self.status.termination_requests = 1;
        if self.status.phase != Phase::ExitUnconfirmed {
            self.status.phase = Phase::TerminationRequested;
        }
        self.terminated = Instant::now();
        self.publish(Some(Call::Terminate));
        if let Err(e) = backend.terminate(!self.resumed) {
            self.status.cleanup_error.get_or_insert(e);
            self.status.phase = Phase::ExitUnconfirmed;
        }
    }
    fn start<B: Backend>(&mut self, backend: &mut B, launch: Launch) {
        if !self.status.phase.replaceable() {
            return;
        }
        if let Err(e) = backend.retire() {
            self.status.cleanup_error.get_or_insert(e);
            self.status.phase = Phase::ExitUnconfirmed;
            self.retire_failed = true;
            return;
        }
        let Some(incarnation) = self.status.incarnation.checked_add(1) else {
            self.status.phase = Phase::ExitUnconfirmed;
            return;
        };
        let last_final = self.status.last_final;
        self.status = Status::new();
        self.status.last_final = last_final;
        self.status.incarnation = incarnation;
        self.status.phase = Phase::Starting;
        self.started = Instant::now();
        self.last_heartbeat = self.started;
        self.last_progress = self.started;
        self.next_challenge = self.started;
        self.sequence = 0;
        self.accepted_challenge = 0;
        self.resumed = false;
        self.healthy_close = false;
        self.publish(Some(Call::Prepare));
        let result = backend.prepare(&launch, incarnation);
        self.status.objects = backend.objects();
        if let Err(e) = result {
            self.status.original_error = Some(e);
            self.status.reason = Some(Reason::StartupFailure);
            if backend.has_process() {
                self.begin_stop(backend, Reason::StartupFailure, true);
            } else {
                self.finish_without_process(backend);
            }
            return;
        }
        // Stop during a blocking prepare is consumed before any ResumeThread.
        if self.signal.owner_gone.load(Ordering::Acquire) {
            self.begin_stop(backend, Reason::OwnerDropped, true);
            return;
        }
        if let Some(Request::Stop(mode)) = self.take() {
            self.begin_stop(
                backend,
                if mode == Stop::Terminate {
                    Reason::ExplicitTermination
                } else {
                    Reason::ExplicitStop
                },
                true,
            );
            return;
        }
        // An OS call may exceed the observation deadline. Once control returns,
        // do not execute a child whose startup window is already expired.
        if self.started.elapsed() >= self.policy.observation {
            self.begin_stop(backend, Reason::StartupDeadline, true);
            return;
        }
        self.publish(Some(Call::Resume));
        match backend.resume() {
            Ok(()) => self.resumed = true,
            Err(e) => {
                self.status.original_error = Some(e);
                self.begin_stop(backend, Reason::StartupFailure, true);
            }
        }
    }
    fn finish_without_process<B: Backend>(&mut self, backend: &mut B) {
        self.publish(Some(Call::Retire));
        match backend.retire() {
            Ok(reading) => {
                self.status.reading = reading;
                self.status.phase = Phase::Stopped;
                self.record_final();
            }
            Err(e) => {
                self.status.cleanup_error.get_or_insert(e);
                self.status.phase = Phase::ExitUnconfirmed;
                self.retire_failed = true;
            }
        }
    }
    fn record_final(&mut self) {
        self.status.last_final = Some(FinalDiagnostic {
            incarnation: self.status.incarnation,
            reason: self.status.reason,
            original_error: self.status.original_error,
            cleanup_error: self.status.cleanup_error,
            exit_code: self.status.exit_code,
            reading: self.status.reading,
        });
    }
    fn observe<B: Backend>(&mut self, backend: &mut B) {
        if !backend.has_process() || self.retire_failed {
            return;
        }
        self.publish(Some(Call::PollExit));
        match backend.poll_exit() {
            Ok(Some(code)) => {
                self.status.alive = Some(false);
                self.status.exit_code = Some(code);
                self.status.reason.get_or_insert(Reason::UnexpectedExit);
                // Close receipt must already have been observed on the previous
                // turn, or sampled once more after actual process exit.
                if let Ok(Some(progress)) = backend.progress() {
                    self.accept_progress(progress);
                }
                let terminal_phase =
                    if self.healthy_close && code == 0 && self.status.termination_requests == 0 {
                        Phase::ExitedHealthy
                    } else {
                        Phase::ExitedContained
                    };
                self.status.phase = Phase::Stopping; // Exit proved; owner retirement still pending.
                self.publish(Some(Call::Retire));
                match backend.retire() {
                    Ok(reading) => {
                        self.status.reading = reading;
                        self.status.phase = terminal_phase;
                    }
                    Err(e) => {
                        if !backend.retirement_pending() {
                            self.status.cleanup_error.get_or_insert(e);
                        }
                        self.status.phase = Phase::ExitUnconfirmed;
                        self.retire_failed = !backend.retirement_pending();
                    }
                }
                self.status.objects = backend.objects();
                self.record_final();
            }
            Ok(None) => self.status.alive = Some(true),
            Err(e) => {
                self.status.alive = None;
                if matches!(self.status.phase, Phase::Starting | Phase::Ready)
                    || (self.status.phase == Phase::ExitUnconfirmed
                        && self.status.termination_requests == 0)
                {
                    self.status.original_error.get_or_insert(e);
                } else if self.status.original_error != Some(e) {
                    // Re-observing the original failure must not occupy the
                    // separate cleanup diagnostic before containment is tried.
                    self.status.cleanup_error.get_or_insert(e);
                }
                self.status.reason.get_or_insert(Reason::OsFailure);
                self.status.phase = Phase::ExitUnconfirmed;
            }
        }
    }
    fn accept_progress(&mut self, progress: Progress) {
        if self.sequence == 0
            || progress.incarnation != self.status.incarnation
            || progress.challenge != self.sequence
        {
            return;
        }
        if progress.challenge > self.accepted_challenge {
            self.accepted_challenge = progress.challenge;
            self.last_heartbeat = Instant::now();
        }
        let advances = progress.owner_counter > self.status.owner_counter;
        if advances {
            self.status.owner_counter = progress.owner_counter;
            self.last_progress = Instant::now();
            if self.status.phase == Phase::Starting {
                self.status.phase = Phase::Ready;
            }
        }
        if self.status.phase == Phase::Stopping && progress.healthy_close && advances {
            self.healthy_close = true;
        }
    }
    fn tick<B: Backend>(&mut self, backend: &mut B) {
        if self.signal.owner_gone.load(Ordering::Acquire) {
            self.begin_stop(backend, Reason::OwnerDropped, false);
        }
        if let Some(request) = self.take() {
            match request {
                Request::Stop(mode) => self.begin_stop(
                    backend,
                    if mode == Stop::Terminate {
                        Reason::ExplicitTermination
                    } else {
                        Reason::ExplicitStop
                    },
                    mode == Stop::Terminate,
                ),
                Request::Restart(launch) if !self.signal.owner_gone.load(Ordering::Acquire) => {
                    self.start(backend, launch)
                }
                _ => {}
            }
        }
        if backend.has_process() && !self.retire_failed && self.status.alive != Some(false) {
            self.publish(Some(Call::Service));
            if let Err(e) = backend.service() {
                self.status.original_error.get_or_insert(e);
                self.begin_stop(backend, Reason::OsFailure, false);
            }
        }
        self.observe(backend);
        let now = Instant::now();
        if matches!(self.status.phase, Phase::Starting | Phase::Ready) && self.resumed {
            self.publish(Some(Call::ReadProgress));
            match backend.progress() {
                Ok(Some(progress)) => self.accept_progress(progress),
                Ok(None) => {}
                Err(e) => {
                    self.status.original_error.get_or_insert(e);
                    self.begin_stop(backend, Reason::OsFailure, false);
                }
            }
            if self.status.phase == Phase::Starting
                && now.duration_since(self.started) >= self.policy.observation
            {
                self.begin_stop(backend, Reason::StartupDeadline, false);
            } else if now.duration_since(self.last_heartbeat) >= self.policy.observation {
                self.begin_stop(backend, Reason::HeartbeatDeadline, false);
            } else if now.duration_since(self.last_progress) >= self.policy.observation {
                self.begin_stop(backend, Reason::OwnerProgressDeadline, false);
            } else if now >= self.next_challenge {
                if let Some(sequence) = self.sequence.checked_add(1) {
                    self.sequence = sequence;
                    self.next_challenge = now + self.policy.challenge;
                    self.publish(Some(Call::Challenge));
                    if let Err(e) = backend.challenge(self.status.incarnation, sequence) {
                        self.status.original_error.get_or_insert(e);
                        self.begin_stop(backend, Reason::OsFailure, false);
                    }
                } else {
                    self.begin_stop(backend, Reason::OsFailure, false);
                }
            }
        }
        if self.status.phase == Phase::Stopping
            && now.duration_since(self.stopping) >= self.policy.graceful
        {
            self.terminate(backend);
        }
        if self.status.phase == Phase::TerminationRequested
            && now.duration_since(self.terminated) >= self.policy.post_termination
        {
            self.status.phase = Phase::ExitUnconfirmed;
        }
        self.status.objects = backend.objects();
        let observing = backend.has_process()
            && self.resumed
            && matches!(
                self.status.phase,
                Phase::Starting | Phase::Ready | Phase::Stopping
            );
        self.status.heartbeat_fresh = observing
            && self.accepted_challenge != 0
            && now.saturating_duration_since(self.last_heartbeat) < self.policy.observation;
        self.status.owner_progress_fresh = observing
            && self.status.owner_counter != 0
            && now.saturating_duration_since(self.last_progress) < self.policy.observation;
        self.publish(None);
    }
    fn run<B: Backend>(&mut self, backend: &mut B) {
        loop {
            // Keep the backend outside the unwind closure: an adapter panic must
            // not drop the original process/Job obligation and permit replacement.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick(backend)))
                .is_err()
            {
                self.status.phase = Phase::ExitUnconfirmed;
                self.status.reason.get_or_insert(Reason::WorkerPanic);
                self.publish(None);
            }
            if self.signal.owner_gone.load(Ordering::Acquire)
                && self.status.phase.replaceable()
                && backend.objects() == lab_windows_job::Objects::default()
            {
                self.status.worker_finished = true;
                self.publish(None);
                return;
            }
            let guard = self.signal.state.lock().unwrap();
            let _ = self
                .signal
                .changed
                .wait_timeout(guard, Duration::from_millis(10))
                .unwrap();
        }
    }
}

#[cfg(test)]
mod native_tests;
#[cfg(test)]
pub(crate) mod tests;

//! Serial Workbench model owner and renderer-independent, coalesced scheduler.
//! An owner is moved into either the legacy v1 adapter or one independent runner.
//! No mutable model replica, renderer callback mailbox or second client exists.

use crate::{
    client::{ClientHandle, ClientUpdate, endpoint::RuntimeEndpoint},
    dispatcher::{LabClient, WorkbenchDispatcher},
    external::{PreparedEndpoint, WorkbenchEndpoint},
    model::{
        ExactRetryWorkflow, OperatorWorkflow, RecoveryStatusTracker, UiCommand, WorkbenchModel,
    },
    presentation::{AxisOptions, Plot, PresentationDocument, RuntimeRef, Trace, TraceStyle},
    rebuild::{RebuildClient, RebuildCoordinator},
};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, mpsc::TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) const UPDATES_PER_TURN: usize = 64;
const IDLE_CHECK: Duration = Duration::from_millis(100);
const SHUTDOWN_OBSERVATION: Duration = Duration::from_secs(3);
type Wake = Arc<dyn Fn() + Send + Sync>;

/// Receive/shutdown extend the existing client seams; there is one update receiver.
/// Polling must be nonblocking; shutdown retains the backend's bounded stop policy.
pub(crate) trait ParentClient: LabClient + RebuildClient {
    fn try_update(&self) -> Result<ClientUpdate, TryRecvError>;
    fn stop(self) -> Result<(), &'static str>;
}
impl ParentClient for ClientHandle {
    fn try_update(&self) -> Result<ClientUpdate, TryRecvError> {
        self.try_recv()
    }
    fn stop(self) -> Result<(), &'static str> {
        self.shutdown()
    }
}

/// One sequential authority, including existing workflow observers and rebuild.
/// Fields are accessible only to the legacy in-process operator adapter. A running
/// independent scheduler never exposes this owner or a mutable dispatcher borrow.
pub(crate) struct ParentOwner<C: ParentClient> {
    pub(crate) dispatcher: WorkbenchDispatcher<C>,
    pub(crate) operator: OperatorWorkflow,
    pub(crate) recovery_status: RecoveryStatusTracker,
    pub(crate) exact_retry: ExactRetryWorkflow,
    pub(crate) selected: Option<RuntimeRef>,
    rebuild: RebuildCoordinator,
    pub(crate) endpoint: Option<WorkbenchEndpoint>,
    stopped: bool,
    shutdown_error: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TurnReport {
    pub(crate) updates: usize,
    pub(crate) external_calls: usize,
    pub(crate) rebuild_steps: usize,
}
impl<C: ParentClient> ParentOwner<C> {
    pub(crate) fn new(
        presentation: PresentationDocument,
        client: C,
        endpoint: Option<WorkbenchEndpoint>,
    ) -> Self {
        Self {
            dispatcher: WorkbenchDispatcher::new(WorkbenchModel::new(presentation), client),
            operator: Default::default(),
            recovery_status: Default::default(),
            exact_retry: Default::default(),
            selected: None,
            rebuild: Default::default(),
            endpoint,
            stopped: false,
            shutdown_error: None,
        }
    }
    /// Preserves the old v1 ordering: FIFO updates and workflow/rebuild observers,
    /// default presentation transition, then one fair external-owner service turn.
    /// One current ClientUpdate replaces the former 64-update staging Vec.
    pub(crate) fn turn(&mut self) -> TurnReport {
        let mut report = TurnReport::default();
        if self.stopped {
            return report;
        }
        for _ in 0..UPDATES_PER_TURN {
            let update = match self.dispatcher.client().map(ParentClient::try_update) {
                Some(Ok(update)) => update,
                _ => break,
            };
            let events = self.dispatcher.apply_client_update(update.clone());
            if let Some(endpoint) = &mut self.endpoint {
                endpoint.route_events(&mut self.dispatcher, events);
            }
            self.recovery_status.after_update(&update);
            self.exact_retry
                .after_update(&update, &self.dispatcher, &self.recovery_status);
            self.operator.after_update(&update);
            self.dispatcher.advance_rebuild(&mut self.rebuild, &update);
            if let Some(event) = self.dispatcher.finish_model_turn()
                && let Some(endpoint) = &mut self.endpoint
            {
                endpoint.route_events(&mut self.dispatcher, [event]);
            }
            report.updates += 1;
            report.rebuild_steps += 1;
        }
        self.ensure_default_plot();
        if let Some(endpoint) = &mut self.endpoint {
            report.external_calls = endpoint.service_owner(&mut self.dispatcher);
        }
        report
    }
    fn ensure_default_plot(&mut self) {
        if self.selected.is_none() {
            self.selected = self
                .dispatcher
                .observations
                .entities
                .keys()
                .find(|identity| matches!(identity, RuntimeRef::Signal { .. }))
                .cloned();
        }
        let Some(signal) = self.selected.clone() else {
            return;
        };
        if let Some(command) = default_plot_command(&self.dispatcher, signal) {
            match self.dispatcher.apply_gui_ui(command) {
                Ok(outcome) => {
                    if let Some(endpoint) = &mut self.endpoint {
                        endpoint.route_events(&mut self.dispatcher, outcome.events);
                    }
                }
                Err(error) => self.dispatcher.set_client_error(error.to_string()),
            }
        }
    }
    /// Root shutdown only, never a renderer detach action. Repeated calls issue no
    /// additional endpoint/client stop. Accepted Runtime work is not canceled.
    pub(crate) fn shutdown(&mut self) -> Result<(), &'static str> {
        if self.stopped {
            return self.shutdown_error.map_or(Ok(()), Err);
        }
        self.stopped = true;
        if let Some(mut endpoint) = self.endpoint.take() {
            endpoint.shutdown(&mut self.dispatcher);
        }
        if let Some(client) = self.dispatcher.take_client()
            && let Err(error) = client.stop()
        {
            self.dispatcher.set_client_error(error.to_owned());
            self.shutdown_error = Some(error);
        }
        self.shutdown_error.map_or(Ok(()), Err)
    }
}
impl<C: ParentClient> Drop for ParentOwner<C> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl ParentOwner<ClientHandle> {
    /// Startup does not require an egui context/window. Failure drops the one
    /// already-created worker/endpoint using their existing shutdown policies.
    pub(crate) fn connect(
        address: RuntimeEndpoint,
        observation_only: bool,
        desired_scope: Option<String>,
        journal_path: PathBuf,
        presentation: PresentationDocument,
        endpoint: Option<PreparedEndpoint>,
        wake: Wake,
    ) -> io::Result<Self> {
        let client = ClientHandle::spawn_configured(
            address,
            observation_only,
            Some(journal_path),
            Some(Arc::clone(&wake)),
        )?;
        client.connect(desired_scope).map_err(io::Error::other)?;
        let readiness = endpoint.as_ref().map(PreparedEndpoint::readiness_line);
        let endpoint = endpoint
            .map(|prepared| WorkbenchEndpoint::start(prepared, wake))
            .transpose()?;
        if let Some(readiness) = readiness {
            println!("{readiness}");
        }
        Ok(Self::new(presentation, client, endpoint))
    }
}

pub(crate) fn default_plot_command<C>(
    model: &WorkbenchDispatcher<C>,
    signal: RuntimeRef,
) -> Option<UiCommand> {
    (model.should_create_default_plot() && model.presentation.windows.is_empty()).then(|| {
        UiCommand::AddPlot {
            plot: Plot {
                id: "default-live-plot".into(),
                title: "Live signal".into(),
                time_window_seconds: 60.0,
                axes: AxisOptions::default(),
                traces: vec![Trace {
                    id: "default-live-trace".into(),
                    source: signal,
                    display_label: "Selected signal".into(),
                    visible: true,
                    style: TraceStyle {
                        color: "cyan".into(),
                        width: 1.5,
                    },
                    display_unit: None,
                }],
            },
        }
    })
}

#[derive(Default)]
struct SignalState {
    pending: bool,
    stop: bool,
    #[cfg(test)]
    applied_updates: usize,
    #[cfg(test)]
    idle_timeouts: usize,
    #[cfg(test)]
    completed_turns: usize,
    #[cfg(test)]
    first_turns: [Option<TurnObservation>; 2],
}
#[cfg(test)]
struct TurnObservation {
    report: TurnReport,
    idle_timeouts: usize,
    client_error: Option<String>,
}
/// Constant storage, not a wake/callback queue. Clear-before-turn prevents a lost
/// publish during processing; producers only set a bit and notify the Condvar.
#[derive(Clone, Default)]
pub(crate) struct PumpSignal(Arc<(Mutex<SignalState>, Condvar)>);
impl PumpSignal {
    pub(crate) fn wake(&self) {
        let (lock, cv) = &*self.0;
        lock.lock().unwrap().pending = true;
        cv.notify_all();
    }
    pub(crate) fn callback(&self) -> Wake {
        let signal = self.clone();
        Arc::new(move || signal.wake())
    }
    fn stop(&self) {
        let (lock, cv) = &*self.0;
        lock.lock().unwrap().stop = true;
        cv.notify_all();
    }
    fn next(&self) -> bool {
        let (lock, cv) = &*self.0;
        let state = lock.lock().unwrap();
        let (mut state, _timeout) = cv
            .wait_timeout_while(state, IDLE_CHECK, |s| !s.pending && !s.stop)
            .unwrap();
        #[cfg(test)]
        if _timeout.timed_out() {
            state.idle_timeouts += 1;
        }
        if state.stop {
            return false;
        }
        state.pending = false;
        true
    }
    #[cfg(test)]
    fn record(&self, report: TurnReport, client_error: &Option<String>) {
        let (lock, cv) = &*self.0;
        let mut state = lock.lock().unwrap();
        state.applied_updates += report.updates;
        let index = state.completed_turns;
        if index < state.first_turns.len() {
            state.first_turns[index] = Some(TurnObservation {
                report,
                idle_timeouts: state.idle_timeouts,
                client_error: client_error.clone(),
            });
        }
        state.completed_turns += 1;
        cv.notify_all();
    }
    #[cfg(test)]
    pub(crate) fn wait_updates(&self, count: usize) {
        let (lock, cv) = &*self.0;
        let (state, timeout) = cv
            .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(2), |s| {
                s.applied_updates < count
            })
            .unwrap();
        assert!(
            state.applied_updates >= count && !timeout.timed_out(),
            "owner update barrier failed"
        );
    }
}

/// A real independently scheduled root. Legacy v1 continues using the same
/// owner cooperatively; selecting a new product host remains a later milestone.
/// The handle contains no dispatcher/model reference while its thread runs.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "independent root awaits separately reviewed product host selection"
    )
)]
pub(crate) struct ParentPump<C: ParentClient + Send + 'static> {
    signal: PumpSignal,
    join: Option<JoinHandle<ParentOwner<C>>>,
    stopped_owner: Option<ParentOwner<C>>,
    terminal_error: Option<&'static str>,
    #[cfg(windows)]
    supervisor: Option<crate::supervisor::Supervisor>,
}
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "independent root awaits separately reviewed product host selection"
    )
)]
impl<C: ParentClient + Send + 'static> ParentPump<C> {
    /// Moves the unique owner into one thread. On thread-start failure its capture
    /// is dropped, so it cannot leave a second client/endpoint behind.
    pub(crate) fn spawn(owner: ParentOwner<C>, signal: PumpSignal) -> io::Result<Self> {
        Self::spawn_with(owner, signal, |owner, signal| {
            thread::Builder::new()
                .name("lab-workbench-parent".into())
                .spawn(move || {
                    let mut owner = owner;
                    signal.wake();
                    while signal.next() {
                        let report = owner.turn();
                        #[cfg(test)]
                        signal.record(report, &owner.dispatcher.client_error);
                        if report.updates == UPDATES_PER_TURN {
                            signal.wake();
                        }
                    }
                    let _ = owner.shutdown();
                    owner
                })
        })
    }
    fn spawn_with(
        owner: ParentOwner<C>,
        signal: PumpSignal,
        spawn: impl FnOnce(ParentOwner<C>, PumpSignal) -> io::Result<JoinHandle<ParentOwner<C>>>,
    ) -> io::Result<Self> {
        let join = spawn(owner, signal.clone())?;
        Ok(Self {
            signal,
            join: Some(join),
            stopped_owner: None,
            terminal_error: None,
            #[cfg(windows)]
            supervisor: None,
        })
    }
    /// Install one optional OS owner without putting OS work on ParentOwner.
    /// Active v1 never calls this inactive composition seam.
    #[cfg(windows)]
    pub(crate) fn install_supervisor<B: crate::supervisor::Backend + 'static>(
        &mut self,
        factory: impl FnOnce() -> B + Send + 'static,
        policy: crate::supervisor::Policy,
    ) -> io::Result<&crate::supervisor::Supervisor> {
        if self.supervisor.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "one supervisor per parent",
            ));
        }
        self.supervisor = Some(crate::supervisor::Supervisor::spawn(factory, policy)?);
        Ok(self.supervisor.as_ref().unwrap())
    }
    #[cfg(windows)]
    pub(crate) fn supervisor(&self) -> Option<&crate::supervisor::Supervisor> {
        self.supervisor.as_ref()
    }
    /// Does not drop an unconfirmed join handle on timeout. Caller can observe
    /// the same stopping root again; no replacement worker is started.
    pub(crate) fn shutdown(&mut self) -> Result<(), &'static str> {
        self.shutdown_until(Instant::now() + SHUTDOWN_OBSERVATION)
    }
    fn shutdown_until(&mut self, deadline: Instant) -> Result<(), &'static str> {
        #[cfg(windows)]
        if let Some(supervisor) = &self.supervisor {
            supervisor.shutdown();
        }
        self.signal.stop();
        while self.join.as_ref().is_some_and(|join| !join.is_finished()) {
            if Instant::now() >= deadline {
                return Err("parent owner stop remains unconfirmed");
            }
            let (lock, cv) = &*self.signal.0;
            let _ = cv
                .wait_timeout(lock.lock().unwrap(), Duration::from_millis(10))
                .unwrap();
        }
        if let Some(join) = self.join.take() {
            match join.join() {
                Ok(owner) => self.stopped_owner = Some(owner),
                Err(_) => self.terminal_error = Some("parent owner panicked"),
            }
        }
        self.terminal_error
            .or_else(|| {
                self.stopped_owner
                    .as_ref()
                    .and_then(|owner| owner.shutdown_error)
            })
            .map_or(Ok(()), Err)?;
        #[cfg(windows)]
        if let Some(supervisor) = &self.supervisor {
            let confirmed = supervisor.status().is_ok_and(|s| {
                matches!(
                    s.phase,
                    crate::supervisor::Phase::Stopped
                        | crate::supervisor::Phase::ExitedHealthy
                        | crate::supervisor::Phase::ExitedContained
                ) && s.objects == lab_windows_job::Objects::default()
                    && s.worker_finished
            });
            if !confirmed || !supervisor.worker_finished() {
                return Err("supervisor child exit remains unconfirmed");
            }
        }
        Ok(())
    }
}
impl<C: ParentClient + Send + 'static> Drop for ParentPump<C> {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Workbench parent shutdown: {error}");
        }
    }
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "independent root awaits separately reviewed product host selection"
    )
)]
impl ParentPump<ClientHandle> {
    pub(crate) fn connect(
        address: RuntimeEndpoint,
        observation_only: bool,
        desired_scope: Option<String>,
        journal_path: PathBuf,
        presentation: PresentationDocument,
        endpoint: Option<PreparedEndpoint>,
    ) -> io::Result<Self> {
        let signal = PumpSignal::default();
        let owner = ParentOwner::connect(
            address,
            observation_only,
            desired_scope,
            journal_path,
            presentation,
            endpoint,
            signal.callback(),
        )?;
        Self::spawn(owner, signal)
    }
}

#[cfg(test)]
pub(crate) mod tests;

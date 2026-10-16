//! Experimental headless Parent composition using the original pump/Job/IPC.
use super::ExperimentalGui;
use crate::{
    client::endpoint::RuntimeEndpoint,
    external::PreparedEndpoint,
    ownership::WorkspaceOwnership,
    parent::{ParentOwner, ParentPump, PumpSignal},
    presentation::PresentationDocument,
    private_ipc::parent_bridge::ParentBridge,
    supervisor::{Launch, Phase, Policy, Rejected, Stop},
};
use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[expect(
    clippy::too_many_arguments,
    reason = "same single-owner startup inputs as v1"
)]
pub(super) fn run(
    options: ExperimentalGui,
    address: RuntimeEndpoint,
    observe: bool,
    scope: Option<String>,
    journal: PathBuf,
    presentation: PresentationDocument,
    problem: Option<String>,
    _ownership: WorkspaceOwnership,
    endpoint: Option<PreparedEndpoint>,
) -> Result<(), String> {
    let signal = PumpSignal::default();
    let mut owner = ParentOwner::connect(
        address,
        observe,
        scope,
        journal,
        presentation,
        endpoint,
        signal.callback(),
    )
    .map_err(|e| e.to_string())?;
    owner.stock_control.enabled = options.controls && !observe;
    if let Some(problem) = problem {
        owner.dispatcher.set_client_error(problem);
    }
    let mut projection = ParentBridge::for_document(&owner.dispatcher).map_err(str::to_owned)?;
    projection.turn(&owner.dispatcher);
    let bridge = projection.bridge.clone();
    owner.private_bridge = Some(projection);
    let mut pump = ParentPump::spawn(owner, signal).map_err(|e| e.to_string())?;
    pump.install_supervisor(
        bridge.worker_factory().map_err(str::to_owned)?,
        Policy {
            observation: Duration::from_secs(8),
            ..Policy::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let mut restarts = 0;
    let mut expected_incarnation = 1;
    let mut close_requested = false;
    let mut requested = false;
    let mut previous = None;
    let mut deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let supervisor = pump.supervisor().expect("installed one supervisor");
        if !requested {
            let launch = Launch {
                executable: options.executable.clone(),
                arguments: vec![format!(
                    "eframe-{}{}",
                    options.renderer,
                    if options.capture { "-capture" } else { "" }
                )],
                limits: lab_windows_job::Limits {
                    process_commit: options.commit_limit,
                    job_commit: options.commit_limit,
                },
                fixture_directory: std::env::current_dir().map_err(|e| e.to_string())?,
            };
            match supervisor.restart(launch) {
                Ok(()) => requested = true,
                Err(Rejected::Busy) if Instant::now() < deadline => {}
                Err(error) => return Err(format!("experimental child launch rejected: {error:?}")),
            }
        }
        if !close_requested && bridge.status().is_ok_and(|s| s.window_close_requested) {
            match supervisor.stop(Stop::Graceful) {
                Ok(()) => close_requested = true,
                Err(Rejected::Busy) => {}
                Err(error) => return Err(format!("window close rejected: {error:?}")),
            }
        }
        if let Ok(status) = supervisor.status() {
            if previous != Some((status.incarnation, status.phase)) {
                let ipc = bridge.status().ok();
                println!(
                    "{}",
                    serde_json::json!({"experimental_eframe":options.renderer,"incarnation":status.incarnation,
                    "phase":format!("{:?}",status.phase),"reason":format!("{:?}",status.reason),"exit_code":status.exit_code,
                    "objects":[status.objects.jobs,status.objects.processes,status.objects.threads],
                    "ipc_handles":ipc.map(|s|s.handles),"ipc_pending_io":ipc.map(|s|s.pending_io),"ipc_retired":ipc.map(|s|s.retired),
                    "peak_process_commit":status.reading.peak_process_commit,"peak_job_commit":status.reading.peak_job_commit,
                    "experimental_commit_limit":options.commit_limit,"activation_qualified":false})
                );
                previous = Some((status.incarnation, status.phase));
            }
            if requested && status.incarnation == expected_incarnation {
                match status.phase {
                    Phase::ExitedHealthy => break,
                    Phase::ExitedContained
                        if !close_requested && restarts < options.restart_limit =>
                    {
                        // Supervisor additionally fences original process and IPC retirement.
                        // Only the GUI is relaunched. Parent/client/scope/journal stay alive.
                        restarts += 1;
                        expected_incarnation += 1;
                        close_requested = false;
                        requested = false;
                        deadline = Instant::now() + Duration::from_secs(3);
                    }
                    Phase::ExitedContained => {
                        return Err(
                            "experimental GUI exited; configured restart budget exhausted".into(),
                        );
                    }
                    Phase::ExitUnconfirmed => {
                        return Err(
                            "original GUI exit/retirement unconfirmed; replacement forbidden"
                                .into(),
                        );
                    }
                    _ => {}
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    // Bounded observation of the same stopping roots, never another owner.
    let stop_deadline = Instant::now() + Duration::from_secs(6);
    loop {
        match pump.shutdown() {
            Ok(()) => return Ok(()),
            Err(_) if Instant::now() < stop_deadline => thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.into()),
        }
    }
}

//! Deterministic lifecycle and native acceptance regressions.
use super::*;
use std::{path::PathBuf, sync::Barrier};

#[derive(Default)]
struct Fake {
    process: bool,
    exited: bool,
    hidden: bool,
    resumes: usize,
    prepares: usize,
    terminations: usize,
    closes: usize,
    sample: Option<Progress>,
    prepare_error: Option<lab_windows_job::Error>,
    terminate_error: Option<lab_windows_job::Error>,
    retire_error: Option<lab_windows_job::Error>,
    before_create: bool,
    wait_error: Option<lab_windows_job::Error>,
    auto_progress: bool,
    exit_on_close: bool,
    retire_entered: Option<Arc<Barrier>>,
    retire_release: Option<Arc<Barrier>>,
}
pub(crate) struct StalledBackend {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    inner: Fake,
}
impl StalledBackend {
    pub(crate) fn new(entered: Arc<Barrier>, release: Arc<Barrier>) -> Self {
        Self {
            entered,
            release,
            inner: Fake::default(),
        }
    }
}
impl Backend for StalledBackend {
    fn prepare(&mut self, l: &Launch, i: u64) -> Result<(), lab_windows_job::Error> {
        self.entered.wait();
        self.release.wait();
        self.inner.prepare(l, i)
    }
    fn resume(&mut self) -> Result<(), lab_windows_job::Error> {
        self.inner.resume()
    }
    fn has_process(&self) -> bool {
        self.inner.has_process()
    }
    fn poll_exit(&mut self) -> Result<Option<u32>, lab_windows_job::Error> {
        self.inner.poll_exit()
    }
    fn challenge(&mut self, i: u64, s: u64) -> Result<(), lab_windows_job::Error> {
        self.inner.challenge(i, s)
    }
    fn progress(&mut self) -> Result<Option<Progress>, lab_windows_job::Error> {
        self.inner.progress()
    }
    fn close(&mut self, i: u64, s: u64) -> Result<(), lab_windows_job::Error> {
        self.inner.close(i, s)
    }
    fn terminate(&mut self, r: bool) -> Result<(), lab_windows_job::Error> {
        self.inner.terminate(r)
    }
    fn retire(&mut self) -> Result<lab_windows_job::Reading, lab_windows_job::Error> {
        self.inner.retire()
    }
    fn objects(&self) -> lab_windows_job::Objects {
        self.inner.objects()
    }
}
impl Backend for Fake {
    fn prepare(&mut self, _: &Launch, _: u64) -> Result<(), lab_windows_job::Error> {
        self.prepares += 1;
        if self.before_create {
            return Err(self.prepare_error.unwrap());
        }
        self.process = true;
        self.exited = false;
        self.prepare_error.map_or(Ok(()), Err)
    }
    fn resume(&mut self) -> Result<(), lab_windows_job::Error> {
        self.resumes += 1;
        Ok(())
    }
    fn has_process(&self) -> bool {
        self.process
    }
    fn poll_exit(&mut self) -> Result<Option<u32>, lab_windows_job::Error> {
        if let Some(e) = self.wait_error {
            return Err(e);
        }
        Ok((self.exited && !self.hidden).then_some(77))
    }
    fn challenge(&mut self, incarnation: u64, sequence: u64) -> Result<(), lab_windows_job::Error> {
        if self.auto_progress {
            self.sample = Some(Progress {
                incarnation,
                challenge: sequence,
                owner_counter: sequence,
                healthy_close: false,
            });
        }
        Ok(())
    }
    fn progress(&mut self) -> Result<Option<Progress>, lab_windows_job::Error> {
        Ok(self.sample)
    }
    fn close(&mut self, _: u64, _: u64) -> Result<(), lab_windows_job::Error> {
        self.closes += 1;
        if self.exit_on_close {
            self.exited = true;
        }
        Ok(())
    }
    fn terminate(&mut self, _: bool) -> Result<(), lab_windows_job::Error> {
        self.terminations += 1;
        if let Some(e) = self.terminate_error {
            Err(e)
        } else {
            self.exited = true;
            Ok(())
        }
    }
    fn retire(&mut self) -> Result<lab_windows_job::Reading, lab_windows_job::Error> {
        if let Some(e) = self.retire_error {
            return Err(e);
        }
        assert!(
            !self.process || self.exited && !self.hidden,
            "false OS release"
        );
        if self.process {
            if let Some(entered) = &self.retire_entered {
                entered.wait();
            }
            if let Some(release) = &self.retire_release {
                release.wait();
            }
        }
        self.process = false;
        Ok(Default::default())
    }
    fn objects(&self) -> lab_windows_job::Objects {
        lab_windows_job::Objects {
            jobs: usize::from(self.process),
            processes: usize::from(self.process),
            threads: 0,
        }
    }
}
pub(crate) fn launch() -> Launch {
    Launch {
        executable: std::env::current_exe().unwrap(),
        arguments: vec![],
        fixture_directory: std::env::temp_dir(),
        limits: lab_windows_job::Limits {
            process_commit: 32 * 1024 * 1024,
            job_commit: 32 * 1024 * 1024,
        },
    }
}
fn machine() -> Worker {
    Worker::new(
        Arc::new(Signal {
            state: Mutex::new(Shared {
                latest: Status::new(),
                pending: None,
            }),
            changed: Condvar::new(),
            owner_gone: AtomicBool::new(false),
        }),
        Policy::default(),
    )
}
fn ready(worker: &mut Worker, backend: &mut Fake) {
    worker.start(backend, launch());
    worker.tick(backend); // Issue challenge one.
    backend.sample = Some(Progress {
        incarnation: 1,
        challenge: 1,
        owner_counter: 1,
        healthy_close: false,
    });
    worker.tick(backend);
    assert_eq!(worker.status.phase, Phase::Ready);
}
#[test]
fn termination_success_timeout_retains_one_episode_then_real_proof_retires() {
    let mut worker = machine();
    let mut backend = Fake::default();
    ready(&mut worker, &mut backend);
    backend.hidden = true;
    worker.begin_stop(&mut backend, Reason::ExplicitTermination, true);
    assert_eq!(worker.status.phase, Phase::TerminationRequested);
    assert!(backend.exited && backend.process);
    worker.terminated = Instant::now() - Duration::from_secs(3);
    worker.tick(&mut backend);
    assert_eq!(worker.status.phase, Phase::ExitUnconfirmed);
    for _ in 0..10 {
        worker.start(&mut backend, launch());
        worker.begin_stop(&mut backend, Reason::ExplicitTermination, true);
        worker.tick(&mut backend);
    }
    assert_eq!(
        (backend.prepares, backend.resumes, backend.terminations),
        (1, 1, 1)
    );
    assert_eq!(worker.status.objects.processes, 1);
    backend.hidden = false;
    worker.tick(&mut backend);
    assert_eq!(worker.status.phase, Phase::ExitedContained);
    assert_eq!(worker.status.exit_code, Some(77));
    assert_eq!(worker.status.objects, lab_windows_job::Objects::default());
    assert_eq!(
        worker.status.last_final.unwrap().reason,
        Some(Reason::ExplicitTermination)
    );
    worker.start(&mut backend, launch());
    assert_eq!(worker.status.incarnation, 2);
    assert_eq!(worker.status.last_final.unwrap().incarnation, 1);
}
#[test]
fn startup_error_is_preserved_independently_of_cleanup_error() {
    let original = lab_windows_job::Error {
        operation: "membership gate",
        code: 5,
    };
    let cleanup = lab_windows_job::Error {
        operation: "termination",
        code: 6,
    };
    let mut worker = machine();
    let mut backend = Fake {
        prepare_error: Some(original),
        terminate_error: Some(cleanup),
        ..Default::default()
    };
    worker.start(&mut backend, launch());
    assert_eq!(worker.status.phase, Phase::ExitUnconfirmed);
    assert_eq!(worker.status.original_error, Some(original));
    assert_eq!(worker.status.cleanup_error, Some(cleanup));
    assert_eq!(backend.resumes, 0);
    worker.tick(&mut backend);
    assert_eq!(backend.terminations, 1);
    assert!(backend.process); // No receipt and no replacement.
}

#[test]
fn unavailable_job_attributes_and_limit_queries_fail_without_child_or_fallback() {
    for (operation, code) in [
        ("CreateJobObjectW", 5),
        ("SetInformationJobObject", 5),
        ("QueryInformationJobObject", 5),
        ("UpdateProcThreadAttribute JOB_LIST", 50),
        ("CreateProcessW nested Job", 1816),
    ] {
        let error = lab_windows_job::Error { operation, code };
        let mut worker = machine();
        let mut backend = Fake {
            before_create: true,
            prepare_error: Some(error),
            ..Default::default()
        };
        worker.start(&mut backend, launch());
        assert_eq!(worker.status.phase, Phase::Stopped);
        assert_eq!(worker.status.original_error, Some(error));
        assert_eq!(worker.status.objects, lab_windows_job::Objects::default());
        assert_eq!(
            (backend.prepares, backend.resumes, backend.terminations),
            (1, 0, 0)
        );
        assert_eq!(
            worker.status.last_final.unwrap().original_error,
            Some(error)
        );
    }
}
#[test]
fn wait_failure_and_retirement_failure_keep_obligations_without_implicit_retries() {
    let mut worker = machine();
    let mut backend = Fake::default();
    ready(&mut worker, &mut backend);
    backend.wait_error = Some(lab_windows_job::Error {
        operation: "WaitForSingleObject",
        code: 6,
    });
    worker.tick(&mut backend);
    assert_eq!(worker.status.phase, Phase::ExitUnconfirmed);
    assert_eq!(worker.status.alive, None);
    assert_eq!(worker.status.original_error, backend.wait_error);
    worker.start(&mut backend, launch());
    assert_eq!(backend.prepares, 1);
    assert_eq!(backend.terminations, 0);
    backend.wait_error = None;
    backend.exited = true;
    backend.retire_error = Some(lab_windows_job::Error {
        operation: "CloseHandle",
        code: 6,
    });
    worker.tick(&mut backend);
    assert_eq!(worker.status.phase, Phase::ExitUnconfirmed);
    assert!(worker.retire_failed);
    assert_eq!(worker.status.objects.processes, 1);
    for _ in 0..4 {
        worker.tick(&mut backend);
    }
    assert_eq!(backend.prepares, 1);
    assert_eq!(backend.terminations, 0);
    assert!(backend.process); // Unresolved release is not a fabricated zero ledger.
}
#[test]
fn helper_pong_and_duplicate_ack_never_fabricate_owner_progress() {
    let mut worker = machine();
    let mut backend = Fake::default();
    ready(&mut worker, &mut backend);
    worker.sequence = 2;
    worker.last_progress = Instant::now() - Duration::from_secs(3);
    backend.sample = Some(Progress {
        incarnation: 1,
        challenge: 2,
        owner_counter: 1,
        healthy_close: false,
    });
    worker.tick(&mut backend);
    assert_eq!(worker.status.phase, Phase::Stopping);
    assert_eq!(worker.status.reason, Some(Reason::OwnerProgressDeadline));
    assert!(worker.status.heartbeat_fresh && !worker.status.owner_progress_fresh);
    assert_eq!(worker.status.owner_counter, 1);
    assert_eq!(backend.closes, 1);
    let mut worker = machine();
    let mut backend = Fake::default();
    ready(&mut worker, &mut backend);
    worker.last_heartbeat = Instant::now() - Duration::from_secs(3);
    worker.last_progress = Instant::now();
    worker.tick(&mut backend); // Same retained ack does not refresh heartbeat.
    assert_eq!(worker.status.reason, Some(Reason::HeartbeatDeadline));
}
#[test]
fn wrong_incarnation_and_unadvanced_close_receipt_are_not_healthy() {
    let mut worker = machine();
    let mut backend = Fake::default();
    ready(&mut worker, &mut backend);
    worker.accept_progress(Progress {
        incarnation: 99,
        challenge: 1,
        owner_counter: 99,
        healthy_close: true,
    });
    assert_eq!(worker.status.owner_counter, 1);
    worker.begin_stop(&mut backend, Reason::ExplicitStop, false);
    worker.accept_progress(Progress {
        incarnation: 1,
        challenge: 1,
        owner_counter: 1,
        healthy_close: true,
    });
    assert!(!worker.healthy_close);
    backend.exited = true;
    worker.tick(&mut backend);
    assert_eq!(worker.status.phase, Phase::ExitedContained);
}
#[test]
fn pending_stop_supersedes_restart_and_drop_finishes_single_worker() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let mut root = Supervisor::spawn(
        {
            let entered = entered.clone();
            let release = release.clone();
            move || {
                entered.wait();
                release.wait();
                Fake::default()
            }
        },
        Policy::default(),
    )
    .unwrap();
    entered.wait();
    root.restart(launch()).unwrap();
    assert_eq!(root.restart(launch()), Err(Rejected::Busy));
    root.stop(Stop::Terminate).unwrap();
    assert_eq!(root.restart(launch()), Err(Rejected::StopPending));
    root.stop(Stop::Graceful).unwrap();
    assert!(matches!(
        root.signal.state.lock().unwrap().pending,
        Some(Request::Stop(Stop::Terminate))
    ));
    release.wait();
    root.signal.owner_gone.store(true, Ordering::Release);
    root.signal.changed.notify_all();
    root.wait_status(|s| s.worker_finished);
    root.join.take().unwrap().join().unwrap();
    assert!(root.worker_finished());
    assert_eq!(root.status().unwrap().incarnation, 0);
}
#[test]
fn status_and_requests_never_wait_on_mutex_or_stalled_os_worker() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let mut root = Supervisor::spawn(
        {
            let entered = entered.clone();
            let release = release.clone();
            move || {
                entered.wait();
                release.wait();
                Fake::default()
            }
        },
        Policy::default(),
    )
    .unwrap();
    entered.wait();
    let guard = root.signal.state.lock().unwrap();
    assert_eq!(root.status().unwrap_err(), Rejected::Busy);
    assert_eq!(root.stop(Stop::Graceful), Err(Rejected::Busy));
    assert_eq!(root.restart(launch()), Err(Rejected::Busy));
    drop(guard);
    root.signal.state.lock().unwrap().latest.worker_progress =
        Instant::now() - Duration::from_secs(3);
    assert!(root.status().unwrap().degraded);
    release.wait();
    root.signal.owner_gone.store(true, Ordering::Release);
    root.signal.changed.notify_all();
    root.wait_status(|s| s.worker_finished);
    root.join.take().unwrap().join().unwrap();
}

#[test]
fn in_flight_handle_retirement_never_publishes_terminal_or_admits_replacement() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let mut root = Supervisor::spawn(
        {
            let entered = entered.clone();
            let release = release.clone();
            move || Fake {
                auto_progress: true,
                exit_on_close: true,
                retire_entered: Some(entered),
                retire_release: Some(release),
                ..Default::default()
            }
        },
        Policy::default(),
    )
    .unwrap();
    root.restart(launch()).unwrap();
    root.wait_status(|s| s.phase == Phase::Ready);
    root.stop(Stop::Graceful).unwrap();
    entered.wait(); // Process proof exists, but the retirement call is stalled.
    let status = root.status().unwrap();
    assert_eq!(status.phase, Phase::Stopping);
    assert_eq!(status.call, Some(Call::Retire));
    assert_eq!(status.alive, Some(false));
    assert_eq!(status.objects.processes, 1);
    assert!(status.last_final.is_none());
    assert_eq!(root.restart(launch()), Err(Rejected::Active));
    release.wait();
    let ended = root.wait_status(|s| s.phase == Phase::ExitedContained);
    assert_eq!(ended.objects, lab_windows_job::Objects::default());
    assert_eq!(ended.last_final.unwrap().exit_code, Some(77));
    root.signal.owner_gone.store(true, Ordering::Release);
    root.signal.changed.notify_all();
    root.wait_status(|s| s.worker_finished);
    root.join.take().unwrap().join().unwrap();
}
#[test]
fn launch_validation_rejects_unbounded_tokens_and_zero_caps() {
    let mut candidate = launch();
    candidate.arguments = vec!["x".repeat(1025)];
    assert!(candidate.validate().is_err());
    candidate.arguments.clear();
    candidate.limits.process_commit = 0;
    assert!(candidate.validate().is_err());
    candidate.limits.process_commit = 1;
    candidate.executable = PathBuf::from("relative.exe");
    assert!(candidate.validate().is_err());
    let mut root = Supervisor::windows(Policy::default()).unwrap();
    assert_eq!(root.restart(candidate), Err(Rejected::InvalidLaunch));
    root.signal.owner_gone.store(true, Ordering::Release);
    root.signal.changed.notify_all();
    root.wait_status(|s| s.worker_finished);
    root.join.take().unwrap().join().unwrap();
}
#[test]
fn architecture_guard_preserves_authority_and_workbench_unsafe_prohibition() {
    println!(
        "supervisor metadata: status={} shared_slot_and_request={} worker={} launch={} objects={} policy={}",
        std::mem::size_of::<Status>(),
        std::mem::size_of::<Shared>(),
        std::mem::size_of::<Worker>(),
        std::mem::size_of::<Launch>(),
        std::mem::size_of::<lab_windows_job::Objects>(),
        std::mem::size_of::<Policy>()
    );
    let supervisor = include_str!("mod.rs");
    let backend = include_str!("windows.rs");
    for source in [supervisor, backend] {
        for forbidden in [
            "ClientHandle",
            "WorkbenchDispatcher",
            "RuntimeObservations",
            "egui::",
            "eframe::",
            "FrameBackend",
            "TextureOwner",
            "HostSession",
            "mpsc::channel(",
            "NamedPipe",
            "AssignProcessToJobObject",
        ] {
            assert!(
                !source.contains(forbidden),
                "supervisor authority/IPC dependency: {forbidden}"
            );
        }
        assert!(!source.contains("unsafe {"));
    }
    assert!(include_str!("../../../../Cargo.toml").contains("unsafe_code = \"forbid\""));
    let manifest = include_str!("../../Cargo.toml");
    assert!(manifest.contains("[lints]") && manifest.contains("workspace = true"));
}

#[test]
fn unexpected_inert_factory_exit_is_unconfirmed_and_never_replaced() {
    let mut root = Supervisor::spawn(
        || -> Fake {
            panic!("injected inert factory panic");
        },
        Policy::default(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !root.worker_finished() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    let status = root.status().unwrap();
    assert_eq!(status.phase, Phase::ExitUnconfirmed);
    assert_eq!(status.reason, Some(Reason::WorkerPanic));
    assert!(status.degraded);
    assert_eq!(root.restart(launch()), Err(Rejected::ExitUnconfirmed));
    assert!(root.join.take().unwrap().join().is_err());
}

//! Real Windows no-GPU fixtures. Exit witnesses originate from CreateProcessW.
use super::*;
use std::{
    path::{Path, PathBuf},
    sync::{Barrier, atomic::AtomicU64},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const CAP: usize = 32 * 1024 * 1024; // FIXTURE only, not deployment C.
fn directory(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/windows-supervisor-tests")
        .join(format!(
            "{}-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    std::fs::create_dir(&dir).unwrap(); // Fresh admission; never reuse old markers.
    dir
}
fn executable() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("LAB_WINDOWS_SUPERVISOR_FIXTURE")
            .expect("run scripts/test-windows-supervisor.ps1"),
    );
    assert!(path.is_file());
    path
}
fn plan(mode: &str) -> Launch {
    let dir = directory(mode);
    Launch {
        executable: executable(),
        arguments: vec!["--loop".into(), mode.into(), dir.to_str().unwrap().into()],
        fixture_directory: dir,
        limits: lab_windows_job::Limits {
            process_commit: CAP,
            job_commit: CAP,
        },
    }
}
fn wait_marker(path: &Path) -> String {
    use std::io::Read;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(file) = std::fs::File::open(path) {
            let mut text = String::with_capacity(129);
            file.take(129).read_to_string(&mut text).unwrap();
            assert!(text.len() <= 128);
            if text.ends_with('\n') {
                return text;
            }
        }
        assert!(
            Instant::now() < deadline,
            "marker observation deadline: {path:?}"
        );
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
fn wait_witness(witness: &lab_windows_job::ProcessWitness) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(code) = witness.poll_exit().unwrap() {
            return code;
        }
        assert!(
            Instant::now() < deadline,
            "original fixture exit not confirmed"
        );
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
fn finish(root: &mut Supervisor) {
    root.signal.owner_gone.store(true, Ordering::Release);
    root.signal.changed.notify_all();
    root.wait_status(|s| s.worker_finished);
    root.join.take().unwrap().join().unwrap();
}
fn stop(root: &Supervisor, mode: Stop) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match root.stop(mode) {
            Ok(()) => return,
            Err(Rejected::Busy) if Instant::now() < deadline => std::thread::yield_now(),
            e => panic!("bounded explicit stop admission: {e:?}"),
        }
    }
}

#[test]
#[ignore = "Windows no-GPU native supervisor; run scripts/test-windows-supervisor.ps1"]
fn atomic_job_membership_limits_suspension_and_last_handle_close() {
    let launch = plan("hang");
    let mut job = lab_windows_job::Job::new(launch.limits).unwrap();
    let args: Vec<_> = launch.arguments.iter().map(String::as_str).collect();
    job.prepare(&launch.executable, &args).unwrap();
    assert_eq!(
        job.objects(),
        lab_windows_job::Objects {
            jobs: 1,
            processes: 1,
            threads: 1
        }
    );
    job.verify_membership().unwrap();
    let reading = job.verify_limits().unwrap();
    assert!(reading.peak_process_commit <= CAP && reading.peak_job_commit <= CAP);
    assert!(!launch.fixture_directory.join("executed").exists());
    let witness = job.witness().unwrap();
    assert!(witness.poll_exit().unwrap().is_none());
    assert!(job.prepare(&launch.executable, &args).is_err());
    job.resume().unwrap();
    assert_eq!(
        wait_marker(&launch.fixture_directory.join("executed")),
        "owner entered\n"
    );
    drop(job); // KILL_ON_JOB_CLOSE is containment, never an exit receipt.
    let code = wait_witness(&witness);
    println!(
        "atomic before-execution gates PASS; second record rejected; last-handle kill observed exit={code}"
    );
}

#[test]
#[ignore = "Windows no-GPU native supervisor; run scripts/test-windows-supervisor.ps1"]
fn normal_close_explicit_restart_and_termination_keep_one_record() {
    let mut root = Supervisor::windows(Policy::default()).unwrap();
    for incarnation in 1..=3 {
        root.restart(plan("normal")).unwrap();
        let ready = root.wait_status(|s| s.phase == Phase::Ready);
        assert_eq!(ready.incarnation, incarnation);
        assert_eq!(ready.alive, Some(true));
        assert_eq!(
            ready.objects,
            lab_windows_job::Objects {
                jobs: 1,
                processes: 1,
                threads: 0
            }
        );
        assert_eq!(root.restart(plan("normal")), Err(Rejected::Active));
        let mode = if incarnation == 2 {
            Stop::Terminate
        } else {
            Stop::Graceful
        };
        stop(&root, mode);
        stop(&root, mode);
        let ended =
            root.wait_status(|s| matches!(s.phase, Phase::ExitedHealthy | Phase::ExitedContained));
        assert_eq!(ended.objects, lab_windows_job::Objects::default());
        assert_eq!(ended.alive, Some(false));
        assert_eq!(
            ended.termination_requests,
            usize::from(mode == Stop::Terminate)
        );
        assert_eq!(
            ended.phase,
            if mode == Stop::Terminate {
                Phase::ExitedContained
            } else {
                Phase::ExitedHealthy
            }
        );
        assert_eq!(
            ended.exit_code,
            Some(if mode == Stop::Terminate { 77 } else { 0 })
        );
        assert_eq!(ended.last_final.unwrap().incarnation, incarnation);
        println!(
            "cycle={incarnation} state={:?} exit={:?} terminations={} process_commit_peak={} job_commit_peak={} objects={:?}",
            ended.phase,
            ended.exit_code,
            ended.termination_requests,
            ended.reading.peak_process_commit,
            ended.reading.peak_job_commit,
            ended.objects
        );
    }
    finish(&mut root);
}

#[test]
#[ignore = "Windows no-GPU native supervisor; run scripts/test-windows-supervisor.ps1"]
fn crash_hang_helper_pong_and_commit_denial_are_contained() {
    for mode in ["crash", "hang", "helper", "commit", "descendant"] {
        let launch = plan(mode);
        let dir = launch.fixture_directory.clone();
        let mut root = Supervisor::windows(Policy::default()).unwrap();
        root.restart(launch).unwrap();
        if mode == "helper" {
            let stale = root.wait_status(|s| {
                s.phase == Phase::Stopping
                    && s.reason == Some(Reason::OwnerProgressDeadline)
                    && s.heartbeat_fresh
                    && !s.owner_progress_fresh
            });
            assert_eq!(stale.alive, Some(true));
            assert_eq!(stale.owner_counter, 1);
        }
        let ended = root.wait_status(|s| s.phase == Phase::ExitedContained);
        assert_eq!(ended.objects, lab_windows_job::Objects::default());
        assert_eq!(ended.alive, Some(false));
        if mode == "commit" {
            assert_eq!(ended.exit_code, Some(90));
            assert_eq!(wait_marker(&dir.join("memory")), "denied=1455\n");
        }
        if mode == "descendant" {
            assert_eq!(ended.exit_code, Some(91));
            assert_eq!(wait_marker(&dir.join("descendant-denied")), "1816\n");
        }
        println!(
            "mode={mode} state={:?} reason={:?} exit={:?} process_commit_peak={} job_commit_peak={} objects={:?}",
            ended.phase,
            ended.reason,
            ended.exit_code,
            ended.reading.peak_process_commit,
            ended.reading.peak_job_commit,
            ended.objects
        );
        finish(&mut root);
    }
}

#[derive(Default)]
struct Controlled {
    inner: WindowsBackend,
    entered: Option<Arc<Barrier>>,
    release: Option<Arc<Barrier>>,
    witness: Arc<Mutex<Option<lab_windows_job::ProcessWitness>>>,
    hide_exit: Arc<AtomicBool>,
    injected_gate: bool,
    poll_failure: Arc<AtomicBool>,
    terminations: Arc<AtomicU64>,
    retire_gate: Option<(Arc<Barrier>, Arc<Barrier>)>,
}
impl Backend for Controlled {
    fn prepare(&mut self, launch: &Launch, inc: u64) -> Result<(), lab_windows_job::Error> {
        self.inner.prepare(launch, inc)?;
        *self.witness.lock().unwrap() = Some(self.inner.job.as_ref().unwrap().witness()?);
        if let Some(entered) = &self.entered {
            entered.wait();
        }
        if let Some(release) = &self.release {
            release.wait();
        }
        if self.injected_gate {
            Err(lab_windows_job::Error {
                operation: "injected post-create gate",
                code: 5,
            })
        } else {
            Ok(())
        }
    }
    fn resume(&mut self) -> Result<(), lab_windows_job::Error> {
        self.inner.resume()
    }
    fn has_process(&self) -> bool {
        self.inner.has_process()
    }
    fn poll_exit(&mut self) -> Result<Option<u32>, lab_windows_job::Error> {
        if self.poll_failure.load(Ordering::Acquire) {
            Err(lab_windows_job::Error {
                operation: "injected original-handle observation failure",
                code: 6,
            })
        } else if self.hide_exit.load(Ordering::Acquire) {
            Ok(None)
        } else {
            self.inner.poll_exit()
        }
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
        self.terminations.fetch_add(1, Ordering::SeqCst);
        self.inner.terminate(r)
    }
    fn retire(&mut self) -> Result<lab_windows_job::Reading, lab_windows_job::Error> {
        if self.inner.has_process()
            && let Some((entered, release)) = self.retire_gate.take()
        {
            entered.wait();
            release.wait();
        }
        self.inner.retire()
    }
    fn objects(&self) -> lab_windows_job::Objects {
        self.inner.objects()
    }
}

#[test]
#[ignore = "Windows no-GPU native supervisor; run scripts/test-windows-supervisor.ps1"]
fn startup_failure_and_cancel_before_resume_never_execute_child() {
    let mut root = Supervisor::windows(Policy::default()).unwrap();
    let mut launch = plan("normal");
    launch.executable = launch.fixture_directory.join("missing-fixture.exe");
    root.restart(launch).unwrap();
    let failure = root.wait_status(|s| s.phase == Phase::Stopped && s.incarnation == 1);
    assert_eq!(failure.original_error.unwrap().operation, "CreateProcessW");
    assert_eq!(failure.objects, lab_windows_job::Objects::default());
    finish(&mut root);
    for variant in 0..3 {
        let injected_gate = variant == 1;
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let witness = Arc::new(Mutex::new(None));
        let mut root = Supervisor::spawn(
            {
                let entered = entered.clone();
                let release = release.clone();
                let witness = witness.clone();
                move || Controlled {
                    inner: WindowsBackend::default(),
                    entered: Some(entered),
                    release: Some(release),
                    witness,
                    hide_exit: Arc::new(AtomicBool::new(false)),
                    injected_gate,
                    ..Default::default()
                }
            },
            Policy::default(),
        )
        .unwrap();
        let launch = plan("normal");
        let dir = launch.fixture_directory.clone();
        root.restart(launch).unwrap();
        entered.wait(); // Exact process exists, gated and still suspended.
        assert!(!dir.join("executed").exists());
        if variant == 0 {
            stop(&root, Stop::Graceful);
        }
        if variant == 2 {
            let guard = root.signal.state.lock().unwrap();
            let (guard, elapsed) = root
                .signal
                .changed
                .wait_timeout_while(guard, Duration::from_millis(2100), |_| true)
                .unwrap();
            assert!(elapsed.timed_out());
            drop(guard);
            assert!(root.status().unwrap().degraded);
        }
        release.wait();
        let ended = root.wait_status(|s| s.phase == Phase::ExitedContained);
        assert_eq!(ended.termination_requests, 1);
        assert_eq!(ended.objects, lab_windows_job::Objects::default());
        if variant == 2 {
            assert_eq!(ended.reason, Some(Reason::StartupDeadline));
        }
        assert!(!dir.join("executed").exists());
        assert_eq!(wait_witness(witness.lock().unwrap().as_ref().unwrap()), 77);
        println!(
            "suspended startup rollback variant={variant} gate_injected={injected_gate} exit confirmed; executed=false"
        );
        finish(&mut root);
    }
}

#[test]
#[ignore = "Windows no-GPU native supervisor; run scripts/test-windows-supervisor.ps1"]
fn suppressed_exit_proof_retains_same_job_and_blocks_replacement() {
    let witness = Arc::new(Mutex::new(None));
    let hide_exit = Arc::new(AtomicBool::new(false));
    let mut root = Supervisor::spawn(
        {
            let witness = witness.clone();
            let hide_exit = hide_exit.clone();
            move || Controlled {
                inner: WindowsBackend::default(),
                entered: None,
                release: None,
                witness,
                hide_exit,
                injected_gate: false,
                ..Default::default()
            }
        },
        Policy::default(),
    )
    .unwrap();
    root.restart(plan("normal")).unwrap();
    root.wait_status(|s| s.phase == Phase::Ready);
    hide_exit.store(true, Ordering::Release);
    stop(&root, Stop::Terminate);
    let pending = root.wait_status(|s| s.phase == Phase::ExitUnconfirmed);
    assert_eq!(pending.incarnation, 1);
    assert_eq!(pending.termination_requests, 1);
    assert_eq!(pending.objects.jobs, 1);
    assert_eq!(pending.objects.processes, 1);
    assert_eq!(root.restart(plan("normal")), Err(Rejected::ExitUnconfirmed));
    for _ in 0..4 {
        stop(&root, Stop::Terminate);
    }
    assert_eq!(wait_witness(witness.lock().unwrap().as_ref().unwrap()), 77);
    // OS exit is REAL and externally confirmed: only supervisor proof is withheld.
    assert_eq!(root.status().unwrap().termination_requests, 1);
    hide_exit.store(false, Ordering::Release);
    root.signal.changed.notify_all();
    let ended = root.wait_status(|s| s.phase == Phase::ExitedContained);
    assert_eq!(ended.objects, lab_windows_job::Objects::default());
    assert_eq!(ended.incarnation, 1);
    println!(
        "simulated exit-proof withholding PASS; same incarnation/Job retained; actual guardian exit=77; later original proof retires"
    );
    finish(&mut root);
}

#[test]
#[ignore = "Windows no-GPU native supervisor; run scripts/test-windows-supervisor.ps1"]
fn poll_failure_then_first_terminate_or_root_shutdown_uses_original_job_and_witness() {
    for shutdown in [false, true] {
        let witness = Arc::new(Mutex::new(None));
        let poll_failure = Arc::new(AtomicBool::new(false));
        let terminations = Arc::new(AtomicU64::new(0));
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let mut root = Supervisor::spawn(
            {
                let witness = witness.clone();
                let poll_failure = poll_failure.clone();
                let terminations = terminations.clone();
                let entered = entered.clone();
                let release = release.clone();
                move || Controlled {
                    witness,
                    poll_failure,
                    terminations,
                    retire_gate: Some((entered, release)),
                    ..Default::default()
                }
            },
            Policy::default(),
        )
        .unwrap();
        root.restart(plan("normal")).unwrap();
        root.wait_status(|s| s.phase == Phase::Ready);
        assert_eq!(
            witness
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .poll_exit()
                .unwrap(),
            None
        );
        poll_failure.store(true, Ordering::Release);
        let failed = root.wait_status(|s| s.phase == Phase::ExitUnconfirmed && s.call.is_none());
        assert_eq!(failed.termination_requests, 0);
        assert_eq!(
            failed.original_error.unwrap().operation,
            "injected original-handle observation failure"
        );
        assert_eq!(root.restart(plan("normal")), Err(Rejected::ExitUnconfirmed));
        if shutdown {
            root.shutdown();
        } else {
            stop(&root, Stop::Terminate);
        }
        let pending = root.wait_status(|s| s.termination_requests == 1 && s.call.is_none());
        assert_eq!(pending.phase, Phase::ExitUnconfirmed);
        assert_eq!(pending.original_error, failed.original_error);
        assert_eq!(pending.cleanup_error, None);
        assert_eq!((pending.objects.jobs, pending.objects.processes), (1, 1));
        assert!(!pending.worker_finished && pending.last_final.is_none());
        assert_eq!(wait_witness(witness.lock().unwrap().as_ref().unwrap()), 77);
        for _ in 0..3 {
            let repeated = super::tests::settled_stop(&root, Stop::Terminate);
            assert_eq!(repeated.phase, Phase::ExitUnconfirmed);
            assert_eq!(repeated.termination_requests, 1);
        }
        assert_eq!(terminations.load(Ordering::SeqCst), 1);
        root.shutdown();
        assert!(!root.worker_finished());
        poll_failure.store(false, Ordering::Release); // Same original process handle, never PID lookup.
        root.signal.changed.notify_all();
        entered.wait();
        let retiring = root.status().unwrap();
        assert_eq!(retiring.phase, Phase::Stopping);
        assert_eq!(retiring.call, Some(Call::Retire));
        assert_eq!(retiring.alive, Some(false));
        assert!(!retiring.worker_finished && !root.worker_finished());
        release.wait();
        let ended = root.wait_status(|s| s.worker_finished);
        assert_eq!(ended.phase, Phase::ExitedContained);
        assert_eq!(ended.incarnation, 1);
        assert_eq!(ended.objects, lab_windows_job::Objects::default());
        assert_eq!(
            ended.last_final.unwrap().original_error,
            failed.original_error
        );
        root.join.take().unwrap().join().unwrap();
        println!(
            "poll failure native root={shutdown}: Ready -> ExitUnconfirmed; one real Job terminate, independent original witness exit=77; original proof + gated retirement -> ExitedContained/worker finished"
        );
    }
}

#[test]
#[ignore = "Windows no-GPU native supervisor; run scripts/test-windows-supervisor.ps1"]
fn parent_death_closes_inner_job_and_incompatible_nested_job_fails_closed() {
    // External guard owns two fixtures and never passes its handle to either.
    let limits = lab_windows_job::Limits {
        process_commit: 64 * 1024 * 1024,
        job_commit: 128 * 1024 * 1024,
    };
    let mut guard = lab_windows_job::fixture_guard(limits).unwrap();
    let dir = directory("parent-death");
    let exe = executable();
    guard
        .prepare(&exe, &["--owner", dir.to_str().unwrap()])
        .unwrap();
    let parent = guard.witness().unwrap();
    guard.resume().unwrap();
    assert_eq!(wait_marker(&dir.join("owner-ready")), "ready\n");
    let identity = wait_marker(&dir.join("identity"));
    let ids: Vec<_> = identity.split_whitespace().collect();
    let child = guard
        .fixture_duplicate_child(ids[1].parse().unwrap(), ids[0].parse().unwrap())
        .unwrap();
    assert!(parent.poll_exit().unwrap().is_none() && child.poll_exit().unwrap().is_none());
    guard.rollback(88).unwrap(); // Kill only the fixture parent; inner child isn't directly targeted.
    assert_eq!(wait_witness(&parent), 88);
    let child_exit = wait_witness(&child);
    guard.retire().unwrap();
    println!(
        "forced parent death PASS; original child witness exit={child_exit}; inner last-handle containment"
    );
    // One-active-process outer Job cannot admit its parent's inner child.
    let mut outer = lab_windows_job::Job::new(limits).unwrap();
    let dir = directory("nested-reject");
    outer
        .prepare(&exe, &["--owner", dir.to_str().unwrap()])
        .unwrap();
    let parent = outer.witness().unwrap();
    outer.resume().unwrap();
    assert_ne!(wait_witness(&parent), 0);
    assert!(!dir.join("executed").exists());
    outer.retire().unwrap();
    println!(
        "incompatible inherited active-count Job: child creation rejected; no uncontrolled fixture execution"
    );
}

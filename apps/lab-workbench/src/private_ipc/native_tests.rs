//! Native no-GPU fixtures use the real supervisor worker, Job and original exit
//! witness. State predicates and completion receipts establish test ordering.
use super::{backend::Bridge, protocol::*};
use crate::{
    presentation::v2,
    supervisor::{Backend, Launch, Phase, Policy, Progress, Stop, Supervisor},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn document() -> v2::AdmittedV2Document {
    v2::admit(br#"{"format_version":2,"binding_version":1,"document_id":"ipc","windows":[],"components":[],"data_bindings":[],"editor_bindings":[],"action_bindings":[],"plots":[]}"#).unwrap()
}

fn finish(bridge: &Bridge, root: &Supervisor) {
    root.shutdown();
    root.wait_status(|s| s.worker_finished);
    until(|| root.worker_finished());
    let status = sampled_status(bridge);
    assert!(status.retired);
    assert_eq!(status.pending_io, 0);
    assert_eq!(status.handles, 0);
    assert_eq!(status.io_bytes, 0);
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_maximum_document_and_selected_observations_reassemble_and_apply() {
    let bridge = Bridge::new([11; 16]);
    let doc = super::tests::maximum_document();
    let epoch = Epoch {
        owner: [12; 16],
        revision: 1,
    };
    assert_eq!(doc.canonical_bytes().len(), SNAPSHOT);
    bridge.document(&doc, epoch, 1).unwrap();
    let (reads, references) = super::tests::maximum_reads();
    bridge
        .observations(&reads, &super::tests::selected(&references), epoch, 2, 1)
        .unwrap();
    let root = Supervisor::spawn(bridge.worker_factory().unwrap(), Policy::default()).unwrap();
    until(|| root.restart(launch("normal")).is_ok());
    root.wait_status(|s| s.phase == Phase::Ready);
    until(|| bridge.status().is_ok_and(|s| s.applied == [1, 2]));
    let status = sampled_status(&bridge);
    assert_eq!(status.handles, 6);
    assert_eq!(status.io_bytes, 2 * CONTROL + 2 * CHUNK);
    assert_eq!(status.credits.iter().sum::<usize>(), 4);
    println!(
        "MAX canonical={} selected_points={} selected_encoded={} status={status:?}",
        SNAPSHOT,
        POINTS,
        super::reads::measure(&reads, &super::tests::selected(&references)).unwrap()
    );
    finish(&bridge, &root);
}

#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_snapshot_bounds_epoch_fragment_order_and_malformed_content_fail_closed() {
    use std::sync::atomic::AtomicU8;
    for fault_code in 1..=5 {
        let bridge = Bridge::new([22; 16]);
        let epoch = Epoch {
            owner: [7; 16],
            revision: 1,
        };
        bridge.document(&document(), epoch, 1).unwrap();
        let fault = Arc::new(AtomicU8::new(0));
        let factory = bridge.worker_factory().unwrap();
        let root = Supervisor::spawn(
            {
                let fault = fault.clone();
                move || {
                    let mut backend = factory();
                    backend.inject_snapshot_fault(fault);
                    backend
                }
            },
            Policy::default(),
        )
        .unwrap();
        until(|| root.restart(launch("normal")).is_ok());
        root.wait_status(|s| s.phase == Phase::Ready);
        assert_eq!(sampled_status(&bridge).applied[0], 1);
        fault.store(fault_code, Ordering::Release);
        until(|| {
            bridge
                .document(
                    &document(),
                    Epoch {
                        revision: 2,
                        ..epoch
                    },
                    2,
                )
                .is_ok()
        });
        let terminal = root.wait_status(|s| s.phase == Phase::ExitedContained);
        assert_eq!(terminal.exit_code, Some(91));
        assert_eq!(sampled_status(&bridge).applied[0], 1);
        println!(
            "SNAPSHOT_FAULT code={fault_code} previous_applied=1 candidate_unapplied=true objects={:?}",
            terminal.objects
        );
        until(|| root.restart(launch("normal")).is_ok());
        root.wait_status(|s| s.phase == Phase::Ready && s.incarnation == 2);
        assert_eq!(sampled_status(&bridge).applied[0], 2);
        finish(&bridge, &root);
    }
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_source_revision_freshness_and_numeric_rejection_preserve_latest_approved_reads() {
    let (bridge, root) = start("normal");
    root.wait_status(|s| s.phase == Phase::Ready);
    let epoch = Epoch {
        owner: [7; 16],
        revision: 1,
    };
    let (mut observations, references) = super::tests::maximum_reads();
    until(|| {
        bridge
            .observations(
                &observations,
                &super::tests::selected(&references),
                epoch,
                2,
                1,
            )
            .is_ok()
    });
    until(|| bridge.status().is_ok_and(|s| s.applied[1] == 2));
    let mut stale = super::tests::selected(&references);
    for selection in &mut stale {
        selection.origin.as_mut().unwrap().revision = Some(19);
    }
    assert_eq!(
        bridge.observations(&observations, &stale, epoch, 3, 2),
        Err("stale source/revision/freshness")
    );
    let wrong_boot: Vec<_> = references
        .iter()
        .map(|target| super::reads::Selection {
            target,
            origin: Some(super::reads::Origin {
                boot: "unapproved-boot",
                projection: Some("frozen-projection"),
                revision: Some(20),
                trace_from: Some(0.),
            }),
        })
        .collect();
    assert_eq!(
        bridge.observations(&observations, &wrong_boot, epoch, 3, 1),
        Err("unapproved source identity change")
    );
    observations
        .live
        .get_mut(&references[0])
        .unwrap()
        .push(crate::model::LivePoint {
            time_seconds: 1000.,
            value: 1.,
        })
        .unwrap();
    assert!(
        bridge
            .observations(
                &observations,
                &super::tests::selected(&references),
                epoch,
                3,
                2
            )
            .is_err()
    );
    let missing: Vec<_> = references
        .iter()
        .map(|target| super::reads::Selection {
            target,
            origin: None,
        })
        .collect();
    assert!(
        bridge
            .observations(&observations, &missing, epoch, 3, 2)
            .is_err()
    );
    assert_eq!(sampled_status(&bridge).applied[1], 2);
    stop(&root, Stop::Terminate);
    root.wait_status(|s| s.phase == Phase::ExitedContained);
    until(|| root.restart(launch("normal")).is_ok());
    root.wait_status(|s| s.phase == Phase::Ready && s.incarnation == 2);
    until(|| bridge.status().is_ok_and(|s| s.applied[1] == 2));
    println!(
        "SOURCE_GATE old_revision_boot_and_max_points_remain_approved=true restored_snapshot=2"
    );
    finish(&bridge, &root);
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_control_fence_invalidates_a_read_while_snapshot_reader_is_stopped() {
    let (bridge, root) = start("slow-after-read");
    root.wait_status(|s| s.phase == Phase::Ready);
    let epoch = Epoch {
        owner: [7; 16],
        revision: 1,
    };
    let (reads, references) = super::tests::maximum_reads();
    until(|| {
        bridge
            .observations(&reads, &super::tests::selected(&references), epoch, 2, 1)
            .is_ok()
    });
    until(|| {
        bridge
            .status()
            .is_ok_and(|s| s.applied[1] == 2 && s.child_read_current)
    });
    let before = root.wait_status(|_| true).owner_counter;
    bridge.announce(1, 2);
    until(|| {
        bridge
            .status()
            .is_ok_and(|s| s.applied[1] == 2 && !s.child_read_current)
    });
    root.wait_status(|s| s.owner_counter > before && s.phase == Phase::Ready);
    assert!(bridge.status().is_ok_and(|s| !s.child_read_current));
    println!(
        "FENCE_READ applied=2 current_before=true current_after=false owner_progress_continued=true"
    );
    finish(&bridge, &root);
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_snapshot_io_failure_keeps_control_close_responsive() {
    let (bridge, root) = start("snapshot-close");
    root.wait_status(|s| s.phase == Phase::Ready);
    let (reads, references) = super::tests::maximum_reads();
    until(|| {
        bridge
            .observations(
                &reads,
                &super::tests::selected(&references),
                Epoch {
                    owner: [7; 16],
                    revision: 1,
                },
                2,
                1,
            )
            .is_ok()
    });
    let terminal = root.wait_status(|s| s.phase == Phase::ExitedHealthy);
    assert_eq!(terminal.exit_code, Some(0));
    assert_eq!(terminal.termination_requests, 0);
    assert!(terminal.original_error.is_some());
    println!(
        "SNAPSHOT_IO_FAILURE control_close_observed=true original_error={:?} termination_requests=0",
        terminal.original_error
    );
    finish(&bridge, &root);
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_slow_snapshot_reader_coalesces_while_control_owner_progress_advances() {
    let (bridge, root) = start("slow");
    let ready = root.wait_status(|s| s.phase == Phase::Ready);
    let epoch = Epoch {
        owner: [7; 16],
        revision: 1,
    };
    let (reads, references) = super::tests::maximum_reads();
    until(|| {
        bridge
            .observations(&reads, &super::tests::selected(&references), epoch, 2, 1)
            .is_ok()
    });
    until(|| {
        bridge
            .status()
            .is_ok_and(|s| s.pending_io >= 2 && s.credits[3] == 1)
    });
    for snapshot in 3..103 {
        until(|| {
            bridge
                .observations(
                    &reads,
                    &super::tests::selected(&references),
                    epoch,
                    snapshot,
                    snapshot,
                )
                .is_ok()
        });
    }
    let progressed =
        root.wait_status(|s| s.phase == Phase::Ready && s.owner_counter > ready.owner_counter);
    let status = sampled_status(&bridge);
    assert!(progressed.heartbeat_fresh && progressed.owner_progress_fresh);
    assert_eq!(status.applied[1], 0);
    assert!(status.coalesced >= 99);
    assert!(status.credits[1] <= 2 && status.credits.iter().sum::<usize>() == 4);
    println!(
        "SLOW before_owner={} after_owner={} status={status:?}",
        ready.owner_counter, progressed.owner_counter
    );
    finish(&bridge, &root);
}

struct Controlled {
    inner: super::backend::IpcBackend,
    witness: Arc<Mutex<Option<lab_windows_job::ProcessWitness>>>,
    prepared: Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>,
    prepared_objects: Option<Arc<Mutex<lab_windows_job::Objects>>>,
}
impl Backend for Controlled {
    fn prepare(
        &mut self,
        launch: &Launch,
        incarnation: u64,
    ) -> std::result::Result<(), lab_windows_job::Error> {
        self.inner.prepare(launch, incarnation)?;
        *self.witness.lock().unwrap() = Some(self.inner.witness()?);
        if let Some(objects) = &self.prepared_objects {
            *objects.lock().unwrap() = self.inner.objects();
        }
        if let Some((entered, release)) = self.prepared.take() {
            entered.wait();
            release.wait();
        }
        Ok(())
    }
    fn service(&mut self) -> std::result::Result<(), lab_windows_job::Error> {
        self.inner.service()
    }
    fn resume(&mut self) -> std::result::Result<(), lab_windows_job::Error> {
        self.inner.resume()
    }
    fn has_process(&self) -> bool {
        self.inner.has_process()
    }
    fn poll_exit(&mut self) -> std::result::Result<Option<u32>, lab_windows_job::Error> {
        self.inner.poll_exit()
    }
    fn challenge(
        &mut self,
        incarnation: u64,
        sequence: u64,
    ) -> std::result::Result<(), lab_windows_job::Error> {
        self.inner.challenge(incarnation, sequence)
    }
    fn progress(&mut self) -> std::result::Result<Option<Progress>, lab_windows_job::Error> {
        self.inner.progress()
    }
    fn close(
        &mut self,
        incarnation: u64,
        sequence: u64,
    ) -> std::result::Result<(), lab_windows_job::Error> {
        self.inner.close(incarnation, sequence)
    }
    fn terminate(&mut self, rollback: bool) -> std::result::Result<(), lab_windows_job::Error> {
        self.inner.terminate(rollback)
    }
    fn retire(&mut self) -> std::result::Result<lab_windows_job::Reading, lab_windows_job::Error> {
        self.inner.retire()
    }
    fn retirement_pending(&self) -> bool {
        self.inner.retirement_pending()
    }
    fn objects(&self) -> lab_windows_job::Objects {
        self.inner.objects()
    }
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_original_process_exit_does_not_retire_unobserved_canceled_io() {
    for shutdown in [false, true] {
        let bridge = Bridge::new([13; 16]);
        let gate = Arc::new(AtomicBool::new(false));
        let io_gate = Arc::new(AtomicBool::new(true));
        let witness = Arc::new(Mutex::new(None));
        let factory = bridge.worker_factory().unwrap();
        let root = Supervisor::spawn(
            {
                let gate = gate.clone();
                let io_gate = io_gate.clone();
                let witness = witness.clone();
                move || {
                    let mut inner = factory();
                    inner.hold_retirement(gate);
                    inner.hold_io_observation(io_gate);
                    Controlled {
                        inner,
                        witness,
                        prepared: None,
                        prepared_objects: None,
                    }
                }
            },
            Policy::default(),
        )
        .unwrap();
        bridge
            .document(
                &document(),
                Epoch {
                    owner: [7; 16],
                    revision: 1,
                },
                1,
            )
            .unwrap();
        until(|| root.restart(launch("slow")).is_ok());
        root.wait_status(|s| s.phase == Phase::Ready);
        let (reads, references) = super::tests::maximum_reads();
        until(|| {
            bridge
                .observations(
                    &reads,
                    &super::tests::selected(&references),
                    Epoch {
                        owner: [7; 16],
                        revision: 1,
                    },
                    2,
                    1,
                )
                .is_ok()
        });
        until(|| bridge.status().is_ok_and(|s| s.pending_io >= 2));
        io_gate.store(false, Ordering::Release);
        until(|| {
            bridge
                .status()
                .is_ok_and(|s| s.io_held && s.pending_io >= 2)
        });
        if shutdown {
            root.shutdown();
        } else {
            stop(&root, Stop::Terminate);
        }
        let retained = root.wait_status(|s| {
            s.phase == Phase::ExitUnconfirmed && s.alive == Some(false) && s.call.is_none()
        });
        assert!(
            witness
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .poll_exit()
                .unwrap()
                .is_some()
        );
        assert_eq!((retained.objects.jobs, retained.objects.processes), (1, 1));
        assert_eq!(retained.cleanup_error, None);
        let status = sampled_status(&bridge);
        assert!(status.retiring && !status.retired);
        assert!(status.pending_io >= 1);
        assert_eq!(status.handles, 6);
        assert!(!root.worker_finished());
        assert!(root.restart(launch("normal")).is_err());
        assert!(bridge.worker_factory().is_err());
        println!("RETAINED shutdown={shutdown} process_proved=true status={status:?}");
        io_gate.store(true, Ordering::Release);
        gate.store(true, Ordering::Release);
        root.wait_status(|s| matches!(s.phase, Phase::ExitedHealthy | Phase::ExitedContained));
        until(|| {
            bridge
                .status()
                .is_ok_and(|s| s.retired && s.handles == 0 && s.pending_io == 0)
        });
        if !shutdown {
            until(|| root.restart(launch("normal")).is_ok());
            root.wait_status(|s| s.phase == Phase::Ready && s.incarnation == 2);
            until(|| bridge.status().is_ok_and(|s| s.applied == [1, 2]));
        }
        finish(&bridge, &root);
    }
}

#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_peer_identity_rejects_foreign_process_before_any_snapshot_or_ready() {
    let identity = [15; 16];
    let bridge = Bridge::new(identity);
    bridge
        .document(
            &document(),
            Epoch {
                owner: [7; 16],
                revision: 1,
            },
            1,
        )
        .unwrap();
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let witness = Arc::new(Mutex::new(None));
    let objects = Arc::new(Mutex::new(lab_windows_job::Objects::default()));
    let factory = bridge.worker_factory().unwrap();
    let root = Supervisor::spawn(
        {
            let entered = entered.clone();
            let release = release.clone();
            let witness = witness.clone();
            let objects = objects.clone();
            move || Controlled {
                inner: factory(),
                witness,
                prepared: Some((entered, release)),
                prepared_objects: Some(objects),
            }
        },
        Policy::default(),
    )
    .unwrap();
    until(|| root.restart(launch("normal")).is_ok());
    entered.wait();
    let suspended = root.status().unwrap();
    assert_eq!(suspended.phase, Phase::Starting);
    assert_eq!(
        *objects.lock().unwrap(),
        lab_windows_job::Objects {
            jobs: 1,
            processes: 1,
            threads: 1
        }
    );
    assert!(!sampled_status(&bridge).handshaken);
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
    let id: String = identity.iter().map(|b| format!("{b:02x}")).collect();
    let name = format!(
        r"\\.\pipe\lab-workbench-{}-{id}-1-control",
        std::process::id()
    );
    let mut foreign = lab_windows_job::pipe::Pipe::<CONTROL>::client(&name).unwrap();
    release.wait();
    root.wait_status(|s| {
        s.original_error
            .is_some_and(|e| e.operation == "wrong pipe peer")
    });
    assert_eq!(sampled_status(&bridge).applied, [0, 0]);
    until(|| foreign.retire().unwrap());
    root.wait_status(|s| s.phase == Phase::ExitedContained);
    finish(&bridge, &root);
}

fn control_fault(mode: &str, error: &str) {
    let (bridge, root) = start(mode);
    if !error.is_empty() {
        root.wait_status(|s| s.original_error.is_some_and(|e| e.operation == error));
    }
    let terminal = root.wait_status(|s| s.phase == Phase::ExitedContained && s.call.is_none());
    assert_eq!(terminal.alive, Some(false));
    assert_eq!(terminal.objects, Default::default());
    assert_eq!(sampled_status(&bridge).applied, [0, 0]);
    println!(
        "CONTROL_FAULT mode={mode} original={:?} exit={:?}",
        terminal.original_error, terminal.exit_code
    );
    finish(&bridge, &root);
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_incarnation_and_server_process_fences() {
    control_fault("wrong-incarnation", "wrong private identity/incarnation");
    control_fault("wrong-server", "");
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_closed_versions_variants_and_declared_lengths_are_rejected() {
    for (mode, error) in [
        ("version", "unsupported private version"),
        ("variant", "unknown private variant"),
        ("oversize", "private frame bound/sequence"),
        ("length", "private frame bound/sequence"),
    ] {
        control_fault(mode, error);
    }
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_duplicate_and_out_of_order_transport_sequence_rejects_channel() {
    for mode in ["duplicate", "order"] {
        control_fault(mode, "duplicate/out-of-order private sequence");
    }
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_truncated_control_and_snapshot_preserve_unapplied_parent_document() {
    control_fault("truncated", "");
    let bridge = Bridge::new([16; 16]);
    bridge
        .document(
            &super::tests::maximum_document(),
            Epoch {
                owner: [7; 16],
                revision: 1,
            },
            1,
        )
        .unwrap();
    let root = Supervisor::spawn(bridge.worker_factory().unwrap(), Policy::default()).unwrap();
    until(|| root.restart(launch("truncate-snapshot")).is_ok());
    let terminal = root.wait_status(|s| s.phase == Phase::ExitedContained && s.call.is_none());
    assert_eq!(terminal.exit_code, Some(74));
    assert_eq!(sampled_status(&bridge).applied, [0, 0]);
    until(|| root.restart(launch("normal")).is_ok());
    root.wait_status(|s| s.phase == Phase::Ready && s.incarnation == 2);
    assert_eq!(sampled_status(&bridge).applied[0], 1);
    finish(&bridge, &root);
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_short_io_submissions_have_original_completion_receipts_and_exact_reassembly() {
    use super::transport::Framed;
    use lab_windows_job::pipe::Pipe;
    let identity = Identity {
        workbench: [21; 16],
        incarnation: 1,
    };
    let name = format!(r"\\.\pipe\lab-workbench-short-{}", std::process::id());
    let mut server = Framed::<CONTROL>::new(Pipe::server(&name).unwrap());
    let mut client = Framed::<CONTROL>::new(Pipe::client(&name).unwrap());
    until(|| server.pipe.connected(std::process::id()).unwrap());
    assert!(client.pipe.connected(std::process::id()).unwrap());
    server.pipe.fixture_write_quantum(17).unwrap();
    let mut header = Header::control(Kind::Diagnostic, identity, 1);
    header.bytes = CONTROL;
    let payload = vec![19; CONTROL - HEADER];
    server.send(header, &payload).unwrap();
    let mut received = false;
    let mut complete = false;
    let mut polls = 0;
    until(|| {
        polls += 1;
        complete |= server.poll_send().unwrap();
        if !received
            && let Some((actual, bytes)) = client.receive(identity, Instant::now()).unwrap()
        {
            assert_eq!(actual, header);
            assert_eq!(bytes, payload);
            received = true;
        }
        complete && received
    });
    assert!(polls >= CONTROL / 17);
    assert_eq!(server.pipe.pending(), 0);
    until(|| server.pipe.retire().unwrap());
    until(|| client.pipe.retire().unwrap());
    println!("SHORT_IO bytes={CONTROL} quantum=17 real_completion_polls={polls}");
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_child_hang_and_crash_keep_one_episode_and_require_explicit_restart() {
    for mode in ["hang", "crash"] {
        let (bridge, root) = start(mode);
        let terminal = root.wait_status(|s| s.phase == Phase::ExitedContained && s.call.is_none());
        assert_eq!(terminal.incarnation, 1);
        assert_eq!(terminal.alive, Some(false));
        assert_eq!(terminal.objects, Default::default());
        if mode == "hang" {
            assert_eq!(terminal.termination_requests, 1);
        } else {
            assert_eq!(terminal.exit_code, Some(73));
        }
        let status = sampled_status(&bridge);
        assert!(status.retired && status.handles == 0);
        assert!(!root.worker_finished());
        println!(
            "FAULT mode={mode} terminal={:?} commit={:?}",
            terminal.phase, terminal.reading
        );
        until(|| root.restart(launch("normal")).is_ok());
        root.wait_status(|s| s.phase == Phase::Ready && s.incarnation == 2);
        finish(&bridge, &root);
    }
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_control_flood_has_fixed_diagnostics_and_bounded_scheduled_turns() {
    let (bridge, root) = start("flood");
    let rejected = root.wait_status(|s| {
        s.original_error
            .is_some_and(|e| e.operation == "private control flood")
    });
    assert_ne!(rejected.phase, Phase::Ready);
    let records_at_rejection = sampled_status(&bridge).diagnostic_records;
    let terminal = root.wait_status(|s| s.phase == Phase::ExitedContained);
    let status = sampled_status(&bridge);
    assert!(status.received_frames <= status.turns * OWNER_MESSAGES as u64);
    assert_eq!(
        status.diagnostic_records, records_at_rejection,
        "rejected channel cannot accept later records"
    );
    assert_eq!(terminal.alive, Some(false));
    assert_eq!(status.credits.iter().sum::<usize>(), 4);
    println!("FLOOD status={status:?} ring_capacity={DIAGNOSTICS}");
    finish(&bridge, &root);
}
pub(crate) fn launch(mode: &str) -> Launch {
    Launch {
        executable: PathBuf::from(
            std::env::var_os("LAB_PRIVATE_IPC_FIXTURE").expect("run scripts/test-private-ipc.ps1"),
        ),
        arguments: vec![mode.into()],
        limits: lab_windows_job::Limits {
            process_commit: 128 * 1024 * 1024,
            job_commit: 128 * 1024 * 1024,
        },
        fixture_directory: std::env::current_dir().unwrap(),
    }
}

#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_parent_pump_api_identity_and_recovery_progress_with_hung_child() {
    crate::parent::tests::assert_native_private_ipc_continuity("hang");
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_fault_and_snapshot_restoration_create_zero_runtime_submissions() {
    crate::parent::tests::assert_native_private_ipc_continuity("slow");
}
#[test]
#[ignore = "Windows private IPC no-GPU handoff; scripts/test-private-ipc.ps1"]
fn native_pending_handoff_keeps_parent_api_recovery_and_zero_replay() {
    crate::parent::tests::assert_native_private_ipc_continuity("handoff");
}
#[test]
#[ignore = "Windows private IPC no-GPU handoff; scripts/test-private-ipc.ps1"]
fn native_crash_during_pending_handoff_preserves_parent_and_restarts_current_snapshots() {
    crate::parent::tests::assert_native_private_ipc_continuity("handoff-crash");
}
#[test]
#[ignore = "Windows private IPC no-GPU handoff; scripts/test-private-ipc.ps1"]
fn native_complete_handoff_survives_incomplete_deadline_and_retires_original_ipc() {
    let (bridge, root) = start("handoff");
    root.wait_status(|s| s.phase == Phase::Ready);
    until(|| {
        bridge
            .document(
                &document(),
                Epoch {
                    owner: [7; 16],
                    revision: 2,
                },
                2,
            )
            .is_ok()
    });
    until(|| bridge.status().is_ok_and(|s| s.diagnostic_records >= 1));
    let before = root.wait_status(|s| s.phase == Phase::Ready).owner_counter;
    let start = Instant::now();
    until(|| {
        start.elapsed() > Duration::from_millis(2200)
            && root
                .status()
                .is_ok_and(|s| s.phase == Phase::Ready && s.owner_counter > before + 4)
    });
    assert_eq!(sampled_status(&bridge).applied[0], 1);
    assert_eq!(sampled_status(&bridge).credits.iter().sum::<usize>(), SLOTS);
    stop(&root, Stop::Graceful);
    let exit = root.wait_status(|s| {
        matches!(s.phase, Phase::ExitedHealthy | Phase::ExitedContained) && s.call.is_none()
    });
    assert_eq!(exit.alive, Some(false));
    let retired = sampled_status(&bridge);
    assert!(retired.retired);
    assert_eq!(
        (retired.pending_io, retired.handles, retired.io_bytes),
        (0, 0, 0)
    );
    until(|| root.restart(launch("normal")).is_ok());
    root.wait_status(|s| s.phase == Phase::Ready && s.incarnation == 2);
    until(|| bridge.status().is_ok_and(|s| s.applied[0] == 2));
    println!(
        "HANDOFF_NATIVE complete_wait_ms={} heartbeat_owner_progress=true applied_before_retirement=1 restart_applied=2 original_ipc_baseline=true",
        start.elapsed().as_millis()
    );
    finish(&bridge, &root);
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_parent_root_shutdown_waits_for_ipc_receipts_and_endpoint_retirement() {
    crate::parent::tests::assert_native_private_ipc_shutdown();
}
fn until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !predicate() {
        assert!(Instant::now() < deadline, "native state deadline");
        std::thread::yield_now();
    }
}
fn sampled_status(bridge: &Bridge) -> super::backend::Status {
    let mut sample = None;
    until(|| {
        sample = bridge.status().ok();
        sample.is_some()
    });
    sample.unwrap()
}
fn stop(root: &Supervisor, mode: Stop) {
    until(|| root.stop(mode).is_ok());
}
fn start(mode: &str) -> (Bridge, Supervisor) {
    let mut identity = [3; 16];
    identity[..4].copy_from_slice(&std::process::id().to_le_bytes());
    let bridge = Bridge::new(identity);
    bridge
        .document(
            &document(),
            Epoch {
                owner: [7; 16],
                revision: 1,
            },
            1,
        )
        .unwrap();
    let supervisor =
        Supervisor::spawn(bridge.worker_factory().unwrap(), Policy::default()).unwrap();
    until(|| supervisor.restart(launch(mode)).is_ok());
    (bridge, supervisor)
}
#[test]
#[ignore = "Windows private IPC no-GPU; scripts/test-private-ipc.ps1"]
fn native_two_pipes_admission_close_and_explicit_incarnation() {
    let (bridge, supervisor) = start("normal");
    let ready = supervisor.wait_status(|s| s.phase == Phase::Ready && s.call.is_none());
    assert_eq!(ready.objects.jobs, 1);
    assert_eq!(ready.objects.processes, 1);
    let status = sampled_status(&bridge);
    assert!(status.connected && status.handshaken);
    assert_eq!(status.applied[0], 1);
    assert!(supervisor.restart(launch("normal")).is_err());
    stop(&supervisor, Stop::Graceful);
    let exited = supervisor.wait_status(|s| {
        matches!(s.phase, Phase::ExitedHealthy | Phase::ExitedContained) && s.call.is_none()
    });
    assert_eq!(exited.alive, Some(false));
    assert_eq!(exited.objects, Default::default());
    until(|| {
        bridge
            .status()
            .is_ok_and(|s| s.retired && s.pending_io == 0)
    });
    println!(
        "first_exit={:?} peak_commit={:?} ipc={:?}",
        exited.phase,
        exited.reading,
        sampled_status(&bridge)
    );
    until(|| supervisor.restart(launch("normal")).is_ok());
    let ready = supervisor.wait_status(|s| s.phase == Phase::Ready && s.incarnation == 2);
    assert_eq!(ready.objects.jobs, 1);
    supervisor.shutdown();
    supervisor.wait_status(|s| s.worker_finished);
    until(|| supervisor.worker_finished());
    assert!(sampled_status(&bridge).retired);
}

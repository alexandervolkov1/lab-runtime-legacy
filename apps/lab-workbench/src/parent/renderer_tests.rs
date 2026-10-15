//! Actual child pixels from the single parent presentation/model owner. Runtime
//! input is an explicitly virtual, bounded Application-worker fixture; the child
//! receives no Runtime events, discovery, client or mutation API.
#[path = "renderer_qualification.rs"]
mod qualification;
use super::*;
use crate::{
    private_ipc::parent_bridge::ParentBridge,
    supervisor::{Launch, Phase, Policy, Stop},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const TEST_ONLY_COMMIT_CAP: usize = 1024 * 1024 * 1024;
fn reading(bridge: &crate::private_ipc::TestBridge, milestone: &str) -> Value {
    let status = bridge.status().unwrap();
    json!({"milestone":milestone,"cumulative_peak_process_commit":status.sampled_commit.map(|r|r.peak_process_commit),"cumulative_peak_job_commit":status.sampled_commit.map(|r|r.peak_job_commit),"ipc_connected":status.connected,"ipc_io_bytes":status.io_bytes,"credits":status.credits,"owner_diagnostic":format!("{:?}",status.renderer)})
}
fn target() -> RuntimeRef {
    RuntimeRef::Signal {
        instrument: "instrument".into(),
        parameter: "s0".into(),
    }
}
fn document(title: &str) -> PresentationDocument {
    serde_json::from_value(json!({"format_version":1,"document_id":"isolated-parent-document","controls":[],
        "windows":[{"id":"window","title":title,"tabs":[{"id":"tab","title":"Read-only","panels":[
            {"id":"scalar-panel","title":"Parent selected scalar","kind":{"kind":"status","source":target()}},
            {"id":"plot-panel","title":"Parent selected Plot","kind":{"kind":"plot","plot_id":"plot"}}
        ]}]}],
        "plots":[{"id":"plot","title":title,"time_window_seconds":100.,"axes":{"y_min":-10.,"y_max":10.},"traces":[
            {"id":"trace","source":target(),"display_label":"Original LRD1 samples","visible":true,"style":{"color":"cyan","width":3.},"display_unit":null}
        ]}]})).unwrap()
}
fn qualification_cap() -> usize {
    let qualification_cap = std::env::var("LAB_RENDERER_QUALIFICATION_COMMIT_CAP")
        .map(|value| {
            value
                .parse::<usize>()
                .expect("explicit qualification commit cap")
        })
        .unwrap_or(TEST_ONLY_COMMIT_CAP);
    assert!((16 * 1024 * 1024..=TEST_ONLY_COMMIT_CAP).contains(&qualification_cap));
    qualification_cap
}
fn launch(mode: &str) -> Launch {
    let qualification_cap = qualification_cap();
    Launch {
        executable: PathBuf::from(
            std::env::var_os("LAB_ISOLATED_RENDERER_CHILD")
                .expect("run scripts/test-isolated-renderer.ps1"),
        ),
        arguments: vec![mode.into()],
        limits: lab_windows_job::Limits {
            process_commit: qualification_cap,
            job_commit: qualification_cap,
        },
        fixture_directory: std::env::current_dir().unwrap(),
    }
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "isolated renderer predicate deadline"
        );
        thread::sleep(Duration::from_millis(2));
    }
}
fn find_frame(directory: &Path, mut predicate: impl FnMut(&Value) -> bool) -> (PathBuf, Value) {
    let mut found = None;
    wait(|| {
        for index in 1..=24 {
            let path = directory.join(format!("frame-{index:02}.json"));
            if let Ok(bytes) = fs::read(&path)
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && predicate(&value)
            {
                found = Some((path, value));
                return true;
            }
        }
        if let Ok(error) = fs::read_to_string(directory.join("failure.json")) {
            panic!("child failure: {error}");
        }
        false
    });
    found.unwrap()
}
fn pixels(path: &Path, value: &Value) -> Vec<u8> {
    let bytes = fs::read(path.with_extension("rgba")).unwrap();
    assert_eq!(bytes.len(), 1280 * 960 * 4);
    assert_eq!(value["width"], 1280);
    assert_eq!(value["height"], 960);
    assert_eq!(value["scale"], 1);
    let cyan = |x: usize, y: usize| {
        let i = ((959 - y) * 1280 + x) * 4;
        bytes[i] < 40 && bytes[i + 1] > 150 && bytes[i + 2] > 170
    };
    for point in value["curve_samples"].as_array().unwrap() {
        let x = point[0].as_f64().unwrap().round() as usize;
        let y = point[1].as_f64().unwrap().round() as usize;
        assert!(
            (y.saturating_sub(2)..=(y + 2).min(959))
                .any(|yy| (x.saturating_sub(2)..=(x + 2).min(1279)).any(|xx| cyan(xx, yy))),
            "actual GL sample missing at {point}"
        );
    }
    assert_eq!(
        value["matched_samples"].as_u64().unwrap() as usize,
        value["curve_samples"].as_array().unwrap().len()
    );
    bytes
}
fn sample(tx: &SyncSender<ClientUpdate>, signal: &PumpSignal, index: u64, value: f64) {
    send_update(
        tx,
        signal,
        ClientUpdate::Event {
            cursor: EventCursor {
                boot_id: "boot".into(),
                seq: 6 + index,
            },
            envelope: json!({"kind":"signal","target":{"instrument":"instrument","parameter":"s0"},"data":{
                "signal":{"instrument":"instrument","parameter":"s0"},"generation":"1","revision":(6+index).to_string(),"quality":"good","status":"available","value":value,"unit":{"id":"volt","symbol":"V"},"observed_at_ns":((index+1)*10_000_000).to_string(),"source_at_ns":((index+1)*10_000_000).to_string()
            }}),
        },
    );
}

#[test]
#[ignore = "Windows actual isolated GL child; scripts/test-isolated-renderer.ps1"]
fn native_first_isolated_renderer_frames_from_parent_approved_snapshots() {
    let (mut owner, tx, rx, stops) = fixture();
    let signal = PumpSignal::default();
    let mut adapter = ParentBridge::new(&owner.dispatcher, vec![target()]).unwrap();
    adapter.turn(&owner.dispatcher);
    let bridge = adapter.bridge.clone();
    owner.private_bridge = Some(adapter);
    let mut stream = open_api(&mut owner, &signal);
    let mut pump = ParentPump::spawn(owner, signal.clone()).unwrap();
    pump.install_supervisor(bridge.worker_factory().unwrap(), Policy::default())
        .unwrap();
    request(&mut stream, "hello", "hello", json!({}));
    response(&mut stream, "hello");
    request(&mut stream, "before", "presentation_get", json!({}));
    let before = response(&mut stream, "before");
    let identity = before["result"]["workbench_id"].clone();
    request(
        &mut stream,
        "document",
        "ui_replace_document",
        json!({"expected":{"workbench_id":identity,"revision":"1"},"document":document("First isolated frame μ Ω")}),
    );
    assert_eq!(
        response(&mut stream, "document")["result"]["presentation_revision"],
        "2"
    );
    // The accepted Job launcher fixes the child cwd to its executable directory.
    let directory = launch("normal")
        .executable
        .parent()
        .unwrap()
        .to_path_buf()
        .join("target/isolated-native-renderer")
        .join(format!(
            "{}-{}-1",
            std::process::id(),
            identity.as_str().unwrap()
        ));
    wait(|| pump.supervisor().unwrap().restart(launch("normal")).is_ok());
    let mut startup = None;
    let mut memory = Vec::new();
    wait(|| {
        if memory.is_empty() && directory.join("startup.json").is_file() {
            memory.push(reading(&bridge, "child_startup_before_ready"));
        }
        if let Ok(status) = pump.supervisor().unwrap().status()
            && matches!(
                status.phase,
                Phase::Ready | Phase::ExitedContained | Phase::ExitedHealthy
            )
        {
            startup = Some(status);
            return true;
        }
        false
    });
    let startup = startup.unwrap();
    memory.push(reading(&bridge, "native_window_context_ready"));
    if startup.phase != Phase::Ready {
        let error = fs::read_to_string(directory.join("startup-error.json")).unwrap_or_default();
        let profile = fs::read_to_string(directory.join("native-profile.json")).unwrap_or_default();
        println!(
            "NATIVE_STARTUP_BLOCKED evidence={} error={error} profile={profile} status={startup:?}",
            directory.display()
        );
        request(
            &mut stream,
            "blocked-presentation",
            "presentation_get",
            json!({}),
        );
        let current = response(&mut stream, "blocked-presentation");
        assert_eq!(current["result"]["workbench_id"], identity);
        assert_eq!(current["result"]["presentation_revision"], "2");
        request(&mut stream, "blocked-status", "client_status", json!({}));
        assert!(
            response(&mut stream, "blocked-status")
                .get("result")
                .is_some()
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(stops.load(Ordering::SeqCst), 0);
        let retired = bridge.status().unwrap();
        assert!(retired.retired);
        assert_eq!(retired.pending_io, 0);
        assert_eq!(retired.handles, 0);
        assert_eq!(retired.credits.iter().sum::<usize>(), 4);
        fs::write(directory.join("parent-startup-blocked.json"),serde_json::to_vec_pretty(&json!({
            "first_isolated_real_gl_frame":"BLOCKED/no frame", "error":error,"profile":serde_json::from_str::<Value>(&profile).unwrap(),
            "parent_api_available":true,"workbench_id":identity,"presentation_revision":"2","extra_runtime_submissions":0,
            "original_process_exit_confirmed":startup.alive==Some(false),"exit_code":startup.exit_code,
            "original_ipc_retired":retired.retired,"ipc_handles":retired.handles,"pending_io":retired.pending_io,"encoded_slots":4,
            "test_only_job_commit_cap":qualification_cap(),"deployment_C":null,
            "startup_peak_process_commit":startup.reading.peak_process_commit,"startup_peak_job_commit":startup.reading.peak_job_commit,
            "renderer_healthy_lifecycle":"UNQUALIFIED/not reached"
        })).unwrap()).unwrap();
        // Do not panic while holding the supervisor's status mutex. Preserve its
        // original process/IPC retirement and observe the parent worker join.
        pump.shutdown().unwrap();
        assert_eq!(stops.load(Ordering::SeqCst), 1);
        panic!("native startup did not reach Ready: {error}");
    }
    let (empty_path, empty) = find_frame(&directory, |v| {
        v["plot"] == true && v["observation_snapshot"] != 0 && v["plot_points"] == 0
    });
    let empty_pixels = pixels(&empty_path, &empty);
    memory.push(reading(&bridge, "first_preview_no_data_readback"));
    assert_eq!(empty["cyan_pixels"], 0);
    assert!(empty["curve_samples"].as_array().unwrap().is_empty());
    bootstrap_records(
        &tx,
        &rx,
        &signal,
        json!([{"kind":"signal","id":{"instrument":"instrument","parameter":"s0"},"generation":"1","value_type":"float","unit":{"id":"volt","symbol":"V"}}]),
    );
    send_update(
        &tx,
        &signal,
        ClientUpdate::RecoveryProjection {
            active: vec![record(KnownAdmission::Accepted)],
            quarantined: vec![],
        },
    );
    signal.wait_updates(7);
    for i in 0..64 {
        sample(&tx, &signal, i, ((i as f64) * 0.15).sin() * 4.);
        signal.wait_updates(8 + i as usize);
    }
    let (scalar_path, scalar) = find_frame(&directory, |v| {
        v["plot"] == false
            && v["current"] == true
            && v["labels"]
                .as_str()
                .is_some_and(|s| s.contains("Runtime unit: V") && s.contains("Fresh\n"))
    });
    pixels(&scalar_path, &scalar);
    memory.push(reading(&bridge, "selected_scalar_readback"));
    let (plot_path, plot) = find_frame(&directory, |v| {
        v["plot"] == true && v["current"] == true && v["plot_points"] == 64
    });
    let plot_pixels = pixels(&plot_path, &plot);
    memory.push(reading(&bridge, "selected_plot_readback"));
    assert!(
        empty_pixels != plot_pixels,
        "selected trace did not change actual GL pixels"
    );
    assert!(plot["cyan_pixels"].as_u64().unwrap() > 100);
    assert!(!plot["curve_samples"].as_array().unwrap().is_empty());
    sample(&tx, &signal, 64, 8.);
    signal.wait_updates(72);
    let (live_path, live) = find_frame(&directory, |v| {
        v["plot"] == true && v["current"] == true && v["plot_points"] == 65
    });
    let live_pixels = pixels(&live_path, &live);
    memory.push(reading(&bridge, "live_snapshot_replacement_readback"));
    assert!(
        plot_pixels != live_pixels,
        "new observations did not change actual GL pixels"
    );
    let mut replacement_document = document("Replacement after checked retirement");
    replacement_document.plots[0].axes.y_max = Some(20.0);
    request(
        &mut stream,
        "replace",
        "ui_replace_document",
        json!({"expected":{"workbench_id":identity,"revision":"2"},"document":replacement_document}),
    );
    assert_eq!(
        response(&mut stream, "replace")["result"]["presentation_revision"],
        "3"
    );
    let (replacement_path, replacement) = find_frame(&directory, |v| {
        v["plot"] == true && v["epoch_revision"] == 3 && v["plot_points"] == 65
    });
    let replacement_pixels = pixels(&replacement_path, &replacement);
    memory.push(reading(&bridge, "document_replacement_readback"));
    wait(|| {
        bridge.status().is_ok_and(|s| {
            s.renderer.is_some_and(|r| {
                r.event_loop > 0
                    && r.render_owner > 0
                    && r.completed > 0
                    && r.submitted >= r.completed
                    && r.released_generations > 0
            })
        })
    });
    assert!(
        live_pixels != replacement_pixels,
        "new document axes did not change actual GL pixels"
    );
    request(
        &mut stream,
        "recovery",
        "recovery_get",
        json!({"kind":"active","index":"0","expected":null}),
    );
    assert_eq!(
        response(&mut stream, "recovery")["result"]["record"]["admission"],
        "accepted"
    );
    request(&mut stream, "status", "client_status", json!({}));
    assert_eq!(
        response(&mut stream, "status")["result"]["runtime_client"]["freshness"],
        "fresh"
    );
    assert!(
        rx.try_recv().is_err(),
        "display created a Runtime query/mutation/status/Exact Retry"
    );
    assert_eq!(stops.load(Ordering::SeqCst), 0);
    wait(|| pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
    let terminal = pump
        .supervisor()
        .unwrap()
        .wait_status(|s| matches!(s.phase, Phase::ExitedHealthy | Phase::ExitedContained));
    assert_eq!(terminal.phase, Phase::ExitedHealthy, "{terminal:?}");
    assert_eq!(terminal.exit_code, Some(0));
    let retired: Value =
        serde_json::from_slice(&fs::read(directory.join("own-retired.json")).unwrap()).unwrap();
    assert_eq!(retired["own_objects"], 0);
    let ipc = bridge.status().unwrap();
    assert!(ipc.retired);
    assert_eq!(ipc.handles, 0);
    assert_eq!(ipc.pending_io, 0);
    assert_eq!(ipc.credits.iter().sum::<usize>(), 4);
    memory.push(json!({"milestone":"normal_shutdown_original_process_exit","cumulative_peak_process_commit":terminal.reading.peak_process_commit,"cumulative_peak_job_commit":terminal.reading.peak_job_commit}));
    fs::write(directory.join("memory-milestones.json"),serde_json::to_vec_pretty(&json!({"kind":"sampled_cumulative_OS_Job_peaks_not_additive_phase_allocations","samples":memory})).unwrap()).unwrap();
    fs::write(directory.join("parent-result.json"),serde_json::to_vec_pretty(&json!({"first_isolated_real_gl_frame":"PASS","test_only_job_commit_cap":qualification_cap(),"deployment_C":null,
        "peak_process_commit":terminal.reading.peak_process_commit,"peak_job_commit":terminal.reading.peak_job_commit,"workbench_id":identity,"presentation_revision":"3",
        "runtime_extra_query_mutation_status_retry":0,"final_phase":format!("{:?}",terminal.phase),"own_objects":0,"original_ipc_retired":true})).unwrap()).unwrap();
    println!(
        "FIRST_ISOLATED_REAL_GL_FRAME PASS evidence={} peak_process_commit={} peak_job_commit={} deployment_C=unselected extra_runtime_submissions=0",
        directory.display(),
        terminal.reading.peak_process_commit,
        terminal.reading.peak_job_commit
    );
    pump.shutdown().unwrap();
    assert_eq!(stops.load(Ordering::SeqCst), 1);
}

/// A single parent and original Job/IPC episode. No test opens a Runtime socket
/// in the renderer or bypasses supervisor startup/retirement.
struct FaultHarness {
    pump: ParentPump<FakeClient>,
    bridge: crate::private_ipc::TestBridge,
    stream: BufReader<TcpStream>,
    tx: SyncSender<ClientUpdate>,
    rx: Receiver<Sent>,
    signal: PumpSignal,
    stops: Arc<AtomicUsize>,
    identity: Value,
}
impl FaultHarness {
    fn new(mode: &str, retirement_gate: Option<Arc<std::sync::atomic::AtomicBool>>) -> Self {
        Self::profile(
            mode,
            retirement_gate,
            document("Isolated fault fixture μ Ω"),
            vec![target()],
        )
    }
    fn profile(
        mode: &str,
        retirement_gate: Option<Arc<std::sync::atomic::AtomicBool>>,
        document: PresentationDocument,
        references: Vec<RuntimeRef>,
    ) -> Self {
        let (mut owner, tx, rx, stops) = fixture();
        let signal = PumpSignal::default();
        let mut adapter = ParentBridge::new(&owner.dispatcher, references).unwrap();
        adapter.turn(&owner.dispatcher);
        let bridge = adapter.bridge.clone();
        owner.private_bridge = Some(adapter);
        let mut stream = open_api(&mut owner, &signal);
        let mut pump = ParentPump::spawn(owner, signal.clone()).unwrap();
        let factory = bridge.worker_factory().unwrap();
        pump.install_supervisor(
            move || {
                let mut backend = factory();
                if let Some(gate) = retirement_gate {
                    backend.hold_retirement(gate);
                }
                backend
            },
            Policy::default(),
        )
        .unwrap();
        request(&mut stream, "hello", "hello", json!({}));
        response(&mut stream, "hello");
        request(&mut stream, "identity", "presentation_get", json!({}));
        let identity = response(&mut stream, "identity")["result"]["workbench_id"].clone();
        request(
            &mut stream,
            "document",
            "ui_replace_document",
            json!({"expected":{"workbench_id":identity,"revision":"1"},"document":document}),
        );
        assert_eq!(
            response(&mut stream, "document")["result"]["presentation_revision"],
            "2"
        );
        if !mode.is_empty() {
            wait(|| pump.supervisor().unwrap().restart(launch(mode)).is_ok());
        }
        Self {
            pump,
            bridge,
            stream,
            tx,
            rx,
            signal,
            stops,
            identity,
        }
    }
    fn directory(&self, incarnation: u64) -> PathBuf {
        launch("normal")
            .executable
            .parent()
            .unwrap()
            .join("target/isolated-native-renderer")
            .join(format!(
                "{}-{}-{incarnation}",
                std::process::id(),
                self.identity.as_str().unwrap()
            ))
    }
    fn seed(&mut self) {
        bootstrap_records(
            &self.tx,
            &self.rx,
            &self.signal,
            json!([{"kind":"signal","id":{"instrument":"instrument","parameter":"s0"},"generation":"1","value_type":"float","unit":{"id":"volt","symbol":"V"}}]),
        );
        send_update(
            &self.tx,
            &self.signal,
            ClientUpdate::RecoveryProjection {
                active: vec![record(KnownAdmission::Accepted)],
                quarantined: vec![],
            },
        );
        self.signal.wait_updates(7);
        for i in 0..64 {
            sample(&self.tx, &self.signal, i, ((i as f64) * 0.15).sin() * 4.);
            self.signal.wait_updates(8 + i as usize);
        }
    }
    fn replace(&mut self) {
        let mut replacement = document("Current parent document after display fault");
        replacement.plots[0].axes.y_max = Some(20.);
        request(
            &mut self.stream,
            "replace",
            "ui_replace_document",
            json!({"expected":{"workbench_id":self.identity,"revision":"2"},"document":replacement}),
        );
        assert_eq!(
            response(&mut self.stream, "replace")["result"]["presentation_revision"],
            "3"
        );
    }
    fn continuity(&mut self, revision: &str) {
        request(
            &mut self.stream,
            "presentation",
            "presentation_get",
            json!({}),
        );
        let view = response(&mut self.stream, "presentation");
        assert_eq!(view["result"]["workbench_id"], self.identity);
        assert_eq!(view["result"]["presentation_revision"], revision);
        request(&mut self.stream, "status", "client_status", json!({}));
        let status = response(&mut self.stream, "status");
        assert_eq!(status["result"]["runtime_client"]["freshness"], "fresh");
        assert_eq!(
            status["result"]["runtime_client"]["hello"]["boot_id"],
            "boot"
        );
        assert_eq!(
            status["result"]["runtime_client"]["hello"]["scope"],
            "scope"
        );
        assert_eq!(status["result"]["runtime_client"]["hello"]["next_seq"], "9");
        request(
            &mut self.stream,
            "recovery",
            "recovery_get",
            json!({"kind":"active","index":"0","expected":null}),
        );
        let recovery = response(&mut self.stream, "recovery");
        assert_eq!(recovery["result"]["record"]["admission"], "accepted");
        assert_eq!(recovery["result"]["record"]["boot_id"], "boot");
        assert_eq!(
            recovery["result"]["record"]["request_id"],
            json!({"scope":"scope","seq":"7"})
        );
        assert_eq!(recovery["result"]["record"]["op"], "reference_retune");
        assert_eq!(
            recovery["result"]["record"]["args"],
            json!({"reference":"1","target":2.0,"rate":1.0})
        );
        // This contains the original accepted identity, not a reconstructed one.
        assert!(
            self.rx.try_recv().is_err(),
            "display fault/restoration submitted Runtime work"
        );
        assert_eq!(self.stops.load(Ordering::SeqCst), 0);
    }
    fn terminal(&self) -> crate::supervisor::Status {
        self.pump
            .supervisor()
            .unwrap()
            .wait_status(|s| matches!(s.phase, Phase::ExitedHealthy | Phase::ExitedContained))
    }
    fn retired(&self) {
        let ipc = self.bridge.status().unwrap();
        assert!(ipc.retired);
        assert_eq!(ipc.handles, 0);
        assert_eq!(ipc.pending_io, 0);
        assert_eq!(ipc.io_bytes, 0);
        assert_eq!(ipc.credits.iter().sum::<usize>(), 4);
    }
    fn restart(&self, revision: u64, points: u64) {
        self.retired();
        wait(|| {
            self.pump
                .supervisor()
                .unwrap()
                .restart(launch("normal"))
                .is_ok()
        });
        let (path, frame) = find_frame(&self.directory(2), |v| {
            v["epoch_revision"] == revision
                && v["plot"] == true
                && v["current"] == true
                && v["plot_points"] == points
        });
        pixels(&path, &frame);
        assert!(frame["completed"].as_u64().unwrap() > 0);
        wait(|| self.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
        let terminal = self.terminal();
        assert_eq!(terminal.phase, Phase::ExitedHealthy, "{terminal:?}");
        self.retired();
    }
    fn finish(&mut self, mode: &str, details: Value) {
        self.continuity("3");
        let terminal = self.terminal();
        fs::write(self.directory(1).join("parent-fault-result.json"),serde_json::to_vec_pretty(&json!({"scenario":mode,"details":details,"original_process_exit_and_ipc_retirement":true,"explicit_restart_latest_snapshots":true,"extra_runtime_query_mutation_status_retry":0,"workbench_id":self.identity,"presentation_revision":"3","final_incarnation":terminal.incarnation,"final_phase":format!("{:?}",terminal.phase),"restart_peak_process_commit":terminal.reading.peak_process_commit,"restart_peak_job_commit":terminal.reading.peak_job_commit,"deployment_C":null,"os_containment_is_not_gpu_cleanup":true})).unwrap()).unwrap();
        println!(
            "ISOLATED_FAULT PASS mode={mode} evidence={} extra_runtime_submissions=0",
            self.directory(1).display()
        );
        self.pump.shutdown().unwrap();
        assert_eq!(self.stops.load(Ordering::SeqCst), 1);
    }
}

#[test]
#[ignore = "Windows actual isolated GL fault/restart qualification"]
fn native_renderer_crash_exit_with_pending_original_ipc_blocks_restart() {
    use std::sync::atomic::AtomicBool;
    let gate = Arc::new(AtomicBool::new(false));
    let mut h = FaultHarness::new("crash", Some(gate.clone()));
    let (path, frame) = find_frame(&h.directory(1), |v| {
        v["completed"].as_u64().is_some_and(|c| c > 0)
    });
    pixels(&path, &frame);
    let unretired = h
        .pump
        .supervisor()
        .unwrap()
        .wait_status(|s| s.phase == Phase::ExitUnconfirmed);
    assert_eq!(unretired.alive, Some(false));
    assert_eq!(unretired.exit_code, Some(73));
    let ipc = h.bridge.status().unwrap();
    assert!(!ipc.retired);
    assert!(ipc.handles > 0);
    assert_eq!(
        h.pump.supervisor().unwrap().restart(launch("normal")),
        Err(crate::supervisor::Rejected::ExitUnconfirmed)
    );
    h.seed();
    h.replace();
    h.continuity("3");
    gate.store(true, Ordering::Release);
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedContained);
    assert_eq!(terminal.exit_code, Some(73));
    h.retired();
    h.restart(3, 64);
    h.finish("crash-exit-before-ipc-retirement",json!({"old_exit_code":73,"restart_rejected_before_original_ipc_receipts":true,"old_own_object_cleanup":"UNQUALIFIED/crash","old_peak_process_commit":terminal.reading.peak_process_commit,"old_peak_job_commit":terminal.reading.peak_job_commit}));
}

#[test]
#[ignore = "Windows actual isolated GL owner stall qualification"]
fn native_renderer_owner_stall_does_not_mask_parent_or_restart() {
    let mut h = FaultHarness::new("owner-stall", None);
    let (path, frame) = find_frame(&h.directory(1), |v| {
        v["completed"].as_u64().is_some_and(|c| c > 0)
    });
    pixels(&path, &frame);
    h.pump
        .supervisor()
        .unwrap()
        .wait_status(|s| s.phase == Phase::Ready);
    h.seed();
    h.replace();
    h.continuity("3");
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedContained);
    assert_eq!(terminal.exit_code, Some(77));
    assert!(terminal.termination_requests > 0);
    assert!(
        matches!(
            terminal.reason,
            Some(
                crate::supervisor::Reason::HeartbeatDeadline
                    | crate::supervisor::Reason::OwnerProgressDeadline
            )
        ),
        "{terminal:?}"
    );
    h.retired();
    h.restart(3, 64);
    h.finish("native-render-owner-stall",json!({"reason":format!("{:?}",terminal.reason),"old_exit_code":77,"old_own_object_cleanup":"UNQUALIFIED/OS containment","old_peak_process_commit":terminal.reading.peak_process_commit,"old_peak_job_commit":terminal.reading.peak_job_commit}));
}

#[test]
#[ignore = "Windows actual GPU pending handoff and stop qualification"]
fn native_renderer_pending_completion_handoff_stop_retains_original_root() {
    let mut h = FaultHarness::new("pending", None);
    let directory = h.directory(1);
    wait(|| directory.join("pending.json").is_file());
    h.seed();
    h.replace();
    h.continuity("3");
    wait(|| directory.join("handoff-pending.json").is_file());
    let pending: Value =
        serde_json::from_slice(&fs::read(directory.join("handoff-pending.json")).unwrap()).unwrap();
    assert_eq!(pending["max_live_parsed"], 1);
    assert_eq!(pending["assembly_retained"], true);
    assert!(pending["ledger"].as_str().unwrap().contains("Some("));
    let old_applied = h.bridge.status().unwrap().applied[0];
    let old_owner = h.pump.supervisor().unwrap().status().unwrap().owner_counter;
    let until = Instant::now() + Duration::from_millis(2200);
    while Instant::now() < until {
        h.continuity("3");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        h.bridge.status().unwrap().applied[0],
        old_applied,
        "new document was Applied while old GPU/parsed ownership remained"
    );
    let live = h.pump.supervisor().unwrap().status().unwrap();
    assert_eq!(live.phase, Phase::Ready);
    assert!(live.owner_counter > old_owner);
    assert!(!directory.join("admitted-2.json").exists());
    assert!(!directory.join("own-retired.json").exists());
    wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedContained);
    assert_eq!(terminal.exit_code, Some(77));
    assert!(!directory.join("own-retired.json").exists());
    h.retired();
    h.restart(3, 64);
    h.finish("pending-gpu-document-handoff-stop",json!({"bounded_complete_assembly_survived_transfer_deadline":true,"old_applied_unchanged":old_applied,"max_live_parsed":1,"old_gpu_cleanup":"BLOCKED/pending until OS containment","pending":pending,"old_peak_process_commit":terminal.reading.peak_process_commit,"old_peak_job_commit":terminal.reading.peak_job_commit}));
}

#[test]
#[ignore = "Windows actual GL stale-read during snapshot stall qualification"]
fn native_renderer_snapshot_stall_preserves_non_current_latch() {
    let mut h = FaultHarness::new("snapshot-stall", None);
    // Establish the normal initial no-data frame before publishing samples.
    find_frame(&h.directory(1), |v| {
        v["plot"] == true && v["observation_snapshot"] != 0 && v["plot_points"] == 0
    });
    h.seed();
    let (path, fresh) = find_frame(&h.directory(1), |v| {
        v["plot"] == true && v["current"] == true && v["plot_points"] == 64
    });
    let fresh_pixels = pixels(&path, &fresh);
    wait(|| h.directory(1).join("snapshot-stalled.json").is_file());
    sample(&h.tx, &h.signal, 64, 8.);
    h.signal.wait_updates(72);
    // A parent-approved source fence invalidates the old replica independently
    // of a still-Fresh Runtime model. No fabricated client event/discovery.
    h.bridge.announce(2, fresh["source"].as_u64().unwrap() + 1);
    let (path, stale) = find_frame(&h.directory(1), |v| {
        v["plot"] == true
            && v["current"] == false
            && v["observation_snapshot"] == fresh["observation_snapshot"]
    });
    let stale_pixels = pixels(&path, &stale);
    assert!(
        fresh_pixels != stale_pixels,
        "source fence did not change actual stale presentation"
    );
    assert!(stale["labels"].as_str().unwrap().contains("Stale"));
    let owner_before = h.pump.supervisor().unwrap().status().unwrap().owner_counter;
    wait(|| {
        h.pump
            .supervisor()
            .unwrap()
            .status()
            .is_ok_and(|s| s.owner_counter > owner_before)
    });
    assert!(
        !h.bridge.status().unwrap().child_read_current,
        "heartbeat falsely promoted the old read"
    );
    h.replace();
    h.continuity("3");
    wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedHealthy);
    h.retired();
    h.restart(3, 65);
    h.finish("snapshot-stall-with-parent-source-fence",json!({"original_fresh_snapshot":fresh["observation_snapshot"],"old_read_non_current_despite_owner_heartbeat":true,"old_own_objects_after_close":0,"old_peak_process_commit":terminal.reading.peak_process_commit,"old_peak_job_commit":terminal.reading.peak_job_commit}));
}

//! Actual child pixels from the single parent presentation/model owner. Runtime
//! input is an explicitly virtual, bounded Application-worker fixture; the child
//! receives no Runtime events, discovery, client or mutation API.
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
fn launch(mode: &str) -> Launch {
    Launch {
        executable: PathBuf::from(
            std::env::var_os("LAB_ISOLATED_RENDERER_CHILD")
                .expect("run scripts/test-isolated-renderer.ps1"),
        ),
        arguments: vec![mode.into()],
        limits: lab_windows_job::Limits {
            process_commit: TEST_ONLY_COMMIT_CAP,
            job_commit: TEST_ONLY_COMMIT_CAP,
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
    let directory = std::env::current_dir()
        .unwrap()
        .join("target/isolated-native-renderer")
        .join(format!(
            "{}-{}-1",
            std::process::id(),
            identity.as_str().unwrap()
        ));
    wait(|| pump.supervisor().unwrap().restart(launch("normal")).is_ok());
    pump.supervisor()
        .unwrap()
        .wait_status(|s| s.phase == Phase::Ready);
    let (empty_path, empty) = find_frame(&directory, |v| {
        v["plot"] == true && v["observation_snapshot"] != 0 && v["plot_points"] == 0
    });
    let empty_pixels = pixels(&empty_path, &empty);
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
    let (plot_path, plot) = find_frame(&directory, |v| {
        v["plot"] == true && v["current"] == true && v["plot_points"] == 64
    });
    let plot_pixels = pixels(&plot_path, &plot);
    assert_ne!(empty_pixels, plot_pixels);
    assert!(plot["cyan_pixels"].as_u64().unwrap() > 100);
    assert!(!plot["curve_samples"].as_array().unwrap().is_empty());
    sample(&tx, &signal, 64, 8.);
    signal.wait_updates(72);
    let (live_path, live) = find_frame(&directory, |v| {
        v["plot"] == true && v["current"] == true && v["plot_points"] == 65
    });
    let live_pixels = pixels(&live_path, &live);
    assert_ne!(plot_pixels, live_pixels);
    request(
        &mut stream,
        "replace",
        "ui_replace_document",
        json!({"expected":{"workbench_id":identity,"revision":"2"},"document":document("Replacement after checked retirement")}),
    );
    assert_eq!(
        response(&mut stream, "replace")["result"]["presentation_revision"],
        "3"
    );
    let (replacement_path, replacement) = find_frame(&directory, |v| {
        v["plot"] == true && v["epoch_revision"] == 3 && v["plot_points"] == 65
    });
    let replacement_pixels = pixels(&replacement_path, &replacement);
    assert_ne!(live_pixels, replacement_pixels);
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
    fs::write(directory.join("parent-result.json"),serde_json::to_vec_pretty(&json!({"first_isolated_real_gl_frame":"PASS","test_only_job_commit_cap":TEST_ONLY_COMMIT_CAP,"deployment_C":null,
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

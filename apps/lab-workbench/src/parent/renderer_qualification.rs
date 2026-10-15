//! Fixed-profile qualification through the original parent/Job/IPC owners.
use super::*;
use crate::presentation::v2;

fn maximum_plot(shared: bool) -> PresentationDocument {
    let mut source = document("Maximum admitted Plot μ Ω");
    source.windows[0].title = crate::gui::readonly_v2::plot_fixtures::unicode_text(0, 0, 512);
    source.plots[0].title = crate::gui::readonly_v2::plot_fixtures::unicode_text(1, 1, 512);
    let template = source.plots[0].traces[0].clone();
    source.plots[0].traces = (0..32)
        .map(|i| {
            let mut trace = template.clone();
            trace.id = format!("trace-{i}");
            trace.source = RuntimeRef::Signal {
                instrument: "instrument".into(),
                parameter: format!("s{}", if shared { 0 } else { i }),
            };
            trace.display_label = format!("{i:02} {}x", "Σ".repeat(254));
            assert_eq!(trace.display_label.len(), 512);
            trace
        })
        .collect();
    source
}

fn converted(source: &PresentationDocument) -> Option<v2::AdmittedV2Document> {
    v2::convert_v1(v2::ValidatedV1Document::check(source).ok()?).ok()
}

/// Tune unused admitted definitions, not wire whitespace or another owner. The
/// child receives the canonical output of ParentBridge's existing conversion.
fn maximum_document() -> PresentationDocument {
    let mut source = maximum_plot(true);
    let template = source.plots[0].clone();
    for p in 1..14 {
        let mut plot = template.clone();
        plot.id = format!("unused-plot-{p}");
        for (i, trace) in plot.traces.iter_mut().enumerate() {
            trace.id = format!("unused-trace-{p}-{i}-");
            trace
                .id
                .extend(std::iter::repeat_n('i', 512 - trace.id.len()));
        }
        source.plots.push(plot);
    }
    let pad = |source: &mut PresentationDocument, bytes: usize| {
        for plot in source.plots.iter_mut().skip(1) {
            for trace in &mut plot.traces {
                trace.source = RuntimeRef::Signal {
                    instrument: "q".repeat(bytes),
                    parameter: "s".repeat(bytes),
                };
            }
        }
    };
    let (mut low, mut high) = (1, 513);
    while low + 1 < high {
        let mid = (low + high) / 2;
        pad(&mut source, mid);
        if converted(&source).is_some() {
            low = mid;
        } else {
            high = mid;
        }
    }
    pad(&mut source, low);
    let admitted = converted(&source).expect("base maximum document must be admitted");
    let mut remaining = 1_048_576 - admitted.canonical_bytes().len();
    drop(admitted);
    for plot in source.plots.iter_mut().skip(1) {
        for trace in &mut plot.traces {
            if let RuntimeRef::Signal {
                instrument,
                parameter,
            } = &mut trace.source
            {
                for text in [instrument, parameter] {
                    let count = remaining.min(512 - text.len());
                    text.extend(std::iter::repeat_n('q', count));
                    remaining -= count;
                }
            }
        }
    }
    assert_eq!(remaining, 0);
    let admitted = converted(&source).expect("exact maximum canonical document admission");
    assert_eq!(admitted.canonical_bytes().len(), 1_048_576);
    println!(
        "MAXIMUM_CANONICAL bytes={} parsed_accounted={} v1_bytes={}",
        admitted.canonical_bytes().len(),
        admitted.allocation().accounted_bytes,
        serde_json::to_vec_pretty(&source).unwrap().len()
    );
    source
}

#[test]
fn maximum_parent_document_uses_existing_canonical_conversion() {
    maximum_document();
}

fn maximum_seed(h: &mut FaultHarness, shared: bool) {
    let sources = if shared { 1 } else { 32 };
    let per_source = if shared { 4096 } else { 512 };
    let records: Vec<_> = (0..sources).map(|i| json!({"kind":"signal","id":{"instrument":"instrument","parameter":format!("s{i}")},"generation":"1","value_type":"float","unit":{"id":"volt","symbol":"V"}})).collect();
    bootstrap_records(&h.tx, &h.rx, &h.signal, json!(records));
    send_update(
        &h.tx,
        &h.signal,
        ClientUpdate::RecoveryProjection {
            active: vec![record(KnownAdmission::Accepted)],
            quarantined: vec![],
        },
    );
    h.signal.wait_updates(7);
    for i in 0..per_source {
        for source in 0..sources {
            let seq = (i * sources + source) as u64;
            send_update(
                &h.tx,
                &h.signal,
                ClientUpdate::Event {
                    cursor: EventCursor {
                        boot_id: "boot".into(),
                        seq: 6 + seq,
                    },
                    envelope: json!({"kind":"signal","target":{"instrument":"instrument","parameter":format!("s{source}")},"data":{
                        "signal":{"instrument":"instrument","parameter":format!("s{source}")},"generation":"1","revision":(6+seq).to_string(),"quality":"good","status":"available","value":((i as f64)*0.15 + source as f64*0.2).sin()*4.,"unit":{"id":"volt","symbol":"V"},"observed_at_ns":((i+1)*10_000_000usize).to_string(),"source_at_ns":((i+1)*10_000_000usize).to_string()
                    }}),
                },
            );
            h.signal.wait_updates(8 + seq as usize);
        }
    }
    h.continuity("2");
}

fn run_maximum(shared: bool, canonical_maximum: bool) {
    let source = if canonical_maximum {
        maximum_document()
    } else {
        maximum_plot(shared)
    };
    let refs = (0..if shared { 1 } else { 32 })
        .map(|i| RuntimeRef::Signal {
            instrument: "instrument".into(),
            parameter: format!("s{i}"),
        })
        .collect();
    let mut h = FaultHarness::profile("", None, source.clone(), refs);
    maximum_seed(&mut h, shared);
    let provenance = h.bridge.trace_provenance();
    assert_eq!(
        provenance.iter().map(|v| v.2).sum::<usize>(),
        if shared { 4096 } else { 16384 }
    );
    let mode = if shared {
        "maximum-repeated"
    } else {
        "maximum-distinct"
    };
    wait(|| h.pump.supervisor().unwrap().restart(launch(mode)).is_ok());
    let mut stages = vec![reading(&h.bridge, "launch")];
    let (path, frame) = find_frame(&h.directory(1), |v| {
        v["plot"] == true && v["plot_points"] == 16384
    });
    pixels(&path, &frame);
    assert_eq!(frame["plot_traces"], 32);
    assert_eq!(
        frame["plot_point_visits"],
        if shared { 262144 } else { 32768 }
    );
    assert_eq!(frame["geometry_segments"], 16384 - 32);
    assert!(frame["cyan_pixels"].as_u64().unwrap() > 0);
    stages.push(reading(&h.bridge, "maximum_plot_readback"));
    for revision in 2..=4 {
        let mut replacement = source.clone();
        replacement.plots[0].axes.y_max = Some(20. + revision as f64);
        request(
            &mut h.stream,
            "maximum-replace",
            "ui_replace_document",
            json!({"expected":{"workbench_id":h.identity,"revision":revision.to_string()},"document":replacement}),
        );
        assert_eq!(
            response(&mut h.stream, "maximum-replace")["result"]["presentation_revision"],
            (revision + 1).to_string()
        );
        let (path, next) = find_frame(&h.directory(1), |v| {
            v["epoch_revision"] == revision + 1 && v["plot_points"] == 16384
        });
        pixels(&path, &next);
        stages.push(reading(&h.bridge, "document_replacement_readback"));
        h.continuity(&(revision + 1).to_string());
    }
    wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedHealthy, "{terminal:?}");
    h.retired();
    let own: Value =
        serde_json::from_slice(&fs::read(h.directory(1).join("own-retired.json")).unwrap())
            .unwrap();
    assert_eq!(own["own_objects"], 0);
    h.continuity("5");
    fs::write(h.directory(1).join("maximum-result.json"),serde_json::to_vec_pretty(&json!({"scenario":mode,"canonical_maximum":canonical_maximum,"selected_points":if shared {4096}else{16384},"stages":stages,"terminal":format!("{terminal:?}"),"peak_process_commit":terminal.reading.peak_process_commit,"peak_job_commit":terminal.reading.peak_job_commit,"extra_runtime_query_mutation_status_retry":0,"test_only_cap":qualification_cap(),"deployment_C":null,"own":own})).unwrap()).unwrap();
    println!(
        "MAXIMUM_RENDERER PASS mode={mode} evidence={} job_peak={}",
        h.directory(1).display(),
        terminal.reading.peak_job_commit
    );
    h.pump.shutdown().unwrap();
}

#[test]
#[ignore = "Windows actual maximum canonical/Plot/repeated-source qualification"]
fn native_maximum_canonical_repeated_source_profile() {
    run_maximum(true, true);
}

#[test]
#[ignore = "Windows actual 32 selected traces / 16384 original LRD1 points"]
fn native_maximum_selected_points_profile() {
    run_maximum(false, false);
}

#[test]
#[ignore = "Windows original Device artificial fault matrix in an isolated child"]
fn native_isolated_gpu_fault_matrix() {
    for mode in [
        "gpu-timeout-proof",
        "gpu-wait-failed",
        "gpu-device-loss",
        "gpu-native-teardown",
        "gpu-original-error",
        "gpu-texture-before",
        "gpu-texture-after",
        "gpu-buffer-before",
        "gpu-buffer-after",
        "gpu-shader-before",
        "gpu-shader-after",
        "gpu-program-before",
        "gpu-program-after",
        "gpu-vao-before",
        "gpu-vao-after",
        "gpu-fence-before",
        "gpu-fence-after",
    ] {
        let mut h = FaultHarness::new(mode, None);
        let directory = h.directory(1);
        if mode != "gpu-original-error" {
            find_frame(&directory, |v| {
                v["completed"].as_u64().is_some_and(|n| n > 0)
            });
        }
        if mode != "gpu-original-error" {
            wait(|| directory.join("fault-armed.json").is_file());
        }
        h.seed();
        if mode == "gpu-timeout-proof" {
            wait(|| directory.join("timeout-proof.json").is_file());
            h.replace();
            find_frame(&directory, |v| {
                v["epoch_revision"] == 3 && v["plot_points"] == 64
            });
        } else if mode != "gpu-native-teardown" {
            h.replace();
        } else {
            h.continuity("2");
        }
        if mode == "gpu-timeout-proof" || mode == "gpu-native-teardown" {
            wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
        }
        if mode != "gpu-timeout-proof" {
            wait(|| {
                directory
                    .join(if mode == "gpu-native-teardown" {
                        "native-close-error.json"
                    } else {
                        "fault-retained.json"
                    })
                    .is_file()
            });
            if mode == "gpu-native-teardown" {
                // Closing preserves the original Close challenge until healthy
                // retirement. Its interim native progress is not a Status pong.
                let receipt = directory.join("native-close-retained.json");
                wait(|| receipt.is_file());
                let receipt: Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
                assert!(
                    receipt["event_loop"].as_u64().unwrap()
                        > receipt["original_event_loop"].as_u64().unwrap()
                );
                assert_eq!(receipt["release_retry"], false);
            } else {
                let before = h.pump.supervisor().unwrap().status().unwrap().owner_counter;
                wait(|| {
                    h.pump
                        .supervisor()
                        .unwrap()
                        .status()
                        .is_ok_and(|s| s.owner_counter > before)
                });
            }
            h.continuity(if mode == "gpu-native-teardown" {
                "2"
            } else {
                "3"
            });
            if mode != "gpu-native-teardown" {
                wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
            }
        }
        let terminal = h.terminal();
        assert_eq!(
            terminal.phase,
            if mode == "gpu-timeout-proof" {
                Phase::ExitedHealthy
            } else {
                Phase::ExitedContained
            },
            "{mode}: {terminal:?}"
        );
        h.retired();
        if mode == "gpu-native-teardown" {
            h.replace();
        }
        h.restart(3, 64);
        h.finish(mode,json!({"artificial":true,"old_exit_code":terminal.exit_code,"old_phase":format!("{:?}",terminal.phase),"old_peak_process_commit":terminal.reading.peak_process_commit,"old_peak_job_commit":terminal.reading.peak_job_commit,"gpu_cleanup":if mode=="gpu-timeout-proof" {"PASS/actual later fence proof"}else{"BLOCKED/original root retained until OS containment"}}));
    }
}

#[test]
#[ignore = "Windows real native resize/minimize/restore with original GL context"]
fn native_window_suspension_preserves_event_loop_and_gpu_identity() {
    let mut h = FaultHarness::new("window-profile", None);
    h.seed();
    let directory = h.directory(1);
    wait(|| directory.join("window-probe-11.json").is_file());
    for stage in [3, 6, 8, 9, 10] {
        let receipt: Value = serde_json::from_slice(
            &fs::read(directory.join(format!("window-probe-{stage}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["completed"], receipt["previous_frames"]);
        assert!(
            receipt["owner_progress"].as_u64().unwrap()
                > receipt["previous_owner"].as_u64().unwrap()
        );
    }
    h.continuity("2");
    h.replace();
    find_frame(&directory, |v| {
        v["epoch_revision"] == 3 && v["plot_points"] == 64
    });
    wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedHealthy, "{terminal:?}");
    h.retired();
    h.restart(3, 64);
    h.finish("actual-window-resize-minimize-restore",json!({"unsupported_dpi_policy_input":"PASS/artificial on original native owner; frames paused","actual_non_one_dpi_native_event":"UNQUALIFIED/no new supported profile","old_peak_process_commit":terminal.reading.peak_process_commit,"old_peak_job_commit":terminal.reading.peak_job_commit}));
}

#[test]
#[ignore = "Windows parent shutdown while actual renderer GPU and IPC retirement are pending"]
fn native_parent_shutdown_with_pending_gpu_and_original_ipc() {
    use std::sync::atomic::AtomicBool;
    let gate = Arc::new(AtomicBool::new(false));
    let mut h = FaultHarness::new("pending", Some(gate.clone()));
    h.seed();
    h.replace();
    let directory = h.directory(1);
    wait(|| directory.join("handoff-pending.json").is_file());
    h.continuity("3");
    let first = h.pump.shutdown();
    assert!(first.is_err());
    let pending = h
        .pump
        .supervisor()
        .unwrap()
        .wait_status(|s| s.phase == Phase::ExitUnconfirmed && s.alive == Some(false));
    assert!(h.bridge.status().unwrap().handles > 0);
    assert!(
        h.pump
            .supervisor()
            .unwrap()
            .restart(launch("normal"))
            .is_err()
    );
    assert_eq!(h.stops.load(Ordering::SeqCst), 1);
    assert!(h.rx.try_recv().is_err());
    assert!(!directory.join("own-retired.json").exists());
    gate.store(true, Ordering::Release);
    h.pump
        .supervisor()
        .unwrap()
        .wait_status(|s| s.worker_finished);
    wait(|| h.pump.supervisor().unwrap().worker_finished());
    h.pump.shutdown().unwrap();
    h.retired();
    let retired = h.pump.supervisor().unwrap().status().unwrap();
    assert!(retired.reading.peak_process_commit > 0 && retired.reading.peak_job_commit > 0);
    fs::write(directory.join("parent-shutdown-result.json"),serde_json::to_vec_pretty(&json!({"initial_shutdown":format!("{first:?}"),"original_exit_before_ipc_retirement":format!("{pending:?}"),"original_receipts_later_retired":true,"runtime_stopped_once":true,"extra_submissions":0,"gpu_cleanup":"BLOCKED/OS containment","peak_process_commit":retired.reading.peak_process_commit,"peak_job_commit":retired.reading.peak_job_commit,"reading_obtained_after_original_job_retirement":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "Windows real renderer startup under deliberately insufficient test-only Job cap"]
fn native_renderer_memory_admission_denial_preserves_parent() {
    let mut h = FaultHarness::profile(
        "",
        None,
        document("Deliberate allocation denial"),
        vec![target()],
    );
    h.seed();
    wait(|| {
        let mut denied = launch("normal");
        denied.limits = lab_windows_job::Limits {
            process_commit: 16 * 1024 * 1024,
            job_commit: 16 * 1024 * 1024,
        };
        h.pump.supervisor().unwrap().restart(denied).is_ok()
    });
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedContained, "{terminal:?}");
    h.retired();
    h.continuity("2");
    h.replace();
    h.restart(3, 64);
    h.finish("deliberate-16MiB-admission-denial",json!({"test_only_denial_cap":16*1024*1024,"old_exit":format!("{terminal:?}"),"gpu_cleanup":"UNQUALIFIED/no healthy receipt","cap_not_deployment_C":true}));
}

#[test]
#[ignore = "Explicit fixed candidate C required; 100 original cold-launch/close/crash/restart incarnations"]
fn native_hundred_fixed_cap_cycles() {
    let cap = launch("normal").limits.job_commit;
    assert!(
        cap < TEST_ONLY_COMMIT_CAP,
        "select/review a measured candidate before this gate; 1 GiB is not C"
    );
    let mut h = FaultHarness::profile(
        "",
        None,
        document("Fixed-cap lifecycle cycles"),
        vec![target()],
    );
    h.seed();
    let mut receipts = Vec::new();
    let root = launch("normal")
        .executable
        .parent()
        .unwrap()
        .join("target/isolated-native-renderer");
    let output = root.join(format!(
        "cycles-{}-{}.json",
        std::process::id(),
        h.identity.as_str().unwrap()
    ));
    for cycle in 1..=101 {
        let healthy = cycle % 2 == 1;
        wait(|| {
            h.pump
                .supervisor()
                .unwrap()
                .restart(launch(if healthy { "normal" } else { "crash" }))
                .is_ok()
        });
        let directory = h.directory(cycle);
        if healthy {
            let (path, frame) =
                find_frame(&directory, |v| v["plot"] == true && v["plot_points"] == 64);
            pixels(&path, &frame);
            let live = h.pump.supervisor().unwrap().status().unwrap();
            assert_eq!(live.objects.jobs, 1);
            assert_eq!(live.objects.processes, 1);
            assert!(
                h.pump
                    .supervisor()
                    .unwrap()
                    .restart(launch("normal"))
                    .is_err()
            );
            wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
        }
        let terminal = h.terminal();
        assert_eq!(terminal.incarnation, cycle);
        assert_eq!(terminal.alive, Some(false));
        assert_eq!(
            terminal.phase,
            if healthy {
                Phase::ExitedHealthy
            } else {
                Phase::ExitedContained
            }
        );
        assert_eq!(terminal.objects, Default::default());
        assert!(
            terminal.reading.peak_process_commit <= cap && terminal.reading.peak_job_commit <= cap
        );
        h.retired();
        h.continuity("2");
        let own = if healthy {
            let own: Value =
                serde_json::from_slice(&fs::read(directory.join("own-retired.json")).unwrap())
                    .unwrap();
            assert_eq!(own["own_objects"], 0);
            assert!(!own["ledger"].as_str().unwrap().contains("Some("));
            own
        } else {
            assert_eq!(terminal.exit_code, Some(73));
            assert!(!directory.join("own-retired.json").exists());
            json!({"own_object_release":"UNQUALIFIED/intentional OS process exit"})
        };
        let startup: Value =
            serde_json::from_slice(&fs::read(directory.join("startup.json")).unwrap()).unwrap();
        assert_eq!(startup["incarnation"], cycle);
        receipts.push(json!({"original_child_pid":startup["pid"],"cycle":cycle,"incarnation":terminal.incarnation,"healthy":healthy,"original_exit":terminal.exit_code,"phase":format!("{:?}",terminal.phase),"objects":format!("{:?}",terminal.objects),"ipc_handles":0,"pending_io":0,"encoded_credits":4,"peak_process_commit":terminal.reading.peak_process_commit,"peak_job_commit":terminal.reading.peak_job_commit,"extra_submissions":0,"own":own,"directory":directory}));
        fs::write(&output,serde_json::to_vec_pretty(&json!({"candidate_C":cap,"required_cycles":100,"completed_cycles":receipts.len(),"cycles":receipts,"deployment_selected":false,"activation_qualified":false})).unwrap()).unwrap();
        println!(
            "LIFECYCLE_CYCLE {cycle}/101 cap={cap} job_peak={} evidence={}",
            terminal.reading.peak_job_commit,
            output.display()
        );
    }
    h.pump.shutdown().unwrap();
}

#[test]
#[ignore = "Windows expected redraw suppressed artificially while actual native owner/IPC continue"]
fn native_missing_redraw_is_not_healthy_idle() {
    let mut h = FaultHarness::new("redraw-stall", None);
    let directory = h.directory(1);
    find_frame(&directory, |v| v["completed"] == 1);
    h.seed();
    let before = h.pump.supervisor().unwrap().status().unwrap().owner_counter;
    h.continuity("2");
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedContained);
    assert_eq!(terminal.exit_code, Some(91));
    let failure: Value =
        serde_json::from_slice(&fs::read(directory.join("failure.json")).unwrap()).unwrap();
    assert!(
        failure["error"]
            .as_str()
            .unwrap()
            .contains("frame progress deadline")
    );
    assert_eq!(failure["frame_completed"], 1);
    assert!(failure["owner_progress"].as_u64().unwrap() > before);
    h.retired();
    h.replace();
    h.restart(3, 64);
    h.finish("expected-redraw-artificially-suppressed",json!({"failure":failure,"owner_event_loop_was_live":true,"old_exit":format!("{terminal:?}")}));
}

#[test]
#[ignore = "Windows actual framebuffer for parent-origin missing/unavailable/rebuilding reads"]
fn native_selected_absence_and_rebuilding_are_not_fresh_values() {
    let mut h = FaultHarness::new("normal", None);
    let directory = h.directory(1);
    find_frame(&directory, |v| v["plot"] == true && v["plot_points"] == 0);
    h.seed();
    find_frame(&directory, |v| v["plot"] == true && v["plot_points"] == 64);
    send_update(
        &h.tx,
        &h.signal,
        ClientUpdate::Event {
            cursor: EventCursor {
                boot_id: "boot".into(),
                seq: 70,
            },
            envelope: json!({"kind":"signal","target":{"instrument":"instrument","parameter":"s0"},"data":{
                "signal":{"instrument":"instrument","parameter":"s0"},"generation":"1","revision":"70","quality":"unavailable","status":"unavailable","value":null,"failure":"qualification_sensor_fault","unit":{"id":"volt","symbol":"V"},"observed_at_ns":"650000000","source_at_ns":"650000000"
            }}),
        },
    );
    h.signal.wait_updates(72);
    let (path, absent) = find_frame(&directory, |v| {
        v["plot"] == true
            && v["labels"]
                .as_str()
                .is_some_and(|s| s.contains("Unavailable"))
    });
    pixels(&path, &absent);
    assert_eq!(absent["plot_points"], 0);
    assert_eq!(absent["cyan_pixels"], 0);
    h.continuity("2");
    // This is an explicit virtual Runtime gap, independent of display lifecycle.
    // Its one normal discover is baseline rebuild work, never a UI-fault query.
    send_update(
        &h.tx,
        &h.signal,
        ClientUpdate::ResnapshotRequired {
            reason: "qualification-original-runtime-gap".into(),
            envelope: None,
            connection_lost: false,
        },
    );
    assert!(query(&h.rx, "discover") > 0);
    h.signal.wait_updates(73);
    let (path, rebuilding) = find_frame(&directory, |v| {
        v["labels"]
            .as_str()
            .is_some_and(|s| s.contains("Rebuilding"))
    });
    pixels(&path, &rebuilding);
    request(
        &mut h.stream,
        "rebuilding-status",
        "client_status",
        json!({}),
    );
    let status = response(&mut h.stream, "rebuilding-status");
    assert_eq!(
        status["result"]["runtime_client"]["freshness"],
        "rebuilding"
    );
    assert_eq!(status["result"]["runtime_client"]["hello"]["next_seq"], "9");
    assert!(h.rx.try_recv().is_err());
    wait(|| h.pump.supervisor().unwrap().stop(Stop::Graceful).is_ok());
    let terminal = h.terminal();
    assert_eq!(terminal.phase, Phase::ExitedHealthy, "{terminal:?}");
    h.retired();
    fs::write(directory.join("absence-result.json"),serde_json::to_vec_pretty(&json!({"missing_initial":true,"unavailable":absent,"rebuilding":rebuilding,"runtime_gap_discover_baseline":1,"additional_display_submissions":0,"terminal":format!("{terminal:?}"),"peak_process_commit":terminal.reading.peak_process_commit,"peak_job_commit":terminal.reading.peak_job_commit})).unwrap()).unwrap();
    h.pump.shutdown().unwrap();
}

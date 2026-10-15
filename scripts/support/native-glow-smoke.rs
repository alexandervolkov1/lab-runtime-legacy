//! Deterministic native rendering witness, separate from CPU allocation probes.
use crate::{
    model::{LivePoint, RuntimeObservations},
    plot_fixtures,
    readonly_v2::{
        self, Preview,
        completion::{Completion, WaitFailure, WaitPolicy, WaitRequest},
        deletion::{DeleteDisposition, ObjectKind},
        frame::{ContextGeneration, FrameError, FrameOutcome},
        host_session::{HostSession, ShutdownOutcome},
        texture_owner::{TextureKey, TextureOwner},
    },
};
use eframe::egui;
use std::{cell::RefCell, rc::Rc};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

#[path = "native-glow-device.rs"]
mod device;
#[path = "native-allocation-meter.rs"]
mod meter;
use device::{Device, HEIGHT, WIDTH};
type Result<T> = std::result::Result<T, String>;
#[path = "native-glow-backend.rs"]
mod backend;
use backend::Backend;
struct Root {
    session: HostSession<Backend>,
    device: Rc<RefCell<Device>>,
    attempted: bool,
    unresolved: bool,
}
#[derive(Default)]
struct App {
    root: Option<Root>,
    result: Option<Result<()>>,
    inject: bool,
    scenario: Option<String>,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if self.root.is_some() {
            return;
        }
        match Device::new(events) {
            Ok(device) => {
                let device = Rc::new(RefCell::new(device));
                let backend = Backend {
                    device: device.clone(),
                    inject_unresolved: self.inject,
                    fail_submission: self.scenario.as_deref() == Some("original-error"),
                };
                self.root = Some(Root {
                    session: HostSession::new(backend),
                    device,
                    attempted: false,
                    unresolved: false,
                });
                self.root
                    .as_ref()
                    .unwrap()
                    .device
                    .borrow()
                    .window
                    .request_redraw();
            }
            Err(error) => {
                self.result = Some(Err(error.to_string()));
                events.exit();
            }
        }
    }
    fn window_event(&mut self, events: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(root) = &mut self.root else {
            return;
        };
        if root.unresolved {
            return;
        } // Keep native root alive; no close/Drop receipt.
        if event != WindowEvent::RedrawRequested || root.attempted {
            return;
        }
        root.attempted = true;
        let result = if let Some(scenario) = &self.scenario {
            fault_exercise(root, scenario)
        } else {
            exercise(root, self.inject)
        };
        // Raw adapter conformance owns its ledger directly. Do not ask an idle
        // session for a release receipt while that separate obligation exists.
        if matches!(
            root.session.status(),
            readonly_v2::host_session::CleanupOutcome::Idle
                | readonly_v2::host_session::CleanupOutcome::Retired(_)
        ) && root.device.borrow().ledger().iter().any(Option::is_some)
        {
            root.unresolved = true;
            println!(
                "HOST_RUST_DELETION_PENDING {:?}; native ledger retained",
                meter::phase()
            );
            println!(
                "TERMINAL GPU LIFECYCLE BLOCKED: GPU object deletion unresolved; native context teardown not attempted; native root retained"
            );
            println!(
                "PENDING_READY current={} counters={:?} ledger={:?}",
                root.device.borrow().current_owner(),
                root.device.borrow().counters,
                root.device.borrow().ledger()
            );
            return;
        }
        if matches!(
            root.session.shutdown(WaitPolicy::default()),
            ShutdownOutcome::Unresolved(_)
        ) {
            root.unresolved = true;
            println!(
                "HOST_RUST_PENDING {:?}; readback/staging remain owned by native root",
                meter::phase()
            );
            println!(
                "TERMINAL GPU LIFECYCLE BLOCKED: retained native root, keys={}, context={}, frame={}, status={:?}; no ordinary Drop, no cleanup receipt",
                root.session.owned_keys(),
                root.session.retained_context(),
                root.session.retained_cpu_frame(),
                root.session.status()
            );
            println!(
                "PENDING_READY current={} counters={:?} ledger={:?}",
                root.device.borrow().current_owner(),
                root.device.borrow().counters,
                root.device.borrow().ledger()
            );
            use std::io::Write;
            let _ = std::io::stdout().flush();
            return;
        }
        let cleanup = root.device.borrow_mut().healthy_close();
        if cleanup.is_err() {
            root.unresolved = true;
            println!(
                "HOST_RUST_TEARDOWN_PENDING {:?}; native context owner retained",
                meter::phase()
            );
            println!(
                "TERMINAL GPU LIFECYCLE BLOCKED: NativeContextTeardown {cleanup:?}; native root retained"
            );
            println!(
                "PENDING_READY current={} counters={:?} ledger={:?}",
                root.device.borrow().current_owner(),
                root.device.borrow().counters,
                root.device.borrow().ledger()
            );
            return;
        }
        println!(
            "FINAL keys={} unresolved=0 delete_calls={} counters={:?}",
            root.session.owned_keys(),
            root.device.borrow().delete_calls,
            root.device.borrow().counters
        );
        self.result = Some(result.and(cleanup));
        // Session and device ledger proved release; only now destroy context/window.
        self.root = None;
        events.exit();
    }
}
pub(crate) fn run() {
    let mut app = App {
        scenario: std::env::args().find_map(|a| a.strip_prefix("--fault=").map(str::to_owned)),
        inject: std::env::args().any(|a| a == "--pending"),
        ..Default::default()
    };
    // Warm standard output before the counting interval. This meter covers the
    // entire Rust native process scope, not OS/driver heaps or isolated Preview.
    println!("native host Rust allocation interval begins (separate from CPU qualification)");
    meter::start();
    // This Windows-only qualification process fixes winit's scale to one. The
    // desktop compositor may scale the displayed window; the GL drawable and
    // Preview/readback coordinates remain exactly 1280x960 at one pixel/point.
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let events = EventLoop::builder()
        .with_dpi_aware(false)
        .build()
        .expect("native event loop");
    if let Err(error) = events.run_app(&mut app) {
        eprintln!(
            "NativeContextTeardown BLOCKED: event loop failed: {error}; child process exit is containment, no cleanup receipt"
        );
        // Never unwind/drop an unresolved native root after losing its loop.
        std::process::exit(1);
    }
    if app.root.is_some() {
        eprintln!(
            "NativeContextTeardown BLOCKED: loop ended with an owned native root; child process exit is containment, no cleanup receipt"
        );
        std::process::exit(1);
    }
    let memory = meter::phase();
    meter::stop();
    meter::attribution();
    println!(
        "HOST_RUST_AFTER_WINDOW {memory:?} (includes Preview during frames; retained platform/singleton allocations are not GPU objects)"
    );
    match app.result {
        Some(Ok(())) if app.scenario.is_some() => println!(
            "FAULT {} verified healthy release PASS; ACTIVATION_QUALIFIED=false",
            app.scenario.as_deref().unwrap()
        ),
        Some(Ok(())) => println!(
            "GPU RENDER SMOKE PASS; HEALTHY GPU LIFECYCLE PASS; TERMINAL GPU LIFECYCLE BLOCKED; HOST BUDGET QUALIFICATION BLOCKED; ACTIVATION_QUALIFIED=false"
        ),
        other => {
            eprintln!("NATIVE GPU SMOKE FAILED: {other:?}");
            std::process::exit(1);
        }
    }
}
fn fault_exercise(root: &mut Root, scenario: &str) -> Result<()> {
    let (document, observations) = fixture();
    let wait = match scenario {
        "timeout-proof" => Some(Completion::Timeout),
        "wait-failed" => Some(Completion::Failed(WaitFailure::WaitError)),
        "missing-fence" => Some(Completion::Failed(WaitFailure::MissingFence)),
        "invalid-fence" => Some(Completion::Failed(WaitFailure::InvalidFence)),
        "device-loss" => Some(Completion::DeviceLost),
        _ => None,
    };
    {
        let mut device = root.device.borrow_mut();
        device.wait_fault = wait;
        device.teardown_fault = scenario == "native-teardown";
        let kind = if scenario == "original-error" {
            Some(ObjectKind::Shader)
        } else {
            match scenario.split('-').next().unwrap_or("") {
                "texture" => Some(ObjectKind::Texture),
                "buffer" => Some(ObjectKind::Buffer),
                "shader" => Some(ObjectKind::Shader),
                "program" => Some(ObjectKind::Program),
                "vao" => Some(ObjectKind::VertexArray),
                "fence" => Some(ObjectKind::Fence),
                _ => None,
            }
        };
        if let Some(kind) = kind {
            device.delete_fault = Some((
                kind,
                if scenario.ends_with("before") {
                    DeleteDisposition::NotStarted
                } else {
                    DeleteDisposition::Ambiguous
                },
                1,
            ));
        }
    }
    if scenario == "partial-texture" {
        return partial_fault(root);
    }
    let mut preview = Preview::with_session(&document, EPOCH, &mut root.session);
    let mut frame = preview
        .run(&observations, Default::default())
        .map_err(|e| format!("fault preparation: {e:?}"))?;
    frame
        .tessellate()
        .map_err(|e| format!("fault tessellation: {e:?}"))?;
    let outcome = frame.finish();
    if wait.is_some() && outcome != Err(FrameError::Completion) {
        return Err("wait injection did not fail completion".into());
    }
    if scenario.starts_with("fence-") && outcome != Err(FrameError::Deletion) {
        return Err("fence delete injection did not fail deletion".into());
    }
    if scenario == "original-error" && outcome != Err(FrameError::Submission) {
        return Err("original execution error lost".into());
    }
    if scenario == "timeout-proof" {
        let before = root.device.borrow().counters.clone();
        if !matches!(
            preview.cleanup_outcome(),
            readonly_v2::host_session::CleanupOutcome::Pending(_)
        ) {
            return Err("timeout was retired".into());
        }
        if before.fences_created != 1 || before.fences_deleted != 0 || before.textures_deleted != 0
        {
            return Err("resources freed before proof".into());
        }
        root.device.borrow_mut().wait_fault = None;
        let cleanup = preview.poll_retirement(WaitPolicy::default());
        if !matches!(
            cleanup,
            readonly_v2::host_session::CleanupOutcome::Retired(_)
        ) {
            return Err("real later fence proof missing".into());
        }
        drop(preview);
        let after = root.device.borrow().counters.clone();
        if after.frames != before.frames
            || after.draws != before.draws
            || after.full_uploads != before.full_uploads
            || after.objects != 0
            || after.completions != 1
        {
            return Err("proof replayed work or missed cleanup".into());
        }
        println!(
            "FAULT timeout-proof PASS: actual fence+flush retained -> timeout -> actual Quiesced -> all deletion receipts; no draw/upload replay; counters={after:?}"
        );
        return Ok(());
    }
    drop(preview);
    if scenario == "native-teardown" {
        println!(
            "FAULT native-teardown armed: session obligations released, context owner retained on teardown failure"
        );
        return Ok(());
    }
    let before = root.device.borrow().ledger();
    if wait.is_some() {
        // Remove the simulated return: the native terminal latch itself must
        // retain the obligation, never wait on a quarantined sync or render.
        let mut device = root.device.borrow_mut();
        device.wait_fault = None;
        let generation = before.iter().flatten().next().unwrap().generation;
        if device.draw(generation, &[], 1.0).is_ok() || device.present().is_ok() {
            return Err("terminal completion admitted native rendering".into());
        }
    }
    if !root.device.borrow().current_owner() {
        return Err("fault root lost its current context".into());
    }
    if scenario == "original-error"
        && !matches!(root.session.status(), readonly_v2::host_session::CleanupOutcome::Pending(s) if s.frame == Some(FrameOutcome::Failed(FrameError::Submission)))
    {
        return Err("cleanup erased the original frame error".into());
    }
    let calls = root.device.borrow().delete_calls;
    for _ in 0..25 {
        if !matches!(
            root.session.shutdown(WaitPolicy::default()),
            ShutdownOutcome::Unresolved(_)
        ) {
            return Err("false Released under injection".into());
        }
        let mut rejected = Preview::with_session(&document, EPOCH, &mut root.session);
        if !matches!(
            rejected.run(&observations, Default::default()),
            Err(FrameError::RetirementPending)
        ) {
            return Err("pending admitted new generation".into());
        }
        drop(rejected);
        if root.device.borrow().ledger() != before || root.device.borrow().delete_calls != calls {
            return Err("shutdown changed unresolved ledger or double-freed".into());
        }
    }
    println!(
        "FAULT {scenario} PASS: deterministic injection; no false Released, 25 repeated shutdowns and generations blocked; delete_calls={calls} ledger={before:?}; simulated loss is not hardware evidence"
    );
    Err("terminal obligations unresolved (expected containment only)".into())
}
fn partial_fault(root: &mut Root) -> Result<()> {
    let generation = ContextGeneration::allocate().map_err(|e| format!("generation: {e:?}"))?;
    let mut device = root.device.borrow_mut();
    for id in [7, 8] {
        device.upload(
            TextureKey {
                generation,
                id: egui::TextureId::Managed(id),
            },
            &egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([4, 4], egui::Color32::WHITE),
                Default::default(),
            ),
        )?;
    }
    device.draw(generation, &[], 1.0)?;
    let start = std::time::Instant::now();
    let mut completion = Completion::Timeout;
    for _ in 0..20 {
        if start.elapsed() >= std::time::Duration::from_millis(100) {
            break;
        }
        completion = device.wait(WaitRequest {
            generation,
            purpose: readonly_v2::completion::WaitPurpose::Retirement,
            timeout: std::time::Duration::from_millis(5),
        });
        if completion != Completion::Timeout {
            break;
        }
    }
    if completion != Completion::Quiesced {
        return Err(format!("partial real completion: {completion:?}"));
    }
    device
        .release_frame(generation)
        .map_err(|e| format!("fence: {e:?}"))?;
    device.delete_fault = Some((ObjectKind::Texture, DeleteDisposition::Ambiguous, 2));
    device
        .free(TextureKey {
            generation,
            id: egui::TextureId::Managed(7),
        })
        .map_err(|e| format!("first texture: {e:?}"))?;
    let second = TextureKey {
        generation,
        id: egui::TextureId::Managed(8),
    };
    let failure = device
        .free(second)
        .expect_err("second delete must be ambiguous");
    let ledger = device.ledger();
    let calls = device.delete_calls;
    for _ in 0..25 {
        if device.free(second) != Err(failure)
            || device.release_generation(generation) != Err(failure)
            || device.delete_calls != calls
            || device.ledger() != ledger
        {
            return Err("partial deletion retried or forgot identity".into());
        }
    }
    if device.counters.textures_deleted != 1
        || ledger.iter().flatten().count() != 7
        || device.counters.gl_errors != 0
    {
        return Err("partial ledger incorrect".into());
    }
    println!(
        "FAULT partial-texture PASS: first texture confirmed, second ambiguous exact key retained, six pipeline objects untouched; calls={calls} ledger={ledger:?}"
    );
    Err("partial native deletion unresolved; root remains owner".into())
}
const EPOCH: readonly_v2::PresentationEpoch = readonly_v2::PresentationEpoch {
    owner: [71; 16],
    revision: 1,
};
fn fixture() -> (
    crate::presentation::v2::AdmittedV2Document,
    RuntimeObservations,
) {
    let mut syntax = plot_fixtures::syntax(32, false);
    syntax["windows"][0]["title"] = serde_json::json!("Измерения μ Ω");
    syntax["plots"][0]["title"] = serde_json::json!("Измерения μ Ω");
    syntax["plots"][0]["axes"]["y_min"] = serde_json::json!(-20.0);
    syntax["plots"][0]["axes"]["y_max"] = serde_json::json!(20.0);
    for (i, trace) in syntax["plots"][0]["traces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        trace["display_label"] = serde_json::json!("Температура μ Ω");
        trace["style"]["width"] = serde_json::json!(if i == 31 { 32.0 } else { 2.5 });
    }
    let mut observations = RuntimeObservations::default();
    for source in 0..32 {
        plot_fixtures::observe(&mut observations, source, "float", serde_json::json!(1.0));
        let buffer = observations
            .live
            .entry(plot_fixtures::target(source))
            .or_default();
        for point in 0..512 {
            // Pairs repeat both coordinates: coincident and ordinary segments.
            let x = (point / 2) as f64;
            buffer
                .push(LivePoint {
                    time_seconds: x,
                    value: source as f64 - 16.0 + (x * 0.03).sin() * 0.2,
                })
                .unwrap();
        }
    }
    observations.complete_rebuild();
    (plot_fixtures::admit(&syntax), observations)
}
struct Inspection {
    text: Vec<(String, egui::Pos2)>,
    points: Vec<egui::Pos2>,
    generation: ContextGeneration,
    digest: u64,
    colored: usize,
}
fn cyan(pixel: &[u8]) -> bool {
    pixel[0] < 40 && pixel[1] > 150 && pixel[2] > 170
}
fn pixel(bytes: &[u8], x: usize, y: usize) -> &[u8] {
    let i = ((HEIGHT - 1 - y) * WIDTH + x) * 4;
    &bytes[i..i + 4]
}
fn score(bytes: &[u8], points: &[egui::Pos2]) -> usize {
    points
        .iter()
        .filter(|point| {
            let x = point.x.round() as usize;
            let y = point.y.round() as usize;
            (y.saturating_sub(2)..=(y + 2).min(HEIGHT - 1)).any(|yy| {
                (x.saturating_sub(2)..=(x + 2).min(WIDTH - 1)).any(|xx| cyan(pixel(bytes, xx, yy)))
            })
        })
        .count()
}
fn frame(
    preview: &mut Preview<'_, Backend>,
    obs: &RuntimeObservations,
    device: &Rc<RefCell<Device>>,
    events: Vec<egui::Event>,
    name: &str,
) -> Result<Inspection> {
    let mut frame = preview
        .run(
            obs,
            egui::RawInput {
                events,
                ..Default::default()
            },
        )
        .map_err(|e| format!("{name}: {e:?}"))?;
    let report = frame.report();
    if report.geometry_failures != 0
        || report.geometry_unavailable != 0
        || report.allocation_faults != 0
    {
        return Err(format!("{name}: invalid geometry {report:?}"));
    }
    let generation = frame.generation();
    let mut text = Vec::new();
    let mut points = Vec::new();
    for shape in frame.shapes() {
        match &shape.shape {
            egui::Shape::Text(t) => text.push((
                t.galley.text().to_owned(),
                t.visual_bounding_rect().center(),
            )),
            egui::Shape::Mesh(m)
                if m.vertices.len() > 200
                    && m.vertices
                        .iter()
                        .all(|v| v.color == egui::Color32::from_rgb(0, 200, 220)) =>
            {
                if let Some(p) = m
                    .vertices
                    .chunks_exact(4)
                    .map(|q| (q[0].pos + q[2].pos.to_vec2()) / 2.0)
                    .find(|p| {
                        p.x > 350.0
                            && p.x < 700.0
                            && p.y > 0.0
                            && p.y < HEIGHT as f32
                            && shape.clip_rect.contains(*p)
                    })
                {
                    points.push(p);
                }
            }
            _ => {}
        }
    }
    frame
        .tessellate()
        .map_err(|e| format!("tessellate: {e:?}"))?;
    if frame.finish() != Ok(FrameOutcome::HostCompleted { degraded: false }) {
        return Err(format!("{name}: no successful frame receipt"));
    }
    let mut device = device.borrow_mut();
    let bytes = device.readback()?;
    let digest = bytes.iter().fold(0xcbf29ce484222325u64, |hash, b| {
        (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    let colored = (80..520)
        .map(|y| (200..1000).filter(|&x| cyan(pixel(bytes, x, y))).count())
        .sum();
    let matched = score(bytes, &points);
    println!(
        "frame {name} generation={generation:?} traces={} points={} curve_samples={} matched={} cyan_pixels={colored} framebuffer_fnv64={digest:016x}",
        report.plot_traces,
        report.plot_points,
        points.len(),
        matched
    );
    if matched != points.len() {
        return Err(format!(
            "{name}: curve pixel mismatch {matched}/{}",
            points.len()
        ));
    }
    device.present()?;
    Ok(Inspection {
        text,
        points,
        generation,
        digest,
        colored,
    })
}
fn click(
    preview: &mut Preview<'_, Backend>,
    obs: &RuntimeObservations,
    device: &Rc<RefCell<Device>>,
    caption: &str,
) -> Result<()> {
    let current = frame(preview, obs, device, vec![], "locate")?;
    let pos = current
        .text
        .iter()
        .find(|(t, _)| t == caption)
        .ok_or_else(|| format!("missing {caption}"))?
        .1;
    for pressed in [true, false] {
        frame(
            preview,
            obs,
            device,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            "click",
        )?;
    }
    Ok(())
}
fn exercise(root: &mut Root, inject: bool) -> Result<()> {
    let (document, mut observations) = fixture();
    println!(
        "HOST_RUST_BEFORE_PREVIEW {:?} (native root, GL loader, virtual fixture; no Preview yet)",
        meter::phase()
    );
    let mut preview = Preview::with_session(&document, EPOCH, &mut root.session);
    if inject {
        if frame(
            &mut preview,
            &observations,
            &root.device,
            vec![],
            "injected-unresolved",
        )
        .is_ok()
        {
            return Err("injection unexpectedly acknowledged".into());
        }
        for _ in 0..25 {
            if !matches!(
                preview.run(&observations, Default::default()),
                Err(FrameError::RetirementPending)
            ) {
                return Err("pending admitted frame".into());
            }
        }
        return Err(
            "injected unresolved completion; terminal path intentionally unqualified".into(),
        );
    }
    let page = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![],
        "Page-Unicode",
    )?;
    if !page.text.iter().any(|(t, _)| t.contains("Измерения")) {
        return Err("Unicode page missing".into());
    }
    click(&mut preview, &observations, &root.device, "Open Plot")?;
    let empty = frame(
        &mut preview,
        &RuntimeObservations::default(),
        &root.device,
        vec![],
        "Plot-no-data-control",
    )?;
    let plot = frame(&mut preview, &observations, &root.device, vec![], "Plot-32")?;
    if plot.points.len() != 32 || plot.colored < 10_000 || empty.colored != 0 {
        return Err(format!(
            "curve/control verification failed: samples={}, color={}, empty={}",
            plot.points.len(),
            plot.colored,
            empty.colored
        ));
    }
    let repeat = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![],
        "Plot-repeat",
    )?;
    if repeat.generation != plot.generation {
        return Err("repeat changed generation".into());
    }
    for buffer in observations.live.values_mut() {
        buffer
            .push(LivePoint {
                time_seconds: 256.0,
                value: 0.0,
            })
            .map_err(|e| format!("fixture: {e:?}"))?;
    }
    let updated = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![],
        "measurements-updated",
    )?;
    if updated.digest == repeat.digest {
        return Err("measurement update did not change framebuffer".into());
    }
    frame(
        &mut preview,
        &observations,
        &root.device,
        vec![egui::Event::PointerMoved(updated.points[16])],
        "hover-enter",
    )?;
    let hover = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![],
        "hover-tooltip",
    )?;
    if !hover
        .text
        .iter()
        .any(|(t, _)| t.contains("Curve position:"))
    {
        return Err("hover tooltip missing".into());
    }
    let zoom = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![
            egui::Event::PointerMoved(egui::pos2(600.0, 280.0)),
            egui::Event::Zoom(2.0),
        ],
        "zoom",
    )?;
    if zoom.digest == updated.digest {
        return Err("zoom did not change pixels".into());
    }
    click(&mut preview, &observations, &root.device, "Title details")?;
    let details = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![],
        "Details-Unicode",
    )?;
    if !details
        .text
        .iter()
        .any(|(t, _)| t.starts_with("UTF-8: ") && t.contains("D0"))
    {
        return Err("Unicode details bytes missing".into());
    }
    click(&mut preview, &observations, &root.device, "Back to Plot")?;
    let restored = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![],
        "Plot-restored",
    )?;
    let before_full = root.device.borrow().counters.full_uploads;
    preview.retire_context();
    if root.device.borrow().counters.objects != 0 {
        return Err("generation resources remain".into());
    }
    let next = frame(
        &mut preview,
        &observations,
        &root.device,
        vec![],
        "next-generation",
    )?;
    if next.generation == restored.generation
        || root.device.borrow().counters.full_uploads != before_full + 1
    {
        return Err("next generation missing full atlas upload".into());
    }
    drop(preview);
    if root.session.owned_keys() != 0
        || root.session.retained_cpu_frame()
        || root.session.retained_context()
    {
        return Err("Preview retirement incomplete".into());
    }
    texture_sequence(
        root,
        ContextGeneration::allocate().map_err(|e| format!("conformance generation: {e:?}"))?,
    )?;
    let c = &root.device.borrow().counters;
    if c.gl_errors != 0
        || c.full_uploads < 2
        || c.partial_uploads < 2
        || c.buffer_reuses == 0
        || c.completions != c.frames
        || c.fences_created != c.fences_deleted
    {
        return Err(format!("incomplete GPU counters: {c:?}"));
    }
    println!(
        "ACCOUNTING fixed_device={} fixed_root={} fixed_native_session={} fixed_cpu_preview={} fixed_cpu_owner={} readback_peak={} host_staging_peak={} texture_payload_peak={} buffer_payload_peak={} simultaneous_generations=1; payload is not driver VRAM; native/driver/transient allocation budget unqualified",
        std::mem::size_of::<Device>(),
        std::mem::size_of::<Root>(),
        std::mem::size_of::<HostSession<Backend>>(),
        std::mem::size_of::<Preview<'_>>(),
        std::mem::size_of::<TextureOwner<readonly_v2::texture_owner::CpuOnly>>(),
        WIDTH * HEIGHT * 4,
        c.peak_staging_capacity,
        c.peak_texture_payload,
        c.peak_buffer_bytes
    );
    Ok(())
}
fn texture_sequence(root: &Root, generation: ContextGeneration) -> Result<()> {
    // Separate bounded backend conformance frame: same TextureId, full -> partial
    // -> partial in ONE ordered delta, then sample the final texel with a quad.
    let mut owner = TextureOwner::new(Backend {
        device: root.device.clone(),
        inject_unresolved: false,
        fail_submission: false,
    });
    owner
        .begin(generation)
        .map_err(|e| format!("begin: {e:?}"))?;
    let id = egui::TextureId::Managed(7);
    {
        let mut device = root.device.borrow_mut();
        let before = (
            device.counters.objects,
            device.counters.staging_capacity,
            device.counters.full_uploads,
        );
        for count in [15, 17] {
            let mut image = egui::ColorImage::filled([4, 4], egui::Color32::RED);
            image.pixels.resize(count, egui::Color32::RED);
            if device
                .upload(
                    TextureKey { generation, id },
                    &egui::epaint::ImageDelta::full(image, egui::TextureOptions::NEAREST),
                )
                .is_ok()
                || before
                    != (
                        device.counters.objects,
                        device.counters.staging_capacity,
                        device.counters.full_uploads,
                    )
            {
                return Err("malformed image escaped pre-copy validation".into());
            }
        }
        println!(
            "malformed image short/long payload rejected before staging or GL allocation PASS"
        );
    }
    let mut output = egui::FullOutput {
        pixels_per_point: 1.0,
        ..Default::default()
    };
    output.viewport_output.insert(
        egui::ViewportId::ROOT,
        egui::ViewportOutput {
            parent: egui::ViewportId::ROOT,
            class: egui::ViewportClass::Root,
            builder: Default::default(),
            viewport_ui_cb: None,
            commands: Vec::new(),
            repaint_delay: std::time::Duration::ZERO,
        },
    );
    let options = egui::TextureOptions::NEAREST;
    output.textures_delta.push(
        id,
        egui::epaint::ImageDelta::full(
            egui::ColorImage::filled([4, 4], egui::Color32::RED),
            options,
        ),
    );
    for color in [egui::Color32::GREEN, egui::Color32::BLUE] {
        output.textures_delta.push(
            id,
            egui::epaint::ImageDelta::partial(
                [1, 1],
                egui::ColorImage::filled([1, 1], color),
                options,
            ),
        );
    }
    let mut mesh = egui::Mesh::with_texture(id);
    mesh.add_rect_with_uv(
        egui::Rect::from_min_max(egui::pos2(200.0, 200.0), egui::pos2(300.0, 300.0)),
        egui::Rect::from_min_max(egui::pos2(0.375, 0.375), egui::pos2(0.375, 0.375)),
        egui::Color32::WHITE,
    );
    let meshes = [egui::ClippedPrimitive {
        clip_rect: egui::Rect::from_min_max(egui::pos2(220.0, 220.0), egui::pos2(280.0, 280.0)),
        primitive: egui::epaint::Primitive::Mesh(mesh),
    }];
    output.textures_delta.free(id);
    owner
        .process(generation, &mut output, &meshes)
        .map_err(|e| format!("texture sequence: {e:?}"))?;
    root.device
        .borrow_mut()
        .release_generation(generation)
        .map_err(|e| format!("conformance release: {e:?}"))?;
    let mut device = root.device.borrow_mut();
    let bytes = device.readback()?;
    if pixel(bytes, 250, 250)[..3] != [0, 0, 255]
        || pixel(bytes, 210, 210)[..3] == [0, 0, 255]
        || owner.owned_keys() != 0
    {
        return Err("ordered partial/scissor readback failed".into());
    }
    println!(
        "texture_sequence full->partial-green->partial-blue->draw->fence->free PASS center={:?} clipped={:?} keys=0",
        pixel(bytes, 250, 250),
        pixel(bytes, 210, 210)
    );
    Ok(())
}

//! Deterministic native rendering witness, separate from CPU allocation probes.
use crate::{
    model::{LivePoint, RuntimeObservations},
    plot_fixtures,
    readonly_v2::{
        self, Preview,
        completion::{Completion, WaitPolicy, WaitRequest},
        frame::{ContextGeneration, FrameError, FrameOutcome},
        host_session::{HostSession, ShutdownOutcome},
        texture_owner::{FrameBackend, TextureKey, TextureOwner},
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
#[path = "allocation-meter.rs"]
mod meter;
use device::{Device, HEIGHT, WIDTH};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone)]
struct Backend {
    device: Rc<RefCell<Device>>,
    inject_unresolved: bool,
}
impl FrameBackend for Backend {
    const CPU_ONLY: bool = false;
    fn set(
        &mut self,
        key: TextureKey,
        delta: &egui::epaint::ImageDelta,
    ) -> std::result::Result<(), FrameError> {
        self.device
            .borrow_mut()
            .upload(key, delta)
            .map_err(|error| {
                eprintln!("upload: {error}");
                FrameError::TextureUpdate
            })
    }
    fn platform(
        &mut self,
        output: &egui::PlatformOutput,
        viewports: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> std::result::Result<(), FrameError> {
        // Fixed read-only smoke accepts only the root viewport and local cursor
        // output. SystemDefault is already the window's creation theme and needs
        // no side effect. Clipboard/URL/secondary native work is not implemented.
        if viewports.len() != 1
            || !viewports.contains_key(&egui::ViewportId::ROOT)
            || !output.commands.is_empty()
            || viewports.values().any(|v| {
                v.commands.iter().any(|command| {
                    !matches!(
                        command,
                        egui::ViewportCommand::SetTheme(egui::SystemTheme::SystemDefault)
                    )
                }) || v.viewport_ui_cb.is_some()
            })
        {
            eprintln!(
                "unsupported platform output: commands={:?}",
                output.commands
            );
            for (id, viewport) in viewports {
                eprintln!(
                    "viewport {id:?} commands={:?} callback={}",
                    viewport.commands,
                    viewport.viewport_ui_cb.is_some()
                );
            }
            return Err(FrameError::PlatformOutput);
        }
        Ok(())
    }
    fn submit(
        &mut self,
        generation: ContextGeneration,
        meshes: &[egui::ClippedPrimitive],
        scale: f32,
    ) -> std::result::Result<(), FrameError> {
        self.device
            .borrow_mut()
            .draw(generation, meshes, scale)
            .map_err(|error| {
                eprintln!("submit: {error}");
                FrameError::Submission
            })
    }
    fn wait(&mut self, request: WaitRequest) -> Completion {
        if self.inject_unresolved {
            Completion::Timeout
        } else {
            self.device.borrow_mut().wait(request)
        }
    }
    fn free(&mut self, key: TextureKey) {
        let mut device = self.device.borrow_mut();
        if let Err(error) = device.free(key) {
            device.fault.get_or_insert(error);
        }
        // The trait's void free is not itself deletion evidence. The native root
        // independently requires the checked device ledger and zero objects.
    }
}
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
                self.result = Some(Err(error));
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
        let result = exercise(root, self.inject);
        if matches!(
            root.session.shutdown(WaitPolicy::default()),
            ShutdownOutcome::Unresolved(_)
        ) {
            root.unresolved = true;
            println!(
                "TERMINAL GPU LIFECYCLE BLOCKED: retained native root, keys={}, context={}, frame={}, status={:?}; no ordinary Drop, no cleanup receipt",
                root.session.owned_keys(),
                root.session.retained_context(),
                root.session.retained_cpu_frame(),
                root.session.status()
            );
            println!("PENDING_READY {:?}", root.device.borrow().counters);
            use std::io::Write;
            let _ = std::io::stdout().flush();
            return;
        }
        let cleanup = root.device.borrow_mut().healthy_close();
        if cleanup.is_err() {
            root.unresolved = true;
            println!("TERMINAL GPU LIFECYCLE BLOCKED: {cleanup:?}; native root retained");
            return;
        }
        println!(
            "FINAL keys={} unresolved=0 counters={:?}",
            root.session.owned_keys(),
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
    events
        .run_app(&mut app)
        .expect("native event loop execution");
    let memory = meter::phase();
    meter::stop();
    println!(
        "HOST_RUST_AFTER_WINDOW {memory:?} (includes Preview during frames; retained platform/singleton allocations are not GPU objects)"
    );
    match app.result {
        Some(Ok(())) => println!(
            "GPU RENDER SMOKE PASS; HEALTHY GPU LIFECYCLE PASS; TERMINAL GPU LIFECYCLE BLOCKED; HOST BUDGET QUALIFICATION BLOCKED; ACTIVATION_QUALIFIED=false"
        ),
        other => {
            eprintln!("NATIVE GPU SMOKE FAILED: {other:?}");
            std::process::exit(1);
        }
    }
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
        .map(|y| (200..1000).filter(|&x| cyan(pixel(&bytes, x, y))).count())
        .sum();
    let matched = score(&bytes, &points);
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
        "ACCOUNTING fixed_device={} fixed_root={} readback_peak={} host_staging_peak={} texture_payload_peak={} buffer_payload_peak={} simultaneous_generations=1; payload is not driver VRAM; native/driver/transient allocation budget unqualified",
        std::mem::size_of::<Device>(),
        std::mem::size_of::<Root>(),
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
    let bytes = root.device.borrow_mut().readback()?;
    if pixel(&bytes, 250, 250)[..3] != [0, 0, 255]
        || pixel(&bytes, 210, 210)[..3] == [0, 0, 255]
        || owner.owned_keys() != 0
    {
        return Err("ordered partial/scissor readback failed".into());
    }
    println!(
        "texture_sequence full->partial-green->partial-blue->draw->fence->free PASS center={:?} clipped={:?} keys=0",
        pixel(&bytes, 250, 250),
        pixel(&bytes, 210, 210)
    );
    Ok(())
}

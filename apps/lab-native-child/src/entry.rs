//! Windows qualification owner. The event loop borrows a lexical Preview scope;
//! it never stores a self-reference or moves a GL owner between threads.
use crate::{
    backend::Backend,
    device::{Device, HEIGHT, WIDTH},
    presentation::RuntimeRef,
    private_ipc::{Replica, protocol::*, transport::Framed},
    readonly_v2::{
        PREVIEW_RESERVATION, PresentationEpoch, Preview,
        completion::WaitPolicy,
        frame::{FrameError, FrameOutcome},
        host_session::{CleanupOutcome, HostSession, ShutdownOutcome},
        observation_source::{ReadOnlyProvider, SelectedRead, Trace},
    },
    readonly_values::{Freshness, ReadBinding},
};
use eframe::egui;
use lab_windows_job::pipe::Pipe;
use serde_json::json;
use std::{
    cell::RefCell,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, Ime, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    platform::pump_events::{EventLoopExtPumpEvents, PumpStatus},
    window::WindowId,
};
type Result<T> = std::result::Result<T, String>;

struct Missing {
    epoch: PresentationEpoch,
    incarnation: u64,
}
impl ReadOnlyProvider for Missing {
    fn epoch(&self) -> Option<PresentationEpoch> {
        Some(self.epoch)
    }
    fn incarnation(&self) -> Option<u64> {
        Some(self.incarnation)
    }
    fn original_freshness(&self) -> Freshness {
        Freshness::Unknown
    }
    fn current(&self) -> bool {
        false
    }
    fn with_selected(
        &self,
        _: &RuntimeRef,
        _: ReadBinding,
        consume: &mut dyn FnMut(Option<SelectedRead<'_>>),
    ) {
        consume(None);
    }
    fn trace(&self, _: &RuntimeRef) -> Option<Trace<'_>> {
        None
    }
}

struct Channels {
    identity: Identity,
    control: Framed<CONTROL>,
    snapshot: Framed<CHUNK>,
    sequence: Sequence,
    hello: bool,
    hello_ack: bool,
    challenge: Option<u64>,
    closing: bool,
    pending: [Option<(Header, bool)>; 2],
    closed_sent: bool,
    diagnostic: Option<crate::private_ipc::renderer_progress::RendererProgress>,
    reported_frame_completion: bool,
    document_applied_sent: bool,
    post_admission_status: bool,
    frame_admitted: bool,
    assembling: bool,
}
impl Channels {
    fn service(
        &mut self,
        replica: &mut Replica,
        owner_progress: u64,
        healthy: bool,
        stall: bool,
        progress: crate::private_ipc::renderer_progress::RendererProgress,
    ) -> Result<()> {
        let sent = self.control.poll_send().map_err(|e| format!("{e:?}"))?;
        if sent && self.post_admission_status {
            self.frame_admitted = true;
            self.post_admission_status = false;
        }
        if sent && self.closed_sent {
            return Ok(());
        }
        let now = Instant::now();
        let mut bytes = 0;
        if let Some((header, payload)) = self
            .control
            .receive(self.identity, now)
            .map_err(|e| format!("{e:?}"))?
        {
            bytes += header.bytes;
            match header.kind {
                Kind::Hello if !self.hello && payload.is_empty() => self.hello = true,
                Kind::Challenge | Kind::Close if self.hello_ack && payload.len() == 8 => {
                    replica.parent_progress(now);
                    self.challenge = Some(u64::from_le_bytes(payload.try_into().unwrap()));
                    if header.kind == Kind::Close {
                        self.closing = true;
                        replica.stop();
                    }
                }
                Kind::Fence if self.hello_ack && payload.len() == 16 => replica
                    .fence(
                        u64::from_le_bytes(payload[..8].try_into().unwrap()),
                        u64::from_le_bytes(payload[8..].try_into().unwrap()),
                        now,
                    )
                    .map_err(str::to_owned)?,
                _ => return Err("renderer control direction/order".into()),
            }
        }
        // A silent partial transfer must expire even when no next chunk arrives.
        // Control/Close remains first; a complete handoff candidate is exempt by
        // Replica's existing rule and retains its original Assembly credit.
        if !self.closing {
            replica.expire(now).map_err(str::to_owned)?;
        }
        if self.hello_ack
            && !self.closing
            && !stall
            && replica.document_candidate().is_none()
            && let Some((header, payload)) = self
                .snapshot
                .receive_bounded(self.identity, now, OWNER_BYTES - bytes)
                .map_err(|e| format!("{e:?}"))?
        {
            let complete = replica.chunk(header, payload, now).map_err(str::to_owned)?;
            self.assembling = header.offset + payload.len() < header.total;
            if complete && header.kind == Kind::Observations {
                self.pending[1] = Some((
                    header,
                    replica
                        .accepted(1)
                        .is_some_and(|accepted| accepted.snapshot == header.snapshot),
                ));
            }
        }
        if !self.control.idle() {
            return Ok(());
        }
        if self.hello && !self.hello_ack {
            self.control
                .send(
                    Header::control(
                        Kind::HelloAck,
                        self.identity,
                        self.sequence.next().map_err(str::to_owned)?,
                    ),
                    &[],
                )
                .map_err(|e| format!("{e:?}"))?;
            self.hello_ack = true;
        } else if self.closing {
            // Preserve the exact Close challenge until real own-object release.
            // An interim Status must not consume it while retirement is pending.
            if healthy {
                self.status(replica, owner_progress, Kind::Closed)?;
                self.closed_sent = true;
            }
        } else if let Some(index) = self.pending.iter().position(Option::is_some) {
            let (decision, applied) = self.pending[index].take().unwrap();
            let mut header = Header::control(
                if applied {
                    Kind::Applied
                } else {
                    Kind::Skipped
                },
                self.identity,
                self.sequence.next().map_err(str::to_owned)?,
            );
            header.bytes += 1;
            header.epoch = decision.epoch;
            header.snapshot = decision.snapshot;
            header.source = decision.source;
            self.control
                .send(header, &[index as u8])
                .map_err(|e| format!("{e:?}"))?;
            self.document_applied_sent |= index == 0 && applied;
        } else if let Some(progress) = self.diagnostic.take() {
            let payload = progress.encode();
            let mut header = Header::control(
                Kind::Diagnostic,
                self.identity,
                self.sequence.next().map_err(str::to_owned)?,
            );
            header.bytes += payload.len();
            self.control
                .send(header, &payload)
                .map_err(|e| format!("{e:?}"))?;
            self.reported_frame_completion |= progress.completed != 0;
        } else if self.challenge.is_some() {
            self.status(replica, owner_progress, Kind::Status)?;
            self.diagnostic = Some(progress);
        }
        Ok(())
    }
    fn status(&mut self, replica: &mut Replica, owner_progress: u64, kind: Kind) -> Result<()> {
        let mut header = Header::control(
            kind,
            self.identity,
            self.sequence.next().map_err(str::to_owned)?,
        );
        header.bytes += 32;
        let mut payload = [0; 32];
        payload[..8].copy_from_slice(&self.challenge.take().unwrap_or(0).to_le_bytes());
        payload[8..16].copy_from_slice(&owner_progress.to_le_bytes());
        payload[16..24]
            .copy_from_slice(&replica.accepted(1).map_or(0, |h| h.snapshot).to_le_bytes());
        payload[24] = u8::from(replica.read_is_current(Instant::now()));
        self.control
            .send(header, &payload)
            .map_err(|e| format!("{e:?}"))?;
        // Order the real owner Status after parsed admission/Applied and finish
        // its original write before the first potentially costly GL frame. This
        // is transport progress, not an assertion that the parent accepted Ready
        // or that a frame/GPU completion already exists.
        self.post_admission_status =
            kind == Kind::Status && owner_progress != 0 && self.document_applied_sent;
        Ok(())
    }
}

#[derive(Default)]
struct Bootstrap {
    device: Option<Device>,
    error: Option<String>,
    profile: Option<serde_json::Value>,
}
impl ApplicationHandler for Bootstrap {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if self.device.is_none() && self.error.is_none() {
            self.profile = Some(json!({
                "primary_scale":events.primary_monitor().map(|m|m.scale_factor()),
                "monitors":events.available_monitors().take(8).map(|monitor|json!({"name":monitor.name().map(|s|s.chars().take(128).collect::<String>()),"scale":monitor.scale_factor(),"size":[monitor.size().width,monitor.size().height],"position":[monitor.position().x,monitor.position().y]})).collect::<Vec<_>>()
            }));
            match Device::new(events) {
                Ok(device) => self.device = Some(device),
                Err(e) => self.error = Some(e),
            }
        }
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

struct State {
    io: Channels,
    device: Rc<RefCell<Device>>,
    directory: PathBuf,
    mode: String,
    owner_progress: u64,
    render_owner_progress: u64,
    completed: u64,
    fatal: Option<String>,
    suspension: crate::drawable::Suspension,
    pending: bool,
    snapshot_stalled: bool,
    fault_armed: bool,
    pending_since: Option<Instant>,
    window_probe: (u8, Instant, u64, u64),
    expected_frame_since: Option<Instant>,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    input: Vec<egui::Event>,
    last_draw: Instant,
    captures: usize,
    admissions: usize,
    capture_key: Option<(u64, u64, bool, bool)>,
    observed_snapshot: u64,
    plot: bool,
    document: Header,
    deadline: Instant,
}
impl State {
    fn note(&self, name: &str, value: serde_json::Value) -> Result<()> {
        let file =
            File::create(self.directory.join(format!("{name}.json"))).map_err(|e| e.to_string())?;
        serde_json::to_writer(file, &value).map_err(|e| e.to_string())
    }
    fn fail(&mut self, error: String) {
        if self.fatal.is_none() {
            let _=self.note("failure",json!({"error":error,"frame_completed":self.completed,"owner_progress":self.owner_progress}));
            self.fatal = Some(error);
        }
    }
    fn expired(&self) {
        if Instant::now() >= self.deadline {
            // OS containment exit: bypass Rust destructors of an unresolved root.
            // This is explicitly NOT a completion/deletion receipt.
            std::process::exit(93);
        }
    }
    fn service(&mut self, replica: &mut Replica, healthy: bool) {
        let stall = self.snapshot_stalled;
        let progress = {
            let device = self.device.borrow();
            crate::private_ipc::renderer_progress::RendererProgress {
                event_loop: self.owner_progress,
                render_owner: self.render_owner_progress,
                submitted: device.counters.frames as u64,
                completed: self.completed,
                own_objects: device.counters.objects as u64,
                released_generations: device.counters.generations_released as u64,
                pending: self.pending,
            }
        };
        if let Err(error) = self
            .io
            .service(replica, self.owner_progress, healthy, stall, progress)
        {
            self.fail(error);
        }
        self.expired();
    }
}

/// All fields are borrows of disjoint lexical owners. `Preview` survives every
/// callback/frame of one admitted document; a handoff ends this scope only after
/// its original session proves retirement. There is no self-referential struct.
struct Display<'p, 'd> {
    preview: Option<&'p mut Preview<'d, Backend>>,
    replica: &'p mut Replica,
    state: &'p mut State,
}
impl Display<'_, '_> {
    fn suspension_receipt(&self, event: &str) {
        let state = &self.state;
        let causes = state.suspension;
        let _ = state.note(
            "drawable-state",
            json!({"event":event,
                "occluded":causes.occluded,"minimized":causes.minimized,"unsupported_size":causes.unsupported_size,
            "unsupported_scale":causes.unsupported_scale,"paused":causes.paused(),
            "event_loop":state.owner_progress,"render_owner":state.render_owner_progress,
            "completed":state.completed,"new_profile_qualified":false}),
        );
    }
    fn probe_window(&mut self) {
        if self.state.mode != "window-profile" {
            return;
        }
        let state = &mut self.state;
        let (stage, since, frames, owner) = state.window_probe;
        let next = match stage {
            0 if state.completed >= 2 => {
                let _ = state
                    .device
                    .borrow()
                    .window
                    .request_inner_size(winit::dpi::PhysicalSize::new(1296, 976));
                Some(1)
            }
            1 if state.suspension.unsupported_size => Some(2),
            2 | 5 if since.elapsed() >= Duration::from_millis(250) => {
                if state.completed != frames || state.owner_progress <= owner {
                    state.fail(
                        "native suspension failed to stop frames/preserve owner progress".into(),
                    );
                    return;
                }
                if stage == 2 {
                    let _ = state
                        .device
                        .borrow()
                        .window
                        .request_inner_size(winit::dpi::PhysicalSize::new(1280, 960));
                } else {
                    state.device.borrow().window.set_minimized(false);
                }
                Some(stage + 1)
            }
            3 if !state.suspension.unsupported_size && state.completed > frames => {
                state.device.borrow().window.set_minimized(true);
                Some(4)
            }
            4 if state.suspension.minimized => Some(5),
            6 if !state.suspension.minimized && state.completed > frames => {
                // Artificial unsupported-scale policy input on the actual
                // native owner. It does not change the OS DPI or qualify a new
                // drawable. Exercise the same transition used by the event.
                state.suspension.scale(1.25);
                state.suspension.occluded = true;
                let _ = state
                    .device
                    .borrow()
                    .window
                    .request_inner_size(winit::dpi::PhysicalSize::new(1296, 976));
                Some(7)
            }
            7..=9 if since.elapsed() >= Duration::from_millis(250) => {
                if state.completed != frames || state.owner_progress <= owner {
                    state.fail("independent scale/size/occlusion suspension failed".into());
                    return;
                }
                match stage {
                    7 => {
                        state.suspension.occluded = false;
                        let _ = state
                            .device
                            .borrow()
                            .window
                            .request_inner_size(winit::dpi::PhysicalSize::new(1280, 960));
                    }
                    8 => {
                        state.suspension.scale(1.);
                        state.suspension.occluded = true;
                    }
                    _ => state.suspension.occluded = false,
                }
                Some(stage + 1)
            }
            10 if !state.suspension.paused() && state.completed > frames => Some(11),
            _ => None,
        };
        if let Some(next) = next {
            let _=state.note(&format!("window-probe-{next}"),json!({"stage":next,"completed":state.completed,
                "owner_progress":state.owner_progress,"previous_frames":frames,"previous_owner":owner,
                "suspension":format!("{:?}",state.suspension),"actual_winit_window_calls":true,
                    "unsupported_dpi_policy_input_artificial":next>=7,
                    "new_dpi_profile_qualified":false,"actual_scale_factor_changed_event":"UNQUALIFIED/not fabricated"}));
            state.window_probe = (next, Instant::now(), state.completed, state.owner_progress);
        }
    }
    fn draw(&mut self) -> Result<()> {
        let state = &mut self.state;
        if state.suspension.paused()
            || !state.io.frame_admitted
            // Do not spend a maximum Plot frame's CPU/GL time between chunks
            // of the same bounded transfer. Keep the old Preview/document and
            // service native/control events; no deadline, credit or buffer grows.
            || state.io.assembling
            || state.pending
            || state.fatal.is_some()
            || state.io.closing
            || self.replica.document_candidate().is_some()
        {
            return Ok(());
        }
        let Some(preview) = self.preview.as_mut() else {
            return Ok(());
        };
        {
            let device = state.device.borrow();
            let size = device.window.inner_size();
            if size.width != WIDTH as u32
                || size.height != HEIGHT as u32
                || device.window.scale_factor() != 1.
            {
                return Ok(());
            }
        }
        state.render_owner_progress = state.render_owner_progress.saturating_add(1);
        let frame_started = Instant::now();
        if state.mode == "pending" && state.completed != 0 {
            state.device.borrow_mut().wait_fault =
                Some(crate::readonly_v2::completion::Completion::Timeout);
        }
        if !state.fault_armed
            && state.completed != 0
            && crate::fault::MODES.contains(&state.mode.as_str())
        {
            crate::fault::arm(&mut state.device.borrow_mut(), &state.mode);
            state.fault_armed = true;
            state.note("fault-armed", json!({"mode":state.mode,"artificial":true,
                "completed_before_fault":state.completed,"ledger":format!("{:?}",state.device.borrow().ledger())}))?;
        }
        let header = self
            .replica
            .accepted(1)
            .filter(|h| h.epoch == state.document.epoch);
        if let Some(header) = header
            && header.snapshot != state.observed_snapshot
        {
            preview.close_plot();
            preview.navigate(crate::readonly_v2::Location::default());
            state.plot = false;
            state.observed_snapshot = header.snapshot;
        }
        let missing = Missing {
            epoch: PresentationEpoch {
                owner: state.document.epoch.owner,
                revision: state.document.epoch.revision,
            },
            incarnation: state.io.identity.incarnation,
        };
        // The selected view is gone before GPU waiting/readback or I/O service.
        let (mut frame, view_bytes, current) = {
            let selected = self
                .replica
                .selected_provider(Instant::now())
                .map_err(str::to_owned)?;
            let provider: &dyn ReadOnlyProvider = if header.is_some() {
                selected.as_ref().map_or(&missing, |p| p)
            } else {
                &missing
            };
            let view_bytes = selected.as_ref().map_or(0, |p| p.accounted_bytes());
            let input = egui::RawInput {
                events: std::mem::take(&mut state.input),
                ..Default::default()
            };
            (
                preview
                    .run(provider, input)
                    .map_err(|e| format!("prepare: {e:?}"))?,
                view_bytes,
                provider.current(),
            )
        };
        let report = frame.report();
        let generation = frame.generation();
        if report.geometry_failures != 0
            || report.geometry_unavailable != 0
            || report.allocation_faults != 0
        {
            return Err(format!("geometry: {report:?}"));
        }
        let key = (
            state.document.snapshot,
            header.map_or(0, |h| h.snapshot),
            current,
            state.plot,
        );
        let capture = state.capture_key != Some(key)
            && match state.mode.as_str() {
                "maximum-repeated" => state.captures == 0 || report.plot_point_visits == 262_144,
                "maximum-distinct" => state.captures == 0 || report.plot_point_visits == 32_768,
                _ => true,
            };
        let mut labels = String::new();
        let mut points = [[0f32; 2]; 32];
        let mut samples = 0;
        if capture {
            for shape in frame.shapes() {
                match &shape.shape {
                    egui::Shape::Text(text)
                        if labels.len() + text.galley.text().len()
                            < crate::readonly_v2::MAX_TEXT_BYTES =>
                    {
                        labels.push_str(text.galley.text());
                        labels.push('\n');
                    }
                    egui::Shape::Mesh(mesh)
                        if samples < points.len()
                            && !mesh.vertices.is_empty()
                            && mesh
                                .vertices
                                .iter()
                                .all(|v| v.color == egui::Color32::from_rgb(0, 200, 220)) =>
                    {
                        if let Some(point) = mesh
                            .vertices
                            .chunks_exact(4)
                            .map(|q| (q[0].pos + q[2].pos.to_vec2()) / 2.)
                            .find(|p| {
                                p.x > 200.
                                    && p.x < 1000.
                                    && p.y > 80.
                                    && p.y < 520.
                                    && shape.clip_rect.contains(*p)
                            })
                        {
                            points[samples] = [point.x, point.y];
                            samples += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        frame
            .tessellate()
            .map_err(|e| format!("tessellation: {e:?}"))?;
        match frame.finish() {
            Ok(FrameOutcome::HostCompleted { degraded: false }) => {}
            Err(FrameError::Completion) => {
                state.pending = true;
                state.pending_since.get_or_insert_with(Instant::now);
                state.note("pending",json!({"cleanup":format!("{:?}",preview.cleanup_outcome()),"ledger":format!("{:?}",state.device.borrow().ledger()),"completed":state.completed,"document_snapshot":state.document.snapshot,"assembly_ready":self.replica.document_candidate().is_some()}))?;
                return Ok(());
            }
            other => return Err(format!("frame completion: {other:?}")),
        }
        state.completed = state.completed.saturating_add(1);
        state.expected_frame_since = None;
        if state.mode == "snapshot-stall"
            && state.plot
            && report.plot_points >= 64
            && !state.snapshot_stalled
        {
            state.snapshot_stalled = true;
            state.note("snapshot-stalled",json!({"completed":state.completed,"source":header.map_or(0,|h|h.source),"observation_snapshot":header.map_or(0,|h|h.snapshot)}))?;
        }
        if capture {
            if state.captures >= 24 {
                return Err("bounded framebuffer evidence limit".into());
            }
            state.captures += 1;
            let name = format!("frame-{:02}", state.captures);
            let mut device = state.device.borrow_mut();
            let counters = device.counters.clone();
            let pixels = device.readback()?;
            let cyan = |x: usize, y: usize| {
                let i = ((HEIGHT - 1 - y) * WIDTH + x) * 4;
                pixels[i] < 40 && pixels[i + 1] > 150 && pixels[i + 2] > 170
            };
            let colored: usize = (80..520)
                .map(|y| (200..1000).filter(|&x| cyan(x, y)).count())
                .sum();
            let matched = points[..samples]
                .iter()
                .filter(|p| {
                    let x = p[0].round() as usize;
                    let y = p[1].round() as usize;
                    (y.saturating_sub(2)..=(y + 2).min(HEIGHT - 1)).any(|yy| {
                        (x.saturating_sub(2)..=(x + 2).min(WIDTH - 1)).any(|xx| cyan(xx, yy))
                    })
                })
                .count();
            File::create(state.directory.join(format!("{name}.rgba")))
                .and_then(|mut f| f.write_all(pixels))
                .map_err(|e| e.to_string())?;
            state.note(&name,json!({"document_snapshot":state.document.snapshot,"epoch_revision":state.document.epoch.revision,"observation_snapshot":header.map_or(0,|h|h.snapshot),"source":header.map_or(0,|h|h.source),"current":current,"plot":state.plot,
                "width":WIDTH,"height":HEIGHT,"scale":1,"generation":format!("{generation:?}"),"labels":labels,
                "plot_points":report.plot_points,"plot_traces":report.plot_traces,"plot_point_visits":report.plot_point_visits,"geometry_segments":report.geometry_segments,"geometry_failures":report.geometry_failures,
                "curve_samples":&points[..samples],"matched_samples":matched,"cyan_pixels":colored,
                "owner_progress":state.owner_progress,"render_owner_progress":state.render_owner_progress,"completed":state.completed,
                "owner_frame_elapsed_ns":frame_started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                "native_counters":format!("{counters:?}"),"own_objects":counters.objects,
                "cpu_preview_allowance":PREVIEW_RESERVATION,"selected_view_accounted":view_bytes,"encoded_slots":SLOTS,
                "encoded_storage_bound":ENCODED_STORAGE,"readback_bytes":crate::device::READBACK_BYTES}))?;
            if matched != samples {
                return Err("actual GL curve sample mismatch".into());
            }
            device.present()?;
            state.capture_key = Some(key);
        } else {
            state.device.borrow_mut().present()?;
        }
        // A costly frame must not make another redraw immediately due. The
        // bounded native/IPC owner gets its ordinary turns after real completion
        // and evidence/present work, even when that frame exceeded 40 ms.
        state.last_draw = Instant::now();
        if !state.plot {
            state.plot = preview.open_plot(1); // local test-profile navigation only
            if !state.plot
                && preview.navigate(crate::readonly_v2::Location {
                    panel: 1,
                    ..Default::default()
                })
            {
                state.plot = preview.open_plot(0);
            }
        }
        if state.mode == "crash" {
            std::process::exit(73);
        }
        // The parent fixture publishes these selected points only after its
        // original supervisor confirms Ready. A pre-admission Status is not
        // evidence that the parent accepted owner progress.
        if state.mode == "owner-stall"
            && state.io.reported_frame_completion
            && report.plot_points >= 64
        {
            loop {
                std::thread::park();
            }
        }
        Ok(())
    }
}
impl ApplicationHandler for Display<'_, '_> {
    fn resumed(&mut self, _: &ActiveEventLoop) {
        self.state.owner_progress = self.state.owner_progress.saturating_add(1);
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        self.state.owner_progress = self.state.owner_progress.saturating_add(1);
        self.state.suspension.minimized = self
            .state
            .device
            .borrow()
            .window
            .is_minimized()
            .unwrap_or(false);
        self.probe_window();
        let expected = self.preview.is_some()
            && !self.state.suspension.paused()
            && !self.state.pending
            && !self.state.io.assembling
            && !self.state.io.closing
            && self.state.fatal.is_none()
            && self.replica.document_candidate().is_none();
        if expected {
            let since = self
                .state
                .expected_frame_since
                .get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_secs(2) {
                self.state
                    .fail("renderer frame progress deadline while drawable admitted".into());
            }
        } else {
            self.state.expected_frame_since = None;
        }
        if self.state.last_draw.elapsed() >= Duration::from_millis(40)
            && !(self.state.mode == "redraw-stall" && self.state.completed != 0)
        {
            self.state.device.borrow().window.request_redraw();
            self.state.last_draw = Instant::now();
        }
    }
    fn window_event(&mut self, _: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.state.device.borrow().window.id() != id {
            return;
        }
        self.state.owner_progress = self.state.owner_progress.saturating_add(1);
        let input = match event {
            WindowEvent::RedrawRequested => {
                if !(self.state.mode == "redraw-stall" && self.state.completed != 0)
                    && let Err(error) = self.draw()
                {
                    self.state.fail(error);
                }
                None
            }
            WindowEvent::CloseRequested => {
                self.state
                    .fail("native close requested; awaiting supervisor containment".into());
                None
            }
            WindowEvent::Resized(size) => {
                self.state.suspension.resize(size.width, size.height);
                self.suspension_receipt("resize");
                None
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.state.suspension.scale(scale_factor);
                self.suspension_receipt("scale");
                None
            }
            WindowEvent::Occluded(hidden) => {
                self.state.suspension.occluded = hidden;
                self.suspension_receipt("occluded");
                None
            }
            WindowEvent::Focused(focus) => Some(egui::Event::WindowFocused(focus)),
            WindowEvent::CursorMoved { position, .. }
                if position.x.is_finite()
                    && position.y.is_finite()
                    && position.x.abs() <= 8192.
                    && position.y.abs() <= 8192. =>
            {
                self.state.pointer = egui::pos2(position.x as f32, position.y as f32);
                Some(egui::Event::PointerMoved(self.state.pointer))
            }
            WindowEvent::CursorLeft { .. } => Some(egui::Event::PointerGone),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => Some(egui::Event::PointerButton {
                pos: self.state.pointer,
                button: egui::PointerButton::Primary,
                pressed: state == ElementState::Pressed,
                modifiers: self.state.modifiers,
            }),
            WindowEvent::ModifiersChanged(modifiers) => {
                let modifiers = modifiers.state();
                self.state.modifiers = egui::Modifiers {
                    alt: modifiers.alt_key(),
                    ctrl: modifiers.control_key(),
                    shift: modifiers.shift_key(),
                    mac_cmd: false,
                    command: modifiers.control_key(),
                };
                None
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::{Key, NamedKey};
                let key = match event.logical_key {
                    Key::Named(NamedKey::ArrowUp) => Some(egui::Key::ArrowUp),
                    Key::Named(NamedKey::ArrowDown) => Some(egui::Key::ArrowDown),
                    Key::Named(NamedKey::ArrowLeft) => Some(egui::Key::ArrowLeft),
                    Key::Named(NamedKey::ArrowRight) => Some(egui::Key::ArrowRight),
                    Key::Named(NamedKey::Tab) => Some(egui::Key::Tab),
                    Key::Named(NamedKey::Enter) => Some(egui::Key::Enter),
                    Key::Named(NamedKey::Escape) => Some(egui::Key::Escape),
                    Key::Named(NamedKey::Space) => Some(egui::Key::Space),
                    Key::Named(NamedKey::Home) => Some(egui::Key::Home),
                    Key::Named(NamedKey::End) => Some(egui::Key::End),
                    _ => None,
                };
                key.map(|key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: event.state == ElementState::Pressed,
                    repeat: event.repeat,
                    modifiers: self.state.modifiers,
                })
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let (unit, x, y) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (egui::MouseWheelUnit::Line, x, y)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        (egui::MouseWheelUnit::Point, pos.x as f32, pos.y as f32)
                    }
                };
                (x.is_finite() && y.is_finite()).then(|| egui::Event::MouseWheel {
                    unit,
                    delta: egui::vec2(x.clamp(-4096., 4096.), y.clamp(-4096., 4096.)),
                    modifiers: self.state.modifiers,
                    phase: match phase {
                        winit::event::TouchPhase::Started => egui::TouchPhase::Start,
                        winit::event::TouchPhase::Moved => egui::TouchPhase::Move,
                        winit::event::TouchPhase::Ended => egui::TouchPhase::End,
                        winit::event::TouchPhase::Cancelled => egui::TouchPhase::Cancel,
                    },
                })
            }
            WindowEvent::Ime(Ime::Enabled | Ime::Disabled) => None,
            WindowEvent::Ime(Ime::Preedit(text, range)) if text.len() <= 512 => {
                let active_range_chars = range.and_then(|(start, end)| {
                    Some(text.get(..start)?.chars().count()..text.get(..end)?.chars().count())
                });
                Some(egui::Event::Ime(egui::ImeEvent::Preedit {
                    text,
                    active_range_chars,
                }))
            }
            WindowEvent::Ime(Ime::Commit(text)) if text.len() <= 512 => {
                Some(egui::Event::Ime(egui::ImeEvent::Commit(text)))
            }
            _ => None,
        };
        if let Some(input) = input
            && self.state.input.len() < 64
        {
            self.state.input.push(input);
        }
    }
}

pub(super) fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 6 || args.iter().any(|s| s.len() > 256) || args[2].len() != 32 {
        return Err("child launch bounds".into());
    }
    let mode = args[5].clone();
    if ![
        "normal",
        "crash",
        "pending",
        "snapshot-stall",
        "owner-stall",
        "maximum-repeated",
        "maximum-distinct",
        "window-profile",
        "redraw-stall",
    ]
    .contains(&mode.as_str())
        && !crate::fault::MODES.contains(&mode.as_str())
    {
        return Err("child qualification mode".into());
    }
    let mut workbench = [0; 16];
    for (index, byte) in workbench.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&args[2][index * 2..index * 2 + 2], 16)
            .map_err(|e| e.to_string())?;
    }
    let identity = Identity {
        workbench,
        incarnation: args[3].parse().map_err(|_| "incarnation")?,
    };
    let parent = args[4].parse().map_err(|_| "parent process identity")?;
    let mut control = Framed::new(Pipe::client(&args[0]).map_err(|e| format!("{e:?}"))?);
    let mut snapshot = Framed::new(Pipe::client(&args[1]).map_err(|e| format!("{e:?}"))?);
    if !control
        .pipe
        .connected(parent)
        .map_err(|e| format!("{e:?}"))?
        || !snapshot
            .pipe
            .connected(parent)
            .map_err(|e| format!("{e:?}"))?
    {
        return Err("original parent pipe identity".into());
    }
    let directory = PathBuf::from("target/isolated-native-renderer")
        .join(format!("{}-{}-{}", parent, args[2], identity.incarnation));
    fs::create_dir_all(directory.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    fs::write(
        directory.join("startup.json"),
        serde_json::to_vec(&json!({"stage":"startup","pid":std::process::id(),
            "original_parent_pid":parent,"workbench_id":args[2],"incarnation":identity.incarnation,
            "deployment_C":null}))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    // Exact existing B2B2 qualification profile: the compositor may scale the
    // window, but Preview, GL drawable and readback stay 1280x960 at scale one.
    // This is process-local, not a change to the desktop's DPI configuration.
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut events = EventLoop::builder()
        .with_dpi_aware(false)
        .build()
        .map_err(|e| {
            let error = e.to_string();
            let _ = fs::write(
                directory.join("startup-error.json"),
                serde_json::to_vec(&json!({"stage":"event_loop","error":error})).unwrap(),
            );
            error
        })?;
    let mut boot = Bootstrap::default();
    while boot.device.is_none() && boot.error.is_none() {
        events.pump_app_events(Some(Duration::from_millis(1)), &mut boot);
    }
    if let Some(profile) = boot.profile {
        let _ = fs::write(
            directory.join("native-profile.json"),
            serde_json::to_vec_pretty(&profile).unwrap(),
        );
    }
    if let Some(error) = boot.error {
        let _ = fs::write(
            directory.join("startup-error.json"),
            serde_json::to_vec(&json!({"stage":"native_device","error":error})).unwrap(),
        );
        return Err(error);
    }
    let device = Rc::new(RefCell::new(boot.device.take().unwrap()));
    if mode == "gpu-original-error" {
        crate::fault::arm(&mut device.borrow_mut(), &mode);
    }
    let mut session = HostSession::new(Backend {
        device: device.clone(),
        inject_unresolved: false,
        fail_submission: mode == "gpu-original-error",
    });
    let (mut replica, mut owner) = Replica::renderer(&mut session);
    let original_error = mode == "gpu-original-error";
    let mut state = State {
        io: Channels {
            identity,
            control,
            snapshot,
            sequence: Sequence::default(),
            hello: false,
            hello_ack: false,
            challenge: None,
            closing: false,
            pending: [None; 2],
            closed_sent: false,
            diagnostic: None,
            reported_frame_completion: false,
            document_applied_sent: false,
            post_admission_status: false,
            frame_admitted: false,
            assembling: false,
        },
        device,
        directory,
        mode,
        owner_progress: 0,
        render_owner_progress: 0,
        completed: 0,
        fatal: None,
        suspension: crate::drawable::Suspension::default(),
        pending: false,
        snapshot_stalled: false,
        fault_armed: original_error,
        pending_since: None,
        window_probe: (0, Instant::now(), 0, 0),
        expected_frame_since: None,
        pointer: egui::Pos2::ZERO,
        modifiers: egui::Modifiers::NONE,
        input: Vec::new(),
        last_draw: Instant::now(),
        captures: 0,
        admissions: 0,
        capture_key: None,
        observed_snapshot: 0,
        plot: false,
        document: Header::control(Kind::Hello, identity, 1),
        deadline: Instant::now() + Duration::from_secs(30),
    };
    if state.note("context",json!({"stage":"window_context","width":WIDTH,"height":HEIGHT,"scale":state.device.borrow().window.scale_factor(),"ledger":format!("{:?}",state.device.borrow().ledger())})).is_err(){std::process::exit(94);}
    loop {
        if owner.document().is_none() {
            state.service(&mut replica, false);
            if replica.document_candidate().is_some() && !state.io.closing && state.fatal.is_none()
            {
                match replica.admit_renderer_document(&mut owner) {
                    Ok(decision) => {
                        state.io.pending[0] = Some((decision.header, decision.applied));
                        if decision.applied {
                            state.document = decision.header;
                            state.plot = false;
                            state.capture_key = None;
                        }
                    }
                    Err(error) => {
                        let _=state.note("rejected",json!({"admission_error":error,"accepted":format!("{:?}",replica.accepted(0))}));
                        let _ = replica.restore_renderer_document(&mut owner);
                    }
                }
            }
        }
        if owner.document().is_some() && !state.io.closing && state.fatal.is_none() {
            let (document, header, session) = owner.parts().unwrap();
            state.document = header;
            let allocation = document.allocation();
            state.admissions += 1;
            if state.admissions > 8 {
                std::process::exit(97);
            }
            if state.note(&format!("admitted-{}",state.admissions),json!({"epoch_revision":header.epoch.revision,"snapshot":header.snapshot,"document_id":document.dto().document_id.as_str(),"canonical_bytes":document.canonical_bytes().len(),"parsed_accounted":allocation.accounted_bytes,"max_live_parsed":1})).is_err(){std::process::exit(94);}
            state.device.borrow().window.set_visible(true);
            let mut preview = Preview::with_session(
                document,
                PresentationEpoch {
                    owner: header.epoch.owner,
                    revision: header.epoch.revision,
                },
                session,
            );
            {
                let mut display = Display {
                    preview: Some(&mut preview),
                    replica: &mut replica,
                    state: &mut state,
                };
                let mut retiring = false;
                let mut handoff_noted = false;
                let mut retained = None;
                let mut retained_polls = 0;
                loop {
                    display.state.service(display.replica, false);
                    if matches!(
                        events.pump_app_events(Some(Duration::from_millis(1)), &mut display),
                        PumpStatus::Exit(_)
                    ) {
                        display
                            .state
                            .fail("event loop exit before retirement".into());
                    }
                    let end = display.replica.document_candidate().is_some()
                        || display.state.io.closing
                        || display.state.fatal.is_some();
                    if end && !retiring {
                        display.preview.as_mut().unwrap().retire_context();
                        retiring = true;
                    }
                    if display.state.pending || retiring {
                        if display.state.mode == "gpu-timeout-proof"
                            && display
                                .state
                                .pending_since
                                .is_some_and(|since| since.elapsed() >= Duration::from_millis(120))
                        {
                            display.state.device.borrow_mut().wait_fault = None;
                        }
                        let outcome = display
                            .preview
                            .as_mut()
                            .unwrap()
                            .poll_retirement(WaitPolicy::default());
                        if matches!(outcome, CleanupOutcome::Idle | CleanupOutcome::Retired(_)) {
                            if display.state.pending && display.state.mode == "gpu-timeout-proof" {
                                let _ = display.state.note("timeout-proof", json!({"cleanup":format!("{outcome:?}"),
                                    "ledger":format!("{:?}",display.state.device.borrow().ledger()),
                                    "counters":format!("{:?}",display.state.device.borrow().counters),
                                    "actual_fence_proof_after_artificial_timeout":true,"replayed_submission":false}));
                            }
                            display.state.pending = false;
                            if retiring {
                                break;
                            }
                        } else if !handoff_noted {
                            let _=display.state.note("handoff-pending",json!({"old_document_snapshot":header.snapshot,"candidate":format!("{:?}",display.replica.document_candidate()),"cleanup":format!("{outcome:?}"),"ledger":format!("{:?}",display.state.device.borrow().ledger()),"max_live_parsed":1,"assembly_retained":true}));
                            handoff_noted = true;
                        }
                        if !matches!(outcome, CleanupOutcome::Idle | CleanupOutcome::Retired(_))
                            && display.state.fault_armed
                            && display.state.mode != "gpu-timeout-proof"
                        {
                            let device = display.state.device.borrow();
                            let witness = (device.ledger(), device.delete_calls);
                            if let Some(original) = retained {
                                if witness != original {
                                    std::process::exit(98);
                                }
                            } else {
                                retained = Some(witness);
                            }
                            retained_polls += 1;
                            if retained_polls == 100 {
                                let _ = display.state.note("fault-retained", json!({
                                    "mode":display.state.mode,"artificial":true,"polls":retained_polls,
                                    "original_ledger":format!("{:?}",witness.0),"delete_calls":witness.1,
                                    "cleanup":format!("{outcome:?}"),"execution_error":display.state.fatal,
                                    "device_error":device.fault,"old_preview_document_retained":true,
                                    "max_live_parsed":1,"new_generation":false,"blind_retry":false}));
                            }
                        }
                    }
                    display.state.expired();
                }
            }
            drop(preview);
            if owner.release_retired().is_err() {
                std::process::exit(95);
            }
            state.device.borrow().window.set_visible(false);
        }
        if state.io.closing || state.fatal.is_some() {
            replica.stop();
            if matches!(
                owner.session_mut().shutdown(WaitPolicy::default()),
                ShutdownOutcome::Released
            ) {
                if owner.release_retired().is_err() {
                    std::process::exit(95);
                }
                let native_release = state.device.borrow_mut().healthy_close();
                if let Err(error) = native_release {
                    let _ = state.note(
                        "native-close-error",
                        json!({"error":error,
                        "own_objects":state.device.borrow().counters.objects,
                        "ledger":format!("{:?}",state.device.borrow().ledger()),
                        "session_released":true,"native_root_retained":true,
                        "retry":false,"final_wgl_destruction":"LIMITED/no new receipt"}),
                    );
                    state.pending = true;
                    let retained_owner = state.owner_progress;
                    let mut retained_pumps = 0;
                    // A failed native release is not retried and cannot become
                    // Closed/ExitedHealthy. Keep these original lexical roots
                    // alive while the owner services IPC/native events, until
                    // original-process OS containment or the qualification limit.
                    loop {
                        state.service(&mut replica, false);
                        events.pump_app_events(
                            Some(Duration::from_millis(1)),
                            &mut Display {
                                preview: None,
                                replica: &mut replica,
                                state: &mut state,
                            },
                        );
                        retained_pumps += 1;
                        if retained_pumps == 100 {
                            let _ = state.note("native-close-retained", json!({
                                "original_event_loop":retained_owner,"event_loop":state.owner_progress,
                                "owner_pumps":retained_pumps,"native_root_retained":true,
                                "ledger":format!("{:?}",state.device.borrow().ledger()),
                                "healthy_close":false,"release_retry":false}));
                        }
                    }
                }
                let _=state.note("own-retired",json!({"own_objects":state.device.borrow().counters.objects,"ledger":format!("{:?}",state.device.borrow().ledger()),"counters":format!("{:?}",state.device.borrow().counters),"final_wgl_destruction":"LIMITED/no new receipt","healthy_own_objects":true}));
                loop {
                    state.service(&mut replica, state.fatal.is_none());
                    if state.fatal.is_some() || (state.io.closed_sent && state.io.control.idle()) {
                        break;
                    }
                    events.pump_app_events(
                        Some(Duration::from_millis(1)),
                        &mut Display {
                            preview: None,
                            replica: &mut replica,
                            state: &mut state,
                        },
                    );
                }
                loop {
                    let a = state.io.control.pipe.retire().unwrap_or(false);
                    let b = state.io.snapshot.pipe.retire().unwrap_or(false);
                    if a && b {
                        break;
                    }
                    state.expired();
                    std::thread::yield_now();
                }
                let _=state.note("ipc-retired",json!({"original_control_receipts":true,"original_snapshot_receipts":true,"own_objects":0}));
                if let Some(error) = state.fatal.take() {
                    return Err(error);
                }
                return Ok(());
            }
        }
        events.pump_app_events(
            Some(Duration::from_millis(1)),
            &mut Display {
                preview: None,
                replica: &mut replica,
                state: &mut state,
            },
        );
        state.expired();
    }
}

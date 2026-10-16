//! Stock eframe App on the original bounded, read-only Parent IPC.
use crate::{
    Result, diagnostics,
    private_ipc::{Replica, UiDocument, protocol::*, transport::Framed},
    ui, write,
};
use eframe::egui;
use lab_windows_job::pipe::Pipe;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::json;
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    time::{Duration, Instant},
};

// Opt-in diagnostic files are finite; the screenshot queue remains capacity one.
const CAPTURE_LIMIT: usize = 128;

struct Channels {
    identity: Identity,
    control: Framed<CONTROL>,
    snapshot: Framed<CHUNK>,
    sequence: Sequence,
    hello: bool,
    acknowledged: bool,
    challenge: Option<u64>,
    closing: bool,
    window_close: bool,
    window_close_sent: bool,
    closed_sent: bool,
    closed_complete: bool,
    pending: [Option<(Header, bool)>; 2],
}
impl Channels {
    fn service(
        &mut self,
        replica: &mut Replica,
        document: &mut UiDocument,
        counter: u64,
    ) -> Result<()> {
        if self.control.poll_send().map_err(|e| format!("{e:?}"))? && self.closed_sent {
            self.closed_complete = true;
        }
        if self.closed_complete {
            return Ok(());
        }
        let now = Instant::now();
        let mut bytes = OWNER_BYTES;
        let mut messages = OWNER_MESSAGES;
        // eframe deliberately throttles hidden-window logic to about 100 ms.
        // A framed read needs separate header/payload completion steps. Drive
        // up to four nonblocking steps here so challenges do not wait several
        // hidden ticks. All steps SHARE the original byte/message allowance;
        // this is neither a worker nor fabricated paint/GPU progress.
        for _ in 0..OWNER_MESSAGES {
            if messages > 0
                && let Some((header, payload)) = self
                    .control
                    .receive_bounded(self.identity, now, bytes)
                    .map_err(|e| format!("{e:?}"))?
            {
                bytes -= header.bytes;
                messages -= 1;
                match header.kind {
                    Kind::Hello if !self.hello && payload.is_empty() => self.hello = true,
                    Kind::Challenge | Kind::Close if self.acknowledged && payload.len() == 8 => {
                        replica.parent_progress(now);
                        self.challenge = Some(u64::from_le_bytes(payload.try_into().unwrap()));
                        if header.kind == Kind::Close {
                            self.closing = true;
                            replica.stop();
                        }
                    }
                    Kind::Fence if self.acknowledged && payload.len() == 16 => replica
                        .fence(
                            u64::from_le_bytes(payload[..8].try_into().unwrap()),
                            u64::from_le_bytes(payload[8..].try_into().unwrap()),
                            now,
                        )
                        .map_err(str::to_owned)?,
                    _ => return Err("stock child control direction/order".into()),
                }
            }
            if !self.closing {
                replica.expire(now).map_err(str::to_owned)?;
                if messages > 0
                    && self.acknowledged
                    && let Some((header, payload)) = self
                        .snapshot
                        .receive_bounded(self.identity, now, bytes)
                        .map_err(|e| format!("{e:?}"))?
                {
                    bytes -= header.bytes;
                    messages -= 1;
                    if !replica.chunk(header, payload, now).map_err(str::to_owned)? {
                        continue;
                    }
                    if header.kind == Kind::Document {
                        let decision =
                            replica.admit_ui_document(document).map_err(str::to_owned)?;
                        self.pending[0] = Some((decision.header, decision.applied));
                    } else {
                        self.pending[1] = Some((
                            header,
                            replica
                                .accepted(1)
                                .is_some_and(|h| h.snapshot == header.snapshot),
                        ));
                    }
                }
            }
        }
        if !self.control.idle() {
            return Ok(());
        }
        if !(self.hello && !self.acknowledged
            || self.closing && !self.closed_sent
            || self.pending.iter().any(Option::is_some)
            || self.window_close && !self.window_close_sent && self.acknowledged
            || self.challenge.is_some() && !self.closed_sent)
        {
            return Ok(());
        }
        let mut header = Header::control(
            Kind::Status,
            self.identity,
            self.sequence.next().map_err(str::to_owned)?,
        );
        if self.hello && !self.acknowledged {
            header.kind = Kind::HelloAck;
            self.control
                .send(header, &[])
                .map_err(|e| format!("{e:?}"))?;
            self.acknowledged = true;
        } else if self.closing && !self.closed_sent {
            header.kind = Kind::Closed;
            self.status(header, replica, counter)?;
            self.closed_sent = true;
        } else if self.challenge.is_some() && !self.closed_sent {
            self.status(header, replica, counter)?;
        } else if self.window_close && !self.window_close_sent && self.acknowledged {
            header.kind = Kind::WindowClose;
            self.control
                .send(header, &[])
                .map_err(|e| format!("{e:?}"))?;
            self.window_close_sent = true;
        } else if let Some(index) = self.pending.iter().position(Option::is_some) {
            let (decision, applied) = self.pending[index].take().unwrap();
            header.kind = if applied {
                Kind::Applied
            } else {
                Kind::Skipped
            };
            header.bytes += 1;
            header.epoch = decision.epoch;
            header.snapshot = decision.snapshot;
            header.source = decision.source;
            self.control
                .send(header, &[index as u8])
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }
    fn status(&mut self, mut header: Header, replica: &mut Replica, counter: u64) -> Result<()> {
        header.bytes += 32;
        let mut payload = [0; 32];
        payload[..8].copy_from_slice(&self.challenge.take().unwrap_or(0).to_le_bytes());
        payload[8..16].copy_from_slice(&counter.to_le_bytes());
        payload[16..24]
            .copy_from_slice(&replica.accepted(1).map_or(0, |h| h.snapshot).to_le_bytes());
        payload[24] = u8::from(replica.read_is_current(Instant::now()));
        self.control
            .send(header, &payload)
            .map_err(|e| format!("{e:?}"))
    }
}
struct Capture {
    image: Arc<egui::ColorImage>,
    index: usize,
}
fn capture_worker(directory: PathBuf) -> Result<SyncSender<Capture>> {
    let (sender, receiver) = sync_channel::<Capture>(1);
    std::thread::Builder::new()
        .name("eframe-test-capture".into())
        .spawn(move || {
            while let Ok(capture) = receiver.recv() {
                let image = capture.image;
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                let temporary = directory.join(format!("capture-{:02}.png", capture.index));
                let destination = directory.join(format!("frame-{:02}.png", capture.index));
                let saved = image::save_buffer(
                    &temporary,
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                )
                .map_err(|e| e.to_string())
                .and_then(|()| std::fs::rename(temporary, destination).map_err(|e| e.to_string()));
                if let Err(error) = saved {
                    write(
                        &directory.join("capture-error.json"),
                        &json!({"error":error.to_string()}),
                    );
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(sender)
}
struct Host {
    capture_sender: SyncSender<Capture>,
    io: Rc<RefCell<Channels>>,
    replica: Replica,
    document: UiDocument,
    directory: PathBuf,
    capture: bool,
    captures: usize,
    last_capture: Instant,
    counter: u64,
    ui_counter: u64,
    last_lifecycle: Instant,
    navigation: ui::Navigation,
    fatal: Option<String>,
    closing_since: Option<Instant>,
}
impl eframe::App for Host {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(16));
        self.counter = self.counter.checked_add(1).expect("UI counter exhausted");
        if ctx.input(|i| i.viewport().close_requested()) && !self.io.borrow().closed_complete {
            self.io.borrow_mut().window_close = true;
            self.closing_since.get_or_insert_with(Instant::now);
        }
        if let Err(error) =
            self.io
                .borrow_mut()
                .service(&mut self.replica, &mut self.document, self.counter)
        {
            write(
                &self.directory.join("failure.json"),
                &json!({"error":error}),
            );
            self.fatal = Some(error);
        }
        if self.capture && self.last_lifecycle.elapsed() >= Duration::from_secs(1) {
            write(
                &self.directory.join("lifecycle.json"),
                &json!({
                "owner_counter":self.counter,"ui_counter":self.ui_counter,
                "minimized":ctx.input(|i|i.viewport().minimized),
                "document_revision":self.document.identity().map(|h|h.epoch.revision),
                "observation_snapshot":self.replica.accepted(1).map(|h|h.snapshot)}),
            );
            self.last_lifecycle = Instant::now();
        }
        if self.io.borrow().closing {
            self.closing_since.get_or_insert_with(Instant::now);
        }
        if self.io.borrow().closed_complete
            || self.fatal.is_some()
            || self
                .closing_since
                .is_some_and(|since| since.elapsed() >= Duration::from_secs(2))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui_counter = self
            .ui_counter
            .checked_add(1)
            .expect("paint counter exhausted");
        let ctx = ui.ctx().clone();
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Screenshot {
                user_data, image, ..
            } = event
            {
                let Some(index) = user_data
                    .data
                    .as_ref()
                    .and_then(|data| data.downcast_ref::<usize>())
                    .copied()
                else {
                    continue;
                };
                if index < CAPTURE_LIMIT && image.size[0] <= 3200 && image.size[1] <= 2000 {
                    match self.capture_sender.try_send(Capture { image, index }) {
                        Ok(()) | Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => {
                            self.fatal = Some("capture worker disconnected".into())
                        }
                    }
                }
            }
        }
        let mut report = ui::Report::default();
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(error) = &self.fatal {
                ui.colored_label(egui::Color32::RED, error);
            }
            if let Some(document) = self.document.document() {
                let header = self.document.identity().unwrap();
                match self.replica.selected_provider(Instant::now()) {
                    Ok(reads) => {
                        report =
                            ui::paint(ui, document, header, reads.as_ref(), &mut self.navigation)
                    }
                    Err(error) => {
                        ui.label(format!("Unavailable: {error}"));
                    }
                }
            } else {
                ui.heading("Waiting for Parent-approved presentation");
            }
        });
        if self.capture
            && self.captures < CAPTURE_LIMIT
            && self.document.document().is_some()
            && self.last_capture.elapsed() >= Duration::from_millis(750)
        {
            let index = self.captures;
            write(
                &self.directory.join(format!("frame-{index:02}.json")),
                &json!({"incarnation":self.io.borrow().identity.incarnation,
                "document_revision":self.document.identity().map(|h|h.epoch.revision),"document_id":self.document.document().map(|d|d.dto().document_id.as_str()),
                "observation_snapshot":self.replica.accepted(1).map(|h|h.snapshot),"owner_counter":self.counter,"ui_counter":self.ui_counter,"display":report,
                "capture_api":"stock eframe Screenshot", "runtime_clients":0}),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                index,
            )));
            self.captures += 1;
            self.last_capture = Instant::now();
        }
        if self.io.borrow().closed_complete
            || self.fatal.is_some()
            || self
                .closing_since
                .is_some_and(|since| since.elapsed() >= Duration::from_secs(2))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        write(
            &self.directory.join("on-exit.json"),
            &json!({"on_exit":true,"closed_write_completed":self.io.borrow().closed_complete,"error":self.fatal}),
        );
    }
}

pub(super) fn run(
    control: &str,
    snapshot: &str,
    id: &str,
    incarnation: &str,
    parent: &str,
    mode: &str,
) -> Result<()> {
    if [control, snapshot, id, incarnation, parent, mode]
        .iter()
        .any(|s| s.len() > 256)
        || id.len() != 32
    {
        return Err("private launch bounds".into());
    }
    let (renderer, capture) = match mode {
        "eframe-wgpu" => (eframe::Renderer::Wgpu, false),
        "eframe-glow" => (eframe::Renderer::Glow, false),
        "eframe-wgpu-capture" => (eframe::Renderer::Wgpu, true),
        "eframe-glow-capture" => (eframe::Renderer::Glow, true),
        _ => return Err("explicit stock renderer required".into()),
    };
    let mut workbench = [0; 16];
    for (i, byte) in workbench.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&id[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
    }
    let identity = Identity {
        workbench,
        incarnation: incarnation.parse().map_err(|_| "incarnation")?,
    };
    let parent = parent.parse().map_err(|_| "parent identity")?;
    let mut control = Framed::new(Pipe::client(control).map_err(|e| format!("{e:?}"))?);
    let mut snapshot = Framed::new(Pipe::client(snapshot).map_err(|e| format!("{e:?}"))?);
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
    let directory = PathBuf::from("target/eframe-2-parent")
        .join(format!("{parent}-{id}-{}", identity.incarnation));
    std::fs::create_dir_all(directory.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
    diagnostics::install(&directory)?;
    let io = Rc::new(RefCell::new(Channels {
        identity,
        control,
        snapshot,
        sequence: Sequence::default(),
        hello: false,
        acknowledged: false,
        challenge: None,
        closing: false,
        window_close: false,
        window_close_sent: false,
        closed_sent: false,
        closed_complete: false,
        pending: [None; 2],
    }));
    let capture_sender = capture_worker(directory.clone())?;
    let channels = io.clone();
    let output = directory.clone();
    let id = id.to_owned();
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("Experimental isolated Workbench")
            .with_inner_size([1100., 720.])
            .with_max_inner_size([1600., 1000.]),
        persist_window: false,
        ..Default::default()
    };
    let result = eframe::run_native(
        "lab-runtime isolated Workbench",
        options,
        Box::new(move |cc| {
            let window = cc.winit_window().ok_or("missing native window")?;
            let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
                return Err("Windows window required".into());
            };
            write(
                &output.join("ready.json"),
                &json!({"pid":std::process::id(),"parent_pid":parent,"workbench_id":id,"incarnation":identity.incarnation,
            "hwnd":handle.hwnd.get(),"renderer":format!("{renderer:?}"),"graphics_owner":"stock eframe","runtime_clients":0,
            "adapter":cc.wgpu_render_state.as_ref().map(|s|format!("{:?}",s.adapter.get_info()))}),
            );
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let (replica, document) = Replica::stock_ui();
            Ok(Box::new(Host {
                capture_sender,
                io: channels,
                replica,
                document,
                directory: output,
                capture,
                captures: 0,
                last_capture: Instant::now(),
                counter: 0,
                ui_counter: 0,
                last_lifecycle: Instant::now(),
                navigation: Default::default(),
                fatal: None,
                closing_since: None,
            }))
        }),
    );
    write(
        &directory.join("returned.json"),
        &json!({"run_native_returned":true,"error":result.as_ref().err().map(ToString::to_string)}),
    );
    // Same original operations are canceled and observed after the eframe owner exits.
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut channels = io.borrow_mut();
    let _ = channels.control.pipe.cancel();
    let _ = channels.snapshot.pipe.cancel();
    let mut retired = false;
    while Instant::now() < deadline {
        let a = channels.control.pipe.retire().unwrap_or(false);
        let b = channels.snapshot.pipe.retire().unwrap_or(false);
        if a && b {
            retired = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    write(
        &directory.join("ipc-exit.json"),
        &json!({"child_pipe_retired":retired,"closed_write_completed":channels.closed_complete}),
    );
    result.map_err(|e| e.to_string())
}

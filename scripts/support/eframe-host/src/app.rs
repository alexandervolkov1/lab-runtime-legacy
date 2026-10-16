//! One shared read-only UI. eframe owns all graphics and window integration.
use crate::{Result, diagnostics, plot_ui, write};
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::json;
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) fn run(renderer: &str, dir: &Path) -> Result<()> {
    let renderer = match renderer {
        "wgpu" => eframe::Renderer::Wgpu,
        "glow" => eframe::Renderer::Glow,
        _ => return Err("explicit stock renderer wgpu|glow required".into()),
    };
    diagnostics::install(dir)?;
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("EFRAME-1 read-only fixture")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 480.0])
            .with_max_inner_size([1600.0, 1000.0]),
        persist_window: false,
        ..Default::default()
    };
    let output = dir.to_owned();
    let result = eframe::run_native(
        "EFRAME-1",
        options,
        Box::new(move |cc| {
            let window = cc.winit_window().ok_or("missing native window")?;
            let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
                return Err("Windows test window required".into());
            };
            let adapter = cc
                .wgpu_render_state
                .as_ref()
                .map(|state| format!("{:?}", state.adapter.get_info()));
            write(
                &output.join("ready.json"),
                &json!({"pid":std::process::id(),
            "hwnd":handle.hwnd.get(),"renderer":format!("{renderer:?}"),"adapter":adapter,
            "glow_context_present":cc.gl.is_some(),"native_scale_factor":window.scale_factor(),
            "graphics_owner":"stock eframe", "runtime_clients":0}),
            );
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Host::new(output)))
        }),
    );
    write(
        &dir.join("returned.json"),
        &json!({"run_native_returned":true,
        "error":result.as_ref().err().map(ToString::to_string)}),
    );
    result.map_err(|e| e.to_string())
}

struct Host {
    dir: PathBuf,
    stage: u8,
    settled: usize,
    captures: usize,
    pointer_moves: usize,
    pointer_buttons: usize,
    keys: usize,
    texts: usize,
    focus_events: usize,
    clicked: usize,
    local_text: String,
    points: [Vec<egui_plot::PlotPoint>; 2],
    button: egui::Pos2,
    editor: egui::Pos2,
    close_requested: bool,
}

impl Host {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            stage: u8::MAX,
            settled: 0,
            captures: 0,
            pointer_moves: 0,
            pointer_buttons: 0,
            keys: 0,
            texts: 0,
            focus_events: 0,
            clicked: 0,
            local_text: String::new(),
            points: Default::default(),
            button: egui::Pos2::ZERO,
            editor: egui::Pos2::ZERO,
            close_requested: false,
        }
    }

    fn receipt(&self, ctx: &egui::Context) -> serde_json::Value {
        // Context accessors acquire the context lock: never nest them in input().
        let pixels_per_point = ctx.pixels_per_point();
        let zoom_factor = ctx.zoom_factor();
        ctx.input(|input| {
            json!({"stage":self.stage,"captures":self.captures,
            "pixels_per_point":pixels_per_point,"zoom_factor":zoom_factor,
            "native_pixels_per_point":input.viewport().native_pixels_per_point,
            "inner_rect":format!("{:?}",input.viewport().inner_rect),
            "pointer_moves":self.pointer_moves,"pointer_buttons":self.pointer_buttons,
            "keys":self.keys,"texts":self.texts,"focus_events":self.focus_events,
            "clicked":self.clicked,"local_text":self.local_text,
            "button_pixels":[self.button.x * pixels_per_point,self.button.y * pixels_per_point],
            "editor_pixels":[self.editor.x * pixels_per_point,self.editor.y * pixels_per_point],
            "close_requested":self.close_requested})
        })
    }

    fn capture(&mut self, ctx: &egui::Context, stage: u8, image: &egui::ColorImage) {
        assert!(stage <= 5 && self.captures < 6);
        let [width, height] = image.size;
        // Bound test capture output, not eframe's internal GPU allocations.
        assert!(width > 0 && height > 0 && width <= 3200 && height <= 2000);
        let mut rgb = Vec::with_capacity(width * height * 3);
        let mut colored = 0_usize;
        let mut bright = 0_usize;
        for pixel in &image.pixels {
            let [r, g, b, _] = pixel.to_array();
            rgb.extend_from_slice(&[r, g, b]);
            colored += usize::from(r.abs_diff(g) > 30 || g.abs_diff(b) > 30);
            bright += usize::from(r > 180 && g > 180 && b > 180);
        }
        assert!(
            bright > 100,
            "standard screenshot contains no readable content"
        );
        if stage > 0 {
            assert!(colored > 100, "missing colored plot pixels");
        }
        image::save_buffer(
            self.dir.join(format!("frame-{stage}.png")),
            &rgb,
            width as u32,
            height as u32,
            image::ColorType::Rgb8,
        )
        .unwrap();
        self.captures += 1;
        let mut receipt = self.receipt(ctx);
        receipt["width"] = json!(width);
        receipt["height"] = json!(height);
        receipt["bright_pixels"] = json!(bright);
        receipt["colored_pixels"] = json!(colored);
        receipt["capture_api"] = json!("eframe ViewportCommand::Screenshot / Event::Screenshot");
        write(&self.dir.join(format!("frame-{stage}.json")), &receipt);
    }
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.request_repaint_after(Duration::from_millis(33));
        let mut command = [0_u8; 1];
        let next = std::fs::File::open(self.dir.join("stage"))
            .and_then(|file| file.take(1).read_exact(&mut command))
            .map_or(0, |_| command[0].saturating_sub(b'0'));
        if next == 6 {
            write(
                &self.dir.join("crash-entered.json"),
                &json!({"test_fault":"process abort after stock frame"}),
            );
            std::process::abort();
        }
        if next == 7 {
            write(
                &self.dir.join("stall-entered.json"),
                &json!({"test_fault":"UI owner stall after stock frame"}),
            );
            loop {
                std::thread::park_timeout(Duration::from_secs(1));
            }
        }
        assert!(next <= 5);
        if next != self.stage {
            self.stage = next;
            self.settled = 0;
            ctx.set_zoom_factor(if next == 4 { 1.5 } else { 1.0 });
            for (index, points) in self.points.iter_mut().enumerate() {
                points.clear();
                if next > 0 && (index == 0 || next > 1) {
                    points.extend((0..128).map(|i| {
                        let x = f64::from(i) / 8.0;
                        egui_plot::PlotPoint::new(
                            x,
                            23.5 + f64::from(next) + (x + index as f64).sin() * 3.0,
                        )
                    }));
                }
            }
        }
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::PointerMoved(_) => self.pointer_moves += 1,
                egui::Event::PointerButton { .. } => self.pointer_buttons += 1,
                egui::Event::Key { .. } => self.keys += 1,
                egui::Event::Text(_) => self.texts += 1,
                egui::Event::WindowFocused(_) => self.focus_events += 1,
                egui::Event::Screenshot {
                    user_data, image, ..
                } => {
                    let stage = *user_data
                        .data
                        .as_ref()
                        .unwrap()
                        .downcast_ref::<u8>()
                        .unwrap();
                    self.capture(&ctx, stage, &image);
                }
                _ => {}
            }
        }
        self.close_requested |= ctx.input(|i| i.viewport().close_requested());
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(if self.stage >= 3 {
                "Temperature — updated / Температура"
            } else {
                "Read-only laboratory display / Лаборатория"
            });
            ui.label("Deterministic fixture • no Runtime connection • display input only");
            ui.horizontal(|ui| {
                let button = ui.button("Page / Plot");
                self.button = button.rect.center();
                if button.clicked() {
                    self.clicked += 1;
                }
                ui.label(format!("Local navigation: {}", self.clicked));
                let editor = ui.add(
                    egui::TextEdit::singleline(&mut self.local_text)
                        .char_limit(64)
                        .desired_width(180.0),
                );
                self.editor = editor.rect.center();
            });
            if self.stage == 0 {
                ui.heading("No data — unavailable");
            } else {
                ui.heading(format!("{:.1} °C", 23.5 + f64::from(self.stage)));
            }
            let curves = [
                plot_ui::Curve {
                    id: egui::Id::new("fixture-a"),
                    label: "Sample A / Образец",
                    points: &self.points[0],
                    width: 2.0,
                    color: Some(egui::Color32::LIGHT_BLUE),
                },
                plot_ui::Curve {
                    id: egui::Id::new("fixture-b"),
                    label: "Sample B",
                    points: &self.points[1],
                    width: 2.0,
                    color: Some(egui::Color32::LIGHT_GREEN),
                },
            ];
            let plot = egui_plot::Plot::new("shared-readonly-plot")
                .legend(Default::default())
                .height((ui.available_height() - 30.0).max(180.0))
                .x_axis_label(if self.stage >= 3 {
                    "Elapsed / seconds"
                } else {
                    "Time / s"
                })
                .y_axis_label("Temperature / °C");
            plot_ui::show(ui, plot, &curves, |name, _, point| {
                Some(format!("{name}: {:.2} °C", point.y))
            });
        });
        self.settled += 1;
        if self.settled == 6 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                self.stage,
            )));
        }
        if self.settled.is_multiple_of(15) {
            write(&self.dir.join("live.json"), &self.receipt(&ctx));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        write(
            &self.dir.join("on-exit.json"),
            &json!({"captures":self.captures,
            "close_requested":self.close_requested,"clicked":self.clicked,
            "keys":self.keys,"texts":self.texts,"local_text":self.local_text}),
        );
    }
}

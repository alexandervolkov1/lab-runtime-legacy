//! Standard egui-wgpu only: no custom shader, texture manager or renderer.
use crate::{
    Result, contained_failure,
    fixtures::{self, Bounds, Fixture, Frame, HEIGHT, READBACK, WIDTH},
    write,
};
use eframe::egui;
use serde_json::{Value, json};
use std::{
    future::Future,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

fn await_bounded<T>(
    future: impl Future<Output = T>,
    mut progress: impl FnMut() -> Result<()>,
) -> Result<T> {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    let start = Instant::now();
    for _ in 0..5000 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return Ok(value);
        }
        progress()?;
        if start.elapsed() >= Duration::from_secs(5) {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Err("bounded future observation timed out".into())
}
fn poll(device: &wgpu::Device) -> Result<()> {
    device
        .poll(wgpu::PollType::Poll)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn memory(device: &wgpu::Device, stage: &str) -> Value {
    let c = device.get_internal_counters().hal;
    let allocator = device.generate_allocator_report().map(|a| {
        assert!(a.allocations.len()<=4096 && a.blocks.len()<=4096, "diagnostic report cap");
        json!({"allocated":a.total_allocated_bytes,"reserved":a.total_reserved_bytes,"allocations":a.allocations.iter().map(|a|json!({"name":a.name,"offset":a.offset,"size":a.size})).collect::<Vec<_>>()})
    });
    json!({"stage":stage,"buffers":c.buffers.read(),"buffer_memory":c.buffer_memory.read(),"textures":c.textures.read(),"texture_memory":c.texture_memory.read(),"texture_views":c.texture_views.read(),"samplers":c.samplers.read(),"bind_groups":c.bind_groups.read(),"command_encoders":c.command_encoders.read(),"fences":c.fences.read(),"memory_allocations":c.memory_allocations.read(),"allocator":allocator})
}

struct Root {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Option<egui_wgpu::Renderer>,
    target: wgpu::Texture,
    readback: wgpu::Buffer,
    errors: Arc<Mutex<Vec<String>>>,
    lost: Arc<Mutex<Option<String>>>,
    snapshots: Vec<Value>,
    submissions: usize,
    completions: usize,
    maps: usize,
    vertex_capacity: usize,
    index_capacity: usize,
}
impl Root {
    fn new(dir: &Path) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        // Explicit validation in both build profiles; no environment selection.
        descriptor.flags = wgpu::InstanceFlags::VALIDATION;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = await_bounded(
            instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                ..Default::default()
            }),
            || Ok(()),
        )?
        .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        let format = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm);
        write(
            &dir.join("adapter.json"),
            &json!({"info":format!("{info:#?}"),"limits":format!("{:#?}",adapter.limits()),"features":format!("{:?}",adapter.features()),"downlevel":format!("{:?}",adapter.get_downlevel_capabilities()),"format":format!("{format:?}"),"surface":false,"target":[WIDTH,HEIGHT],"scale":1,"renderer":"egui-wgpu 0.36.2","backend":"DX12","selection":"HighPerformance; no hardware identity admission"}),
        );
        if !format
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC)
        {
            return Err("UnsupportedCapability: RGBA8 render/copy".into());
        }
        let (device, queue) = await_bounded(
            adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("RENDER-2A"),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                ..Default::default()
            }),
            || Ok(()),
        )?
        .map_err(|e| e.to_string())?;
        let errors = Arc::new(Mutex::new(Vec::with_capacity(8)));
        let errors_sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e| {
            let mut out = errors_sink.lock().unwrap();
            if out.len() < 8 {
                out.push(format!("{e}").chars().take(4096).collect());
            }
        }));
        let lost = Arc::new(Mutex::new(None));
        let lost_sink = lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            *lost_sink.lock().unwrap() = Some(format!(
                "{reason:?}: {}",
                message.chars().take(4096).collect::<String>()
            ));
        });
        let mut snapshots = Vec::with_capacity(192);
        snapshots.push(memory(&device, "device-before-renderer"));
        let renderer = egui_wgpu::Renderer::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        snapshots.push(memory(&device, "renderer-initial"));
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("proof-offscreen"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("proof-readback"),
            size: READBACK,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        snapshots.push(memory(&device, "target-readback"));
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            renderer: Some(renderer),
            target,
            readback,
            errors,
            lost,
            snapshots,
            submissions: 0,
            completions: 0,
            maps: 0,
            vertex_capacity: 20480,
            index_capacity: 12288,
        })
    }
    fn snap(&mut self, stage: &str) {
        assert!(self.snapshots.len() < 192);
        self.snapshots.push(memory(&self.device, stage));
    }
    fn complete(&mut self) -> Result<()> {
        let done = Arc::new(AtomicBool::new(false));
        let sink = done.clone();
        self.queue
            .on_submitted_work_done(move || sink.store(true, Ordering::Release));
        let start = Instant::now();
        for _ in 0..5000 {
            poll(&self.device)?;
            if done.load(Ordering::Acquire) {
                self.completions += 1;
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err("submitted-work completion not observed".into())
    }
    fn drain(&mut self) -> Result<()> {
        self.queue.submit([]);
        self.submissions += 1;
        self.complete()
    }
    fn check(&self) -> Result<()> {
        if !self.errors.lock().unwrap().is_empty() || self.lost.lock().unwrap().is_some() {
            return Err(format!(
                "uncaptured={:?}; lost={:?}",
                self.errors.lock().unwrap(),
                self.lost.lock().unwrap()
            ));
        }
        Ok(())
    }
    fn frame(&mut self, dir: &Path, generation: usize, frame: &Frame) -> Result<()> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        for (id, delta) in fixtures::sorted_updates(&frame.output) {
            self.renderer
                .as_mut()
                .unwrap()
                .update_texture(&self.device, &self.queue, id, delta);
        }
        self.snap(&format!(
            "g{generation}-f{}-texture-writes-pending",
            frame.index
        ));
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [WIDTH, HEIGHT],
            pixels_per_point: 1.0,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("proof-frame"),
            });
        let callbacks = self.renderer.as_mut().unwrap().update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &frame.meshes,
            &screen,
        );
        if !callbacks.is_empty() {
            return Err("unexpected callback commands".into());
        }
        let vb = frame.receipt["vertex_bytes"].as_u64().unwrap() as usize;
        let ib = frame.receipt["index_bytes"].as_u64().unwrap() as usize;
        if vb > self.vertex_capacity {
            self.vertex_capacity = (self.vertex_capacity * 2).max(vb);
        }
        if ib > self.index_capacity {
            self.index_capacity = (self.index_capacity * 2).max(ib);
        }
        self.snap(&format!("g{generation}-f{}-buffers-written", frame.index));
        let view = self.target.create_view(&Default::default());
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("standard-egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer.as_ref().unwrap().render(
                &mut pass.forget_lifetime(),
                &frame.meshes,
                &screen,
            );
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH * 4),
                    rows_per_image: Some(HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        self.submissions += 1;
        self.snap(&format!("g{generation}-f{}-submitted", frame.index));
        self.complete()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.readback.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        let result = self.map_result(&rx)?;
        result.map_err(|e| e.to_string())?;
        {
            let bytes = self
                .readback
                .get_mapped_range(..)
                .map_err(|e| e.to_string())?;
            fixtures::pixels(dir, generation, frame, &bytes, false)?;
        }
        self.readback.unmap();
        self.maps += 1;
        let error = await_bounded(scope.pop(), || poll(&self.device))?;
        if let Some(error) = error {
            return Err(error.to_string());
        }
        for id in &frame.output.textures_delta.free {
            self.renderer.as_mut().unwrap().free_texture(id);
        }
        self.snap(&format!(
            "g{generation}-f{}-completed-unmapped-free",
            frame.index
        ));
        self.check()
    }
    fn map_result(
        &self,
        rx: &mpsc::Receiver<std::result::Result<(), wgpu::BufferAsyncError>>,
    ) -> Result<std::result::Result<(), wgpu::BufferAsyncError>> {
        let start = Instant::now();
        for _ in 0..5000 {
            poll(&self.device)?;
            if let Ok(result) = rx.try_recv() {
                return Ok(result);
            }
            if start.elapsed() > Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Err("mapping observation timeout".into())
    }
    fn exercise(&mut self, case: &str, dir: &Path) -> Result<Value> {
        if case == "frames" || case == "low-cap" {
            for generation in 0..2 {
                let mut fixture = Fixture::new();
                let mut bounds = Bounds::default();
                for index in 0..fixtures::FRAMES {
                    let mut frame = fixture.frame(index);
                    if index == 7 {
                        let before = bounds.textures.clone();
                        let id = egui::TextureId::Managed(1);
                        let update = frame
                            .output
                            .textures_delta
                            .set
                            .get_mut(&id)
                            .ok_or("fixture partial ID")?
                            .first_mut()
                            .ok_or("fixture partial update")?;
                        let position = update.pos.replace([63, 63]);
                        let refused = bounds.validate(&frame).is_err();
                        if !refused || bounds.textures != before {
                            contained_failure(
                                dir,
                                "partial gate must reject atomically before any GPU write",
                            );
                        }
                        frame.output.textures_delta.set.get_mut(&id).unwrap()[0].pos = position;
                        frame.receipt["invalid_partial_rejected_before_gpu"] = json!(true);
                    }
                    bounds.validate(&frame)?;
                    if let Err(error) = self.frame(dir, generation, &frame) {
                        write(
                            &dir.join("memory-before-failure.json"),
                            &json!(self.snapshots),
                        );
                        contained_failure(dir, error);
                    }
                    bounds.freed(&frame);
                    // egui 0.36 requires consuming the applied delta before Drop.
                    frame.output.textures_delta.clear();
                }
                // Numeric IDs are reused only after the previous Context's last
                // submission and mapping completed and all its textures freed.
                for id in bounds.textures.keys() {
                    self.renderer.as_mut().unwrap().free_texture(id);
                }
                self.snap(&format!("generation-{generation}-references-freed"));
            }
            return Ok(
                json!({"frames":24,"generations":2,"standard_renderer":true,"numeric_texture_ids_recreated_after_completion":true}),
            );
        }
        if case == "unsupported-format" {
            let format = wgpu::TextureFormat::Astc {
                block: wgpu::AstcBlock::B4x4,
                channel: wgpu::AstcChannel::Unorm,
            };
            let features = self.adapter.get_texture_format_features(format);
            if features
                .allowed_usages
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
            {
                return Err("negative format unexpectedly renderable".into());
            }
            return Ok(
                json!({"diagnostic":"UnsupportedCapability","format":format!("{format:?}"),"features":format!("{features:?}"),"surface_format":"UNQUALIFIED: offscreen proof"}),
            );
        }
        let id = egui::TextureId::Managed(7);
        let full = egui::epaint::ImageDelta::full(
            egui::ColorImage::filled([64, 64], egui::Color32::RED),
            egui::TextureOptions::NEAREST,
        );
        self.renderer
            .as_mut()
            .unwrap()
            .update_texture(&self.device, &self.queue, id, &full);
        self.snap("texture-write-before-error-or-observation");
        let outcome = match case {
            "write-drain" => {
                self.drain()?;
                json!({"draw":false,"empty_submit_drained_pending_texture_write":true})
            }
            "validation" | "partial-error" => {
                let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
                if case == "partial-error" {
                    // Deliberate bypass of the normal bounds gate in one process.
                    let partial = egui::epaint::ImageDelta::partial(
                        [63, 63],
                        egui::ColorImage::filled([4, 4], egui::Color32::GREEN),
                        egui::TextureOptions::NEAREST,
                    );
                    self.renderer.as_mut().unwrap().update_texture(
                        &self.device,
                        &self.queue,
                        id,
                        &partial,
                    );
                } else {
                    let mut encoder = self.device.create_command_encoder(&Default::default());
                    encoder.copy_buffer_to_buffer(
                        &self.readback,
                        0,
                        &self.readback,
                        0,
                        READBACK + 4,
                    );
                    self.queue.submit([encoder.finish()]);
                    self.submissions += 1;
                }
                let diagnostic = await_bounded(scope.pop(), || poll(&self.device))?
                    .ok_or("validation control produced no error")?
                    .to_string();
                write(
                    &dir.join("original-error.json"),
                    &json!({"diagnostic":diagnostic}),
                );
                self.drain()?;
                json!({"original_error":diagnostic,"draw":false,"pending_writes_drained_by_empty_submit":true})
            }
            "map-error" => {
                self.drain()?;
                let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
                let (tx, rx) = mpsc::sync_channel(1);
                self.readback.map_async(wgpu::MapMode::Write, .., move |r| {
                    let _ = tx.send(r);
                });
                let diagnostic = self
                    .map_result(&rx)?
                    .expect_err("MAP_WRITE forbidden on MAP_READ buffer")
                    .to_string();
                let error = await_bounded(scope.pop(), || poll(&self.device))?;
                json!({"mapping_error":diagnostic,"validation":error.map(|e|e.to_string()),"buffer_never_read":true})
            }
            "device-destroy" => {
                self.drain()?;
                self.device.destroy();
                for _ in 0..100 {
                    let _ = poll(&self.device);
                    if self.lost.lock().unwrap().is_some() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                let lost = self
                    .lost
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or("missing forced device destruction callback")?;
                json!({"device_loss":lost,"injection":"Device::destroy; not physical TDR/device removal","native_memory_free":"UNQUALIFIED"})
            }
            "delayed" => {
                self.queue.submit([]);
                self.submissions += 1;
                std::thread::sleep(Duration::from_millis(100));
                let before = self.completions;
                self.complete()?;
                json!({"withheld_observation_ms":100,"completion_receipts_before_poll":before,"physical_gpu_stall":false,"resubmitted":false})
            }
            "timeout" => {
                self.queue.submit([]);
                self.submissions += 1;
                write(
                    &dir.join("pending.json"),
                    &json!({"injection":"withheld completion observation","submissions":self.submissions,"completion_receipts":self.completions,"original_root_retained":true,"gpu_cleanup_claim":false}),
                );
                contained_failure(
                    dir,
                    "intentional observation timeout; original Job owns containment",
                );
            }
            _ => return Err("unknown case".into()),
        };
        if case != "device-destroy" {
            self.check()?;
            self.renderer.as_mut().unwrap().free_texture(&id);
        }
        Ok(outcome)
    }
}

pub(super) fn run(case: &str, dir: &Path) -> Result<()> {
    if case == "no-adapter" {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::empty();
        let instance = wgpu::Instance::new(desc);
        let result = await_bounded(instance.request_adapter(&Default::default()), || Ok(()))?;
        let error = result.expect_err("empty backend mask must have no adapter");
        write(
            &dir.join("result.json"),
            &json!({"original_error":error.to_string(),"injection":"empty backend mask","gpu_created":false}),
        );
        return Ok(());
    }
    let mut root = Root::new(dir)?;
    // Do not return or unwind this root on an error after texture writes.
    let outcome = match root.exercise(case, dir) {
        Ok(value) => value,
        Err(error) => {
            write(
                &dir.join("memory-before-failure.json"),
                &json!(root.snapshots),
            );
            contained_failure(dir, error)
        }
    };
    root.snap("before-release-application-references");
    drop(root.renderer.take());
    root.target.destroy();
    root.readback.destroy();
    if case != "device-destroy"
        && let Err(error) = poll(&root.device)
    {
        contained_failure(dir, error);
    }
    root.snap("after-release-application-references");
    write(&dir.join("memory.json"), &json!(root.snapshots));
    write(
        &dir.join("result.json"),
        &json!({"outcome":outcome,"submissions":root.submissions,"completion_callbacks":root.completions,"mapped_readbacks":root.maps,"source_derived_vertex_capacity":root.vertex_capacity,"source_derived_index_capacity":root.index_capacity,"uncaptured_errors":*root.errors.lock().unwrap(),"lost_callback":*root.lost.lock().unwrap(),"instance_report":format!("{:?}",root.instance.generate_report()),"native_memory_cleanup":"NOT PROVEN by callbacks, destroy, handles or process exit","surface":"not created","activation_qualified":false}),
    );
    drop(root);
    write(
        &dir.join("references-released.json"),
        &json!({"application_references_released":true,"native_memory_cleanup":false}),
    );
    Ok(())
}

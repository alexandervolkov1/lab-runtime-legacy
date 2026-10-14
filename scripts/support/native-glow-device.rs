//! Windows smoke-only GL boundary. Never linked into Workbench.
//!
//! Invariants: this non-Send owner creates one unshared context on the event-loop
//! thread; every entry checks that thread and the exact current context. Window
//! and surface outlive the context. No foreign painter can consume GL errors.
//! CPU byte slices are packed explicitly (no casts, layout or alignment claims).
//! The caller must retain this entire owner on unresolved work or failed delete.
//! Drop is deliberately NOT a cleanup mechanism or a completion receipt.
#![allow(unsafe_code)]

use crate::readonly_v2::{
    completion::{Completion, WaitFailure, WaitRequest},
    texture_owner::TextureKey,
};
use eframe::{
    egui,
    glow::{self, HasContext},
};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, GlProfile, PossiblyCurrentContext, Version},
    display::GetGlDisplay,
    prelude::*,
    surface::{Surface, WindowSurface},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{
    ffi::CString,
    thread::{self, ThreadId},
};
use winit::{event_loop::ActiveEventLoop, window::Window};

pub(super) const WIDTH: usize = 1280;
pub(super) const HEIGHT: usize = 960;
const BUFFER_BYTES: usize = 2 * 1024 * 1024;
const STAGING_BYTES: usize = 4 * 1024 * 1024;
type Result<T> = std::result::Result<T, String>;

#[derive(Default, Debug, Clone)]
pub(super) struct Counters {
    pub textures_created: usize,
    pub textures_deleted: usize,
    pub programs_created: usize,
    pub programs_deleted: usize,
    pub shaders_created: usize,
    pub shaders_deleted: usize,
    pub vaos_created: usize,
    pub vaos_deleted: usize,
    pub full_uploads: usize,
    pub partial_uploads: usize,
    pub frames: usize,
    pub draws: usize,
    pub vertices: usize,
    pub indices: usize,
    pub buffers_created: usize,
    pub buffers_deleted: usize,
    pub buffer_reuses: usize,
    pub fences_created: usize,
    pub fences_deleted: usize,
    pub flushes: usize,
    pub waits: usize,
    pub completions: usize,
    pub timeouts: usize,
    pub gl_errors: usize,
    pub objects: usize,
    pub peak_objects: usize,
    pub texture_payload: usize,
    pub peak_texture_payload: usize,
    pub vbo_bytes: usize,
    pub ebo_bytes: usize,
    pub peak_vbo_bytes: usize,
    pub peak_ebo_bytes: usize,
    pub peak_buffer_bytes: usize,
    pub staging_capacity: usize,
    pub peak_staging_capacity: usize,
    pub staging_realloc_overlap: usize,
    pub generations_released: usize,
}
#[derive(Clone, Copy)]
struct Texture {
    key: TextureKey,
    name: glow::Texture,
    size: [usize; 2],
}
#[derive(Default)]
struct Pipeline {
    program: Option<glow::Program>,
    shaders: [Option<glow::Shader>; 2],
    vao: Option<glow::VertexArray>,
    vbo: Option<glow::Buffer>,
    ebo: Option<glow::Buffer>,
}
pub(super) struct Device {
    // Fields drop in this order; explicit healthy cleanup precedes ALL drops.
    gl: glow::Context,
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    pub window: Window,
    thread: ThreadId,
    textures: [Option<Texture>; 8],
    pipeline: Pipeline,
    fence: Option<glow::Fence>,
    busy: bool,
    pub fault: Option<String>,
    device_lost: bool,
    staging: Vec<u8>,
    pub counters: Counters,
}
impl Device {
    pub fn new(events: &ActiveEventLoop) -> Result<Self> {
        let attrs = Window::default_attributes()
            .with_title("Isolated v2 GL qualification")
            .with_inner_size(winit::dpi::PhysicalSize::new(WIDTH as u32, HEIGHT as u32))
            .with_resizable(false);
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(
                events,
                ConfigTemplateBuilder::new()
                    .with_alpha_size(8)
                    .with_depth_size(0)
                    .with_stencil_size(0),
                |configs| {
                    configs
                        .min_by_key(|c| c.num_samples())
                        .expect("GL config supplied by glutin")
                },
            )
            .map_err(|e| e.to_string())?;
        let window = window.ok_or("missing native window")?;
        println!(
            "native framebuffer config color={:?} alpha={} depth={} stencil={} samples={}",
            config.color_buffer_type(),
            config.alpha_size(),
            config.depth_size(),
            config.stencil_size(),
            config.num_samples()
        );
        if window.inner_size() != winit::dpi::PhysicalSize::new(WIDTH as u32, HEIGHT as u32) {
            return Err("physical viewport must be 1280x960".into());
        }
        let raw = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let attrs = ContextAttributesBuilder::new()
            .with_profile(GlProfile::Core)
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(raw));
        let surface_attrs = window
            .build_surface_attributes(Default::default())
            .map_err(|e| e.to_string())?;
        // SAFETY: the live native window owns raw; the selected config/display
        // came from this same window. Creation and currentness stay on this thread.
        let (context, surface) = unsafe {
            (
                config
                    .display()
                    .create_context(&config, &attrs)
                    .map_err(|e| e.to_string())?,
                config
                    .display()
                    .create_window_surface(&config, &surface_attrs)
                    .map_err(|e| e.to_string())?,
            )
        };
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        // SAFETY: function pointers come from this live current desktop GL display.
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                config
                    .display()
                    .get_proc_address(&CString::new(name).expect("GL symbol contains no NUL"))
            })
        };
        let mut device = Self {
            gl,
            context,
            surface,
            window,
            thread: thread::current().id(),
            textures: [None; 8],
            pipeline: Pipeline::default(),
            fence: None,
            busy: false,
            fault: None,
            device_lost: false,
            staging: Vec::new(),
            counters: Counters::default(),
        };
        device.check("context creation")?;
        let v = device.gl.version();
        if v.is_embedded || (v.major, v.minor) < (3, 3) {
            return Err("desktop GL 3.3 required (sync, VAO, RGBA8)".into());
        }
        // SAFETY: currentness checked above; these are bounded identity queries.
        unsafe {
            println!(
                "GL vendor={:?} renderer={:?} version={:?} OS_scale={} pixels_per_point=1 viewport=1280x960",
                device.gl.get_parameter_string(glow::VENDOR),
                device.gl.get_parameter_string(glow::RENDERER),
                device.gl.get_parameter_string(glow::VERSION),
                device.window.scale_factor()
            );
        }
        device.check("capabilities")?;
        if device.window.scale_factor() != 1.0 {
            return Err("native qualification scale must be one".into());
        }
        // SAFETY: current desktop GL 3.3 context supports these queries.
        let max_texture = unsafe { device.gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE) };
        device.check("texture capability")?;
        if max_texture < 2048 {
            return Err("font texture capability too small".into());
        }
        Ok(device)
    }
    fn current(&self) -> Result<()> {
        if self.thread != thread::current().id() || !self.context.is_current() {
            Err("wrong thread or non-current creating GL context".into())
        } else {
            Ok(())
        }
    }
    fn check(&mut self, operation: &str) -> Result<()> {
        self.current()?;
        // SAFETY: exact creating context is current. No library error consumer.
        let error = unsafe { self.gl.get_error() };
        if error != glow::NO_ERROR {
            self.device_lost |= error == glow::CONTEXT_LOST;
            self.counters.gl_errors += 1;
            let message = format!("{operation}: GL error 0x{error:04x}");
            self.fault.get_or_insert(message.clone());
            Err(message)
        } else {
            Ok(())
        }
    }
    fn enter(&mut self) -> Result<()> {
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        self.check("entry")
    }
    fn added(&mut self) {
        self.counters.objects += 1;
        self.counters.peak_objects = self.counters.peak_objects.max(self.counters.objects);
    }
    fn removed(&mut self) {
        self.counters.objects -= 1;
    }
    fn stage(&mut self, bytes: usize) -> Result<()> {
        if bytes > STAGING_BYTES {
            return Err("host staging bound".into());
        }
        self.staging.clear();
        if self.staging.capacity() < bytes {
            let old_capacity = self.staging.capacity();
            self.staging
                .try_reserve_exact(bytes)
                .map_err(|e| e.to_string())?;
            self.counters.staging_realloc_overlap = self
                .counters
                .staging_realloc_overlap
                .max(old_capacity + self.staging.capacity());
        }
        self.counters.staging_capacity = self.staging.capacity();
        self.counters.peak_staging_capacity = self
            .counters
            .peak_staging_capacity
            .max(self.staging.capacity());
        Ok(())
    }
    pub fn upload(&mut self, key: TextureKey, delta: &egui::epaint::ImageDelta) -> Result<()> {
        self.enter()?;
        if self.fence.is_some()
            || self
                .textures
                .iter()
                .flatten()
                .any(|t| t.key.generation != key.generation)
        {
            return Err("generation/fence conflict".into());
        }
        let size = delta.image.size();
        let bytes = size[0]
            .checked_mul(size[1])
            .and_then(|n| n.checked_mul(4))
            .ok_or("image overflow")?;
        let egui::ImageData::Color(image) = &delta.image;
        // Validate before copying: an inconsistent public ColorImage must not
        // grow staging beyond the dimensions used for the allocation bound.
        if image.pixels.len() != bytes / 4
            || size.contains(&0)
            || delta.options.mipmap_mode.is_some()
        {
            return Err("invalid or unsupported image".into());
        }
        self.stage(bytes)?;
        for pixel in &image.pixels {
            self.staging.extend_from_slice(&pixel.to_array());
        }
        let existing = self
            .textures
            .iter()
            .position(|t| t.is_some_and(|t| t.key == key));
        let index = existing
            .or_else(|| self.textures.iter().position(Option::is_none))
            .ok_or("texture slots full")?;
        if delta.pos.is_some() && existing.is_none() {
            return Err("unknown partial texture".into());
        }
        if existing.is_none() {
            // SAFETY: current context; immediately register even before error check.
            let name = unsafe { self.gl.create_texture()? };
            self.textures[index] = Some(Texture {
                key,
                name,
                size: [0, 0],
            });
            self.counters.textures_created += 1;
            self.added();
            self.check("create texture")?;
        }
        let texture = self.textures[index].unwrap();
        let width = i32::try_from(size[0]).map_err(|_| "texture width")?;
        let height = i32::try_from(size[1]).map_err(|_| "texture height")?;
        self.busy = true;
        // SAFETY: registered owned name; explicit contiguous RGBA bytes, checked
        // dimensions; unpack buffer disabled. Slice lives through the native call.
        unsafe {
            self.gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            self.gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            self.gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture.name));
            let filter = |f| if f == egui::TextureFilter::Nearest { glow::NEAREST } else { glow::LINEAR } as i32;
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                filter(delta.options.minification),
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                filter(delta.options.magnification),
            );
            let wrap = match delta.options.wrap_mode {
                egui::TextureWrapMode::ClampToEdge => glow::CLAMP_TO_EDGE,
                egui::TextureWrapMode::Repeat => glow::REPEAT,
                egui::TextureWrapMode::MirroredRepeat => glow::MIRRORED_REPEAT,
            } as i32;
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, wrap);
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, wrap);
        }
        self.check("texture parameters")?;
        if let Some(pos) = delta.pos {
            if (0..2).any(|a| {
                pos[a]
                    .checked_add(size[a])
                    .is_none_or(|end| end > texture.size[a])
            }) {
                return Err("partial bounds".into());
            }
            // SAFETY: the region fits existing storage, and exact byte length checked.
            unsafe {
                self.gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    pos[0] as i32,
                    pos[1] as i32,
                    width,
                    height,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&self.staging)),
                );
            }
            self.check("partial upload")?;
            self.counters.partial_uploads += 1;
        } else {
            // SAFETY: bounded, exact RGBA8 payload; level zero only, no mipmaps.
            unsafe {
                self.gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    width,
                    height,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&self.staging)),
                );
            }
            self.check("full upload")?;
            // SAFETY: bound owned texture. Validate allocated dimensions/format.
            let actual = unsafe {
                [
                    self.gl
                        .get_tex_level_parameter_i32(glow::TEXTURE_2D, 0, glow::TEXTURE_WIDTH),
                    self.gl
                        .get_tex_level_parameter_i32(glow::TEXTURE_2D, 0, glow::TEXTURE_HEIGHT),
                    self.gl.get_tex_level_parameter_i32(
                        glow::TEXTURE_2D,
                        0,
                        glow::TEXTURE_INTERNAL_FORMAT,
                    ),
                ]
            };
            self.check("texture storage query")?;
            if actual != [width, height, glow::RGBA8 as i32] {
                return Err("texture storage mismatch".into());
            }
            self.counters.texture_payload -= texture.size[0] * texture.size[1] * 4;
            self.counters.texture_payload += bytes;
            self.counters.peak_texture_payload = self
                .counters
                .peak_texture_payload
                .max(self.counters.texture_payload);
            self.textures[index].as_mut().unwrap().size = size;
            self.counters.full_uploads += 1;
            println!("texture full key={key:?} size={size:?} format=RGBA8 payload={bytes}");
        }
        Ok(())
    }
    fn pipeline(&mut self) -> Result<()> {
        if self.pipeline.program.is_some() {
            return Ok(());
        }
        const VERTEX: &str = "#version 330 core\nlayout(location=0) in vec2 position; layout(location=1) in vec2 uv; layout(location=2) in vec4 color; out vec2 tc; out vec4 rgba; void main(){gl_Position=vec4(2.0*position.x/1280.0-1.0,1.0-2.0*position.y/960.0,0,1);tc=uv;rgba=color;}";
        const FRAGMENT: &str = "#version 330 core\nin vec2 tc; in vec4 rgba; uniform sampler2D atlas; out vec4 pixel; void main(){pixel=rgba*texture(atlas,tc);}";
        for (index, (kind, source)) in [
            (glow::VERTEX_SHADER, VERTEX),
            (glow::FRAGMENT_SHADER, FRAGMENT),
        ]
        .into_iter()
        .enumerate()
        {
            // SAFETY: current context; every created handle immediately owned.
            let shader = unsafe { self.gl.create_shader(kind)? };
            self.pipeline.shaders[index] = Some(shader);
            self.counters.shaders_created += 1;
            self.added();
            self.check("create shader")?;
            unsafe {
                self.gl.shader_source(shader, source);
                self.gl.compile_shader(shader);
            }
            self.check("compile shader")?;
            if !unsafe { self.gl.get_shader_compile_status(shader) } {
                return Err(unsafe { self.gl.get_shader_info_log(shader) });
            }
        }
        let program = unsafe { self.gl.create_program()? };
        self.pipeline.program = Some(program);
        self.counters.programs_created += 1;
        self.added();
        self.check("create program")?;
        unsafe {
            for shader in self.pipeline.shaders.iter().flatten() {
                self.gl.attach_shader(program, *shader);
            }
            self.gl.link_program(program);
        }
        self.check("link program")?;
        if !unsafe { self.gl.get_program_link_status(program) } {
            return Err(unsafe { self.gl.get_program_info_log(program) });
        }
        let vao = unsafe { self.gl.create_vertex_array()? };
        self.pipeline.vao = Some(vao);
        self.counters.vaos_created += 1;
        self.added();
        self.check("create VAO")?;
        let vbo = unsafe { self.gl.create_buffer()? };
        self.pipeline.vbo = Some(vbo);
        self.added();
        self.counters.buffers_created += 1;
        self.check("create VBO")?;
        let ebo = unsafe { self.gl.create_buffer()? };
        self.pipeline.ebo = Some(ebo);
        self.added();
        self.counters.buffers_created += 1;
        self.check("create EBO")?;
        unsafe {
            self.gl.bind_vertex_array(Some(vao));
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            self.gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            for (index, size, kind, normalized, offset) in [
                (0, 2, glow::FLOAT, false, 0),
                (1, 2, glow::FLOAT, false, 8),
                (2, 4, glow::UNSIGNED_BYTE, true, 16),
            ] {
                self.gl.enable_vertex_attrib_array(index);
                self.gl
                    .vertex_attrib_pointer_f32(index, size, kind, normalized, 20, offset);
            }
        }
        self.check("vertex layout")
    }
    fn buffer(&mut self, target: u32, bytes: usize) -> Result<()> {
        if bytes > BUFFER_BYTES {
            return Err("GPU buffer bound".into());
        }
        let old = if target == glow::ARRAY_BUFFER {
            self.counters.vbo_bytes
        } else {
            self.counters.ebo_bytes
        };
        if old < bytes {
            // SAFETY: owned buffer bound, size bounded below i32::MAX.
            unsafe {
                self.gl
                    .buffer_data_size(target, bytes as i32, glow::DYNAMIC_DRAW);
            }
            self.check("allocate buffer")?;
            let actual = unsafe { self.gl.get_buffer_parameter_i32(target, glow::BUFFER_SIZE) };
            self.check("buffer storage query")?;
            if actual != bytes as i32 {
                return Err("buffer allocation mismatch".into());
            }
            if target == glow::ARRAY_BUFFER {
                self.counters.vbo_bytes = bytes;
                self.counters.peak_vbo_bytes = self.counters.peak_vbo_bytes.max(bytes);
            } else {
                self.counters.ebo_bytes = bytes;
                self.counters.peak_ebo_bytes = self.counters.peak_ebo_bytes.max(bytes);
            }
            self.counters.peak_buffer_bytes = self
                .counters
                .peak_buffer_bytes
                .max(self.counters.vbo_bytes + self.counters.ebo_bytes);
        } else {
            self.counters.buffer_reuses += 1;
        }
        unsafe {
            self.gl.buffer_sub_data_u8_slice(target, 0, &self.staging);
        }
        self.check("buffer upload")
    }
    pub fn draw(
        &mut self,
        generation: crate::readonly_v2::frame::ContextGeneration,
        meshes: &[egui::ClippedPrimitive],
        scale: f32,
    ) -> Result<()> {
        self.enter()?;
        if scale != 1.0 || self.fence.is_some() {
            return Err("scale/fence conflict".into());
        }
        self.busy = true;
        self.pipeline()?;
        // SAFETY: owned objects, fixed viewport, no foreign GL users or callbacks.
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::CULL_FACE);
            self.gl.disable(glow::FRAMEBUFFER_SRGB);
            self.gl.color_mask(true, true, true, true);
            self.gl.clear_color(0.02, 0.03, 0.04, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
            self.gl.enable(glow::BLEND);
            self.gl.blend_equation(glow::FUNC_ADD);
            self.gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            self.gl.enable(glow::SCISSOR_TEST);
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.use_program(self.pipeline.program);
            self.gl.bind_vertex_array(self.pipeline.vao);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, self.pipeline.vbo);
            self.gl
                .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, self.pipeline.ebo);
            let sampler = self
                .gl
                .get_uniform_location(self.pipeline.program.unwrap(), "atlas")
                .ok_or("missing sampler")?;
            self.gl.uniform_1_i32(Some(&sampler), 0);
        }
        self.check("draw state/clear")?;
        for primitive in meshes {
            let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                return Err("callback rejected".into());
            };
            if !mesh.is_valid()
                || !primitive.clip_rect.is_finite()
                || mesh
                    .vertices
                    .iter()
                    .any(|v| !v.pos.is_finite() || !v.uv.is_finite())
            {
                return Err("invalid mesh".into());
            }
            let texture = self
                .textures
                .iter()
                .flatten()
                .find(|t| {
                    t.key
                        == TextureKey {
                            generation,
                            id: mesh.texture_id,
                        }
                })
                .ok_or("unowned texture")?
                .name;
            self.stage(
                mesh.vertices
                    .len()
                    .checked_mul(20)
                    .ok_or("vertex overflow")?,
            )?;
            for vertex in &mesh.vertices {
                for value in [vertex.pos.x, vertex.pos.y, vertex.uv.x, vertex.uv.y] {
                    self.staging.extend_from_slice(&value.to_ne_bytes());
                }
                self.staging.extend_from_slice(&vertex.color.to_array());
            }
            self.buffer(glow::ARRAY_BUFFER, self.staging.len())?;
            self.stage(mesh.indices.len().checked_mul(4).ok_or("index overflow")?)?;
            for index in &mesh.indices {
                self.staging.extend_from_slice(&index.to_ne_bytes());
            }
            self.buffer(glow::ELEMENT_ARRAY_BUFFER, self.staging.len())?;
            let min = primitive
                .clip_rect
                .min
                .max(egui::Pos2::ZERO)
                .min(egui::pos2(WIDTH as f32, HEIGHT as f32));
            let max = primitive
                .clip_rect
                .max
                .max(min)
                .min(egui::pos2(WIDTH as f32, HEIGHT as f32));
            let (x, y, right, bottom) = (
                min.x.round() as i32,
                min.y.round() as i32,
                max.x.round() as i32,
                max.y.round() as i32,
            );
            unsafe {
                self.gl
                    .scissor(x, HEIGHT as i32 - bottom, right - x, bottom - y);
                self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                self.gl.draw_elements(
                    glow::TRIANGLES,
                    mesh.indices.len() as i32,
                    glow::UNSIGNED_INT,
                    0,
                );
            }
            self.check("draw_elements")?;
            self.counters.draws += 1;
            self.counters.vertices += mesh.vertices.len();
            self.counters.indices += mesh.indices.len();
        }
        self.counters.frames += 1;
        Ok(())
    }
    pub fn wait(&mut self, request: WaitRequest) -> Completion {
        // An operation error does not excuse missing its cleanup fence. A GL
        // error remains sticky for admission, but a genuine fence may prove idle.
        if self.device_lost {
            return Completion::DeviceLost;
        }
        if self.check("wait entry").is_err() {
            return Completion::Failed(WaitFailure::WaitError);
        }
        if self
            .textures
            .iter()
            .flatten()
            .any(|t| t.key.generation != request.generation)
        {
            return Completion::Failed(WaitFailure::InvalidFence);
        }
        if self.fence.is_none() {
            // SAFETY: fence follows ALL dependent commands in this same context.
            let fence = match unsafe { self.gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0) } {
                Ok(f) if !f.0.is_null() => f,
                _ => return Completion::Failed(WaitFailure::MissingFence),
            };
            self.fence = Some(fence);
            self.added();
            self.counters.fences_created += 1;
            if self.check("fence creation").is_err() {
                return Completion::Failed(WaitFailure::InvalidFence);
            }
            if !unsafe { self.gl.is_sync(fence) } {
                return Completion::Failed(WaitFailure::InvalidFence);
            }
            if self.check("validate fence").is_err() {
                return Completion::Failed(WaitFailure::InvalidFence);
            }
            unsafe {
                self.gl.flush();
            }
            self.counters.flushes += 1;
            if self.check("flush").is_err() {
                return Completion::Failed(WaitFailure::WaitError);
            }
        }
        let nanos = request.timeout.as_nanos().min(5_000_000) as i32;
        let fence = self.fence.unwrap();
        let status = unsafe { self.gl.client_wait_sync(fence, 0, nanos) };
        self.counters.waits += 1;
        if self.check("client wait").is_err() {
            return Completion::Failed(WaitFailure::WaitError);
        }
        match status {
            glow::ALREADY_SIGNALED | glow::CONDITION_SATISFIED => {
                unsafe {
                    self.gl.delete_sync(fence);
                }
                if self.check("delete completed fence").is_err() {
                    return Completion::Failed(WaitFailure::WaitError);
                }
                if unsafe { self.gl.is_sync(fence) } || self.check("verify fence deletion").is_err()
                {
                    return Completion::Failed(WaitFailure::WaitError);
                }
                self.fence = None;
                self.removed();
                self.counters.fences_deleted += 1;
                self.busy = false;
                self.counters.completions += 1;
                Completion::Quiesced
            }
            glow::TIMEOUT_EXPIRED => {
                self.counters.timeouts += 1;
                Completion::Timeout
            }
            _ => Completion::Failed(WaitFailure::WaitError),
        }
    }
    pub fn free(&mut self, key: TextureKey) -> Result<()> {
        self.check("free entry")?;
        if self.busy || self.fence.is_some() {
            return Err("free before Quiesced".into());
        }
        let index = self
            .textures
            .iter()
            .position(|t| t.is_some_and(|t| t.key == key))
            .ok_or("free unknown key")?;
        let texture = self.textures[index].unwrap();
        // SAFETY: all dependent work proved complete; owned nonzero texture.
        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            self.gl.delete_texture(texture.name);
        }
        self.check("delete texture")?;
        if unsafe { self.gl.is_texture(texture.name) } {
            return Err("texture remains after deletion".into());
        }
        self.check("verify texture deletion")?;
        self.textures[index] = None;
        self.removed();
        self.counters.textures_deleted += 1;
        self.counters.texture_payload -= texture.size[0] * texture.size[1] * 4;
        if self.textures.iter().all(Option::is_none) {
            self.release_pipeline()?;
            self.counters.generations_released += 1;
        }
        Ok(())
    }
    fn release_pipeline(&mut self) -> Result<()> {
        if self.busy || self.fence.is_some() {
            return Err("pipeline still busy".into());
        }
        // SAFETY: no GPU work; unbind references before deleting owned objects.
        unsafe {
            self.gl.use_program(None);
            self.gl.bind_vertex_array(None);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        self.check("unbind pipeline")?;
        if let Some(vao) = self.pipeline.vao {
            unsafe {
                self.gl.delete_vertex_array(vao);
            }
            self.check("delete VAO")?;
            self.pipeline.vao = None;
            self.counters.vaos_deleted += 1;
            self.removed();
        }
        for vertex in [true, false] {
            if let Some(buffer) = if vertex {
                self.pipeline.vbo
            } else {
                self.pipeline.ebo
            } {
                unsafe {
                    self.gl.delete_buffer(buffer);
                }
                self.check("delete buffer")?;
                if unsafe { self.gl.is_buffer(buffer) } {
                    return Err("buffer remains".into());
                }
                if vertex {
                    self.pipeline.vbo = None;
                    self.counters.vbo_bytes = 0;
                } else {
                    self.pipeline.ebo = None;
                    self.counters.ebo_bytes = 0;
                }
                self.removed();
                self.counters.buffers_deleted += 1;
            }
        }
        if let Some(program) = self.pipeline.program {
            unsafe {
                self.gl.delete_program(program);
            }
            self.check("delete program")?;
            if unsafe { self.gl.is_program(program) } {
                return Err("program remains".into());
            }
            self.pipeline.program = None;
            self.counters.programs_deleted += 1;
            self.removed();
        }
        for index in 0..2 {
            if let Some(shader) = self.pipeline.shaders[index] {
                unsafe {
                    self.gl.delete_shader(shader);
                }
                self.check("delete shader")?;
                if unsafe { self.gl.is_shader(shader) } {
                    return Err("shader remains".into());
                }
                self.pipeline.shaders[index] = None;
                self.counters.shaders_deleted += 1;
                self.removed();
            }
        }
        self.check("pipeline deletion complete")
    }
    pub fn readback(&mut self) -> Result<Vec<u8>> {
        self.enter()?;
        if self.busy || self.fence.is_some() {
            return Err("readback before completion".into());
        }
        let mut bytes = vec![0; WIDTH * HEIGHT * 4];
        // SAFETY: exact framebuffer-sized output; pack buffer disabled. This is
        // controlled verification after a real fence, never completion by readback.
        unsafe {
            self.gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.pixel_store_i32(glow::PACK_ROW_LENGTH, 0);
            self.gl.read_buffer(glow::BACK);
            self.gl.read_pixels(
                0,
                0,
                WIDTH as i32,
                HEIGHT as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bytes)),
            );
        }
        self.check("framebuffer readback")?;
        Ok(bytes)
    }
    pub fn present(&mut self) -> Result<()> {
        self.enter()?;
        self.surface
            .swap_buffers(&self.context)
            .map_err(|e| e.to_string())?;
        self.check("swap buffers")
    }
    pub fn healthy_close(&mut self) -> Result<()> {
        self.enter()?;
        if self.busy || self.fence.is_some() || self.textures.iter().any(Option::is_some) {
            return Err("unresolved GL ownership".into());
        }
        self.release_pipeline()?;
        if self.counters.objects != 0 {
            return Err("own GL objects remain".into());
        }
        self.staging = Vec::new();
        self.counters.staging_capacity = 0;
        self.check("healthy close")?;
        // No GL call is permitted beyond this point. Session has no registered
        // generation; its subsequent Drop cannot call backend free again.
        self.context
            .make_not_current_in_place()
            .map_err(|e| e.to_string())?;
        if self.context.is_current() {
            return Err("context remained current after release".into());
        }
        println!(
            "lifecycle resource deletion verified -> zero objects -> current-context release -> window destruction"
        );
        Ok(())
    }
}

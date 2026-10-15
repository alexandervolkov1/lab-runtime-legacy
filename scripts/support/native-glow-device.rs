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
    deletion::{DeleteDisposition, DeletionFailure, DeletionResult, ObjectIdentity, ObjectKind},
    frame::ContextGeneration,
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

#[path = "native-glow-capabilities.rs"]
mod capabilities;
use capabilities::{Capabilities, FailureKind, InitializationError, Report, UnsupportedCapability};

pub(super) const WIDTH: usize = 1280;
pub(super) const HEIGHT: usize = 960;
const BUFFER_BYTES: usize = 2 * 1024 * 1024;
const STAGING_BYTES: usize = 4 * 1024 * 1024;
pub(super) const READBACK_BYTES: usize = WIDTH * HEIGHT * 4;
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
    generation: Option<ContextGeneration>,
    deletion: Option<DeletionFailure>,
    pub delete_fault: Option<(ObjectKind, DeleteDisposition, usize)>,
    pub delete_calls: usize,
    pub wait_fault: Option<Completion>,
    terminal_wait: Option<Completion>,
    pub teardown_fault: bool,
    delete_matches: usize,
    is_vertex_array: unsafe extern "system" fn(u32) -> u8,
    busy: bool,
    pub fault: Option<String>,
    device_lost: bool,
    staging: Box<[u8]>,
    staging_len: usize,
    readback: Box<[u8]>,
    pub counters: Counters,
    initialization: Report,
}
impl Device {
    pub fn new(events: &ActiveEventLoop) -> std::result::Result<Self, InitializationError> {
        let mut report = Report::default();
        // initialize creates no own textures/programs/buffers/VAOs/fences. Only
        // this pre-payload constructor may unwind into an initialization record;
        // submitted work and retained GPU roots never cross this catch boundary.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::initialize(events, &mut report)
        }))
        .unwrap_or_else(|payload| Err(capabilities::initialization_panic(&mut report, payload)));
        report.admitted = result.is_ok();
        if let Err(error) = &result {
            report.error = Some(error.clone());
            report
                .error_kind
                .get_or_insert(FailureKind::InitializationFailure);
        }
        // One bounded retained record, including the original failed stage. A
        // successful capability check never stands in for later GPU receipts.
        println!(
            "GL_INITIALIZATION={}",
            serde_json::to_string(&report).unwrap()
        );
        match result {
            Ok(mut device) => {
                device.initialization = report;
                Ok(device)
            }
            Err(_) => Err(InitializationError {
                report: Box::new(report),
            }),
        }
    }
    pub fn initialization(&self) -> &Report {
        &self.initialization
    }
    fn initialize(events: &ActiveEventLoop, report: &mut Report) -> Result<Self> {
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
                    configs.min_by_key(|c| c.num_samples()).unwrap_or_else(|| {
                        std::panic::panic_any(UnsupportedCapability(
                            "no matching default framebuffer configuration",
                        ))
                    })
                },
            )
            .map_err(|e| e.to_string())?;
        let window = window.ok_or("missing native window")?;
        report.diagnostics.platform_api = match config.display() {
            glutin::display::Display::Wgl(_) => "WGL",
            glutin::display::Display::Egl(_) => "EGL",
        };
        report.diagnostics.framebuffer_config = format!(
            "color={:?} alpha={} depth={} stencil={} samples={}",
            config.color_buffer_type(),
            config.alpha_size(),
            config.depth_size(),
            config.stencil_size(),
            config.num_samples()
        )
        .chars()
        .take(512)
        .collect();
        report.scale = Some(window.scale_factor());
        println!(
            "native framebuffer config color={:?} alpha={} depth={} stencil={} samples={}",
            config.color_buffer_type(),
            config.alpha_size(),
            config.depth_size(),
            config.stencil_size(),
            config.num_samples()
        );
        if window.inner_size() != winit::dpi::PhysicalSize::new(WIDTH as u32, HEIGHT as u32) {
            report.error_kind = Some(FailureKind::UnsupportedCapability);
            return Err(UnsupportedCapability("fixed physical viewport 1280x960").to_string());
        }
        report.stage = "context_surface";
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
                    .map_err(|e| {
                        report.error_kind = Some(capabilities::context_failure(
                            report.diagnostics.platform_api,
                            e.not_supported(),
                            e.raw_code(),
                        ));
                        e.to_string()
                    })?,
                config
                    .display()
                    .create_window_surface(&config, &surface_attrs)
                    .map_err(|e| e.to_string())?,
            )
        };
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        report.stage = "entry_points";
        if !context.is_current() {
            return Err("creating context is not current".into());
        }
        capabilities::require_symbols(|name| {
            let pointer = config
                .display()
                .get_proc_address(&CString::new(name).unwrap()) as usize;
            // WGL also defines small integer/-1 failure sentinels. Do not pass
            // any such value to Glow or transmute it into an entry point.
            pointer > 3 && pointer != usize::MAX
        })
        .map_err(|error| {
            report.error_kind = Some(FailureKind::UnsupportedCapability);
            error.to_string()
        })?;
        report.required_entry_points = capabilities::REQUIRED_SYMBOLS.len();
        report.stage = "glow_loader";
        // SAFETY: function pointers come from this live current desktop GL display.
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                config
                    .display()
                    .get_proc_address(&CString::new(name).expect("GL symbol contains no NUL"))
            })
        };
        let is_vao = config
            .display()
            .get_proc_address(&CString::new("glIsVertexArray").unwrap());
        if is_vao.is_null() {
            return Err("glIsVertexArray unavailable".into());
        }
        // SAFETY: desktop GL 3.3 glIsVertexArray has GLuint -> GLboolean with
        // the Windows system ABI. The live display supplies the checked symbol.
        let is_vertex_array = unsafe {
            std::mem::transmute::<*const std::ffi::c_void, unsafe extern "system" fn(u32) -> u8>(
                is_vao,
            )
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
            generation: None,
            deletion: None,
            delete_fault: None,
            delete_calls: 0,
            wait_fault: None,
            terminal_wait: None,
            teardown_fault: false,
            delete_matches: 0,
            is_vertex_array,
            busy: false,
            fault: None,
            device_lost: false,
            staging: Box::default(),
            staging_len: 0,
            readback: Box::default(),
            counters: Counters::default(),
            initialization: Report::default(),
        };
        device.check("context creation")?;
        report.stage = "desktop_api";
        let v = device.gl.version().clone();
        // SAFETY: exact creating context is current, and the required queries
        // were resolved before constructing Glow. Retained identity is bounded.
        unsafe {
            report.diagnostics.vendor = device
                .gl
                .get_parameter_string(glow::VENDOR)
                .chars()
                .take(512)
                .collect();
            report.diagnostics.renderer = device
                .gl
                .get_parameter_string(glow::RENDERER)
                .chars()
                .take(512)
                .collect();
            report.diagnostics.version = device
                .gl
                .get_parameter_string(glow::VERSION)
                .chars()
                .take(512)
                .collect();
            report.diagnostics.shading_language_version = device
                .gl
                .get_parameter_string(glow::SHADING_LANGUAGE_VERSION)
                .chars()
                .take(512)
                .collect();
        }
        device.check("graphics identity")?;
        if v.is_embedded || (v.major, v.minor) < (3, 3) {
            report.error_kind = Some(FailureKind::UnsupportedCapability);
            return Err(UnsupportedCapability("desktop OpenGL 3.3 or newer").to_string());
        }
        report.stage = "framebuffer_capabilities";
        // SAFETY: current desktop GL >=3.3 supports these queries. No own GPU
        // object or rendering obligation has been created. Framebuffer zero is
        // the original glutin double-buffered drawable, never a foreign painter.
        unsafe {
            device.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            let mut viewport = [0; 2];
            device
                .gl
                .get_parameter_i32_slice(glow::MAX_VIEWPORT_DIMS, &mut viewport);
            let color_bits = [
                glow::FRAMEBUFFER_ATTACHMENT_RED_SIZE,
                glow::FRAMEBUFFER_ATTACHMENT_GREEN_SIZE,
                glow::FRAMEBUFFER_ATTACHMENT_BLUE_SIZE,
                glow::FRAMEBUFFER_ATTACHMENT_ALPHA_SIZE,
            ]
            .map(|parameter| {
                device.gl.get_framebuffer_attachment_parameter_i32(
                    glow::FRAMEBUFFER,
                    glow::BACK_LEFT,
                    parameter,
                )
            });
            report.diagnostics.context_flags = device.gl.get_parameter_i32(glow::CONTEXT_FLAGS);
            report.capabilities = Some(Capabilities {
                version: [v.major, v.minor],
                embedded: v.is_embedded,
                profile_mask: device.gl.get_parameter_i32(glow::CONTEXT_PROFILE_MASK),
                max_texture_size: device.gl.get_parameter_i32(glow::MAX_TEXTURE_SIZE),
                max_viewport: viewport,
                color_bits,
                double_buffered: device.gl.get_parameter_i32(glow::DOUBLEBUFFER) != 0,
                default_framebuffer_complete: device.gl.check_framebuffer_status(glow::FRAMEBUFFER)
                    == glow::FRAMEBUFFER_COMPLETE,
            });
        }
        device.check("capabilities")?;
        report
            .capabilities
            .as_ref()
            .unwrap()
            .admit()
            .map_err(|error| {
                report.error_kind = Some(FailureKind::UnsupportedCapability);
                error.to_string()
            })?;
        report.stage = "fixed_drawable_scale";
        if device.window.scale_factor() != 1.0 {
            report.error_kind = Some(FailureKind::UnsupportedCapability);
            return Err(
                UnsupportedCapability("fixed native scale one; PORT-2 pending").to_string(),
            );
        }
        report.stage = "ready";
        Ok(device)
    }
    fn current(&self) -> Result<()> {
        if self.thread != thread::current().id() || !self.context.is_current() {
            Err("wrong thread or non-current creating GL context".into())
        } else {
            Ok(())
        }
    }
    pub fn current_owner(&self) -> bool {
        self.current().is_ok()
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
        if self.terminal_wait.is_some() {
            return Err("terminal completion prohibits GPU work".into());
        }
        if self.deletion.is_some() {
            return Err("unresolved deletion prohibits GPU work".into());
        }
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
        self.staging_len = 0;
        if self.staging.len() < bytes {
            let old = self.staging.len();
            // Exact logical storage, no geometric growth. Old and new coexist
            // only for this assignment; both are bounded by STAGING_BYTES.
            self.staging = vec![0; bytes].into_boxed_slice();
            self.counters.staging_realloc_overlap = self
                .counters
                .staging_realloc_overlap
                .max(old + self.staging.len());
        }
        self.counters.staging_capacity = self.staging.len();
        self.counters.peak_staging_capacity =
            self.counters.peak_staging_capacity.max(self.staging.len());
        Ok(())
    }
    fn append(&mut self, bytes: &[u8]) {
        let end = self
            .staging_len
            .checked_add(bytes.len())
            .expect("validated packing length");
        self.staging[self.staging_len..end].copy_from_slice(bytes);
        self.staging_len = end;
    }
    pub fn upload(&mut self, key: TextureKey, delta: &egui::epaint::ImageDelta) -> Result<()> {
        self.enter()?;
        if self.generation.is_some_and(|g| g != key.generation) {
            return Err("generation still owned".into());
        }
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
        let max_texture = self
            .initialization
            .capabilities
            .as_ref()
            .unwrap()
            .max_texture_size as usize;
        if size.iter().any(|dimension| *dimension > max_texture) {
            return Err(
                UnsupportedCapability("texture dimensions exceed GL_MAX_TEXTURE_SIZE").to_string(),
            );
        }
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
        self.generation = Some(key.generation);
        for pixel in &image.pixels {
            self.append(&pixel.to_array());
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
                    glow::PixelUnpackData::Slice(Some(&self.staging[..self.staging_len])),
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
                    glow::PixelUnpackData::Slice(Some(&self.staging[..self.staging_len])),
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
                return Err("fixed shader compilation failed".into());
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
            return Err("fixed program linking failed".into());
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
            self.gl
                .buffer_sub_data_u8_slice(target, 0, &self.staging[..self.staging_len]);
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
        if self.generation.is_some_and(|g| g != generation) {
            return Err("generation still owned".into());
        }
        // Bound iteration and total packing before any new pipeline allocation.
        // Native adapter limits leave Preview/B1/FiniteTraceItem unchanged.
        if meshes.len() > crate::readonly_v2::MAX_SHAPES
            || meshes
                .iter()
                .try_fold((0usize, 0usize), |(v, i), p| {
                    let egui::epaint::Primitive::Mesh(m) = &p.primitive else {
                        return None;
                    };
                    Some((
                        v.checked_add(m.vertices.len().checked_mul(20)?)?,
                        i.checked_add(m.indices.len().checked_mul(4)?)?,
                    ))
                })
                .is_none_or(|(v, i)| v > BUFFER_BYTES || i > BUFFER_BYTES)
        {
            return Err("native mesh work bound".into());
        }
        self.generation = Some(generation);
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
                    self.append(&value.to_ne_bytes());
                }
                self.append(&vertex.color.to_array());
            }
            self.buffer(glow::ARRAY_BUFFER, self.staging_len)?;
            self.stage(mesh.indices.len().checked_mul(4).ok_or("index overflow")?)?;
            for index in &mesh.indices {
                self.append(&index.to_ne_bytes());
            }
            self.buffer(glow::ELEMENT_ARRAY_BUFFER, self.staging_len)?;
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
    fn latch_wait(&mut self, completion: Completion) -> Completion {
        if !matches!(completion, Completion::Quiesced | Completion::Timeout) {
            self.terminal_wait = Some(completion);
        }
        completion
    }
    pub fn wait(&mut self, request: WaitRequest) -> Completion {
        if let Some(completion) = self.terminal_wait {
            return completion;
        }
        // An operation error does not excuse missing its cleanup fence. A GL
        // error remains sticky for admission, but a genuine fence may prove idle.
        if self.device_lost {
            return self.latch_wait(Completion::DeviceLost);
        }
        if self.check("wait entry").is_err() {
            return self.latch_wait(Completion::Failed(WaitFailure::WaitError));
        }
        if self.generation.is_some_and(|g| g != request.generation) {
            return self.latch_wait(Completion::Failed(WaitFailure::InvalidFence));
        }
        self.generation = Some(request.generation);
        if self
            .textures
            .iter()
            .flatten()
            .any(|t| t.key.generation != request.generation)
        {
            return self.latch_wait(Completion::Failed(WaitFailure::InvalidFence));
        }
        if self.fence.is_none() {
            // SAFETY: fence follows ALL dependent commands in this same context.
            let fence = match unsafe { self.gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0) } {
                Ok(f) if !f.0.is_null() => f,
                _ => return self.latch_wait(Completion::Failed(WaitFailure::MissingFence)),
            };
            self.fence = Some(fence);
            self.added();
            self.counters.fences_created += 1;
            if self.check("fence creation").is_err() {
                return self.latch_wait(Completion::Failed(WaitFailure::InvalidFence));
            }
            if !unsafe { self.gl.is_sync(fence) } {
                return self.latch_wait(Completion::Failed(WaitFailure::InvalidFence));
            }
            if self.check("validate fence").is_err() {
                return self.latch_wait(Completion::Failed(WaitFailure::InvalidFence));
            }
            unsafe {
                self.gl.flush();
            }
            self.counters.flushes += 1;
            if self.check("flush").is_err() {
                return self.latch_wait(Completion::Failed(WaitFailure::WaitError));
            }
        }
        // Deterministic injection AFTER a real owned fence and flush. It never
        // calls a wait with an invalid pointer or fabricates a Quiesced proof.
        if let Some(completion) = self.wait_fault {
            return self.latch_wait(completion);
        }
        let nanos = request.timeout.as_nanos().min(5_000_000) as i32;
        let fence = self.fence.unwrap();
        let status = unsafe { self.gl.client_wait_sync(fence, 0, nanos) };
        self.counters.waits += 1;
        if self.check("client wait").is_err() {
            return self.latch_wait(Completion::Failed(WaitFailure::WaitError));
        }
        match status {
            glow::ALREADY_SIGNALED | glow::CONDITION_SATISFIED => {
                self.busy = false;
                self.counters.completions += 1;
                Completion::Quiesced
            }
            glow::TIMEOUT_EXPIRED => {
                self.counters.timeouts += 1;
                Completion::Timeout
            }
            _ => self.latch_wait(Completion::Failed(WaitFailure::WaitError)),
        }
    }
    fn identity(
        &self,
        generation: ContextGeneration,
        kind: ObjectKind,
        name: u64,
        texture: Option<TextureKey>,
    ) -> ObjectIdentity {
        ObjectIdentity {
            generation,
            kind,
            name,
            texture,
        }
    }
    fn failed(
        &mut self,
        object: ObjectIdentity,
        disposition: DeleteDisposition,
    ) -> DeletionFailure {
        let failure = DeletionFailure {
            object,
            disposition,
        };
        *self.deletion.get_or_insert(failure)
    }
    /// No retry after either failure disposition. The handle ledger remains exact;
    /// an ambiguous handle is retained as an obligation, never used again.
    fn delete(
        &mut self,
        object: ObjectIdentity,
        call: impl FnOnce(&glow::Context),
        exists: impl FnOnce(&Self) -> bool,
    ) -> DeletionResult {
        if let Some(failure) = self.deletion {
            return Err(failure);
        }
        if self.terminal_wait.is_some()
            || self.busy
            || self.device_lost
            || self.check("deletion entry").is_err()
        {
            return Err(self.failed(object, DeleteDisposition::NotStarted));
        }
        let inject = if self.delete_fault.is_some_and(|f| f.0 == object.kind) {
            self.delete_matches += 1;
            self.delete_fault
                .filter(|f| f.2 == self.delete_matches)
                .map(|f| f.1)
        } else {
            None
        };
        if inject == Some(DeleteDisposition::NotStarted) {
            return Err(self.failed(object, DeleteDisposition::NotStarted));
        }
        self.delete_calls += 1;
        call(&self.gl);
        if self.check("object deletion").is_err() || inject == Some(DeleteDisposition::Ambiguous) {
            return Err(self.failed(object, DeleteDisposition::Ambiguous));
        }
        let remains = exists(self);
        if self.check("deletion proof query").is_err() || remains {
            return Err(self.failed(object, DeleteDisposition::Ambiguous));
        }
        self.removed();
        Ok(())
    }
    pub fn release_frame(&mut self, generation: ContextGeneration) -> DeletionResult {
        if let Some(failure) = self.deletion {
            return Err(failure);
        }
        if let Some(fence) = self.fence {
            let creating_generation = self.generation.expect("registered fence generation");
            let object = self.identity(
                creating_generation,
                ObjectKind::Fence,
                fence.0 as usize as u64,
                None,
            );
            if generation != creating_generation {
                return Err(self.failed(object, DeleteDisposition::NotStarted));
            }
            // SAFETY: delete checks currentness/quiescence, only one call per
            // identity. is_sync observes the original pointer without reuse.
            self.delete(
                object,
                |gl| unsafe { gl.delete_sync(fence) },
                |d| unsafe { d.gl.is_sync(fence) },
            )?;
            self.fence = None;
            self.counters.fences_deleted += 1;
        }
        Ok(())
    }
    pub fn free(&mut self, key: TextureKey) -> DeletionResult {
        if let Some(failure) = self.deletion {
            return Err(failure);
        }
        let Some(index) = self
            .textures
            .iter()
            .position(|t| t.is_some_and(|t| t.key == key))
        else {
            // An admitted upload rejected before native allocation has no handle.
            return Ok(());
        };
        let texture = self.textures[index].unwrap();
        let object = self.identity(
            key.generation,
            ObjectKind::Texture,
            texture.name.0.get() as u64,
            Some(key),
        );
        // SAFETY: checked current context, no shared context or FBO attachment;
        // only TEXTURE0/2D is used. Remove its binding before the one delete.
        self.delete(
            object,
            |gl| unsafe {
                gl.bind_texture(glow::TEXTURE_2D, None);
                gl.delete_texture(texture.name);
            },
            |d| unsafe { d.gl.is_texture(texture.name) },
        )?;
        self.textures[index] = None;
        self.counters.textures_deleted += 1;
        self.counters.texture_payload -= texture.size[0] * texture.size[1] * 4;
        Ok(())
    }
    pub fn release_generation(&mut self, generation: ContextGeneration) -> DeletionResult {
        if let Some(failure) = self.deletion {
            return Err(failure);
        }
        let generation_object = self.identity(generation, ObjectKind::Generation, 0, None);
        if self.generation.is_some_and(|g| g != generation)
            || self.busy
            || self.fence.is_some()
            || self.textures.iter().any(Option::is_some)
            || self.check("generation release").is_err()
        {
            return Err(self.failed(generation_object, DeleteDisposition::NotStarted));
        }
        // SAFETY: one unshared current context; unbind program and VAO before
        // deletion. Deleting VAO removes its VBO/EBO attachment references.
        unsafe {
            self.gl.use_program(None);
            self.gl.bind_vertex_array(None);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        if self.check("unbind pipeline").is_err() {
            return Err(self.failed(generation_object, DeleteDisposition::NotStarted));
        }
        if let Some(vao) = self.pipeline.vao {
            let object = self.identity(
                generation,
                ObjectKind::VertexArray,
                vao.0.get() as u64,
                None,
            );
            self.delete(
                object,
                |gl| unsafe { gl.delete_vertex_array(vao) },
                |d| unsafe { (d.is_vertex_array)(vao.0.get()) != 0 },
            )?;
            self.pipeline.vao = None;
            self.counters.vaos_deleted += 1;
        }
        for vertex in [true, false] {
            if let Some(buffer) = if vertex {
                self.pipeline.vbo
            } else {
                self.pipeline.ebo
            } {
                let object =
                    self.identity(generation, ObjectKind::Buffer, buffer.0.get() as u64, None);
                self.delete(
                    object,
                    |gl| unsafe { gl.delete_buffer(buffer) },
                    |d| unsafe { d.gl.is_buffer(buffer) },
                )?;
                if vertex {
                    self.pipeline.vbo = None;
                    self.counters.vbo_bytes = 0;
                } else {
                    self.pipeline.ebo = None;
                    self.counters.ebo_bytes = 0;
                }
                self.counters.buffers_deleted += 1;
            }
        }
        if let Some(program) = self.pipeline.program {
            let object = self.identity(
                generation,
                ObjectKind::Program,
                program.0.get() as u64,
                None,
            );
            self.delete(
                object,
                |gl| unsafe { gl.delete_program(program) },
                |d| unsafe { d.gl.is_program(program) },
            )?;
            self.pipeline.program = None;
            self.counters.programs_deleted += 1;
        }
        for index in 0..2 {
            if let Some(shader) = self.pipeline.shaders[index] {
                let object =
                    self.identity(generation, ObjectKind::Shader, shader.0.get() as u64, None);
                self.delete(
                    object,
                    |gl| unsafe { gl.delete_shader(shader) },
                    |d| unsafe { d.gl.is_shader(shader) },
                )?;
                self.pipeline.shaders[index] = None;
                self.counters.shaders_deleted += 1;
            }
        }
        self.generation = None;
        self.counters.generations_released += 1;
        Ok(())
    }
    /// Bounded snapshot of remaining obligations, including one latched ambiguous
    /// identity. Confirmed objects are absent; names are never recreated here.
    pub fn ledger(&self) -> [Option<ObjectIdentity>; 15] {
        let mut objects = [None; 15];
        let Some(generation) = self.generation else {
            return objects;
        };
        for (slot, texture) in self.textures.iter().enumerate() {
            objects[slot] = texture.map(|t| {
                self.identity(
                    generation,
                    ObjectKind::Texture,
                    t.name.0.get() as u64,
                    Some(t.key),
                )
            });
        }
        objects[8] = self
            .pipeline
            .vao
            .map(|n| self.identity(generation, ObjectKind::VertexArray, n.0.get() as u64, None));
        objects[9] = self
            .pipeline
            .vbo
            .map(|n| self.identity(generation, ObjectKind::Buffer, n.0.get() as u64, None));
        objects[10] = self
            .pipeline
            .ebo
            .map(|n| self.identity(generation, ObjectKind::Buffer, n.0.get() as u64, None));
        objects[11] = self
            .pipeline
            .program
            .map(|n| self.identity(generation, ObjectKind::Program, n.0.get() as u64, None));
        for i in 0..2 {
            objects[12 + i] = self.pipeline.shaders[i]
                .map(|n| self.identity(generation, ObjectKind::Shader, n.0.get() as u64, None));
        }
        objects[14] = self
            .fence
            .map(|n| self.identity(generation, ObjectKind::Fence, n.0 as usize as u64, None));
        objects
    }
    pub fn readback(&mut self) -> Result<&[u8]> {
        self.enter()?;
        if self.busy || self.fence.is_some() {
            return Err("readback before completion".into());
        }
        if self.readback.is_empty() {
            self.readback = vec![0; READBACK_BYTES].into_boxed_slice();
        }
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
                glow::PixelPackData::Slice(Some(&mut self.readback)),
            );
        }
        self.check("framebuffer readback")?;
        Ok(&self.readback)
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
        if self.pipeline.program.is_some() || self.generation.is_some() || self.deletion.is_some() {
            return Err("unreleased generation or deletion obligation".into());
        }
        if self.counters.objects != 0 {
            return Err("own GL objects remain".into());
        }
        self.staging = Box::default();
        self.staging_len = 0;
        self.readback = Box::default();
        self.counters.staging_capacity = 0;
        self.check("healthy close")?;
        if self.teardown_fault {
            return Err("NativeContextTeardown: injected failure before make-not-current".into());
        }
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

//! Shared safe FrameBackend adapter for the single audited Glow Device.
use super::device::Device;
use crate::readonly_v2::{
    self,
    completion::{Completion, WaitRequest},
    frame::{ContextGeneration, FrameError},
    texture_owner::{FrameBackend, TextureKey},
};
use eframe::egui;
use std::{cell::RefCell, rc::Rc};
#[derive(Clone)]
pub(super) struct Backend {
    pub(super) device: Rc<RefCell<Device>>,
    pub(super) inject_unresolved: bool,
    pub(super) fail_submission: bool,
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
        let result = self
            .device
            .borrow_mut()
            .draw(generation, meshes, scale)
            .map_err(|error| {
                eprintln!("submit: {error}");
                FrameError::Submission
            });
        result?;
        if self.fail_submission {
            Err(FrameError::Submission)
        } else {
            Ok(())
        }
    }
    fn wait(&mut self, request: WaitRequest) -> Completion {
        if self.inject_unresolved {
            Completion::Timeout
        } else {
            self.device.borrow_mut().wait(request)
        }
    }
    fn release_frame(
        &mut self,
        generation: ContextGeneration,
    ) -> readonly_v2::deletion::DeletionResult {
        self.device.borrow_mut().release_frame(generation)
    }
    fn free(&mut self, key: TextureKey) -> readonly_v2::deletion::DeletionResult {
        self.device.borrow_mut().free(key)
    }
    fn release_generation(
        &mut self,
        generation: ContextGeneration,
    ) -> readonly_v2::deletion::DeletionResult {
        self.device.borrow_mut().release_generation(generation)
    }
}

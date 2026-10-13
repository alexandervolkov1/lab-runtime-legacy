//! Compile-only public API/lifetime witness. Never invoked by the CPU probe.
//! This neither creates a native window nor qualifies a GPU backend.

use crate::{presentation::v2::AdmittedV2Document, readonly_v2};
use eframe::{egui, egui_glow::Painter};

struct BorrowedApp<'a> {
    _preview: readonly_v2::Preview<'a>,
}
impl eframe::App for BorrowedApp<'_> {
    fn ui(&mut self, _: &mut egui::Ui, _: &mut eframe::Frame) {}
}

// Type-check the real Preview/document lifetime through run_native. No owning
// document clone, self-reference, leaked allocation or 'static cast is needed.
fn borrowed_native_app(document: &AdmittedV2Document) -> eframe::Result {
    let creator: eframe::AppCreator<'_> = Box::new(move |_| {
        Ok(Box::new(BorrowedApp {
            _preview: readonly_v2::Preview::new(
                document,
                readonly_v2::PresentationEpoch {
                    owner: [1; 16],
                    revision: 0,
                },
            ),
        }))
    });
    eframe::run_native("compile-only witness", Default::default(), creator)
}

// Only public, already pinned reexports; these signatures do not claim that
// Painter reports GL errors or implements the FrameBackend drain contract.
fn painter_api() {
    let _constructor = Painter::new;
    let _: fn(&mut Painter, egui::TextureId, &egui::epaint::ImageDelta) = Painter::set_texture;
    let _: fn(&mut Painter, [u32; 2], f32, &[egui::ClippedPrimitive]) = Painter::paint_primitives;
    let _: fn(&Painter, egui::TextureId) -> Option<eframe::glow::Texture> = Painter::texture;
    let _: fn(&mut Painter, egui::TextureId) = Painter::free_texture;
    let _: fn(&mut Painter) = Painter::destroy;
}

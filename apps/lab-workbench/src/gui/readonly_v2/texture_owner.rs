//! Bounded, serial host seam. No GPU backend or GPU qualification lives here.
use super::frame::{ContextGeneration, FrameError};
use eframe::egui::{self, TextureId, epaint::ImageDelta};

pub(crate) const MAX_TEXTURES: usize = 8;
pub(crate) const MAX_UPDATES: usize = 16;

/// A texture name is meaningful only within its creating Context generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureKey {
    pub(crate) generation: ContextGeneration,
    pub(crate) id: TextureId,
}

/// Trusted host contract, deliberately synchronous and without a submission queue.
/// `submit`/`complete` errors may leave work in flight. `drain` MUST establish
/// that no submitted draw/upload can still use any resource before returning,
/// including after partial failure. `free` then releases the entire key, including
/// partially created resources. Neither cleanup operation may panic. A real GPU
/// implementation needs independently reviewed fences, device-loss handling and
/// bounded shutdown; an implementation that merely returns violates this trait.
pub(crate) trait FrameBackend {
    const CPU_ONLY: bool;
    fn set(&mut self, key: TextureKey, delta: &ImageDelta) -> Result<(), FrameError>;
    fn platform(
        &mut self,
        output: &egui::PlatformOutput,
        viewports: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> Result<(), FrameError>;
    fn submit(
        &mut self,
        generation: ContextGeneration,
        meshes: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) -> Result<(), FrameError>;
    fn complete(&mut self) -> Result<(), FrameError>;
    fn drain(&mut self);
    fn free(&mut self, key: TextureKey);
}

/// Explicit CPU-only sink: consumes CPU images, never acknowledges a GPU render.
#[derive(Default)]
pub(crate) struct CpuOnly;
impl FrameBackend for CpuOnly {
    const CPU_ONLY: bool = true;
    fn set(&mut self, _: TextureKey, _: &ImageDelta) -> Result<(), FrameError> {
        Ok(())
    }
    fn platform(
        &mut self,
        _: &egui::PlatformOutput,
        _: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> Result<(), FrameError> {
        Ok(())
    }
    fn submit(
        &mut self,
        _: ContextGeneration,
        _: &[egui::ClippedPrimitive],
        _: f32,
    ) -> Result<(), FrameError> {
        Ok(())
    }
    fn complete(&mut self) -> Result<(), FrameError> {
        Ok(())
    }
    fn drain(&mut self) {}
    fn free(&mut self, _: TextureKey) {}
}

#[derive(Clone, Copy)]
struct Texture {
    id: TextureId,
    size: [usize; 2],
}

/// One namespace, eight fixed slots, one possible upload/draw in flight. Slots
/// are registered BEFORE calling the backend, so a failed partial set is still
/// owned and retired. There is no retry queue or orphaned-generation history.
pub(crate) struct TextureOwner<B: FrameBackend> {
    backend: B,
    generation: Option<ContextGeneration>,
    textures: [Option<Texture>; MAX_TEXTURES],
    in_flight: bool,
}
impl<B: FrameBackend> TextureOwner<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self {
            backend,
            generation: None,
            textures: [None; MAX_TEXTURES],
            in_flight: false,
        }
    }
    pub(crate) fn begin(&mut self, generation: ContextGeneration) {
        if self.generation != Some(generation) {
            self.retire();
            self.generation = Some(generation);
        }
    }
    pub(crate) fn retire(&mut self) {
        if self.in_flight {
            self.backend.drain();
            self.in_flight = false;
        }
        if let Some(generation) = self.generation.take() {
            for texture in &mut self.textures {
                if let Some(texture) = texture.take() {
                    self.backend.free(TextureKey {
                        generation,
                        id: texture.id,
                    });
                }
            }
        }
    }
    pub(crate) fn process(
        &mut self,
        generation: ContextGeneration,
        output: &mut egui::FullOutput,
        meshes: &[egui::ClippedPrimitive],
    ) -> Result<(), FrameError> {
        if self.generation != Some(generation) || self.in_flight {
            return Err(FrameError::TextureProtocol);
        }
        let delta = &output.textures_delta;
        if delta.set.len() > MAX_TEXTURES
            || delta.free.len() > MAX_TEXTURES
            || delta.set.values().map(|d| d.len()).sum::<usize>() > MAX_UPDATES
        {
            return Err(FrameError::TextureCapacity);
        }
        // Stable order without allocating or relying on randomized hash iteration.
        let mut ids = [TextureId::Managed(0); MAX_TEXTURES];
        for (slot, id) in ids.iter_mut().zip(delta.set.keys()) {
            *slot = *id;
        }
        let ids = &mut ids[..delta.set.len()];
        ids.sort_unstable();
        for id in ids {
            for update in &delta.set[id] {
                let size = update.image.size();
                let existing = self
                    .textures
                    .iter()
                    .position(|t| t.is_some_and(|t| t.id == *id));
                let slot = if let Some(pos) = update.pos {
                    let slot = existing.ok_or(FrameError::TextureProtocol)?;
                    let old = self.textures[slot].unwrap().size;
                    if (0..2).any(|axis| {
                        pos[axis]
                            .checked_add(size[axis])
                            .is_none_or(|end| end > old[axis])
                    }) {
                        return Err(FrameError::TextureProtocol);
                    }
                    slot
                } else {
                    let slot = existing
                        .or_else(|| self.textures.iter().position(Option::is_none))
                        .ok_or(FrameError::TextureCapacity)?;
                    self.textures[slot] = Some(Texture { id: *id, size });
                    slot
                };
                let key = TextureKey {
                    generation,
                    id: self.textures[slot].unwrap().id,
                };
                // Upload failure can be ambiguous too; drain before freeing.
                self.in_flight = true;
                self.backend.set(key, update)?;
            }
        }
        self.backend
            .platform(&output.platform_output, &output.viewport_output)?;
        // No user textures or callbacks enter this isolated seam. Validate every
        // reference before a draw; a numeric id from another Context is not enough.
        for primitive in meshes {
            let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                return Err(FrameError::TextureProtocol);
            };
            if !self
                .textures
                .iter()
                .any(|t| t.is_some_and(|t| t.id == mesh.texture_id))
            {
                return Err(FrameError::TextureProtocol);
            }
        }
        self.in_flight = true;
        self.backend
            .submit(generation, meshes, output.pixels_per_point)?;
        self.backend.complete()?;
        self.in_flight = false;
        // A free is effective only after the last dependent draw completed.
        for texture in &mut self.textures {
            if texture.is_some_and(|t| delta.free.contains(&t.id)) {
                let texture = texture.take().unwrap();
                self.backend.free(TextureKey {
                    generation,
                    id: texture.id,
                });
            }
        }
        output.textures_delta.clear();
        Ok(())
    }
}
impl<B: FrameBackend> Drop for TextureOwner<B> {
    fn drop(&mut self) {
        self.retire();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::readonly_v2::frame_tests::{Event, Log, Mock};
    use std::{cell::RefCell, rc::Rc};

    fn image(partial: bool) -> ImageDelta {
        let image = egui::ColorImage::filled([2, 2], egui::Color32::WHITE);
        if partial {
            ImageDelta::partial([1, 1], image, Default::default())
        } else {
            ImageDelta::full(
                egui::ColorImage::filled([4, 4], egui::Color32::WHITE),
                Default::default(),
            )
        }
    }
    fn output(id: TextureId, partial: bool, free: bool) -> egui::FullOutput {
        let mut output = egui::FullOutput::default();
        output.textures_delta.push(id, image(partial));
        if free {
            output.textures_delta.free(id);
        }
        output
    }
    #[test]
    fn whole_partial_draw_completion_and_free_order() {
        let log = Rc::new(RefCell::new(Log::default()));
        let mut owner = TextureOwner::new(Mock(log.clone()));
        let generation = ContextGeneration::allocate().unwrap();
        owner.begin(generation);
        let key = TextureKey {
            generation,
            id: TextureId::Managed(0),
        };
        let mut out = output(key.id, false, false);
        out.textures_delta.push(key.id, image(true));
        out.textures_delta.free(key.id);
        owner.process(generation, &mut out, &[]).unwrap();
        assert!(out.textures_delta.is_empty());
        assert_eq!(
            log.borrow().events,
            [
                Event::Set(key, false),
                Event::Set(key, true),
                Event::Platform,
                Event::Submit(generation),
                Event::Complete,
                Event::Free(key)
            ]
        );
        assert!(owner.textures.iter().all(Option::is_none));
        owner.retire();
        assert_eq!(log.borrow().events.len(), 6);
    }
    #[test]
    fn unknown_or_out_of_bounds_partial_never_reaches_backend() {
        for existing in [false, true] {
            let log = Rc::new(RefCell::new(Log::default()));
            let mut owner = TextureOwner::new(Mock(log.clone()));
            let generation = ContextGeneration::allocate().unwrap();
            owner.begin(generation);
            if existing {
                owner
                    .process(
                        generation,
                        &mut output(TextureId::Managed(0), false, false),
                        &[],
                    )
                    .unwrap();
            }
            let before = log.borrow().events.len();
            let mut out = egui::FullOutput::default();
            out.textures_delta.push(
                TextureId::Managed(0),
                ImageDelta::partial(
                    [usize::MAX, 0],
                    egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
                    Default::default(),
                ),
            );
            assert_eq!(
                owner.process(generation, &mut out, &[]),
                Err(FrameError::TextureProtocol)
            );
            out.textures_delta.clear();
            assert_eq!(log.borrow().events.len(), before);
        }
    }
    #[test]
    fn bounded_slots_updates_and_single_generation_have_no_retirement_queue() {
        let log = Rc::new(RefCell::new(Log::default()));
        let mut owner = TextureOwner::new(Mock(log.clone()));
        let generation = ContextGeneration::allocate().unwrap();
        owner.begin(generation);
        for id in 0..MAX_TEXTURES {
            owner
                .process(
                    generation,
                    &mut output(TextureId::Managed(id as u64), false, false),
                    &[],
                )
                .unwrap();
        }
        let mut overflow = output(TextureId::Managed(MAX_TEXTURES as u64), false, false);
        assert_eq!(
            owner.process(generation, &mut overflow, &[]),
            Err(FrameError::TextureCapacity)
        );
        overflow.textures_delta.clear();
        let mut out = output(TextureId::Managed(0), false, false);
        for _ in 0..MAX_UPDATES {
            out.textures_delta.push(TextureId::Managed(0), image(true));
        }
        assert_eq!(
            owner.process(generation, &mut out, &[]),
            Err(FrameError::TextureCapacity)
        );
        out.textures_delta.clear();
        let next = ContextGeneration::allocate().unwrap();
        owner.begin(next);
        assert_eq!(owner.generation, Some(next));
        assert!(owner.textures.iter().all(Option::is_none));
        assert_eq!(
            log.borrow()
                .events
                .iter()
                .filter(|e| matches!(e, Event::Free(_)))
                .count(),
            MAX_TEXTURES
        );
        assert_eq!(
            owner.process(generation, &mut Default::default(), &[]),
            Err(FrameError::TextureProtocol)
        );
    }
    #[test]
    fn failure_after_an_applied_texture_keeps_all_partial_obligations_until_drain() {
        let log = Rc::new(RefCell::new(Log {
            fault: Some(FrameError::TextureUpdate),
            fail_set_on: 2,
            ..Default::default()
        }));
        let mut owner = TextureOwner::new(Mock(log.clone()));
        let generation = ContextGeneration::allocate().unwrap();
        owner.begin(generation);
        let mut out = output(TextureId::Managed(0), false, false);
        out.textures_delta.push(TextureId::Managed(1), image(false));
        out.textures_delta.push(TextureId::Managed(2), image(false));
        // Same frame: first set succeeds, second is ambiguous, third is never
        // attempted. Only the first two keys must be drained/freed.
        assert_eq!(
            owner.process(generation, &mut out, &[]),
            Err(FrameError::TextureUpdate)
        );
        assert!(owner.in_flight);
        assert_eq!(owner.textures.iter().flatten().count(), 2);
        assert!(!out.textures_delta.is_empty());
        owner.retire();
        out.textures_delta.clear();
        let events = &log.borrow().events;
        assert_eq!(
            &events[events.len() - 3..],
            &[
                Event::Drain,
                Event::Free(TextureKey {
                    generation,
                    id: TextureId::Managed(0)
                }),
                Event::Free(TextureKey {
                    generation,
                    id: TextureId::Managed(1)
                })
            ]
        );
    }
}

//! Bounded, serial host seam. No GPU backend or GPU qualification lives here.
use super::completion::{self, Completion, WaitObservation, WaitPolicy, WaitPurpose, WaitRequest};
use super::deletion::{DeletionFailure, DeletionResult};
use super::frame::{ContextGeneration, FrameError};
use eframe::egui::{self, TextureId, epaint::ImageDelta};

pub(crate) const MAX_TEXTURES: usize = 8;
pub(crate) const MAX_UPDATES: usize = 16;

pub(crate) use super::native_types::TextureKey;

/// Trusted serial host. Every operation may start work before reporting failure.
/// `wait` observes ALL dependent uses, including partial upload/platform failure;
/// only Quiesced permits free. A real adapter must establish/flush a valid fence
/// on its current creating context and distinguish timeout, wait error and loss.
/// Borrowed input cannot be retained after a call: backend staging is host-owned.
/// `release_frame` confirms disposal of completion objects after Quiesced.
/// `free` confirms an entire registered key, including partial creation.
/// `release_generation` confirms ALL remaining non-texture objects. A failure
/// retains its exact identity and forbids retries until separately resolved.
/// Backend destruction must not release in-use or ambiguous objects.
/// Real context/device teardown remains a separate qualification obligation.
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
    fn wait(&mut self, request: WaitRequest) -> Completion;
    fn release_frame(&mut self, generation: ContextGeneration) -> DeletionResult;
    fn free(&mut self, key: TextureKey) -> DeletionResult;
    fn release_generation(&mut self, generation: ContextGeneration) -> DeletionResult;
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
    fn wait(&mut self, _: WaitRequest) -> Completion {
        Completion::Quiesced
    }
    fn release_frame(&mut self, _: ContextGeneration) -> DeletionResult {
        Ok(())
    }
    fn free(&mut self, _: TextureKey) -> DeletionResult {
        Ok(())
    }
    fn release_generation(&mut self, _: ContextGeneration) -> DeletionResult {
        Ok(())
    }
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
    pub(super) deletion: Option<DeletionFailure>,
    pub(super) last_wait: WaitObservation,
}
impl<B: FrameBackend> TextureOwner<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self {
            backend,
            generation: None,
            textures: [None; MAX_TEXTURES],
            in_flight: false,
            deletion: None,
            last_wait: WaitObservation::IDLE,
        }
    }
    pub(crate) fn begin(&mut self, generation: ContextGeneration) -> Result<(), FrameError> {
        if self.deletion.is_some()
            || self.in_flight
            || (self.generation.is_some() && self.generation != Some(generation))
        {
            return Err(FrameError::RetirementPending);
        }
        if self.generation != Some(generation) {
            self.last_wait = WaitObservation::IDLE;
        }
        self.generation = Some(generation);
        Ok(())
    }
    pub(super) fn generation(&self) -> Option<ContextGeneration> {
        self.generation
    }
    pub(crate) fn owned_keys(&self) -> usize {
        self.textures.iter().flatten().count()
    }
    pub(super) fn quiesced(&self) -> bool {
        !self.in_flight
    }
    fn started(&mut self) {
        self.in_flight = true;
        self.last_wait = WaitObservation {
            completion: Completion::Unobserved,
            ..WaitObservation::IDLE
        };
    }
    pub(super) fn wait(&mut self, purpose: WaitPurpose, policy: WaitPolicy) -> WaitObservation {
        if self.in_flight {
            self.last_wait =
                completion::observe(&mut self.backend, self.generation.unwrap(), purpose, policy);
            if self.last_wait.completion == Completion::Quiesced {
                self.in_flight = false;
            }
        }
        self.last_wait
    }
    /// Releases only after a structural or backend quiescence proof. No wait.
    pub(super) fn release(&mut self) -> bool {
        if self.in_flight || self.deletion.is_some() {
            return false;
        }
        if let Some(generation) = self.generation {
            if let Err(error) = self.backend.release_frame(generation) {
                self.deletion = Some(error);
                return false;
            }
            for slot in 0..MAX_TEXTURES {
                if let Some(texture) = self.textures[slot] {
                    if let Err(error) = self.backend.free(TextureKey {
                        generation,
                        id: texture.id,
                    }) {
                        self.deletion = Some(error);
                        return false;
                    }
                    self.textures[slot] = None;
                }
            }
            if let Err(error) = self.backend.release_generation(generation) {
                self.deletion = Some(error);
                return false;
            }
            self.generation = None;
        }
        true
    }
    #[cfg(test)]
    fn retire(&mut self) {
        self.wait(WaitPurpose::Retirement, WaitPolicy::default());
        assert!(self.release());
    }
    pub(crate) fn process(
        &mut self,
        generation: ContextGeneration,
        output: &mut egui::FullOutput,
        meshes: &[egui::ClippedPrimitive],
    ) -> Result<(), FrameError> {
        if self.generation != Some(generation) || self.in_flight || self.deletion.is_some() {
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
                self.started();
                self.backend.set(key, update)?;
            }
        }
        // Platform output can start dependent work even with an empty set delta.
        // Arm cleanup before calling it, including its ambiguous error path.
        self.started();
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
        self.started();
        self.backend
            .submit(generation, meshes, output.pixels_per_point)?;
        if self
            .wait(WaitPurpose::Frame, WaitPolicy::default())
            .completion
            != Completion::Quiesced
        {
            return Err(FrameError::Completion);
        }
        if let Err(error) = self.backend.release_frame(generation) {
            self.deletion = Some(error);
            return Err(FrameError::Deletion);
        }
        // A free is effective only after the last dependent draw completed.
        for texture in &mut self.textures {
            if texture.is_some_and(|t| delta.free.contains(&t.id)) {
                let key = TextureKey {
                    generation,
                    id: texture.unwrap().id,
                };
                if let Err(error) = self.backend.free(key) {
                    self.deletion = Some(error);
                    return Err(FrameError::Deletion);
                }
                *texture = None;
            }
        }
        output.textures_delta.clear();
        Ok(())
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
        owner.begin(generation).unwrap();
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
    fn errors_without_new_uploads_drain_before_retirement() {
        for existing in [false, true] {
            for error in [
                FrameError::PlatformOutput,
                FrameError::Submission,
                FrameError::Completion,
            ] {
                let log = Rc::new(RefCell::new(Log::default()));
                let mut owner = TextureOwner::new(Mock(log.clone()));
                let generation = ContextGeneration::allocate().unwrap();
                owner.begin(generation).unwrap();
                let key = TextureKey {
                    generation,
                    id: TextureId::Managed(0),
                };
                if existing {
                    owner
                        .process(generation, &mut output(key.id, false, false), &[])
                        .unwrap();
                }
                log.borrow_mut().events.clear();
                log.borrow_mut().fault = Some(error);
                // This frame contains NO upload, including no font atlas update.
                let mut out = egui::FullOutput::default();
                assert_eq!(owner.process(generation, &mut out, &[]), Err(error));
                assert!(log.borrow().busy);
                owner.retire();
                let events = log.borrow().events.clone();
                let expected_calls = match error {
                    FrameError::PlatformOutput => vec![Event::Platform],
                    FrameError::Submission => vec![Event::Platform, Event::Submit(generation)],
                    FrameError::Completion => {
                        vec![Event::Platform, Event::Submit(generation), Event::Complete]
                    }
                    _ => unreachable!(),
                };
                let mut expected = expected_calls;
                expected.push(Event::Drain);
                if existing {
                    expected.push(Event::Free(key));
                }
                assert_eq!(events, expected);
                assert!(!log.borrow().busy);
                assert!(owner.generation.is_none());
                assert!(owner.textures.iter().all(Option::is_none));
                owner.retire();
                drop(owner);
                assert_eq!(log.borrow().events, events); // No retry or double free.
            }
        }
    }
    #[test]
    fn unknown_or_out_of_bounds_partial_never_reaches_backend() {
        for existing in [false, true] {
            let log = Rc::new(RefCell::new(Log::default()));
            let mut owner = TextureOwner::new(Mock(log.clone()));
            let generation = ContextGeneration::allocate().unwrap();
            owner.begin(generation).unwrap();
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
        owner.begin(generation).unwrap();
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
        assert_eq!(owner.begin(next), Err(FrameError::RetirementPending));
        owner.retire();
        owner.begin(next).unwrap();
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
        owner.begin(generation).unwrap();
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

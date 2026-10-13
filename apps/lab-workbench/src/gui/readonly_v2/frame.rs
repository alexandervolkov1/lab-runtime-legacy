//! Single-slot CPU frame ownership and process-local Context identities.
use super::{
    Preview, RenderReport,
    texture_owner::{CpuOnly, FrameBackend},
};
use eframe::egui;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Unique across all Preview lifetimes in this process; never recycled/wrapped.
/// Not a persistent or cross-process GPU resource identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ContextGeneration(u64);
impl ContextGeneration {
    pub(super) fn allocate() -> Result<Self, FrameError> {
        Self::from_counter(&NEXT_GENERATION)
    }
    fn from_counter(counter: &AtomicU64) -> Result<Self, FrameError> {
        let mut candidate = counter.load(Ordering::Relaxed);
        // Bound contention as well as exhaustion. No automatic caller retry.
        for _ in 0..8 {
            if candidate == 0 {
                return Err(FrameError::GenerationExhausted);
            }
            match counter.compare_exchange(
                candidate,
                candidate.checked_add(1).unwrap_or(0),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(Self(candidate)),
                Err(current) => candidate = current,
            }
        }
        Err(FrameError::GenerationContended)
    }
}

/// Fixed diagnostics; errors never carry unbounded host strings or imply retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameError {
    OutstandingFrame,
    Preparation(&'static str),
    GenerationExhausted,
    GenerationContended,
    AlreadyTessellated,
    NotTessellated,
    Tessellation,
    TextureCapacity,
    TextureProtocol,
    TextureUpdate,
    PlatformOutput,
    Submission,
    Completion,
    RetirementPending,
    HostClosed,
}

/// Preparation diagnostics are distinct from a completed host acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameOutcome {
    CpuOnly { degraded: bool },
    HostCompleted { degraded: bool },
    Discarded,
    Abandoned,
    Failed(FrameError),
}

pub(super) struct FrameData {
    pub(super) output: egui::FullOutput,
    pub(super) report: RenderReport,
    pub(super) generation: ContextGeneration,
    pub(super) meshes: Vec<egui::ClippedPrimitive>,
    attempted: bool,
    fault: Option<FrameError>,
}
impl FrameData {
    pub(super) fn new(
        output: egui::FullOutput,
        report: RenderReport,
        generation: ContextGeneration,
    ) -> Self {
        Self {
            output,
            report,
            generation,
            meshes: Vec::new(),
            attempted: false,
            fault: None,
        }
    }
}
impl Drop for FrameData {
    fn drop(&mut self) {
        // Only CPU image bytes are discarded here. The session owns host
        // obligations and retains this payload until retirement is proven.
        self.output.textures_delta.clear();
    }
}

/// Borrows the presentation owner exclusively until finish/discard/Drop. The
/// payload remains in Preview so even forgetting this guard cannot authorize a
/// second FullOutput or leak CPU buffers past Preview shutdown.
#[must_use = "finish or discard the frame; Drop aborts and retires its Context"]
pub(crate) struct PreviewFrame<'p, 'd, B: FrameBackend = CpuOnly> {
    pub(super) preview: &'p mut Preview<'d, B>,
    pub(super) ended: bool,
}
impl PreviewFrame<'_, '_> {
    /// Probe convenience, still using the same one-attempt frame and ledger.
    pub(crate) fn finish_cpu(mut self) -> Result<FrameOutcome, FrameError> {
        if !self.data().attempted {
            self.tessellate()?;
        }
        self.finish()
    }
}
impl<B: FrameBackend> PreviewFrame<'_, '_, B> {
    pub(crate) fn generation(&self) -> ContextGeneration {
        self.data().generation
    }
    pub(crate) fn report(&self) -> RenderReport {
        self.data().report
    }
    /// Borrowed inspection only; retaining cloned shapes is outside the CPU budget.
    pub(crate) fn shapes(&self) -> &[egui::epaint::ClippedShape] {
        &self.data().output.shapes
    }
    pub(crate) fn completed_passes(&self) -> usize {
        self.data().output.platform_output.num_completed_passes
    }
    fn data(&self) -> &FrameData {
        self.preview.pending.as_ref().unwrap()
    }

    /// Exactly one attempt, using the Context that created this output. Pinned
    /// egui tessellation is infallible at its API (allocator OOM may abort).
    /// Invalid returned CPU meshes are an error, never a successful empty render.
    pub(crate) fn tessellate(&mut self) -> Result<&[egui::ClippedPrimitive], FrameError> {
        self.tessellate_with(|context, shapes, scale| Ok(context.tessellate(shapes, scale)))
    }
    fn tessellate_with(
        &mut self,
        tessellate: impl FnOnce(
            &egui::Context,
            Vec<egui::epaint::ClippedShape>,
            f32,
        ) -> Result<Vec<egui::ClippedPrimitive>, FrameError>,
    ) -> Result<&[egui::ClippedPrimitive], FrameError> {
        let data = self.preview.pending.as_mut().unwrap();
        if data.attempted {
            data.fault.get_or_insert(FrameError::AlreadyTessellated);
            return Err(FrameError::AlreadyTessellated);
        }
        data.attempted = true;
        // A panic also leaves a failed slot; no catch_unwind/OOM recovery here.
        data.fault = Some(FrameError::Tessellation);
        let had_shapes = !data.output.shapes.is_empty();
        let meshes = tessellate(
            self.preview.context.as_ref().unwrap(),
            std::mem::take(&mut data.output.shapes),
            data.output.pixels_per_point,
        )?;
        if (had_shapes && meshes.is_empty())
            || meshes.iter().any(|p| match &p.primitive {
                egui::epaint::Primitive::Mesh(m) => {
                    !m.is_valid()
                        || !p.clip_rect.is_finite()
                        || m.vertices
                            .iter()
                            .any(|v| !v.pos.is_finite() || !v.uv.is_finite())
                }
                _ => true,
            })
        {
            return Err(FrameError::Tessellation);
        }
        data.meshes = meshes;
        data.fault = None;
        Ok(&data.meshes)
    }
    /// Consume output through the backend, including texture updates and draw
    /// completion. CPU-only completion is explicitly not a render acknowledgement.
    pub(crate) fn finish(mut self) -> Result<FrameOutcome, FrameError> {
        let data = self.preview.pending.as_mut().unwrap();
        let result = if let Some(fault) = data.fault {
            Err(fault)
        } else if !data.attempted {
            Err(FrameError::NotTessellated)
        } else {
            self.preview
                .session
                .textures
                .process(data.generation, &mut data.output, &data.meshes)
        };
        let outcome = match result {
            Ok(()) => {
                let degraded = data.report.allocation_faults != 0
                    || data.report.geometry_failures != 0
                    || data.report.geometry_unavailable != 0;
                if B::CPU_ONLY {
                    FrameOutcome::CpuOnly { degraded }
                } else {
                    FrameOutcome::HostCompleted { degraded }
                }
            }
            Err(error) => FrameOutcome::Failed(error),
        };
        if result.is_err() {
            self.preview.retire_with(
                super::host_session::RetirementReason::FrameFailed,
                Some(outcome),
            );
            // An unsuccessful completion already spent its bounded observation
            // budget. Preserve it; only the caller may request another wait.
            if result != Err(FrameError::Completion) {
                self.preview
                    .poll_retirement(super::completion::WaitPolicy::default());
            }
        } else {
            self.preview.pending = None;
        }
        self.preview.last_outcome = Some(outcome);
        self.ended = true;
        result.map(|()| outcome)
    }
    pub(crate) fn discard(mut self) {
        self.preview.abort_frame(FrameOutcome::Discarded);
        self.ended = true;
    }
}
impl<B: FrameBackend> Drop for PreviewFrame<'_, '_, B> {
    fn drop(&mut self) {
        if !self.ended {
            self.preview.abort_frame(FrameOutcome::Abandoned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_exhaustion_never_wraps_or_restarts() {
        let counter = AtomicU64::new(u64::MAX - 1);
        assert_eq!(
            ContextGeneration::from_counter(&counter),
            Ok(ContextGeneration(u64::MAX - 1))
        );
        assert_eq!(
            ContextGeneration::from_counter(&counter),
            Ok(ContextGeneration(u64::MAX))
        );
        for _ in 0..3 {
            assert_eq!(
                ContextGeneration::from_counter(&counter),
                Err(FrameError::GenerationExhausted)
            );
        }
    }
    #[test]
    fn tessellation_error_is_not_an_acknowledgement() {
        let doc = crate::gui::readonly_v2::plot_fixtures::admit(
            &crate::gui::readonly_v2::plot_fixtures::syntax(1, false),
        );
        let mut preview = Preview::new(&doc, crate::gui::readonly_v2::plot_widget_tests::EPOCH);
        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        assert!(matches!(
            frame.tessellate_with(|_, _, _| Err(FrameError::Tessellation)),
            Err(FrameError::Tessellation)
        ));
        assert_eq!(frame.finish(), Err(FrameError::Tessellation));
        assert!(preview.context.is_none());
        assert_eq!(
            preview.last_outcome(),
            Some(FrameOutcome::Failed(FrameError::Tessellation))
        );
    }
}

//! Isolated bounded read-only preview. No active presentation consumer or drafts.
#![allow(dead_code, reason = "isolated pending activation qualification")]

pub(crate) mod completion;
pub(crate) mod deletion;
#[cfg(test)]
mod deletion_tests;
mod finite_trace;
#[cfg(test)]
mod fixtures;
pub(crate) mod frame;
#[cfg(test)]
mod frame_tests;
pub(crate) mod host_session;
mod native_types;
mod navigation;
#[cfg(test)]
mod navigation_tests;
pub(crate) mod observation_source;
mod page;
mod plot_bounds;
#[cfg(test)]
pub(crate) mod plot_fixtures;
#[cfg(test)]
mod plot_geometry_fixtures;
#[cfg(test)]
mod plot_geometry_tests;
pub(crate) mod plot_projection;
#[cfg(test)]
mod plot_projection_tests;
#[cfg(test)]
mod plot_seam_tests;
mod plot_text;
#[cfg(test)]
mod plot_text_tests;
pub(crate) mod plot_widget;
#[cfg(test)]
mod plot_widget_tests;
mod read;
#[cfg(test)]
mod retirement_tests;
#[cfg(test)]
mod tests;
pub(crate) mod texture_owner;

#[cfg(test)]
use crate::model::RuntimeObservations;
use crate::presentation::v2;
use completion::WaitPolicy;
use eframe::egui;
use frame::{ContextGeneration, FrameData, FrameError, FrameOutcome, PreviewFrame};
use host_session::{CleanupOutcome, HostSession, RetirementPayload, RetirementReason, Session};
pub(crate) use navigation::Location;
use observation_source::ReadOnlyProvider;
use page::{Page, TextBuffer, label};
use texture_owner::{CpuOnly, FrameBackend};

/// Identity supplied by the owner; replacement must advance revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PresentationEpoch {
    pub(crate) owner: [u8; 16],
    pub(crate) revision: u64,
}

/// Exact borrowed semantic identity, including each repeated occurrence's path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct OccurrenceIdentity<'a> {
    pub(crate) epoch: PresentationEpoch,
    pub(crate) occurrence: v2::ViewOccurrenceId<'a>,
}
impl OccurrenceIdentity<'_> {
    pub(crate) fn widget_id(&self) -> egui::Id {
        egui::Id::new(("readonly-v2", self))
    }
}

pub(crate) const PAGE_SIZE: usize = 4;
pub(crate) const MAX_LABELS: usize = 64;
pub(crate) const MAX_TEXT_BYTES: usize = 12 * 1024;
/// Explicit widgets plus conservatively charged layout/scroll containers.
pub(crate) const MAX_WIDGETS: usize = 128;
/// Four visible cells plus their bounded ancestor walks and the group header.
pub(crate) const MAX_PATH_NODES: usize = 28;
#[allow(
    unused_imports,
    reason = "private native backend bound; active host does not call it"
)]
pub(crate) use native_types::MAX_SHAPES;
/// Uniform read-only reservation; includes Context, scratch, output and overlap.
/// This replaces unmaterialized view credits; it is not added to 20,480 views.
pub(crate) const PREVIEW_RESERVATION: usize = 8 * 1024 * 1024;
pub(crate) const NOTICE_RESERVATION: usize = 1024 * 1024;
pub(crate) const ACTIVATION_QUALIFIED: bool = false;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderReport {
    pub(crate) windows: usize,
    pub(crate) tabs: usize,
    pub(crate) panels: usize,
    pub(crate) occurrences: usize,
    pub(crate) placeholders: usize,
    pub(crate) statically_disabled: usize,
    pub(crate) maximum_path: usize,
    pub(crate) path_nodes: usize,
    pub(crate) labels: usize,
    pub(crate) text_bytes: usize,
    pub(crate) widgets: usize,
    pub(crate) guarded: bool,
    pub(crate) plot_points: usize,
    pub(crate) plot_traces: usize,
    pub(crate) geometry_segments: usize,
    pub(crate) geometry_unavailable: usize,
    pub(crate) geometry_failures: usize,
    /// Failed fallible curve reservations; never count as rendered geometry.
    pub(crate) allocation_faults: usize,
}

/// Owns one isolated Context, one fixed page of text and a four-slot navigation
/// path. The document/observations are borrowed, never copied into a GUI model.
/// No Context handle escapes: another consumer cannot expand its caches.
pub(crate) struct Preview<'a, B: FrameBackend = CpuOnly> {
    document: &'a v2::AdmittedV2Document,
    epoch: PresentationEpoch,
    location: Location,
    context: Option<egui::Context>,
    generation: Option<ContextGeneration>,
    session: Session<'a, B>,
    pending: Option<FrameData>,
    last_outcome: Option<FrameOutcome>,
    previous: Option<Page>,
    scroll: egui::Vec2,
    allowance: usize,
    plot: Option<plot_widget::PlotState<'a>>,
}

impl<'a> Preview<'a> {
    pub(crate) fn new(document: &'a v2::AdmittedV2Document, epoch: PresentationEpoch) -> Self {
        Self::with_owner(document, epoch, Session::Cpu(HostSession::new(CpuOnly)))
    }
}

impl<'a, B: FrameBackend> Preview<'a, B> {
    pub(crate) fn with_session(
        document: &'a v2::AdmittedV2Document,
        epoch: PresentationEpoch,
        session: &'a mut HostSession<B>,
    ) -> Self {
        Self::with_owner(document, epoch, Session::Host(session))
    }

    fn with_owner(
        document: &'a v2::AdmittedV2Document,
        epoch: PresentationEpoch,
        session: Session<'a, B>,
    ) -> Self {
        Self {
            document,
            epoch,
            location: Location::default(),
            context: None,
            generation: None,
            session,
            pending: None,
            last_outcome: None,
            previous: None,
            scroll: egui::Vec2::ZERO,
            allowance: PREVIEW_RESERVATION,
            plot: None,
        }
    }

    pub(crate) fn replace(
        &mut self,
        document: &'a v2::AdmittedV2Document,
        epoch: PresentationEpoch,
    ) -> bool {
        if self.session.admit_work().is_err() {
            return false;
        }
        // Retaining the actual borrow prevents allocator-address reuse from
        // confusing two immutable admissions, even if the caller forgot revision.
        if !std::ptr::eq(self.document, document) || self.epoch != epoch {
            self.retire();
            self.plot = None;
            self.location = Location::default();
            self.scroll = egui::Vec2::ZERO;
            self.document = document;
            self.epoch = epoch;
        }
        true
    }

    /// Local navigation only. Invalid addresses fail without changing the view.
    pub(crate) fn navigate(&mut self, location: Location) -> bool {
        if self.session.admit_work().is_err() {
            return false;
        }
        if location == self.location {
            return true;
        }
        if !location.valid(self.document) {
            return false;
        }
        if location != self.location {
            self.retire();
            self.plot = None;
            self.location = location;
            self.scroll = egui::Vec2::ZERO;
        }
        true
    }

    pub(crate) fn location(&self) -> Location {
        self.location
    }

    /// Below the notice allowance, denial returns only a static reason.
    pub(crate) fn set_allowance(&mut self, bytes: usize) {
        self.allowance = bytes.min(PREVIEW_RESERVATION);
        if self.allowance < PREVIEW_RESERVATION {
            self.retire();
        }
    }

    fn retire(&mut self) {
        let outcome = self.pending.as_ref().map(|_| FrameOutcome::Abandoned);
        self.retire_with(RetirementReason::ViewChanged, outcome);
    }

    fn retire_with(
        &mut self,
        reason: RetirementReason,
        outcome: Option<FrameOutcome>,
    ) -> CleanupOutcome {
        let payload = RetirementPayload {
            context: self.context.take(),
            frame: self.pending.take(),
        };
        self.generation = None;
        self.previous = None;
        if let Some(outcome) = outcome {
            self.last_outcome = Some(outcome);
        }
        self.session.retire(reason, outcome, payload)
    }

    fn abort_frame(&mut self, outcome: FrameOutcome) {
        let reason = if outcome == FrameOutcome::Discarded {
            RetirementReason::Discarded
        } else {
            RetirementReason::Abandoned
        };
        self.retire_with(reason, Some(outcome));
    }

    pub(crate) fn last_outcome(&self) -> Option<FrameOutcome> {
        self.last_outcome
    }

    fn ensure_context(&mut self) -> Result<(), FrameError> {
        if self.context.is_none() {
            let generation = ContextGeneration::allocate()?;
            self.session.begin(generation)?;
            self.context = Some(new_context());
            self.generation = Some(generation);
        }
        Ok(())
    }

    /// Request retirement, preserving the logical Plot slot. Unresolved host
    /// work retains the caches/frame in HostSession and blocks further rendering.
    pub(crate) fn retire_context(&mut self) -> CleanupOutcome {
        let outcome = self.pending.as_ref().map(|_| FrameOutcome::Abandoned);
        self.retire_with(RetirementReason::Explicit, outcome)
    }

    pub(crate) fn cleanup_outcome(&self) -> CleanupOutcome {
        self.session.status()
    }

    pub(crate) fn poll_retirement(&mut self, policy: WaitPolicy) -> CleanupOutcome {
        self.session.poll_retirement(policy)
    }

    pub(crate) fn close_plot(&mut self) -> bool {
        if self.session.admit_work().is_err() {
            return false;
        }
        self.retire();
        self.plot = None;
        true
    }

    /// Open only an admitted Plot on the current four-cell page. No projection
    /// or allocation is needed merely to choose this local viewing mode.
    pub(crate) fn open_plot(&mut self, index: usize) -> bool {
        if self.session.admit_work().is_err() {
            return false;
        }
        if self.select_plot(index) {
            self.retire();
            true
        } else {
            false
        }
    }

    pub(crate) fn select_plot(&mut self, index: usize) -> bool {
        if index / PAGE_SIZE != self.location.page || !self.location.valid(self.document) {
            return false;
        }
        let (root, path) = self.location.occurrence(index);
        let Some(view) = self.document.source().occurrence(
            self.location.window,
            self.location.tab,
            self.location.panel,
            root,
            &path[..self.location.depth],
        ) else {
            return false;
        };
        let v2::Component::Plot { plot_id, .. } = view.component else {
            return false;
        };
        let Some(definition) = self
            .document
            .dto()
            .plots
            .0
            .iter()
            .find(|p| p.id == *plot_id)
        else {
            return false;
        };
        self.plot = Some(plot_widget::PlotState::from_definition(
            OccurrenceIdentity {
                epoch: self.epoch,
                occurrence: view.identity,
            },
            definition,
            index,
        ));
        true
    }

    /// Prepare one exclusively borrowed frame at the fixed CPU viewport. The
    /// private payload stays in Preview, including if its guard is forgotten.
    pub(crate) fn run(
        &mut self,
        observations: &dyn ReadOnlyProvider,
        input: egui::RawInput,
    ) -> Result<PreviewFrame<'_, 'a, B>, FrameError> {
        self.session.admit_work()?;
        if self.pending.is_some() {
            return Err(FrameError::OutstandingFrame);
        }
        self.last_outcome = None;
        let (output, report) = match self.prepare(observations, input) {
            Ok(frame) => frame,
            Err(error) => {
                self.retire_with(
                    RetirementReason::FrameFailed,
                    Some(FrameOutcome::Failed(error)),
                );
                return Err(error);
            }
        };
        self.pending = Some(FrameData::new(output, report, self.generation.unwrap()));
        Ok(PreviewFrame {
            preview: self,
            ended: false,
        })
    }

    fn prepare(
        &mut self,
        observations: &dyn ReadOnlyProvider,
        mut input: egui::RawInput,
    ) -> Result<(egui::FullOutput, RenderReport), FrameError> {
        if observations
            .epoch()
            .is_some_and(|epoch| epoch != self.epoch)
        {
            return Err(FrameError::Preparation("Read-only snapshot epoch mismatch"));
        }
        if self.allowance < NOTICE_RESERVATION {
            return Err(FrameError::Preparation(
                "Read-only preview paused: visual reservation unavailable",
            ));
        }
        if self.plot.is_some() && self.allowance >= PREVIEW_RESERVATION {
            return self.run_plot(observations, input);
        }
        let page = if self.allowance < PREVIEW_RESERVATION {
            Page::notice(self.epoch)
        } else {
            Page::prepare(self.document, observations, self.epoch, self.location)
        };
        if self.previous.as_ref() != Some(&page) {
            self.retire();
        }
        self.ensure_context()?;
        let context = self.context.as_ref().unwrap();
        // Arbitrary DPI, zoom, fonts, secondary viewports and screen sizes are
        // not accepted as another allocation authority in this isolated preview.
        input = bounded_input(input, false);
        let mut next = self.location;
        let mut plot = None;
        let output = context.run_ui(input, |ui| {
            page.paint(ui, &mut next, &mut self.scroll, &mut plot);
        });
        let mut report = page.report;
        if next != self.location {
            report.path_nodes += next.depth;
        }
        self.previous = Some(page);
        // Navigation changes the next view, but this output still needs the
        // generating Context's font atlas for CPU tessellation. No second
        // Context is created here. The next run retires it via the changed page
        // or PlotState::prepare, after the caller has consumed this output.
        // Change only the next logical selection. Defer the actual retirement
        // until the next prepare, retaining this frame's exact generation and
        // host namespace too (not just its Context handle).
        if next != self.location {
            self.location = next;
            self.scroll = egui::Vec2::ZERO;
            self.plot = None;
        }
        if let Some(index) = plot {
            // Selection does not touch the generating resource owner.
            self.select_plot(index);
        }
        Ok((output, report))
    }

    fn run_plot(
        &mut self,
        observations: &dyn ReadOnlyProvider,
        input: egui::RawInput,
    ) -> Result<(egui::FullOutput, RenderReport), FrameError> {
        // One combined reservation, checked before the first projection buffer.
        let view = plot_projection::project(
            self.document,
            observations,
            self.epoch,
            self.location,
            self.plot.as_ref().unwrap().index,
            self.allowance.min(plot_projection::PROJECTION_RESERVATION),
        )
        .map_err(|_| {
            FrameError::Preparation("Read-only Plot unavailable: bounded projection failed")
        })?;
        // Fixed-size text planning precedes font, widget and galley allocation.
        // A changed fragment retires the complete Context before new glyphs.
        let (changed, text_page) = self.plot.as_mut().unwrap().prepare(&view);
        if changed {
            self.retire();
        }
        self.ensure_context()?;
        let context = self.context.as_ref().unwrap();
        let state = self.plot.as_mut().unwrap();
        let mut result = (false, RenderReport::default());
        let output = context.run_ui(bounded_input(input, text_page.is_none()), |ui| {
            result = state.paint(ui, &view, text_page.as_ref());
        });
        if result.0 {
            // Keep the generating Context until this frame is consumed. The
            // next page has no previous-page key, so run retires it first.
            self.plot = None;
            self.previous = None;
        }
        Ok((output, result.1))
    }
}

impl<B: FrameBackend> Drop for Preview<'_, B> {
    fn drop(&mut self) {
        let outcome = self.pending.as_ref().map(|_| FrameOutcome::Abandoned);
        self.retire_with(RetirementReason::PreviewDropped, outcome);
    }
}

fn new_context() -> egui::Context {
    let context = egui::Context::default();
    context.options_mut(|options| {
        options.max_passes = std::num::NonZeroUsize::new(1).unwrap();
        options.zoom_with_keyboard = false;
    });
    context
}

fn bounded_input(mut input: egui::RawInput, plot: bool) -> egui::RawInput {
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(1280.0, 960.0),
    ));
    input.viewport_id = egui::ViewportId::ROOT;
    // collect() can reuse the caller's arbitrarily large spare capacity even
    // after take(64). Build a separate bounded buffer so Context cannot retain
    // that allocation. A future host must bound/charge the incoming RawInput
    // storage before transferring ownership to Preview.
    let mut events = Vec::new();
    for event in input.events.into_iter().take(64) {
        let accepted = match event {
            egui::Event::Zoom(factor)
                if plot && factor.is_finite() && (0.1..=10.0).contains(&factor) =>
            {
                Some(egui::Event::Zoom(factor))
            }
            egui::Event::PointerMoved(pos) if bounded_position(pos) => Some(event),
            egui::Event::PointerGone => Some(event),
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                ..
            } if bounded_position(pos) => Some(egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }),
            egui::Event::MouseWheel {
                unit, delta, phase, ..
            } if bounded_position(delta.to_pos2()) => Some(egui::Event::MouseWheel {
                unit,
                delta,
                phase,
                modifiers: egui::Modifiers::NONE,
            }),
            _ => None,
        };
        if let Some(event) = accepted {
            events.push(event);
        }
    }
    egui::RawInput {
        screen_rect: input.screen_rect,
        events,
        time: input.time,
        ..Default::default()
    }
}

// Finite alone is insufficient: egui computes f32 pointer deltas before the
// Plot callback. Events far outside this fixed CPU surface have no authority to
// overflow that arithmetic. This bounds input, never sample values.
fn bounded_position(pos: egui::Pos2) -> bool {
    pos.is_finite() && pos.x.abs() <= 16_384.0 && pos.y.abs() <= 16_384.0
}

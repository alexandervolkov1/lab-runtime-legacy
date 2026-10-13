//! Isolated bounded read-only preview. No active presentation consumer or drafts.
#![allow(dead_code, reason = "isolated pending activation qualification")]

#[cfg(test)]
mod fixtures;
mod navigation;
#[cfg(test)]
mod navigation_tests;
mod page;
#[cfg(test)]
mod plot_fixtures;
pub(crate) mod plot_projection;
#[cfg(test)]
mod plot_projection_tests;
mod plot_text;
#[cfg(test)]
mod plot_text_tests;
pub(crate) mod plot_widget;
#[cfg(test)]
mod plot_widget_tests;
mod read;
#[cfg(test)]
mod tests;

use crate::{model::RuntimeObservations, presentation::v2};
use eframe::egui;
pub(crate) use navigation::Location;
use page::{Page, TextBuffer, label};

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
pub(crate) const MAX_SHAPES: usize = 192;
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
}

/// Owns one isolated Context, one fixed page of text and a four-slot navigation
/// path. The document/observations are borrowed, never copied into a GUI model.
/// No Context handle escapes: another consumer cannot expand its caches.
pub(crate) struct Preview<'a> {
    document: &'a v2::AdmittedV2Document,
    epoch: PresentationEpoch,
    location: Location,
    context: Option<egui::Context>,
    previous: Option<Page>,
    scroll: egui::Vec2,
    allowance: usize,
    plot: Option<plot_widget::PlotState<'a>>,
}

impl<'a> Preview<'a> {
    pub(crate) fn new(document: &'a v2::AdmittedV2Document, epoch: PresentationEpoch) -> Self {
        Self {
            document,
            epoch,
            location: Location::default(),
            context: None,
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
    ) {
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
    }

    /// Local navigation only. Invalid addresses fail without changing the view.
    pub(crate) fn navigate(&mut self, location: Location) -> bool {
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
        self.context = None;
        self.previous = None;
    }

    /// Release only the library caches, preserving the single current Plot slot.
    pub(crate) fn retire_context(&mut self) {
        self.retire();
    }

    pub(crate) fn close_plot(&mut self) {
        self.retire();
        self.plot = None;
    }

    /// Open only an admitted Plot on the current four-cell page. No projection
    /// or allocation is needed merely to choose this local viewing mode.
    pub(crate) fn open_plot(&mut self, index: usize) -> bool {
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
        self.retire();
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

    /// Consume shapes while the bounded Context is alive. Callers must drop the
    /// returned CPU meshes and FullOutput before the next frame. No GPU host.
    pub(crate) fn tessellate(&self, output: &mut egui::FullOutput) -> Vec<egui::ClippedPrimitive> {
        self.context.as_ref().map_or_else(Vec::new, |context| {
            context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point)
        })
    }

    /// CPU preview at a fixed, bounded logical viewport and default fonts/style.
    /// Consume/drop output before the next frame. A GPU host must retire texture
    /// resources too; this module does not qualify such a host. Changed text
    /// retires font caches before new glyphs/layout are submitted. Exact page
    /// equality (not a content hash) decides reuse; no history persists.
    pub(crate) fn run(
        &mut self,
        observations: &RuntimeObservations,
        mut input: egui::RawInput,
    ) -> Result<(egui::FullOutput, RenderReport), &'static str> {
        if self.allowance < NOTICE_RESERVATION {
            return Err("Read-only preview paused: visual reservation unavailable");
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
        let context = self.context.get_or_insert_with(new_context);
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
        self.navigate(next);
        if let Some(index) = plot {
            self.open_plot(index);
        }
        Ok((output, report))
    }

    fn run_plot(
        &mut self,
        observations: &RuntimeObservations,
        input: egui::RawInput,
    ) -> Result<(egui::FullOutput, RenderReport), &'static str> {
        // One combined reservation, checked before the first projection buffer.
        let view = plot_projection::project(
            self.document,
            observations,
            self.epoch,
            self.location,
            self.plot.as_ref().unwrap().index,
            self.allowance.min(plot_projection::PROJECTION_RESERVATION),
        )
        .map_err(|_| "Read-only Plot unavailable: bounded projection failed")?;
        // Fixed-size text planning precedes font, widget and galley allocation.
        // A changed fragment retires the complete Context before new glyphs.
        let (changed, text_page) = self.plot.as_mut().unwrap().prepare(&view);
        if changed {
            self.retire();
        }
        let restored = self.context.is_none();
        let context = self.context.get_or_insert_with(new_context);
        let state = self.plot.as_mut().unwrap();
        let mut result = (false, RenderReport::default());
        let output = context.run_ui(bounded_input(input, text_page.is_none()), |ui| {
            result = state.paint(ui, &view, text_page.as_ref(), restored);
        });
        if result.0 {
            self.close_plot();
        }
        Ok((output, result.1))
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
    input.events = input
        .events
        .into_iter()
        .take(64)
        .filter_map(|event| match event {
            egui::Event::Zoom(factor)
                if plot && factor.is_finite() && (0.1..=10.0).contains(&factor) =>
            {
                Some(egui::Event::Zoom(factor))
            }
            egui::Event::PointerMoved(pos) if pos.is_finite() => Some(event),
            egui::Event::PointerGone => Some(event),
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                ..
            } if pos.is_finite() => Some(egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }),
            egui::Event::MouseWheel {
                unit, delta, phase, ..
            } if delta.is_finite() => Some(egui::Event::MouseWheel {
                unit,
                delta,
                phase,
                modifiers: egui::Modifiers::NONE,
            }),
            _ => None,
        })
        .collect();
    egui::RawInput {
        screen_rect: input.screen_rect,
        events: input.events,
        time: input.time,
        ..Default::default()
    }
}

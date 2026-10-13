//! One read-only plot and one bounded, owner-held viewport/legend slot.
use super::{OccurrenceIdentity, RenderReport, egui, page, plot_projection as projection};
use crate::presentation::v2;
use egui_plot::{HoverPosition, Legend, LegendGrouping, Line, Plot, PlotBounds, PlotMemory};

/// Persistent state contains no samples, owned strings, history or PlotMemory.
pub(crate) struct PlotState<'a> {
    pub(super) identity: OccurrenceIdentity<'a>,
    pub(super) definition: &'a v2::Plot,
    pub(super) index: usize,
    pub(super) bounds: Option<PlotBounds>,
    pub(super) auto: egui::Vec2b,
    pub(super) hidden: u32,
    // Only changing non-ASCII observation text can grow the font repertoire.
    // Exact bounded equality retires that Context; numeric/diagnostic text is ASCII.
    units: [(projection::Text<32>, projection::Text<16>); projection::MAX_TRACES],
}

// The contract reserves 4096 bytes per Plot viewport; this preview owns one.
const _: () = assert!(std::mem::size_of::<PlotState<'static>>() <= 4096);

impl<'a> PlotState<'a> {
    pub(crate) fn from_definition(
        identity: OccurrenceIdentity<'a>,
        definition: &'a v2::Plot,
        index: usize,
    ) -> Self {
        Self {
            identity,
            definition,
            index,
            bounds: None,
            auto: true.into(),
            hidden: 0,
            units: std::array::from_fn(|_| Default::default()),
        }
    }

    pub(super) fn id(&self) -> egui::Id {
        self.identity.widget_id().with(self.definition.id.as_str())
    }

    /// Full Trace identity, including the definition ordinal and occurrence.
    pub(super) fn trace_id(trace: &projection::TraceView<'_>) -> egui::Id {
        egui::Id::new(("v2-plot-trace", &trace.identity))
    }

    pub(super) fn refresh_units(&mut self, view: &projection::PlotView<'_>) -> bool {
        let mut changed = false;
        for (slot, trace) in self.units.iter_mut().zip(&view.traces) {
            if slot.0 != trace.runtime_unit_id || slot.1 != trace.runtime_unit_symbol {
                *slot = (
                    trace.runtime_unit_id.clone(),
                    trace.runtime_unit_symbol.clone(),
                );
                changed = true;
            }
        }
        changed
    }

    /// Low-level widget. Preview must pass preflight before calling this. Kept
    /// separate so the native probe can reproduce the rejected counterexample.
    pub(crate) fn paint(
        &mut self,
        ui: &mut egui::Ui,
        view: &projection::PlotView<'_>,
        restored: bool,
    ) -> (bool, RenderReport) {
        let mut back = false;
        let mut reset = false;
        ui.horizontal(|ui| {
            back = page::navigate_button(ui, true, "Back");
            reset = page::navigate_button(ui, true, "Reset / auto bounds");
            ui.label("Read-only Plot · monotonic elapsed time · no Runtime commands");
        });
        ui.add(egui::Label::new(view.definition.title.as_str()).selectable(false));
        let id = self.id();
        let mut plot = Plot::new(id.value())
            .id(id)
            .width(1200.0)
            .height(540.0)
            .x_grid_spacer(grid)
            .y_grid_spacer(grid)
            .legend(
                Legend::default()
                    .grouping(LegendGrouping::ById)
                    .follow_insertion_order(true)
                    .hidden_items(
                        view.traces
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| self.hidden & (1 << i) != 0)
                            .map(|(_, trace)| Self::trace_id(trace)),
                    ),
            )
            .x_axis_label("Monotonic elapsed time")
            .y_axis_label("Runtime value (no unit conversion)")
            .x_axis_formatter(|mark, _| {
                projection::elapsed_time_label(mark.value)
                    .map_or_else(String::new, |text| text.as_str().to_owned())
            })
            // Scientific formatting has bounded output even at f64 extrema.
            .y_axis_formatter(|mark, _| format!("{:.3e}", mark.value))
            .label_formatter(|position| match position {
                HoverPosition::NearDataPoint {
                    plot_name,
                    position,
                    ..
                } => view
                    .traces
                    .iter()
                    .find(|trace| trace.legend.as_str() == *plot_name)
                    .and_then(|trace| projection::hover(trace, *position))
                    .map(|text| text.as_str().to_owned()),
                HoverPosition::Elsewhere { .. } => None,
            });
        if let Some(min) = view.definition.axes.y_min {
            plot = plot.include_y(min.0);
        }
        if let Some(max) = view.definition.axes.y_max {
            plot = plot.include_y(max.0);
        }
        plot.show(ui, |plot_ui| {
            // Restore only on Context creation. Applying this every frame would
            // override egui_plot's double-click reset before its interaction pass.
            if restored {
                if let Some(bounds) = self.bounds {
                    plot_ui.set_plot_bounds(bounds);
                }
                plot_ui.set_auto_bounds(self.auto);
            }
            if reset {
                plot_ui.set_auto_bounds(true);
            }
            for (index, trace) in view.traces.iter().enumerate() {
                if !trace.definition.visible {
                    continue;
                }
                // Legend::hidden_items restores checkboxes, but this version
                // filters curves using PREVIOUS PlotMemory. Empty borrowed data
                // prevents a one-frame flash when restoring a hidden curve.
                let points = if self.hidden & (1 << index) == 0 {
                    view.points(index)
                } else {
                    &[]
                };
                // add(), unlike line(), preserves empty legend entries. There
                // is no fabricated point, even for unavailable/hidden curves.
                plot_ui.add(
                    Line::new(trace.legend.as_str(), points)
                        .id(Self::trace_id(trace))
                        .color(color(trace.definition.style.color.as_str()))
                        .width(trace.definition.style.width.0 as f32),
                );
            }
        });
        if let Some(memory) = PlotMemory::load(ui.ctx(), id) {
            let bounds = *memory.bounds();
            if bounds.is_valid() && bounds.width().is_finite() && bounds.height().is_finite() {
                self.bounds = Some(bounds);
                self.auto = memory.auto_bounds;
            } else {
                self.bounds = None;
                self.auto = true.into();
            }
            self.hidden = view.traces.iter().enumerate().fold(0, |mask, (i, trace)| {
                mask | (u32::from(memory.hidden_items.contains(&Self::trace_id(trace))) << i)
            });
        }
        // At most 32 fixed-format diagnostic rows; scrolling adds no occurrences.
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            if view.traces.is_empty() {
                ui.label("Empty Plot: no traces");
            }
            for (index, trace) in view.traces.iter().enumerate() {
                ui.add(egui::Label::new(format!(
                    "Trace {}: {:?}; {:?}; quality {:?}; {} points; decimated {}; buffer evictions {}{}",
                    index + 1, trace.state, trace.freshness, trace.quality,
                    view.points(index).len(), trace.decimated_points, trace.buffer_dropped,
                    if trace.definition.visible { "" } else { "; definition hidden" },
                )).selectable(false));
            }
        });
        (
            back,
            RenderReport {
                occurrences: 1,
                plot_points: view.prepared_points(),
                plot_traces: view.traces.len(),
                labels: 128,
                widgets: 256,
                text_bytes: 16 * 1024,
                ..Default::default()
            },
        )
    }
}

/// This is a preview qualification guard, NOT a document/admission limit. The
/// full legal Unicode legend counterexample exceeds 8 MiB with tessellation.
/// Include every possible hover unit, even before it is visited, so glyphs
/// cannot accumulate across changing hover selections beyond this repertoire.
pub(crate) const MAX_PLOT_TEXT: usize = 1024;
pub(super) fn preflight(view: &projection::PlotView<'_>) -> bool {
    // The built-in overlay legend has no scrolling API. Reject, rather than
    // truncate, labels that can push controls outside this bounded viewport.
    if view.definition.title.as_str().contains(['\n', '\r']) {
        return false;
    }
    let mut bytes = view.definition.title.as_str().len();
    for trace in &view.traces {
        if trace.legend.as_str().len() > 80 || trace.legend.as_str().contains(['\n', '\r']) {
            return false;
        }
        for count in [
            trace.legend.as_str().len(),
            trace.runtime_unit_id.as_str().len(),
            trace.runtime_unit_symbol.as_str().len(),
            trace
                .definition
                .display_unit
                .as_ref()
                .map_or(0, |u| u.as_str().len()),
        ] {
            let Some(total) = bytes.checked_add(count) else {
                return false;
            };
            bytes = total;
        }
    }
    bytes <= MAX_PLOT_TEXT
}

/// Fixed work even for extreme finite ranges; no range-sized tick allocation.
fn grid(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    const MARKS: usize = 12;
    let step = (input.bounds.1 - input.bounds.0) / MARKS as f64;
    if !step.is_finite() || step <= 0.0 {
        return Vec::new();
    }
    (0..MARKS)
        .filter_map(|i| {
            let value = input.bounds.0 + i as f64 * step;
            value.is_finite().then_some(egui_plot::GridMark {
                value,
                step_size: step,
            })
        })
        .collect()
}

fn color(value: &str) -> egui::Color32 {
    match value {
        "cyan" => egui::Color32::from_rgb(0, 200, 220),
        "red" => egui::Color32::LIGHT_RED,
        "green" => egui::Color32::LIGHT_GREEN,
        _ => egui::Color32::from_hex(value).unwrap_or(egui::Color32::LIGHT_BLUE),
    }
}

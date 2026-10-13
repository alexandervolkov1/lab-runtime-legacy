//! One read-only plot and one bounded, owner-held viewport/legend slot.
use super::{
    OccurrenceIdentity, RenderReport, egui,
    finite_trace::{FiniteTraceItem, TraceGeometry},
    page,
    plot_bounds::{self, GeometryUnavailable, Preflight},
    plot_projection as projection,
    plot_text::{self, Cursor, Field, PageKey, TRACES_PER_PAGE, TextPage},
};
use crate::presentation::v2;
use egui_plot::{HoverPosition, Legend, LegendGrouping, Plot, PlotBounds, PlotMemory};
use std::cell::Cell;

/// Persistent state contains no samples, owned strings, history or PlotMemory.
pub(crate) struct PlotState<'a> {
    pub(super) identity: OccurrenceIdentity<'a>,
    pub(super) definition: &'a v2::Plot,
    pub(super) index: usize,
    pub(super) bounds: Option<PlotBounds>,
    pub(super) auto: egui::Vec2b,
    pub(super) hidden: u32,
    pub(super) details: Option<Cursor>,
    trace_page: usize,
    painted: Option<Option<PageKey>>,
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
            details: None,
            trace_page: 0,
            painted: None,
        }
    }

    pub(super) fn id(&self) -> egui::Id {
        self.identity.widget_id().with(self.definition.id.as_str())
    }

    /// Full Trace identity, including the definition ordinal and occurrence.
    pub(super) fn trace_id(trace: &projection::TraceView<'_>) -> egui::Id {
        egui::Id::new(("v2-plot-trace", &trace.identity))
    }

    pub(super) fn prepare(&mut self, view: &projection::PlotView<'_>) -> (bool, Option<TextPage>) {
        let page = self.details.map(|cursor| TextPage::new(view, cursor));
        if let Some(page) = &page {
            self.details = Some(page.cursor);
        }
        let key = page.as_ref().map(|page| page.key);
        let changed = self.painted != Some(key);
        self.painted = Some(key);
        (changed, page)
    }

    /// Paint exactly the preflighted graph or text page, never both together.
    pub(super) fn paint(
        &mut self,
        ui: &mut egui::Ui,
        view: &projection::PlotView<'_>,
        text_page: Option<&TextPage>,
    ) -> (bool, RenderReport) {
        let mut back = false;
        let mut reset = false;
        ui.horizontal(|ui| {
            back = page::navigate_button(ui, true, "Back");
            reset = page::navigate_button(ui, true, "Reset / auto bounds");
            ui.label("Read-only Plot - monotonic elapsed time - no Runtime commands");
        });
        if reset {
            self.auto = true.into();
        }
        if let Some(text_page) = text_page {
            self.paint_details(ui, text_page);
            return (
                back,
                RenderReport {
                    occurrences: 1,
                    labels: 24,
                    widgets: 48,
                    text_bytes: 1024,
                    ..Default::default()
                },
            );
        }
        if page::navigate_button(ui, true, "Title details") {
            self.details = Some(Cursor {
                trace: 0,
                field: Field::Title,
                page: 0,
            });
        }
        let id = self.id();
        let preflight = Preflight::new(view, self.hidden);
        let mut viewport_fault = preflight.as_ref().err().copied();
        let auto_bounds = preflight
            .as_ref()
            .map_or(PlotBounds::new_symmetrical(1.0), |p| p.bounds);
        if self.bounds.is_some_and(|b| !plot_bounds::valid(b)) {
            self.bounds = None;
            self.auto = true.into();
            viewport_fault = Some(GeometryUnavailable::Bounds);
        }
        let status: [Cell<TraceGeometry>; 32] = std::array::from_fn(|i| {
            Cell::new(TraceGeometry {
                fault: preflight.as_ref().ok().and_then(|p| p.faults[i]),
                segments: 0,
            })
        });
        let plot = Plot::new(id.value())
            .id(id)
            // All viewport mutation is checked below, BEFORE axes and items.
            // The library's unchecked post-closure transforms are disabled.
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .allow_axis_zoom_drag(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            .sense(egui::Sense::click_and_drag())
            .default_x_bounds(auto_bounds.min()[0], auto_bounds.max()[0])
            .default_y_bounds(auto_bounds.min()[1], auto_bounds.max()[1])
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
                } => plot_text::hover(view, plot_name, *position)
                    .map(|text| text.as_str().to_owned()),
                HoverPosition::Elsewhere { .. } => None,
            });
        if preflight.is_ok() {
            plot.show(ui, |plot_ui| {
                if reset || plot_ui.response().double_clicked() {
                    self.auto = true.into();
                }
                let mut bounds = if self.auto.all() {
                    auto_bounds
                } else {
                    self.bounds.unwrap_or(auto_bounds)
                };
                match plot_bounds::interact(bounds, plot_ui.response(), plot_ui.ctx()) {
                    Ok(Some(candidate)) => {
                        bounds = candidate;
                        self.auto = false.into();
                    }
                    Ok(None) => {}
                    Err(reason) => {
                        viewport_fault = Some(reason);
                    }
                }
                plot_ui.set_plot_bounds(bounds);
                plot_ui.set_auto_bounds(self.auto);
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
                    plot_ui.add(FiniteTraceItem::new(
                        trace,
                        points,
                        egui::Stroke::new(
                            trace.definition.style.width.0 as f32,
                            color(trace.definition.style.color.as_str()),
                        ),
                        &status[index],
                    ));
                }
            });
            if let Some(memory) = PlotMemory::load(ui.ctx(), id) {
                let bounds = *memory.bounds();
                if plot_bounds::valid(bounds) {
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
        }
        if let Some(reason) = viewport_fault {
            ui.label(format!(
                "GeometryUnavailable: {reason:?}; unsafe viewport request was not applied"
            ));
        }
        let allocation_faults = status
            .iter()
            .filter(|s| s.get().fault == Some(GeometryUnavailable::Allocation))
            .count();
        if allocation_faults != 0 {
            // Visible even when the affected trace's detail selector is on a
            // different page. No failed trace is reported as a complete curve.
            ui.label(format!(
                "GeometryUnavailable: Allocation ({allocation_faults} traces)"
            ));
        }
        // Only four ASCII detail selectors/diagnostics at once. This pages text,
        // NOT curve data: all 32 B1 traces remain submitted to the plot above.
        ui.horizontal(|ui| {
            if page::navigate_button(ui, self.trace_page > 0, "Previous traces") {
                self.trace_page -= 1;
            }
            if page::navigate_button(
                ui,
                (self.trace_page + 1) * TRACES_PER_PAGE < view.traces.len(),
                "Next traces",
            ) {
                self.trace_page += 1;
            }
        });
        ui.vertical(|ui| {
            if view.traces.is_empty() {
                ui.label("Empty Plot: no traces");
            }
            for (index, trace) in view.traces.iter().enumerate().skip(self.trace_page * TRACES_PER_PAGE).take(TRACES_PER_PAGE) {
                if page::navigate_button(ui, true, &format!("Details {}", plot_text::alias(index))) {
                    self.details = Some(Cursor { trace: index, field: Field::Label, page: 0 });
                }
                ui.add(egui::Label::new(format!(
                    "Trace {}: {:?}; {:?}; quality {:?}; {} points; decimated {}; buffer evictions {}{}",
                    index + 1, trace.state, trace.freshness, trace.quality,
                    view.points(index).len(), trace.decimated_points, trace.buffer_dropped,
                    if trace.definition.visible { "" } else { "; definition hidden" },
                )).selectable(false));
                if let Some(reason) = status[index].get().fault {
                    ui.label(format!("GeometryUnavailable {}: {reason:?}", plot_text::alias(index)));
                }
            }
        });
        (
            back,
            RenderReport {
                occurrences: 1,
                plot_points: view.prepared_points(),
                plot_traces: view.traces.len(),
                allocation_faults,
                geometry_segments: status.iter().map(|s| s.get().segments).sum(),
                geometry_unavailable: status.iter().filter(|s| s.get().fault.is_some()).count()
                    + usize::from(viewport_fault.is_some()),
                geometry_failures: status
                    .iter()
                    .filter(|s| {
                        matches!(
                            s.get().fault,
                            Some(GeometryUnavailable::Allocation | GeometryUnavailable::Transform)
                        )
                    })
                    .count(),
                labels: 112,
                widgets: 224,
                text_bytes: 8 * 1024,
                ..Default::default()
            },
        )
    }

    fn paint_details(&mut self, ui: &mut egui::Ui, text: &TextPage) {
        if page::navigate_button(ui, true, "Back to Plot") {
            self.details = None;
        }
        if text.cursor.field != Field::Title {
            ui.label(format!("Details {}", plot_text::alias(text.cursor.trace)));
            ui.horizontal(|ui| {
                for field in Field::TRACE_FIELDS {
                    if page::navigate_button(ui, true, field.caption()) {
                        self.details = Some(Cursor {
                            field,
                            page: 0,
                            ..text.cursor
                        });
                    }
                }
            });
        }
        ui.label(text.cursor.field.caption());
        ui.label(format!(
            "Fragment {} / {}; bytes {}..{} of {}",
            text.cursor.page + 1,
            text.pages,
            text.start,
            text.end,
            text.total
        ));
        ui.horizontal(|ui| {
            if page::navigate_button(ui, text.cursor.page > 0, "Previous fragment") {
                self.details = Some(Cursor {
                    page: text.cursor.page - 1,
                    ..text.cursor
                });
            }
            if page::navigate_button(ui, text.cursor.page + 1 < text.pages, "Next fragment") {
                self.details = Some(Cursor {
                    page: text.cursor.page + 1,
                    ..text.cursor
                });
            }
        });
        ui.label("Text fragment (invisible characters escaped):");
        ui.add(
            egui::Label::new(text.display.as_str())
                .wrap()
                .selectable(false),
        );
        ui.label(format!("UTF-8: {}", text.hex.as_str()));
        if text.absent {
            ui.label("Not available from validated metadata / definition");
        } else if text.total == 0 {
            ui.label("Empty string");
        }
        if text.cursor.field == Field::DisplayUnit {
            ui.label("Cosmetic label only; no conversion");
        }
    }
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

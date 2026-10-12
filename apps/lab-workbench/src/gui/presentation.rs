//! Read-only declarative layout over the committed document and observation cache.

use crate::{
    model::{Freshness, RuntimeObservations},
    presentation::{PanelKind, Plot, PresentationDocument, RuntimeRef, Trace},
};
use eframe::egui::{self, Color32};
use egui_plot::{HoverPosition, Line, Plot as EguiPlot, PlotPoint, PlotPoints};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

/// Ephemeral navigation and plot IDs; the dispatcher retains the sole document owner.
#[derive(Default)]
pub(super) struct PresentationRenderer {
    document_id: String,
    selected_tabs: BTreeMap<String, String>,
    revision: Option<u64>,
    // At most one ID per current plot panel (64), or per legacy plot (32).
    plot_ids: HashSet<egui::Id>,
}

impl PresentationRenderer {
    /// Renders logical windows, or the legacy plot list when no windows exist.
    /// Returns whether declarative windows replace the legacy signal graphs.
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        document: &PresentationDocument,
        observations: &RuntimeObservations,
        revision: u64,
    ) -> bool {
        if self.revision != Some(revision) || self.document_id != document.document_id {
            // egui_plot retains temporary zoom/legend state by ID. Retire old
            // revisions explicitly so repeated replacements do not retain history.
            ui.ctx().data_mut(|data| {
                for id in &self.plot_ids {
                    data.remove::<egui_plot::PlotMemory>(*id);
                }
            });
            self.plot_ids.clear();
            self.revision = Some(revision);
        }
        self.reconcile(document);
        if document.windows.is_empty() {
            for plot in &document.plots {
                let response = ui.push_id(
                    (
                        &document.document_id,
                        revision,
                        "presentation-plot",
                        &plot.id,
                    ),
                    |ui| render_plot(ui, plot, observations),
                );
                self.plot_ids.insert(response.inner);
            }
            return false;
        }

        // Revision scopes egui's zoom/legend memory so replaced plot definitions
        // cannot inherit a previous configuration's hidden lines or manual bounds.
        ui.push_id((&document.document_id, revision), |ui| {
            for window in &document.windows {
                ui.push_id(&window.id, |ui| {
                    ui.group(|ui| {
                        ui.heading(&window.title);
                        if window.tabs.is_empty() {
                            ui.label("No tabs configured.");
                            return;
                        }
                        let selected = self.selected_tabs.entry(window.id.clone()).or_default();
                        ui.horizontal_wrapped(|ui| {
                            for tab in &window.tabs {
                                ui.push_id(&tab.id, |ui| {
                                    if ui
                                        .selectable_label(*selected == tab.id, &tab.title)
                                        .clicked()
                                    {
                                        selected.clone_from(&tab.id);
                                    }
                                });
                            }
                        });
                        if let Some(tab) = window.tabs.iter().find(|tab| tab.id == *selected) {
                            if tab.panels.is_empty() {
                                ui.label("No panels configured.");
                            }
                            for panel in &tab.panels {
                                ui.push_id(&panel.id, |ui| {
                                    ui.group(|ui| {
                                        ui.strong(&panel.title);
                                        match &panel.kind {
                                            PanelKind::Status { source } => {
                                                render_status(ui, source, observations);
                                            }
                                            PanelKind::Plot { plot_id } => {
                                                if let Some(plot) = document
                                                    .plots
                                                    .iter()
                                                    .find(|plot| &plot.id == plot_id)
                                                {
                                                    self.plot_ids.insert(render_plot(
                                                        ui,
                                                        plot,
                                                        observations,
                                                    ));
                                                }
                                            }
                                            PanelKind::Controls { .. } => {
                                                ui.label(
                                                    "Controls are not available in this view.",
                                                );
                                            }
                                        }
                                    });
                                });
                            }
                        }
                    });
                });
            }
        });
        true
    }

    fn reconcile(&mut self, document: &PresentationDocument) {
        if self.document_id != document.document_id {
            self.selected_tabs.clear();
            self.document_id.clone_from(&document.document_id);
        }
        // No retained layout history: navigation is bounded by current windows.
        self.selected_tabs.retain(|id, selected| {
            document
                .windows
                .iter()
                .any(|window| &window.id == id && window.tabs.iter().any(|tab| &tab.id == selected))
        });
        for window in &document.windows {
            if let Some(tab) = window.tabs.first() {
                self.selected_tabs
                    .entry(window.id.clone())
                    .or_insert_with(|| tab.id.clone());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceState {
    Fresh,
    Stale,
    Unknown,
    Rebuilding,
    Unresolved,
}

fn source_state(observations: &RuntimeObservations, source: &RuntimeRef) -> SourceState {
    let Some(observation) = observations.entities.get(source) else {
        return SourceState::Unresolved;
    };
    // A fresh entity update alone does not complete the authoritative rebuild.
    let freshness = if observation.freshness == Freshness::Fresh {
        observations.freshness
    } else {
        observation.freshness
    };
    match freshness {
        Freshness::Fresh => SourceState::Fresh,
        Freshness::Stale => SourceState::Stale,
        Freshness::Unknown => SourceState::Unknown,
        Freshness::Rebuilding => SourceState::Rebuilding,
    }
}

fn state_label(ui: &mut egui::Ui, state: SourceState) {
    let (text, color) = match state {
        SourceState::Fresh => ("Fresh", Color32::LIGHT_GREEN),
        SourceState::Stale => ("Stale — cached observation", Color32::YELLOW),
        SourceState::Unknown => ("Unknown — freshness not established", Color32::GRAY),
        SourceState::Rebuilding => (
            "Rebuilding — freshness not established",
            Color32::LIGHT_BLUE,
        ),
        SourceState::Unresolved => ("Unresolved — no Runtime observation", Color32::LIGHT_RED),
    };
    ui.colored_label(color, text);
}

fn render_status(ui: &mut egui::Ui, source: &RuntimeRef, observations: &RuntimeObservations) {
    ui.label(super::app::runtime_ref_label(source));
    state_label(ui, source_state(observations, source));
    if let Some(observation) = observations.entities.get(source) {
        ui.monospace(super::app::bounded_value_text(&observation.value));
    }
}

fn trace_points(observations: &RuntimeObservations, trace: &Trace, seconds: f64) -> Vec<PlotPoint> {
    let Some(buffer) = observations.live.get(&trace.source) else {
        return Vec::new();
    };
    let newest = buffer
        .points()
        .back()
        .map_or(0.0, |point| point.time_seconds);
    buffer
        .points()
        .iter()
        .filter(|point| point.time_seconds >= newest - seconds)
        .map(|point| PlotPoint::new(point.time_seconds, point.value))
        .collect()
}

/// Frame-local curve data shared by configured plots and the selected-signal view.
struct ObservedCurve<'a> {
    id: egui::Id,
    source: &'a RuntimeRef,
    label: &'a str,
    display_unit: Option<&'a str>,
    points: Vec<PlotPoint>,
    color: Option<Color32>,
    width: f32,
}

fn nearest_sample(points: &[PlotPoint], time: f64) -> Option<&PlotPoint> {
    if !time.is_finite() {
        return None;
    }
    points.iter().min_by(|left, right| {
        (left.x - time)
            .abs()
            .total_cmp(&(right.x - time).abs())
            // Equal distances choose the earlier actual timestamp, not a midpoint.
            .then_with(|| left.x.total_cmp(&right.x))
    })
}

fn elapsed_time_label(seconds: f64, fraction_digits: u32) -> Option<String> {
    // observed_at_ns is Runtime's monotonic duration since its process clock
    // origin. Workbench has no trustworthy calendar anchor for live points.
    let duration = Duration::try_from_secs_f64(seconds).ok()?;
    let divisor = 10_u128.pow(9 - fraction_digits);
    let ticks = (duration.as_nanos() + divisor / 2) / divisor;
    let ticks_per_second = 10_u128.pow(fraction_digits);
    let hours = ticks / (3_600 * ticks_per_second);
    let minutes = (ticks / (60 * ticks_per_second)) % 60;
    let seconds = (ticks / ticks_per_second) % 60;
    Some(format!(
        "T+{hours:02}:{minutes:02}:{seconds:02}.{:0width$}",
        ticks % ticks_per_second,
        width = fraction_digits as usize,
    ))
}

fn sample_value_label(
    value: f64,
    curve: &ObservedCurve<'_>,
    observations: &RuntimeObservations,
) -> String {
    let unit = observations
        .entities
        .get(curve.source)
        .and_then(|observation| {
            observation
                .value
                .pointer("/unit/symbol")
                .and_then(serde_json::Value::as_str)
        })
        .filter(|unit| !unit.is_empty());
    // Live buffers retain the raw number. Prefer Runtime's unit over a possibly
    // different presentation label; this renderer performs no unit conversion.
    if let Some(unit) = unit {
        format!("{value} {unit}")
    } else if let Some(unit) = curve.display_unit.filter(|unit| !unit.is_empty()) {
        format!("{value} {unit} (display label; no conversion)")
    } else {
        format!("{value} (unit unknown)")
    }
}

fn sample_tooltip(
    curves: &[ObservedCurve<'_>],
    observations: &RuntimeObservations,
    time: f64,
    is_hidden: impl Fn(egui::Id) -> bool,
) -> Option<String> {
    let samples = curves
        .iter()
        .filter(|curve| !is_hidden(curve.id))
        .filter_map(|curve| nearest_sample(&curve.points, time).map(|point| (curve, point)))
        .collect::<Vec<_>>();
    // Milliseconds are normally enough. Keep asynchronous sub-millisecond
    // samples distinguishable rather than presenting rounded times as equal.
    let mut fraction_digits = 3;
    for (_, left) in &samples {
        for (_, right) in &samples {
            let difference = (left.x - right.x).abs();
            if difference > 0.0 && difference < 0.000_001 {
                fraction_digits = 9;
            } else if difference > 0.0 && difference < 0.001 {
                fraction_digits = fraction_digits.max(6);
            }
        }
    }
    let mut rows = Vec::new();
    for (curve, point) in samples {
        let Some(timestamp) = elapsed_time_label(point.x, fraction_digits) else {
            continue;
        };
        let state = match source_state(observations, curve.source) {
            SourceState::Fresh => "Fresh view",
            SourceState::Stale => "Stale — cached observation",
            SourceState::Unknown => "Unknown — freshness not established",
            SourceState::Rebuilding => "Rebuilding — freshness not established",
            SourceState::Unresolved => "Unresolved — no Runtime observation",
        };
        // Each curve keeps its own sample time, even when others have nearby Xs.
        rows.push(format!(
            "{}: {} · {timestamp} · {state}",
            curve.label,
            sample_value_label(point.y, curve, observations)
        ));
    }
    (!rows.is_empty()).then(|| format!("Nearest observed samples\n{}", rows.join("\n")))
}

fn show_observed_plot<'a>(
    ui: &mut egui::Ui,
    view: EguiPlot<'a>,
    id: egui::Id,
    curves: &'a [ObservedCurve<'a>],
    observations: &'a RuntimeObservations,
) {
    let memory = egui_plot::PlotMemory::load(ui.ctx(), id);
    view.label_formatter(move |position| {
        let time = match position {
            HoverPosition::Elsewhere { position }
            | HoverPosition::NearDataPoint { position, .. } => position.x,
        };
        sample_tooltip(curves, observations, time, |curve_id| {
            memory
                .as_ref()
                .is_some_and(|memory| memory.hidden_items.contains(&curve_id))
        })
    })
    .show(ui, |plot_ui| {
        for curve in curves.iter().filter(|curve| !curve.points.is_empty()) {
            let mut line = Line::new(curve.label, PlotPoints::Borrowed(&curve.points))
                .id(curve.id)
                .width(curve.width)
                // Use the cursor X for every curve, rather than snapping all
                // readouts to one line's nearest endpoint. The plot still owns
                // crosshairs, tooltip placement, pan/zoom and legend interaction.
                .allow_hover(false);
            if let Some(color) = curve.color {
                line = line.color(color);
            }
            plot_ui.line(line);
        }
    });
}

/// The ordinary selected-signal graph uses the same read-only sample tooltip.
pub(super) fn render_signal_plot(
    ui: &mut egui::Ui,
    source: &RuntimeRef,
    observations: &RuntimeObservations,
) {
    let label = super::app::runtime_ref_label(source);
    let curves = [ObservedCurve {
        id: egui::Id::new("observed"),
        source,
        label: &label,
        display_unit: None,
        points: observations
            .live
            .get(source)
            .map_or_else(Vec::new, |buffer| {
                buffer
                    .points()
                    .iter()
                    .map(|point| PlotPoint::new(point.time_seconds, point.value))
                    .collect()
            }),
        color: None,
        width: 1.5,
    }];
    let id = ui.make_persistent_id("selected-live-signal");
    show_observed_plot(
        ui,
        EguiPlot::new("selected-live-signal").id(id).height(360.0),
        id,
        &curves,
        observations,
    );
}

/// Shared egui_plot path for the legacy list and declarative plot panels.
fn render_plot(ui: &mut egui::Ui, plot: &Plot, observations: &RuntimeObservations) -> egui::Id {
    ui.strong(&plot.title);
    let id = ui.make_persistent_id(("plot", &plot.id));
    let mut view = EguiPlot::new(&plot.id)
        .id(id)
        .height(160.0)
        .legend(Default::default());
    if let Some(minimum) = plot.axes.y_min {
        view = view.include_y(minimum);
    }
    if let Some(maximum) = plot.axes.y_max {
        view = view.include_y(maximum);
    }
    let curves = plot
        .traces
        .iter()
        .filter(|trace| trace.visible)
        .map(|trace| {
            let color = match trace.style.color.as_str() {
                "cyan" => Color32::from_rgb(0, 200, 220),
                "red" => Color32::LIGHT_RED,
                "green" => Color32::LIGHT_GREEN,
                _ => Color32::from_hex(&trace.style.color).unwrap_or(Color32::LIGHT_BLUE),
            };
            ObservedCurve {
                id: egui::Id::new(&trace.id),
                source: &trace.source,
                label: &trace.display_label,
                display_unit: trace.display_unit.as_deref(),
                points: trace_points(observations, trace, plot.time_window_seconds),
                color: Some(color),
                width: trace.style.width as f32,
            }
        })
        .collect::<Vec<_>>();
    show_observed_plot(ui, view, id, &curves, observations);
    for trace in plot.traces.iter().filter(|trace| trace.visible) {
        ui.horizontal_wrapped(|ui| {
            ui.label(&trace.display_label);
            if let Some(unit) = &trace.display_unit {
                ui.label(unit);
            }
            state_label(ui, source_state(observations, &trace.source));
        });
        match observations.live.get(&trace.source) {
            None => {
                ui.label("Waiting for good finite signal events.");
            }
            Some(buffer) if buffer.dropped() > 0 => {
                ui.colored_label(
                    Color32::YELLOW,
                    format!("Display window truncated {} old point(s)", buffer.dropped()),
                );
            }
            _ => {}
        }
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::types::{CommandSendError, MutationIdentity},
        dispatcher::{CallOrigin, CallerId, LabClient, WorkbenchDispatcher, WorkbenchRequest},
        model::{LivePoint, WorkbenchModel},
    };
    use serde_json::{Value, json};
    use std::cell::Cell;

    fn document() -> PresentationDocument {
        serde_json::from_value(json!({
            "format_version":1,"document_id":"dashboard",
            "windows":[{"id":"window","title":"Temperatures","tabs":[
                {"id":"live","title":"Live tab","panels":[
                    {"id":"status","title":"Sensor status","kind":{"kind":"status",
                        "source":{"kind":"signal","instrument":"1","parameter":"1"}}},
                    {"id":"plot-panel","title":"Temperature panel","kind":{"kind":"plot","plot_id":"temperatures"}}
                ]},
                {"id":"other","title":"Other tab","panels":[
                    {"id":"controls","title":"Inactive controls","kind":{"kind":"controls","control_ids":[]}}
                ]}
            ]}],
            "plots":[{"id":"temperatures","title":"Two temperature curves","time_window_seconds":30.0,
                "axes":{"y_min":null,"y_max":null},"traces":(1..=2).map(|id| json!({
                    "id":format!("trace-{id}"),"source":{"kind":"signal","instrument":"1","parameter":id.to_string()},
                    "display_label":format!("Temperature {id}"),"visible":true,
                    "style":{"color":if id == 1 { "cyan" } else { "red" },"width":1.5},"display_unit":"K"
                })).collect::<Vec<_>>()
            }],"controls":[]
        })).unwrap()
    }

    fn model() -> WorkbenchModel {
        let mut model = WorkbenchModel::new(document());
        model.presentation.validate().unwrap();
        for trace in model.presentation.plots[0].traces.clone() {
            let offset = if trace.id == "trace-1" { 0.0 } else { 10.0 };
            model.observations.observe(
                trace.source.clone(),
                json!({"value":293.0 + offset,"unit":{"id":"kelvin","symbol":"K"}}),
                None,
            );
            for (time_seconds, value) in [(0.0, 270.0), (40.0, 292.0), (60.0, 293.0)] {
                model
                    .push_live_point(
                        trace.source.clone(),
                        LivePoint {
                            time_seconds,
                            value: value + offset,
                        },
                    )
                    .unwrap();
            }
        }
        model.observations.complete_rebuild();
        model.refresh_unresolved();
        model
    }

    fn frame(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        revision: u64,
        events: Vec<egui::Event>,
    ) -> (bool, Vec<(String, egui::Rect)>) {
        let mut declared = false;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 1600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                declared = renderer.show(ui, &model.presentation, &model.observations, revision);
            },
        );
        (declared, painted_text(output))
    }

    fn painted_text(output: egui::FullOutput) -> Vec<(String, egui::Rect)> {
        fn collect(shape: &egui::epaint::Shape, text: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(value) => {
                    text.push((value.galley.text().to_owned(), value.visual_bounding_rect()));
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, text);
                    }
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, &mut text);
        }
        output.drop_without_applying_deltas();
        text
    }

    fn labels(text: &[(String, egui::Rect)]) -> Vec<&str> {
        text.iter().map(|(label, _)| label.as_str()).collect()
    }

    fn hover(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        revision: u64,
        time: f64,
    ) -> Vec<(String, egui::Rect)> {
        frame(context, renderer, model, revision, vec![]);
        frame(context, renderer, model, revision, vec![]);
        let memory =
            egui_plot::PlotMemory::load(context, *renderer.plot_ids.iter().next().unwrap())
                .unwrap();
        let transform = memory.transform();
        let position = egui::pos2(
            transform.position_from_point_x(time),
            transform.frame().center().y,
        );
        frame(
            context,
            renderer,
            model,
            revision,
            vec![egui::Event::PointerMoved(position)],
        );
        frame(context, renderer, model, revision, vec![]).1
    }

    fn tooltip(text: &[(String, egui::Rect)]) -> Option<&str> {
        text.iter()
            .find(|(text, _)| text.starts_with("Nearest observed samples\n"))
            .map(|(text, _)| text.as_str())
    }

    #[test]
    fn elapsed_sample_time_is_relative_rounded_and_never_a_calendar_timestamp() {
        for (seconds, expected) in [
            (0.0, "T+00:00:00.000"),
            (1.2345, "T+00:00:01.235"),
            (59.9996, "T+00:01:00.000"),
            (3599.9996, "T+01:00:00.000"),
            (86400.001, "T+24:00:00.001"),
        ] {
            assert_eq!(elapsed_time_label(seconds, 3).as_deref(), Some(expected));
        }
        assert_eq!(
            elapsed_time_label(1.0001, 6).as_deref(),
            Some("T+00:00:01.000100")
        );
        for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(elapsed_time_label(invalid, 3).is_none());
        }
    }

    #[test]
    fn nearest_sample_uses_actual_points_and_earlier_ties_without_interpolation() {
        let points = [
            PlotPoint::new(4.0, 100.0),
            PlotPoint::new(1.0, 10.0),
            PlotPoint::new(2.0, 30.0),
        ];
        for (time, expected) in [
            (-100.0, points[1]),
            (1.5, points[1]),
            (1.9, points[2]),
            (3.0, points[2]),
            (100.0, points[0]),
        ] {
            assert_eq!(nearest_sample(&points, time), Some(&expected));
        }
        assert!(nearest_sample(&[], 1.0).is_none());
        assert!(nearest_sample(&points, f64::NAN).is_none());
    }

    #[test]
    fn hover_shows_one_tooltip_with_real_values_and_each_curves_own_time() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = model();
        for (index, time) in [(0, 40.125), (1, 40.375)] {
            let source = model.presentation.plots[0].traces[index].source.clone();
            model.observations.live.remove(&source);
            for time_seconds in [time, 60.0] {
                model
                    .push_live_point(
                        source.clone(),
                        LivePoint {
                            time_seconds,
                            value: 292.0 + index as f64 * 10.0,
                        },
                    )
                    .unwrap();
            }
        }
        let source = model.presentation.plots[0].traces[0].source.clone();
        model.observations.entities.get_mut(&source).unwrap().value["unit"] =
            json!({"id":"degC","symbol":"°C"});
        let before = model.clone();
        let text = hover(&context, &mut renderer, &model, 1, 40.25);
        let label = tooltip(&text).unwrap();
        assert_eq!(
            text.iter()
                .filter(|(text, _)| text.starts_with("Nearest observed samples"))
                .count(),
            1
        );
        assert!(label.contains("Temperature 1: 292 °C · T+00:00:40.125 · Fresh view"));
        assert!(label.contains("Temperature 2: 302 K · T+00:00:40.375 · Fresh view"));
        assert!(!label.contains("292 K"));
        assert_eq!(model, before);
    }

    #[test]
    fn tooltip_units_fallback_is_explicit_and_close_sample_times_stay_distinct() {
        let mut model = model();
        model.observations.entities.clear();
        let traces = &model.presentation.plots[0].traces;
        let mut curves = traces
            .iter()
            .enumerate()
            .map(|(index, trace)| ObservedCurve {
                id: egui::Id::new(&trace.id),
                source: &trace.source,
                label: &trace.display_label,
                display_unit: if index == 0 {
                    trace.display_unit.as_deref()
                } else {
                    None
                },
                points: vec![PlotPoint::new(1.0001 + index as f64 * 0.0001, 0.00000025)],
                color: None,
                width: 1.5,
            })
            .collect::<Vec<_>>();
        let label = sample_tooltip(&curves, &model.observations, 1.0, |_| false).unwrap();
        assert!(label.contains("0.00000025 K (display label; no conversion) · T+00:00:01.000100"));
        assert!(label.contains("0.00000025 (unit unknown) · T+00:00:01.000200"));
        assert!(!label.contains("Fresh view"));
        curves[0].points = vec![PlotPoint::new(1.0000001, 1.0)];
        curves[1].points = vec![PlotPoint::new(1.0000002, 2.0)];
        let label = sample_tooltip(&curves, &model.observations, 1.0, |_| false).unwrap();
        assert!(label.contains("T+00:00:01.000000100"));
        assert!(label.contains("T+00:00:01.000000200"));
    }

    #[test]
    fn hover_omits_presentation_hidden_legend_hidden_and_absent_samples() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = model();
        model.presentation.plots[0].traces[1].visible = false;
        let text = hover(&context, &mut renderer, &model, 1, 41.0);
        let label = tooltip(&text).unwrap();
        assert!(label.contains("Temperature 1: 292 K"));
        assert!(!label.contains("Temperature 2"));
        assert!(!label.contains("270")); // outside the configured trailing window
        let id = *renderer.plot_ids.iter().next().unwrap();
        let mut memory = egui_plot::PlotMemory::load(&context, id).unwrap();
        memory.hidden_items.insert(egui::Id::new("trace-1"));
        memory.store(&context, id);
        assert!(tooltip(&hover(&context, &mut renderer, &model, 1, 41.0)).is_none());
        let mut memory = egui_plot::PlotMemory::load(&context, id).unwrap();
        memory.hidden_items.clear();
        memory.store(&context, id);
        model.observations.live.clear();
        assert!(tooltip(&hover(&context, &mut renderer, &model, 1, 41.0)).is_none());
    }

    #[test]
    fn hover_freshness_respects_stale_unknown_and_the_rebuild_barrier() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = model();
        for (state, expected) in [
            (Freshness::Stale, "Stale — cached observation"),
            (
                Freshness::Rebuilding,
                "Rebuilding — freshness not established",
            ),
            (Freshness::Unknown, "Unknown — freshness not established"),
        ] {
            model.observations.freshness = state;
            let text = hover(&context, &mut renderer, &model, 1, 41.0);
            let label = tooltip(&text).unwrap();
            assert_eq!(label.matches(expected).count(), 2);
            assert!(!label.contains("Fresh view"));
        }
    }

    #[test]
    fn hover_preserves_dispatcher_revision_and_runtime_silence_in_legacy_and_declared_views() {
        for declared in [true, false] {
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut model = model();
            if !declared {
                model.presentation.windows.clear();
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            assert!(tooltip(&hover(&context, &mut renderer, &dispatcher, 1, 41.0)).is_some());
            assert_eq!(*dispatcher, before);
            assert_eq!(dispatcher.presentation_revision(), 1);
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
        }
    }

    #[test]
    fn selected_signal_graph_keeps_full_buffer_and_uses_the_shared_tooltip() {
        let context = egui::Context::default();
        let model = model();
        let before = model.clone();
        let source = &model.presentation.plots[0].traces[0].source;
        let id = Cell::new(egui::Id::NULL);
        let run = |events| {
            painted_text(context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100.0, 1600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    id.set(ui.make_persistent_id("selected-live-signal"));
                    render_signal_plot(ui, source, &model.observations);
                },
            ))
        };
        run(vec![]);
        run(vec![]);
        let memory = egui_plot::PlotMemory::load(&context, id.get()).unwrap();
        let position = egui::pos2(
            memory.transform().position_from_point_x(0.0),
            memory.transform().frame().center().y,
        );
        run(vec![egui::Event::PointerMoved(position)]);
        let text = run(vec![]);
        assert!(
            tooltip(&text)
                .unwrap()
                .contains("Signal 1/1: 270 K · T+00:00:00.000 · Fresh view")
        );
        assert_eq!(model, before);
    }

    #[test]
    fn tooltip_keeps_legend_hiding_zoom_and_pan_owned_by_egui_plot() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let model = model();
        hover(&context, &mut renderer, &model, 1, 50.0);
        let id = *renderer.plot_ids.iter().next().unwrap();
        let mut memory = egui_plot::PlotMemory::load(&context, id).unwrap();
        memory.hidden_items.insert(egui::Id::new("trace-2"));
        memory.store(&context, id);
        let before = *egui_plot::PlotMemory::load(&context, id).unwrap().bounds();
        frame(
            &context,
            &mut renderer,
            &model,
            1,
            vec![egui::Event::Zoom(2.0)],
        );
        let zoomed = egui_plot::PlotMemory::load(&context, id).unwrap();
        assert!(zoomed.bounds().width() < before.width());
        let position = zoomed.transform().frame().center();
        frame(
            &context,
            &mut renderer,
            &model,
            1,
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        );
        let moved = position + egui::vec2(30.0, 0.0);
        frame(
            &context,
            &mut renderer,
            &model,
            1,
            vec![egui::Event::PointerMoved(moved)],
        );
        frame(
            &context,
            &mut renderer,
            &model,
            1,
            vec![egui::Event::PointerButton {
                pos: moved,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let panned = egui_plot::PlotMemory::load(&context, id).unwrap();
        assert_ne!(panned.bounds().min()[0], zoomed.bounds().min()[0]);
        assert!(!panned.auto_bounds.x);
        assert!(panned.hidden_items.contains(&egui::Id::new("trace-2")));
        let text = hover(&context, &mut renderer, &model, 1, 50.0);
        assert!(!tooltip(&text).unwrap().contains("Temperature 2"));
        assert_eq!(
            *egui_plot::PlotMemory::load(&context, id).unwrap().bounds(),
            *panned.bounds()
        );
    }

    #[test]
    fn logical_window_tab_status_and_two_observed_curves_render_once() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = model();
        let mut unreferenced = model.presentation.plots[0].clone();
        unreferenced.id = "unreferenced".into();
        unreferenced.title = "Legacy-only plot".into();
        unreferenced.traces.clear();
        model.presentation.plots.push(unreferenced);
        let before = model.clone();
        frame(&context, &mut renderer, &model, 1, vec![]);
        let (declared, text) = frame(&context, &mut renderer, &model, 1, vec![]);
        let text = labels(&text);
        assert!(declared);
        for label in [
            "Temperatures",
            "Live tab",
            "Other tab",
            "Sensor status",
            "Temperature panel",
            "Two temperature curves",
        ] {
            assert_eq!(
                text.iter().filter(|value| **value == label).count(),
                1,
                "{label}"
            );
        }
        assert!(text.contains(&"Fresh"));
        assert!(text.iter().any(|value| value.contains("293.0")));
        assert!(!text.contains(&"Legacy-only plot"));
        for trace in &model.presentation.plots[0].traces {
            let offset = if trace.id == "trace-1" { 0.0 } else { 10.0 };
            // One status row and one egui_plot legend entry per visible curve.
            assert_eq!(
                text.iter()
                    .filter(|value| **value == trace.display_label)
                    .count(),
                2
            );
            assert_eq!(
                trace_points(&model.observations, trace, 30.0),
                vec![
                    PlotPoint::new(40.0, 292.0 + offset),
                    PlotPoint::new(60.0, 293.0 + offset)
                ]
            );
        }
        assert_eq!(model, before);
    }

    #[test]
    fn clicking_tabs_changes_only_navigation_and_controls_remain_inactive() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let model = model();
        let before = model.clone();
        let (_, text) = frame(&context, &mut renderer, &model, 1, vec![]);
        let center = text
            .iter()
            .find(|(text, _)| text == "Other tab")
            .unwrap()
            .1
            .center();
        for pressed in [true, false] {
            frame(
                &context,
                &mut renderer,
                &model,
                1,
                vec![
                    egui::Event::PointerMoved(center),
                    egui::Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let (_, text) = frame(&context, &mut renderer, &model, 1, vec![]);
        let text = labels(&text);
        assert!(text.contains(&"Inactive controls"));
        assert!(text.contains(&"Controls are not available in this view."));
        assert!(!text.contains(&"Sensor status"));
        assert!(!text.contains(&"Two temperature curves"));
        assert_eq!(renderer.selected_tabs["window"], "other");
        assert_eq!(model, before);
    }

    #[test]
    fn missing_unknown_stale_and_rebuilding_data_are_never_labelled_fresh() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = model();
        let source = model.presentation.plots[0].traces[0].source.clone();
        for (freshness, expected) in [
            (Freshness::Unknown, "Unknown — freshness not established"),
            (Freshness::Stale, "Stale — cached observation"),
            (
                Freshness::Rebuilding,
                "Rebuilding — freshness not established",
            ),
        ] {
            model.observations.freshness = freshness;
            let (_, text) = frame(&context, &mut renderer, &model, 1, vec![]);
            let text = labels(&text);
            assert!(text.contains(&expected));
            assert!(!text.contains(&"Fresh"));
            assert!(text.iter().any(|value| value.contains("293.0")));
        }
        model.observations.complete_rebuild();
        model
            .observations
            .entities
            .get_mut(&source)
            .unwrap()
            .freshness = Freshness::Stale;
        assert_eq!(
            source_state(&model.observations, &source),
            SourceState::Stale
        );
        model
            .observations
            .entities
            .get_mut(&source)
            .unwrap()
            .freshness = Freshness::Unknown;
        assert_eq!(
            source_state(&model.observations, &source),
            SourceState::Unknown
        );
        model.observations.entities.clear();
        model.observations.live.clear();
        let (_, text) = frame(&context, &mut renderer, &model, 1, vec![]);
        let text = labels(&text);
        assert!(text.contains(&"Unresolved — no Runtime observation"));
        assert!(text.contains(&"Waiting for good finite signal events."));
        assert!(!text.iter().any(|value| value.contains("293.0")));
        assert!(!text.contains(&"Fresh"));
    }

    #[test]
    fn empty_windows_keep_the_legacy_plot_path_and_clear_navigation() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = model();
        frame(&context, &mut renderer, &model, 1, vec![]);
        model.presentation.windows.clear();
        let before = model.clone();
        let (declared, text) = frame(&context, &mut renderer, &model, 2, vec![]);
        assert!(!declared);
        let text = labels(&text);
        assert!(text.contains(&"Two temperature curves"));
        assert!(!text.contains(&"Live tab"));
        assert!(renderer.selected_tabs.is_empty());
        assert_eq!(model, before);
    }

    #[derive(Default)]
    struct FakeClient(Cell<usize>);

    impl LabClient for FakeClient {
        fn query(&self, _op: &str, _args: Value) -> Result<u64, CommandSendError> {
            self.0.set(self.0.get() + 1);
            Ok(1)
        }
        fn mutation(&self, _op: &str, _args: Value) -> Result<u64, CommandSendError> {
            self.0.set(self.0.get() + 1);
            Ok(1)
        }
        fn operation_status(&self, _identity: MutationIdentity) -> Result<u64, CommandSendError> {
            self.0.set(self.0.get() + 1);
            Ok(1)
        }
    }

    #[test]
    fn api_replacement_reconciles_tabs_and_rerenders_without_runtime_work() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut dispatcher = WorkbenchDispatcher::new(model(), FakeClient::default());
        frame(
            &context,
            &mut renderer,
            &dispatcher,
            dispatcher.presentation_revision(),
            vec![],
        );
        let old_plot_ids = renderer.plot_ids.clone();
        assert_eq!(old_plot_ids.len(), 1);
        for id in &old_plot_ids {
            let mut memory = egui_plot::PlotMemory::load(&context, *id).unwrap();
            memory.hidden_items.insert(egui::Id::new("trace-1"));
            memory.store(&context, *id);
        }
        renderer
            .selected_tabs
            .insert("window".into(), "other".into());
        let mut replacement = dispatcher.presentation.clone();
        replacement.windows[0].tabs.pop();
        replacement.windows[0].title = "Replaced window".into();
        replacement.windows[0].tabs[0].title = "Replacement tab".into();
        replacement.plots[0].traces[0].display_label = "Replacement temperature".into();
        let outcome = dispatcher
            .dispatch(
                CallOrigin {
                    caller_id: CallerId(1),
                    call_id: "replace".into(),
                },
                WorkbenchRequest::UiReplaceDocument {
                    expected: dispatcher.presentation_expectation(),
                    document: serde_json::to_value(&replacement).unwrap(),
                },
            )
            .unwrap();
        assert_eq!(outcome.events.len(), 1);
        let before = (*dispatcher).clone();
        let (_, text) = frame(&context, &mut renderer, &dispatcher, 2, vec![]);
        let text = labels(&text);
        assert!(text.contains(&"Replaced window"));
        assert!(text.contains(&"Replacement tab"));
        assert!(text.contains(&"Replacement temperature"));
        assert!(!text.contains(&"Other tab"));
        assert_eq!(renderer.selected_tabs["window"], "live");
        assert_eq!(renderer.plot_ids.len(), 1);
        for id in &old_plot_ids {
            assert!(egui_plot::PlotMemory::load(&context, *id).is_none());
        }
        for id in &renderer.plot_ids {
            assert!(
                egui_plot::PlotMemory::load(&context, *id)
                    .unwrap()
                    .hidden_items
                    .is_empty()
            );
        }
        assert_eq!(dispatcher.presentation, replacement);
        assert_eq!(*dispatcher, before);
        assert_eq!(dispatcher.presentation_revision(), 2);
        assert_eq!(dispatcher.client().unwrap().0.get(), 0);

        replacement.document_id = "new-document".into();
        replacement.windows[0].id = "new-window".into();
        dispatcher
            .dispatch(
                CallOrigin {
                    caller_id: CallerId(1),
                    call_id: "new".into(),
                },
                WorkbenchRequest::UiReplaceDocument {
                    expected: dispatcher.presentation_expectation(),
                    document: serde_json::to_value(replacement).unwrap(),
                },
            )
            .unwrap();
        frame(&context, &mut renderer, &dispatcher, 3, vec![]);
        assert_eq!(renderer.selected_tabs.len(), 1);
        assert!(renderer.selected_tabs.contains_key("new-window"));
        assert_eq!(dispatcher.client().unwrap().0.get(), 0);
    }
}

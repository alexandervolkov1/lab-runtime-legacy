//! Declarative layout returning operator intent without Runtime submission authority.

use super::controls::{ControlEditors, OperatorControlUi};

use crate::{
    model::{Freshness, OperatorIntent, RuntimeObservations},
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
    editors: ControlEditors,
}

pub(super) struct PresentationResponse {
    pub(super) declarative: bool,
    pub(super) intent: Option<OperatorIntent>,
}

impl PresentationRenderer {
    /// Renders logical windows, or the legacy plot list when no windows exist.
    /// Returns view mode and at most one intent; the owner must request confirmation.
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        document: &PresentationDocument,
        observations: &RuntimeObservations,
        revision: u64,
        controls: &mut OperatorControlUi<'_>,
    ) -> PresentationResponse {
        let mut intent = None;
        let replaced = self.revision != Some(revision) || self.document_id != document.document_id;
        self.editors.reconcile(ui.ctx(), document, replaced);
        if replaced {
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
            return PresentationResponse {
                declarative: false,
                intent,
            };
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
                                            PanelKind::Controls { control_ids } => {
                                                let clicked = controls.show_panel(
                                                    ui,
                                                    &document.controls,
                                                    control_ids,
                                                    &mut self.editors,
                                                );
                                                if intent.is_none() {
                                                    intent = clicked;
                                                }
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
        PresentationResponse {
            declarative: true,
            intent,
        }
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
pub(super) enum SourceState {
    Fresh,
    Stale,
    Unknown,
    Rebuilding,
    Unresolved,
}

pub(super) fn source_state(observations: &RuntimeObservations, source: &RuntimeRef) -> SourceState {
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

pub(super) fn state_label(ui: &mut egui::Ui, state: SourceState) {
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

fn elapsed_time_label(seconds: f64) -> Option<String> {
    // observed_at_ns is Runtime's monotonic duration since its process clock
    // origin. Workbench has no trustworthy calendar anchor for live points.
    let duration = Duration::try_from_secs_f64(seconds).ok()?;
    let milliseconds = (duration.as_nanos() + 500_000) / 1_000_000;
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds / 60_000) % 60;
    let seconds = (milliseconds / 1_000) % 60;
    Some(format!(
        "T+{hours:02}:{minutes:02}:{seconds:02}.{:03}",
        milliseconds % 1_000,
    ))
}

fn line_value_label(
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

fn line_tooltip(
    plot_name: &str,
    curve: &ObservedCurve<'_>,
    observations: &RuntimeObservations,
    position: PlotPoint,
) -> Option<String> {
    if !position.y.is_finite() {
        return None;
    }
    let timestamp = elapsed_time_label(position.x)?;
    let state_suffix = match source_state(observations, curve.source) {
        SourceState::Fresh => "",
        SourceState::Stale => "\nStale",
        SourceState::Unknown => "\nUnknown",
        SourceState::Rebuilding => "\nRebuilding",
        SourceState::Unresolved => "\nUnresolved",
    };
    // Format the library's line position, without claiming an extra measurement
    // if a future egui_plot version returns a point inside a segment.
    Some(format!(
        "{}\n{timestamp}, {}{state_suffix}",
        plot_name,
        line_value_label(position.y, curve, observations)
    ))
}

fn show_observed_plot<'a>(
    ui: &mut egui::Ui,
    view: EguiPlot<'a>,
    curves: &'a [ObservedCurve<'a>],
    observations: &'a RuntimeObservations,
) {
    // HoverPosition carries a name, not an item ID. Keep legend names unique
    // even when presentation labels repeat, so units/freshness use the selected
    // curve's source. IDs, buffer contents and the document remain unchanged.
    let mut named_curves: Vec<(String, &ObservedCurve<'_>)> = Vec::with_capacity(curves.len());
    for curve in curves {
        let mut name = curve.label.to_owned();
        let mut suffix = 2;
        while named_curves.iter().any(|(used, _)| used == &name) {
            name = format!("{} ({suffix})", curve.label);
            suffix += 1;
        }
        named_curves.push((name, curve));
    }
    view.label_formatter(|hover| {
        let HoverPosition::NearDataPoint {
            plot_name,
            position,
            ..
        } = hover
        else {
            return None;
        };
        let (_, curve) = named_curves.iter().find(|(name, _)| name == plot_name)?;
        line_tooltip(plot_name, curve, observations, *position)
    })
    .show(ui, |plot_ui| {
        for (name, curve) in named_curves
            .iter()
            .filter(|(_, curve)| !curve.points.is_empty())
        {
            let mut line = Line::new(name, PlotPoints::Borrowed(&curve.points))
                .id(curve.id)
                .width(curve.width);
            if let Some(color) = curve.color {
                line = line.color(color);
            }
            plot_ui.line(line);
        }
    });
}

/// The ordinary selected-signal graph uses the same read-only line tooltip.
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
    show_observed_plot(ui, view, &curves, observations);
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
        client::{
            ClientUpdate,
            types::{
                CommandSendError, ConnectionState, EventCursor, HelloState, KnownAdmission,
                MutationIdentity, QuarantinedRecoveryRecord, RecoveryQuarantineReason,
                RecoveryRecord, ReplyKind,
            },
        },
        dispatcher::{CallOrigin, CallerId, LabClient, WorkbenchDispatcher, WorkbenchRequest},
        model::{
            ControllerLifecycleIntent, LivePoint, MutationSubmitter, OperatorWarning,
            OperatorWorkflow, OperatorWorkflowState, PROPERTY_TEXT_BYTES,
            PropertyMutationCandidate, RECORDING_LABEL_BYTES, WorkbenchModel,
        },
        presentation::{ConfigurationOwner, Control, ControlKind},
    };
    use serde_json::{Value, json};
    use std::cell::{Cell, RefCell};

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
                let response = renderer.show(
                    ui,
                    &model.presentation,
                    &model.observations,
                    revision,
                    &mut OperatorControlUi {
                        model,
                        operator: &OperatorWorkflow::default(),
                        observation_only: false,
                        label: &mut "Workbench run".to_owned(),
                    },
                );
                declared = response.declarative;
                assert!(response.intent.is_none());
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

    fn hover_model() -> WorkbenchModel {
        let mut model = model();
        // Keep both curves and empty space well inside stable Y bounds.
        model.presentation.plots[0].axes.y_min = Some(280.0);
        model.presentation.plots[0].axes.y_max = Some(320.0);
        model
    }

    fn hover(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        revision: u64,
        point: PlotPoint,
    ) -> Vec<(String, egui::Rect)> {
        frame(context, renderer, model, revision, vec![]);
        frame(context, renderer, model, revision, vec![]);
        let memory =
            egui_plot::PlotMemory::load(context, *renderer.plot_ids.iter().next().unwrap())
                .unwrap();
        let transform = memory.transform();
        // Exercise egui_plot's screen-space hit testing in both dimensions.
        let position = transform.position_from_point(&point) + egui::vec2(0.0, 2.0);
        assert!(transform.frame().contains(position));
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
            .find(|(text, _)| text.contains("\nT+"))
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
            assert_eq!(elapsed_time_label(seconds).as_deref(), Some(expected));
        }
        for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(elapsed_time_label(invalid).is_none());
        }
    }

    #[test]
    fn hover_switches_between_curves_by_screen_distance_and_is_absent_in_empty_space() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = hover_model();
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
        // Same cursor X throughout: only the actual screen Y changes selection.
        // X=42 lies on a segment, away from either retained endpoint. In 0.37
        // Line returns the closer endpoint, not cursor X or an interpolated Y.
        for (value, expected) in [
            (292.0, Some("Temperature 1\nT+00:00:40.125, 292 °C")),
            (302.0, Some("Temperature 2\nT+00:00:40.375, 302 K")),
            (297.0, None), // between the separated curves
            (315.0, None), // far from both, but still inside the plot
            (292.0, Some("Temperature 1\nT+00:00:40.125, 292 °C")),
        ] {
            let text = hover(
                &context,
                &mut renderer,
                &model,
                1,
                PlotPoint::new(42.0, value),
            );
            assert_eq!(tooltip(&text), expected);
            if let Some(label) = tooltip(&text) {
                assert!(!label.contains("Fresh view"));
                assert_eq!(label.lines().count(), 2);
            }
            assert_eq!(
                text.iter()
                    .filter(|(text, _)| text.contains("\nT+"))
                    .count(),
                usize::from(expected.is_some())
            );
        }
        assert_eq!(model, before);
    }

    #[test]
    fn tooltip_formats_the_returned_position_with_honest_unit_fallbacks() {
        let mut model = model();
        model.observations.entities.clear();
        let traces = &model.presentation.plots[0].traces;
        let curves = traces
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
        // This position is intentionally not a retained point. The formatter
        // uses the library's coordinates and does not call it a measured sample.
        let position = PlotPoint::new(1.2345, 0.00000025);
        let label =
            line_tooltip(curves[0].label, &curves[0], &model.observations, position).unwrap();
        assert!(label.contains("T+00:00:01.235, 0.00000025 K (display label; no conversion)"));
        assert!(!label.contains("sample"));
        assert!(!label.contains("Fresh view"));
        let label =
            line_tooltip(curves[1].label, &curves[1], &model.observations, position).unwrap();
        assert!(label.contains("T+00:00:01.235, 0.00000025 (unit unknown)"));
        assert!(
            line_tooltip(
                curves[0].label,
                &curves[0],
                &model.observations,
                PlotPoint::new(1.0, f64::NAN)
            )
            .is_none()
        );
    }

    #[test]
    fn repeated_curve_names_keep_the_selected_sources_unit_and_freshness() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = hover_model();
        model.presentation.plots[0].traces[1].display_label = "Temperature 1".into();
        let source = model.presentation.plots[0].traces[1].source.clone();
        let observation = model.observations.entities.get_mut(&source).unwrap();
        observation.value["unit"] = json!({"id":"degC","symbol":"°C"});
        observation.freshness = Freshness::Stale;
        for (point, expected) in [
            (
                PlotPoint::new(40.0, 292.0),
                "Temperature 1\nT+00:00:40.000, 292 K",
            ),
            (
                PlotPoint::new(40.0, 302.0),
                "Temperature 1 (2)\nT+00:00:40.000, 302 °C\nStale",
            ),
        ] {
            let text = hover(&context, &mut renderer, &model, 1, point);
            assert_eq!(tooltip(&text), Some(expected));
        }
    }

    #[test]
    fn hover_omits_presentation_hidden_legend_hidden_and_absent_samples() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = hover_model();
        let point = PlotPoint::new(40.0, 292.0);
        let text = hover(&context, &mut renderer, &model, 1, point);
        let label = tooltip(&text).unwrap();
        assert!(label.contains("292 K"));
        assert!(!label.contains("Temperature 2"));
        assert!(!label.contains("270")); // outside the configured trailing window
        model.presentation.plots[0].traces[0].visible = false;
        assert!(tooltip(&hover(&context, &mut renderer, &model, 1, point)).is_none());
        model.presentation.plots[0].traces[0].visible = true;
        let id = *renderer.plot_ids.iter().next().unwrap();
        let mut memory = egui_plot::PlotMemory::load(&context, id).unwrap();
        memory.hidden_items.insert(egui::Id::new("trace-1"));
        memory.store(&context, id);
        assert!(tooltip(&hover(&context, &mut renderer, &model, 1, point)).is_none());
        let mut memory = egui_plot::PlotMemory::load(&context, id).unwrap();
        memory.hidden_items.clear();
        memory.store(&context, id);
        let source = model.presentation.plots[0].traces[0].source.clone();
        model.observations.live.remove(&source);
        assert!(tooltip(&hover(&context, &mut renderer, &model, 1, point)).is_none());
    }

    #[test]
    fn hover_freshness_respects_stale_unknown_and_the_rebuild_barrier() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut model = hover_model();
        model.presentation.plots[0].traces.truncate(1);
        for (state, expected) in [
            (Freshness::Stale, "Stale"),
            (Freshness::Rebuilding, "Rebuilding"),
            (Freshness::Unknown, "Unknown"),
        ] {
            model.observations.freshness = state;
            let text = hover(
                &context,
                &mut renderer,
                &model,
                1,
                PlotPoint::new(40.0, 292.0),
            );
            let label = tooltip(&text).unwrap();
            assert_eq!(
                label,
                format!("Temperature 1\nT+00:00:40.000, 292 K\n{expected}")
            );
            assert!(!label.contains("Fresh"));
        }
    }

    #[test]
    fn hover_preserves_dispatcher_revision_and_runtime_silence_in_legacy_and_declared_views() {
        for declared in [true, false] {
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut model = hover_model();
            if !declared {
                model.presentation.windows.clear();
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            for point in [
                PlotPoint::new(40.0, 292.0),
                PlotPoint::new(40.0, 302.0),
                PlotPoint::new(40.0, 297.0),
            ] {
                hover(&context, &mut renderer, &dispatcher, 1, point);
            }
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
        let position = memory
            .transform()
            .position_from_point(&PlotPoint::new(0.0, 270.0));
        run(vec![egui::Event::PointerMoved(position)]);
        let text = run(vec![]);
        assert_eq!(tooltip(&text), Some("Signal 1/1\nT+00:00:00.000, 270 K"));
        assert_eq!(model, before);
    }

    #[test]
    fn tooltip_keeps_legend_hiding_zoom_and_pan_owned_by_egui_plot() {
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let model = hover_model();
        let text = hover(
            &context,
            &mut renderer,
            &model,
            1,
            PlotPoint::new(50.0, 292.5),
        );
        let id = *renderer.plot_ids.iter().next().unwrap();
        // Click the real legend entry, rather than injecting its hidden state.
        let legend_position = text
            .iter()
            .find(|(label, _)| label == "Temperature 2")
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
                    egui::Event::PointerMoved(legend_position),
                    egui::Event::PointerButton {
                        pos: legend_position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert!(
            egui_plot::PlotMemory::load(&context, id)
                .unwrap()
                .hidden_items
                .contains(&egui::Id::new("trace-2"))
        );
        assert!(
            tooltip(&hover(
                &context,
                &mut renderer,
                &model,
                1,
                PlotPoint::new(50.0, 302.5)
            ))
            .is_none()
        );
        assert!(
            tooltip(&hover(
                &context,
                &mut renderer,
                &model,
                1,
                PlotPoint::new(50.0, 292.5)
            ))
            .is_some()
        );
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
        let text = hover(
            &context,
            &mut renderer,
            &model,
            1,
            PlotPoint::new(50.0, 292.5),
        );
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
    fn clicking_tabs_changes_only_navigation_and_empty_controls_stay_inert() {
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
        assert!(text.contains(&"No controls configured."));
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
    struct FakeClient(Cell<usize>, RefCell<Vec<(String, Value)>>);

    impl LabClient for FakeClient {
        fn query(&self, _op: &str, _args: Value) -> Result<u64, CommandSendError> {
            self.0.set(self.0.get() + 1);
            Ok(1)
        }
        fn mutation(&self, op: &str, args: Value) -> Result<u64, CommandSendError> {
            self.0.set(self.0.get() + 1);
            self.1.borrow_mut().push((op.to_owned(), args));
            Ok(1)
        }
        fn operation_status(&self, _identity: MutationIdentity) -> Result<u64, CommandSendError> {
            self.0.set(self.0.get() + 1);
            Ok(1)
        }
    }

    impl MutationSubmitter for FakeClient {
        fn submit_mutation(&self, op: &str, args: Value) -> Result<u64, CommandSendError> {
            LabClient::mutation(self, op, args)
        }
    }

    fn recorder_model() -> WorkbenchModel {
        let mut model = model();
        model.presentation.windows[0].tabs.truncate(1);
        model.presentation.windows[0].tabs[0].panels = vec![crate::presentation::Panel {
            id: "record-panel".into(),
            title: "Recording panel".into(),
            kind: PanelKind::Controls {
                control_ids: vec!["record".into()],
            },
        }];
        model.presentation.controls = vec![Control {
            id: "record".into(),
            label: "Dashboard recording".into(),
            target: RuntimeRef::Recorder,
            kind: ControlKind::RecordingLifecycle,
            confirm: true,
        }];
        model.presentation.validate().unwrap();
        model.connection = ConnectionState::Ready;
        model.hello = Some(HelloState {
            boot_id: "boot".into(),
            scope: "scope".into(),
            next_seq: 1,
            operations: vec!["recording_start".into(), "recording_stop".into()],
            capabilities: json!([]),
            limits: json!({}),
            event_oldest: EventCursor {
                boot_id: "boot".into(),
                seq: 0,
            },
            event_latest: EventCursor {
                boot_id: "boot".into(),
                seq: 0,
            },
        });
        model.observations.observe(
            RuntimeRef::Recorder,
            json!({"state":"idle","active_run":null}),
            None,
        );
        model.observations.complete_rebuild();
        model
    }

    fn controller_model(state: Option<&str>) -> WorkbenchModel {
        let mut model = recorder_model();
        model.presentation.controls[0] = Control {
            id: "record".into(),
            label: "Heater controller".into(),
            target: RuntimeRef::Controller {
                controller: "1".into(),
            },
            kind: ControlKind::ControllerLifecycle,
            confirm: false,
        };
        model.hello.as_mut().unwrap().operations.extend(
            [
                "controller_start",
                "controller_pause",
                "controller_resume",
                "controller_reset_failed",
            ]
            .map(str::to_owned),
        );
        model.observations.observe(
            model.presentation.controls[0].target.clone(),
            json!({"controller":"1", "state":state}),
            None,
        );
        model.presentation.validate().unwrap();
        model
    }

    #[test]
    fn controller_buttons_follow_runtime_state_and_require_separate_confirmation() {
        for confirm in [false, true] {
            for (state, button, action, operation) in [
                (
                    "ready",
                    "Start",
                    ControllerLifecycleIntent::Start,
                    "controller_start",
                ),
                (
                    "warming",
                    "Pause",
                    ControllerLifecycleIntent::Pause,
                    "controller_pause",
                ),
                (
                    "running",
                    "Pause",
                    ControllerLifecycleIntent::Pause,
                    "controller_pause",
                ),
                (
                    "paused",
                    "Resume",
                    ControllerLifecycleIntent::Resume,
                    "controller_resume",
                ),
                (
                    "failed",
                    "Reset failed",
                    ControllerLifecycleIntent::ResetFailed,
                    "controller_reset_failed",
                ),
            ] {
                let mut model = controller_model(Some(state));
                model.presentation.controls[0].confirm = confirm;
                let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
                let before = (*dispatcher).clone();
                let context = egui::Context::default();
                let mut renderer = PresentationRenderer::default();
                let mut operator = OperatorWorkflow::default();
                let (_, text) = recorder_frame(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut String::new(),
                    vec![],
                );
                let text = labels(&text);
                assert!(text.contains(&"Heater controller"));
                assert!(text.contains(&"Controller: 1"));
                assert!(text.contains(&format!("Controller state (cached): {state}").as_str()));
                for label in ["Start", "Pause", "Resume", "Reset failed"] {
                    assert_eq!(text.contains(&label), label == button);
                }
                let intent = click_recorder(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut String::new(),
                    button,
                )
                .unwrap();
                assert_eq!(
                    intent,
                    OperatorIntent::ControllerLifecycle {
                        controller: "1".into(),
                        action
                    }
                );
                operator.begin(&dispatcher, intent.clone()).unwrap();
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::AwaitingConfirmation(_)
                ));
                assert_eq!(dispatcher.client().unwrap().0.get(), 0);
                assert!(
                    click_recorder(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        &mut String::new(),
                        button
                    )
                    .is_none()
                );
                operator.cancel_or_acknowledge();
                assert_eq!(
                    dispatcher.client().unwrap().0.get(),
                    0,
                    "Cancel sends nothing"
                );
                operator.begin(&dispatcher, intent).unwrap();
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert!(
                    operator
                        .confirm(&dispatcher, dispatcher.client().unwrap())
                        .is_err()
                );
                assert_eq!(dispatcher.client().unwrap().0.get(), 1);
                assert_eq!(
                    dispatcher.client().unwrap().1.borrow().as_slice(),
                    [(operation.to_owned(), json!({"controller":"1"}))]
                );
                assert!(
                    click_recorder(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        &mut String::new(),
                        button
                    )
                    .is_none()
                );
                assert_eq!(
                    *dispatcher, before,
                    "render/click/prepare does not mutate the model"
                );
                assert_eq!(dispatcher.presentation_revision(), 1);
            }
        }
    }

    #[test]
    fn controller_unavailable_states_and_readiness_fences_send_nothing() {
        for scenario in [
            "unknown",
            "missing_state",
            "disabled",
            "stopped",
            "starting",
            "observe",
            "stale",
            "rebuilding",
            "target_stale",
            "target_rebuilding",
            "missing",
            "unadvertised",
            "disconnected",
            "recovery",
            "quarantine",
            "reconciliation",
            "journal_failure",
            "capacity",
        ] {
            let mut model = controller_model(Some("ready"));
            let target = model.presentation.controls[0].target.clone();
            let mut button = "Start";
            match scenario {
                "observe" => {}
                "stale" => model.observations.freshness = Freshness::Stale,
                "rebuilding" => model.observations.freshness = Freshness::Rebuilding,
                "target_stale" | "target_rebuilding" => {
                    model
                        .observations
                        .entities
                        .get_mut(&target)
                        .unwrap()
                        .freshness = if scenario == "target_stale" {
                        Freshness::Stale
                    } else {
                        Freshness::Rebuilding
                    }
                }
                "missing" => {
                    model.observations.entities.remove(&target);
                    button = "No lifecycle action available";
                }
                "unadvertised" => model.hello.as_mut().unwrap().operations.clear(),
                "disconnected" => model.connection = ConnectionState::Stale,
                "recovery" => model.recovery.mutations.push(ambiguous_record()),
                "quarantine" => model.recovery.quarantined.push(QuarantinedRecoveryRecord {
                    record: ambiguous_record(),
                    reason: RecoveryQuarantineReason::AttachedBootMismatch,
                }),
                "reconciliation" => model
                    .recovery
                    .reconciliation_required
                    .push(ambiguous_record().identity),
                "journal_failure" => model.recovery_problem = Some("journal unavailable".into()),
                "capacity" => {
                    for id in 0..crate::model::MAX_OPERATOR_ACTIONS as u64 {
                        model.track_operator_intent(id).unwrap();
                    }
                }
                _ => {
                    model.observations.entities.get_mut(&target).unwrap().value =
                        if scenario == "missing_state" {
                            json!({})
                        } else {
                            json!({"state":scenario})
                        };
                    button = "No lifecycle action available";
                }
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            assert!(
                click_recorder(
                    &egui::Context::default(),
                    &mut PresentationRenderer::default(),
                    &dispatcher,
                    &OperatorWorkflow::default(),
                    scenario == "observe",
                    &mut String::new(),
                    button
                )
                .is_none(),
                "{scenario}"
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0, "{scenario}");
            assert_eq!(*dispatcher, before, "{scenario}");
        }
    }

    #[test]
    fn controller_confirm_rechecks_exact_source_state_and_never_retries_failure() {
        for (state, changed, button) in [
            ("ready", "failed", "Start"),
            ("warming", "running", "Pause"),
            ("running", "paused", "Pause"),
            ("paused", "running", "Resume"),
            ("failed", "ready", "Reset failed"),
        ] {
            let mut model = controller_model(Some(state));
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut operator = OperatorWorkflow::default();
            let intent = click_recorder(
                &context,
                &mut renderer,
                &model,
                &operator,
                false,
                &mut String::new(),
                button,
            )
            .unwrap();
            operator.begin(&model, intent).unwrap();
            let target = model.presentation.controls[0].target.clone();
            model.observations.entities.get_mut(&target).unwrap().value["state"] = json!(changed);
            let client = FakeClient::default();
            assert_eq!(
                operator.confirm(&model, &client),
                Err(crate::model::OperatorIntentError::DraftStale)
            );
            assert_eq!(client.0.get(), 0);
        }
        let model = controller_model(Some("ready"));
        let client = FakeClient::default();
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut operator = OperatorWorkflow::default();
        operator
            .begin(
                &model,
                click_recorder(
                    &context,
                    &mut renderer,
                    &model,
                    &OperatorWorkflow::default(),
                    false,
                    &mut String::new(),
                    "Start",
                )
                .unwrap(),
            )
            .unwrap();
        operator.confirm(&model, &client).unwrap();
        operator.after_update(&ClientUpdate::Reply {
            command_id: 1,
            msg_id: "1".into(),
            op: "controller_start".into(),
            kind: ReplyKind::MutationFailed,
            envelope: json!({"code":"revision_conflict"}),
            recovery: None,
        });
        assert!(matches!(
            operator.state,
            OperatorWorkflowState::Failed { conflict: true, .. }
        ));
        assert!(
            click_recorder(
                &context,
                &mut renderer,
                &model,
                &operator,
                false,
                &mut String::new(),
                "Start"
            )
            .is_none()
        );
        assert_eq!(client.0.get(), 1, "failure never retries");
    }

    #[test]
    fn controller_replacement_preserves_prepared_target_and_ambiguous_workflow() {
        for ambiguous in [false, true] {
            let mut model = controller_model(Some("ready"));
            model.observations.observe(
                RuntimeRef::Controller {
                    controller: "2".into(),
                },
                json!({"controller":"2", "state":"ready"}),
                None,
            );
            let mut dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut operator = OperatorWorkflow::default();
            operator
                .begin(
                    &dispatcher,
                    click_recorder(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &OperatorWorkflow::default(),
                        false,
                        &mut String::new(),
                        "Start",
                    )
                    .unwrap(),
                )
                .unwrap();
            if ambiguous {
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                let mut record = ambiguous_record();
                record.op = "controller_start".into();
                record.args = json!({"controller":"1"});
                operator.after_update(&ClientUpdate::ReconciliationRequired {
                    records: vec![record.clone()],
                });
                dispatcher.apply_client_update(ClientUpdate::RecoveryProjection {
                    active: vec![record.clone()],
                    quarantined: vec![],
                });
                dispatcher.apply_client_update(ClientUpdate::ReconciliationRequired {
                    records: vec![record],
                });
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::Ambiguous { .. }
                ));
            }
            let original = operator.clone();
            let recovery = dispatcher.recovery.clone();
            let calls = dispatcher.client().unwrap().0.get();
            let mut replacement = dispatcher.presentation.clone();
            replacement.controls[0].target = RuntimeRef::Controller {
                controller: "2".into(),
            };
            dispatcher
                .dispatch(
                    CallOrigin {
                        caller_id: CallerId(1),
                        call_id: "replace-controller".into(),
                    },
                    WorkbenchRequest::UiReplaceDocument {
                        expected: dispatcher.presentation_expectation(),
                        document: serde_json::to_value(replacement).unwrap(),
                    },
                )
                .unwrap();
            let output = context.run_ui(Default::default(), |ui| {
                let response = renderer.show(
                    ui,
                    &dispatcher.presentation,
                    &dispatcher.observations,
                    dispatcher.presentation_revision(),
                    &mut OperatorControlUi {
                        model: &dispatcher,
                        operator: &operator,
                        observation_only: false,
                        label: &mut String::new(),
                    },
                );
                assert!(response.intent.is_none());
            });
            output.drop_without_applying_deltas();
            assert_eq!(renderer.revision, Some(dispatcher.presentation_revision()));
            assert!(
                click_recorder(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut String::new(),
                    "Start"
                )
                .is_none()
            );
            assert_eq!(operator, original);
            assert_eq!(dispatcher.recovery, recovery);
            assert_eq!(dispatcher.client().unwrap().0.get(), calls);
            if !ambiguous {
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert_eq!(
                    dispatcher.client().unwrap().1.borrow()[0],
                    ("controller_start".into(), json!({"controller":"1"}))
                );
            } else {
                assert!(
                    operator
                        .confirm(&dispatcher, dispatcher.client().unwrap())
                        .is_err()
                );
            }
        }
    }

    fn reference_model() -> WorkbenchModel {
        let mut model = recorder_model();
        model.presentation.controls = (1..=2)
            .map(|id| Control {
                id: format!("reference-control-{id}"),
                label: format!("Setpoint {id}"),
                target: RuntimeRef::Reference {
                    reference: id.to_string(),
                },
                kind: ControlKind::SetReference,
                confirm: false,
            })
            .collect();
        model.presentation.windows[0].tabs[0].panels[0].kind = PanelKind::Controls {
            control_ids: model
                .presentation
                .controls
                .iter()
                .map(|control| control.id.clone())
                .collect(),
        };
        model
            .hello
            .as_mut()
            .unwrap()
            .operations
            .push("reference_configure".into());
        for (id, value, revision, unit) in [("1", 20.25, "7", "°C"), ("2", 300.5, "12", "K")] {
            model.observations.observe(RuntimeRef::Reference { reference: id.into() },
                json!({"reference":id,"value":value,"revision":revision,"unit":{"symbol":unit},"kind":"fixed"}), None);
        }
        model.presentation.validate().unwrap();
        model
    }

    fn property_model() -> WorkbenchModel {
        let mut model = reference_model();
        let properties = [
            (
                ConfigurationOwner::Instrument {
                    instrument: "1".into(),
                },
                "limit",
                "integer",
                json!(4),
                "7",
                "ordinary_live",
            ),
            (
                ConfigurationOwner::Instrument {
                    instrument: "1".into(),
                },
                "title",
                "text",
                json!("Old title"),
                "8",
                "live_safe",
            ),
            (
                ConfigurationOwner::Component {
                    component: "1".into(),
                },
                "limit",
                "integer",
                json!(9),
                "12",
                "reinitialize",
            ),
        ];
        model.presentation.controls = properties
            .into_iter()
            .enumerate()
            .map(|(index, (owner, property, kind, current, revision, class))| {
                let target = RuntimeRef::ConfigurationProperty { owner, property: property.into() };
                model.observations.observe(
                    target.clone(),
                    json!({
                        "current":current, "value_type":kind, "access":"read_write", "revision":revision,
                        "mutation_class":class,
                        "unit":if index == 0 { json!({"id":"ms","symbol":"ms"}) } else { Value::Null },
                        "constraints":if index == 0 { json!({"minimum":-10,"maximum":10}) } else { Value::Null },
                    }),
                    None,
                );
                Control {
                    id:format!("property-{index}"), label:format!("Property editor {index}"), target,
                    kind:ControlKind::ApplyConfiguration, confirm:false,
                }
            })
            .collect();
        model.presentation.windows[0].tabs[0].panels[0].kind = PanelKind::Controls {
            control_ids: model
                .presentation
                .controls
                .iter()
                .map(|control| control.id.clone())
                .collect(),
        };
        model
            .hello
            .as_mut()
            .unwrap()
            .operations
            .push("property_configure".into());
        model.presentation.validate().unwrap();
        model
    }

    #[test]
    fn property_editors_keep_independent_full_targets_and_require_confirmation() {
        for confirm in [false, true] {
            let mut model = property_model();
            for control in &mut model.presentation.controls {
                control.confirm = confirm;
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut operator = OperatorWorkflow::default();
            for (index, text) in [(0, "-10"), (1, "New µm title"), (2, "9223372036854775807")] {
                input_control(&context, &mut renderer, &dispatcher, &operator, index, text);
            }
            let (_, text) = recorder_frame(
                &context,
                &mut renderer,
                &dispatcher,
                &operator,
                false,
                &mut String::new(),
                vec![],
            );
            for label in [
                "Property editor 0",
                "Owner: instrument 1; property: limit",
                "Owner: component 1; property: limit",
                "Current value (cached): 4",
                "Current value (cached): Old title",
                "type: integer; access: read_write",
                "Unit: ms",
            ] {
                assert!(labels(&text).contains(&label), "{label}");
            }
            for (index, candidate, owner_kind, property, revision, class) in [
                (
                    0,
                    PropertyMutationCandidate::Integer(-10),
                    "instrument",
                    "limit",
                    "7",
                    "ordinary_live",
                ),
                (
                    1,
                    PropertyMutationCandidate::Text("New µm title".into()),
                    "instrument",
                    "title",
                    "8",
                    "live_safe",
                ),
                (
                    2,
                    PropertyMutationCandidate::Integer(i64::MAX),
                    "component",
                    "limit",
                    "12",
                    "reinitialize",
                ),
            ] {
                let intent = click_editor(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    index,
                    "Configure property",
                )
                .unwrap();
                assert_eq!(
                    intent,
                    OperatorIntent::ConfigureProperty {
                        target: dispatcher.presentation.controls[index].target.clone(),
                        value: candidate.clone(),
                    }
                );
                operator.begin(&dispatcher, intent.clone()).unwrap();
                let OperatorWorkflowState::AwaitingConfirmation(prepared) = &operator.state else {
                    panic!()
                };
                assert_eq!(
                    prepared.warning,
                    Some(OperatorWarning::PropertyMutationClass(class.into()))
                );
                let value = match candidate {
                    PropertyMutationCandidate::Integer(value) => json!(value),
                    PropertyMutationCandidate::Text(value) => json!(value),
                };
                let args = json!({"target":{"kind":owner_kind,"id":"1"}, "property":property,
                    "expected_revision":revision,"value":value});
                assert_eq!(prepared.args, args);
                assert_eq!(dispatcher.client().unwrap().0.get(), index);
                assert!(
                    click_editor(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        (index + 1) % 3,
                        "Configure property"
                    )
                    .is_none()
                );
                operator.cancel_or_acknowledge();
                assert_eq!(
                    dispatcher.client().unwrap().0.get(),
                    index,
                    "Cancel sends nothing"
                );
                operator.begin(&dispatcher, intent).unwrap();
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert!(
                    operator
                        .confirm(&dispatcher, dispatcher.client().unwrap())
                        .is_err()
                );
                assert_eq!(dispatcher.client().unwrap().0.get(), index + 1);
                assert_eq!(
                    dispatcher.client().unwrap().1.borrow()[index],
                    ("property_configure".into(), args)
                );
                operator.after_update(&ClientUpdate::Reply {
                    command_id: 1,
                    msg_id: "1".into(),
                    op: "property_configure".into(),
                    kind: ReplyKind::MutationCompleted,
                    envelope: json!({}),
                    recovery: None,
                });
                operator.cancel_or_acknowledge();
            }
            assert_eq!(
                *dispatcher, before,
                "typing/rendering/operator preparation preserves the model"
            );
            assert_eq!(dispatcher.presentation_revision(), 1);
        }
    }

    #[test]
    fn property_integer_parser_and_advertised_bounds_preserve_exact_i64_values() {
        for (input, constraints, expected) in [
            ("-9223372036854775808", Value::Null, Some(i64::MIN)),
            ("9223372036854775807", Value::Null, Some(i64::MAX)),
            (
                "-9223372036854775808",
                json!({"minimum":i64::MIN,"maximum":i64::MAX}),
                Some(i64::MIN),
            ),
            (
                "9223372036854775807",
                json!({"minimum":i64::MIN,"maximum":i64::MAX}),
                Some(i64::MAX),
            ),
            (
                "-9223372036854775808",
                json!({"minimum":i64::MIN + 1,"maximum":i64::MAX}),
                None,
            ),
            (
                "9223372036854775807",
                json!({"minimum":i64::MIN,"maximum":i64::MAX - 1}),
                None,
            ),
            ("-9223372036854775809", Value::Null, None),
            ("9223372036854775808", Value::Null, None),
            (" -10 ", json!({"minimum":-10,"maximum":10}), Some(-10)),
            ("+10", json!({"minimum":-10,"maximum":10}), Some(10)),
            ("-11", json!({"minimum":-10,"maximum":10}), None),
            ("11", json!({"minimum":-10,"maximum":10}), None),
            ("1.0", Value::Null, None),
            ("1e3", Value::Null, None),
            ("0x10", Value::Null, None),
            ("NaN", Value::Null, None),
            ("Infinity", Value::Null, None),
            ("1 2", Value::Null, None),
            ("", Value::Null, None),
            ("5", json!({}), None),
            ("5", json!({"minimum":0}), None),
            ("5", json!({"minimum":0.0,"maximum":10}), None),
            ("5", json!({"minimum":10,"maximum":0}), None),
        ] {
            let mut model = property_model();
            let target = model.presentation.controls[0].target.clone();
            model.observations.entities.get_mut(&target).unwrap().value["constraints"] =
                constraints;
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            input_control(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                0,
                input,
            );
            let intent = click_editor(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                false,
                0,
                "Configure property",
            );
            assert_eq!(
                intent,
                expected.map(|value| OperatorIntent::ConfigureProperty {
                    target,
                    value: PropertyMutationCandidate::Integer(value),
                }),
                "{input}"
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
            assert_eq!(*dispatcher, before);
        }
    }

    #[test]
    fn property_text_uses_utf8_byte_limit_without_submitting_truncated_prefixes() {
        for (input, valid) in [
            (String::new(), true),
            ("x".repeat(PROPERTY_TEXT_BYTES), true),
            ("x".repeat(PROPERTY_TEXT_BYTES + 1), false),
            ("é".repeat(256), true),
            ("é".repeat(257), false),
            ("🌡".repeat(128), true),
            ("🌡".repeat(129), false),
            ("x".repeat(10_000), false),
            ("🌡".repeat(10_000), false),
        ] {
            let dispatcher = WorkbenchDispatcher::new(property_model(), FakeClient::default());
            let before = (*dispatcher).clone();
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            input_control(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                1,
                &input,
            );
            let stored = renderer
                .editors
                .editor_mut(&dispatcher.presentation.controls[1])
                .unwrap()
                .text
                .clone();
            assert!(stored.len() <= (PROPERTY_TEXT_BYTES + 1) * 4);
            let intent = click_editor(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                false,
                1,
                "Configure property",
            );
            if valid {
                assert_eq!(stored, input);
                assert_eq!(
                    intent,
                    Some(OperatorIntent::ConfigureProperty {
                        target: dispatcher.presentation.controls[1].target.clone(),
                        value: PropertyMutationCandidate::Text(input),
                    })
                );
            } else {
                assert!(
                    stored.len() > PROPERTY_TEXT_BYTES,
                    "overflow stays visible and invalid"
                );
                assert!(intent.is_none());
            }
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
            assert_eq!(*dispatcher, before);
        }
    }

    #[test]
    fn property_type_change_retires_draft_but_observation_updates_do_not() {
        let mut model = property_model();
        let target = model.presentation.controls[0].target.clone();
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        input_control(
            &context,
            &mut renderer,
            &model,
            &OperatorWorkflow::default(),
            0,
            "7",
        );
        model.observations.entities.get_mut(&target).unwrap().value["current"] = json!(8);
        recorder_frame(
            &context,
            &mut renderer,
            &model,
            &OperatorWorkflow::default(),
            false,
            &mut String::new(),
            vec![],
        );
        assert_eq!(
            renderer
                .editors
                .editor_mut(&model.presentation.controls[0])
                .unwrap()
                .text,
            "7",
            "a normal observed-value update cannot overwrite local input"
        );
        model.observations.entities.get_mut(&target).unwrap().value["value_type"] = json!("text");
        recorder_frame(
            &context,
            &mut renderer,
            &model,
            &OperatorWorkflow::default(),
            false,
            &mut String::new(),
            vec![],
        );
        let editor = renderer
            .editors
            .editor_mut(&model.presentation.controls[0])
            .unwrap();
        assert!(editor.text.is_empty());
        assert!(editor.widget_ids.len() <= 1);
    }

    #[test]
    fn property_access_type_and_readiness_fences_block_mutation_and_unsupported_editing() {
        for scenario in [
            "read_only",
            "deployment_only",
            "boolean",
            "number",
            "float",
            "bytes",
            "missing_type",
            "missing",
            "stale",
            "rebuilding",
            "unknown",
            "target_stale",
            "target_rebuilding",
            "observe",
            "unadvertised",
            "disconnected",
            "recovery",
            "quarantine",
            "reconciliation",
            "journal_failure",
            "capacity",
        ] {
            let mut model = property_model();
            let target = model.presentation.controls[0].target.clone();
            match scenario {
                "read_only" | "deployment_only" => {
                    model.observations.entities.get_mut(&target).unwrap().value["access"] =
                        json!(scenario)
                }
                "boolean" | "number" | "float" | "bytes" | "missing_type" => {
                    model.observations.entities.get_mut(&target).unwrap().value["value_type"] =
                        if scenario == "missing_type" {
                            Value::Null
                        } else {
                            json!(scenario)
                        }
                }
                "missing" => {
                    model.observations.entities.remove(&target);
                }
                "stale" => model.observations.freshness = Freshness::Stale,
                "rebuilding" => model.observations.freshness = Freshness::Rebuilding,
                "unknown" => model.observations.freshness = Freshness::Unknown,
                "target_stale" | "target_rebuilding" => {
                    model
                        .observations
                        .entities
                        .get_mut(&target)
                        .unwrap()
                        .freshness = if scenario == "target_stale" {
                        Freshness::Stale
                    } else {
                        Freshness::Rebuilding
                    }
                }
                "observe" => {}
                "unadvertised" => model
                    .hello
                    .as_mut()
                    .unwrap()
                    .operations
                    .retain(|op| op != "property_configure"),
                "disconnected" => model.connection = ConnectionState::Stale,
                "recovery" => model.recovery.mutations.push(ambiguous_record()),
                "quarantine" => model.recovery.quarantined.push(QuarantinedRecoveryRecord {
                    record: ambiguous_record(),
                    reason: RecoveryQuarantineReason::AttachedBootMismatch,
                }),
                "reconciliation" => model
                    .recovery
                    .reconciliation_required
                    .push(ambiguous_record().identity),
                "journal_failure" => model.recovery_problem = Some("journal unavailable".into()),
                "capacity" => {
                    for id in 0..crate::model::MAX_OPERATOR_ACTIONS as u64 {
                        model.track_operator_intent(id).unwrap();
                    }
                }
                _ => unreachable!(),
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            input_control(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                0,
                "5",
            );
            if [
                "read_only",
                "deployment_only",
                "boolean",
                "number",
                "float",
                "bytes",
                "missing_type",
                "missing",
            ]
            .contains(&scenario)
            {
                assert!(
                    renderer
                        .editors
                        .editor_mut(&dispatcher.presentation.controls[0])
                        .unwrap()
                        .text
                        .is_empty(),
                    "{scenario}"
                );
            }
            assert!(
                click_editor(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &OperatorWorkflow::default(),
                    scenario == "observe",
                    0,
                    "Configure property"
                )
                .is_none(),
                "{scenario}"
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
            assert_eq!(*dispatcher, before, "{scenario}");
        }
    }

    #[test]
    fn property_revision_change_blocks_confirm_and_failure_never_retries() {
        for index in [0, 1] {
            let mut model = property_model();
            let target = model.presentation.controls[index].target.clone();
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut operator = OperatorWorkflow::default();
            input_control(&context, &mut renderer, &model, &operator, index, "5");
            let intent = click_editor(
                &context,
                &mut renderer,
                &model,
                &operator,
                false,
                index,
                "Configure property",
            )
            .unwrap();
            operator.begin(&model, intent).unwrap();
            model.observations.entities.get_mut(&target).unwrap().value["revision"] = json!("99");
            let client = FakeClient::default();
            assert_eq!(
                operator.confirm(&model, &client),
                Err(crate::model::OperatorIntentError::DraftStale)
            );
            assert_eq!(client.0.get(), 0);
            operator.cancel_or_acknowledge();
            let intent = click_editor(
                &context,
                &mut renderer,
                &model,
                &operator,
                false,
                index,
                "Configure property",
            )
            .unwrap();
            operator.begin(&model, intent).unwrap();
            operator.confirm(&model, &client).unwrap();
            operator.after_update(&ClientUpdate::Reply {
                command_id: 1,
                msg_id: "1".into(),
                op: "property_configure".into(),
                kind: ReplyKind::MutationFailed,
                envelope: json!({"code":"revision_conflict"}),
                recovery: None,
            });
            assert!(matches!(
                operator.state,
                OperatorWorkflowState::Failed { conflict: true, .. }
            ));
            assert!(
                click_editor(
                    &context,
                    &mut renderer,
                    &model,
                    &operator,
                    false,
                    index,
                    "Configure property"
                )
                .is_none()
            );
            assert_eq!(client.0.get(), 1);
        }
    }

    #[test]
    fn property_replacement_retires_edits_and_preserves_active_workflow() {
        for workflow_state in ["prepared", "submitted", "ambiguous"] {
            let mut model = property_model();
            let next_target = RuntimeRef::ConfigurationProperty {
                owner: ConfigurationOwner::Instrument {
                    instrument: "2".into(),
                },
                property: "limit".into(),
            };
            model.observations.observe(
                next_target.clone(),
                json!({"value_type":"integer", "access":"read_write", "revision":"7"}),
                None,
            );
            let mut dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut operator = OperatorWorkflow::default();
            input_control(&context, &mut renderer, &dispatcher, &operator, 0, "5");
            operator
                .begin(
                    &dispatcher,
                    click_editor(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &OperatorWorkflow::default(),
                        false,
                        0,
                        "Configure property",
                    )
                    .unwrap(),
                )
                .unwrap();
            let old_ids = renderer
                .editors
                .editor_mut(&dispatcher.presentation.controls[0])
                .unwrap()
                .widget_ids
                .clone();
            if workflow_state != "prepared" {
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::Submitted { .. }
                ));
            }
            if workflow_state == "ambiguous" {
                let mut record = ambiguous_record();
                record.op = "property_configure".into();
                record.args = dispatcher.client().unwrap().1.borrow()[0].1.clone();
                operator.after_update(&ClientUpdate::ReconciliationRequired {
                    records: vec![record.clone()],
                });
                dispatcher.apply_client_update(ClientUpdate::RecoveryProjection {
                    active: vec![record.clone()],
                    quarantined: vec![],
                });
                dispatcher.apply_client_update(ClientUpdate::ReconciliationRequired {
                    records: vec![record],
                });
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::Ambiguous { .. }
                ));
            }
            let original = operator.clone();
            let recovery = dispatcher.recovery.clone();
            let calls = dispatcher.client().unwrap().0.get();
            let mut replacement = dispatcher.presentation.clone();
            replacement.controls[0].target = next_target;
            dispatcher
                .dispatch(
                    CallOrigin {
                        caller_id: CallerId(1),
                        call_id: "replace-property".into(),
                    },
                    WorkbenchRequest::UiReplaceDocument {
                        expected: dispatcher.presentation_expectation(),
                        document: serde_json::to_value(replacement).unwrap(),
                    },
                )
                .unwrap();
            let output = context.run_ui(Default::default(), |ui| {
                assert!(
                    renderer
                        .show(
                            ui,
                            &dispatcher.presentation,
                            &dispatcher.observations,
                            dispatcher.presentation_revision(),
                            &mut OperatorControlUi {
                                model: &dispatcher,
                                operator: &operator,
                                observation_only: false,
                                label: &mut String::new(),
                            }
                        )
                        .intent
                        .is_none()
                );
            });
            output.drop_without_applying_deltas();
            for id in old_ids {
                assert!(egui::text_edit::TextEditState::load(&context, id).is_none());
                assert!(!context.memory(|memory| memory.has_focus(id)));
            }
            assert!(
                renderer
                    .editors
                    .editor_mut(&dispatcher.presentation.controls[0])
                    .unwrap()
                    .text
                    .is_empty()
            );
            assert!(
                click_editor(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    0,
                    "Configure property"
                )
                .is_none()
            );
            assert_eq!(operator, original);
            assert_eq!(dispatcher.recovery, recovery);
            assert_eq!(dispatcher.client().unwrap().0.get(), calls);
            if workflow_state == "prepared" {
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert_eq!(
                    dispatcher.client().unwrap().1.borrow()[0].1["target"],
                    json!({"kind":"instrument","id":"1"})
                );
            } else if workflow_state == "ambiguous" {
                assert!(
                    operator
                        .confirm(&dispatcher, dispatcher.client().unwrap())
                        .is_err()
                );
            } else {
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::Submitted { .. }
                ));
            }
        }
    }

    fn input_control(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        operator: &OperatorWorkflow,
        index: usize,
        value: &str,
    ) {
        for _ in 0..2 {
            recorder_frame(
                context,
                renderer,
                model,
                operator,
                false,
                &mut String::new(),
                vec![],
            );
        }
        let id = *renderer
            .editors
            .editor_mut(&model.presentation.controls[index])
            .unwrap()
            .widget_ids
            .iter()
            .next()
            .unwrap();
        let center = context.read_response(id).unwrap().rect.center();
        for pressed in [true, false] {
            recorder_frame(
                context,
                renderer,
                model,
                operator,
                false,
                &mut String::new(),
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
        recorder_frame(
            context,
            renderer,
            model,
            operator,
            false,
            &mut String::new(),
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text(value.to_owned()),
            ],
        );
    }

    fn click_fixed(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        operator: &OperatorWorkflow,
        observe: bool,
        index: usize,
    ) -> Option<OperatorIntent> {
        click_editor(
            context,
            renderer,
            model,
            operator,
            observe,
            index,
            "Configure fixed",
        )
    }

    fn click_editor(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        operator: &OperatorWorkflow,
        observe: bool,
        index: usize,
        button: &str,
    ) -> Option<OperatorIntent> {
        let (_, text) = recorder_frame(
            context,
            renderer,
            model,
            operator,
            observe,
            &mut String::new(),
            vec![],
        );
        let center = text
            .iter()
            .filter(|(text, _)| text == button)
            .nth(index)
            .unwrap()
            .1
            .center();
        let mut intent = None;
        for pressed in [true, false] {
            let (response, _) = recorder_frame(
                context,
                renderer,
                model,
                operator,
                observe,
                &mut String::new(),
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
            if response.intent.is_some() {
                assert!(intent.is_none());
                intent = response.intent;
            }
        }
        intent
    }

    #[test]
    fn reference_controls_keep_independent_keyboard_editors_and_require_confirm() {
        for confirm in [false, true] {
            let mut model = reference_model();
            for control in &mut model.presentation.controls {
                control.confirm = confirm;
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut operator = OperatorWorkflow::default();
            input_control(&context, &mut renderer, &dispatcher, &operator, 0, "123.5");
            input_control(&context, &mut renderer, &dispatcher, &operator, 1, "-2.25");
            let (_, text) = recorder_frame(
                &context,
                &mut renderer,
                &dispatcher,
                &operator,
                false,
                &mut String::new(),
                vec![],
            );
            assert!(labels(&text).contains(&"Current setpoint (cached): 20.25 °C"));
            assert!(labels(&text).contains(&"Current setpoint (cached): 300.5 K"));
            for (index, reference, value, revision) in [(0, "1", 123.5, "7"), (1, "2", -2.25, "12")]
            {
                let intent = click_fixed(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    index,
                )
                .unwrap();
                assert_eq!(
                    intent,
                    OperatorIntent::ConfigureReferenceFixed {
                        reference: reference.into(),
                        value
                    }
                );
                operator.begin(&dispatcher, intent.clone()).unwrap();
                let OperatorWorkflowState::AwaitingConfirmation(prepared) = &operator.state else {
                    panic!()
                };
                assert_eq!(
                    prepared.args,
                    json!({"reference":reference,"expected_revision":revision,"kind":"fixed","value":value})
                );
                assert_eq!(dispatcher.client().unwrap().0.get(), index);
                assert!(
                    click_fixed(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        index
                    )
                    .is_none()
                );
                operator.cancel_or_acknowledge();
                assert_eq!(
                    dispatcher.client().unwrap().0.get(),
                    index,
                    "Cancel sends nothing"
                );
                operator.begin(&dispatcher, intent).unwrap();
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert!(
                    operator
                        .confirm(&dispatcher, dispatcher.client().unwrap())
                        .is_err()
                );
                assert_eq!(dispatcher.client().unwrap().0.get(), index + 1);
                assert_eq!(
                    dispatcher.client().unwrap().1.borrow()[index],
                    (
                        "reference_configure".to_owned(),
                        json!({"reference":reference,"expected_revision":revision,"kind":"fixed","value":value})
                    )
                );
                operator.after_update(&ClientUpdate::Reply {
                    command_id: 1,
                    msg_id: "1".into(),
                    op: "reference_configure".into(),
                    kind: if index == 0 {
                        ReplyKind::MutationFailed
                    } else {
                        ReplyKind::MutationCompleted
                    },
                    envelope: json!({"code":"revision_conflict"}),
                    recovery: None,
                });
                if index == 0 {
                    assert!(matches!(
                        operator.state,
                        OperatorWorkflowState::Failed { conflict: true, .. }
                    ));
                    assert!(
                        click_fixed(
                            &context,
                            &mut renderer,
                            &dispatcher,
                            &operator,
                            false,
                            index
                        )
                        .is_none()
                    );
                    assert_eq!(
                        dispatcher.client().unwrap().0.get(),
                        1,
                        "conflict never retries"
                    );
                }
                operator.cancel_or_acknowledge();
            }
            assert_eq!(*dispatcher, before);
            assert_eq!(dispatcher.presentation_revision(), 1);
        }
    }

    #[test]
    fn reference_invalid_input_and_readiness_fences_never_emit_intent_or_runtime_work() {
        for scenario in [
            "NaN",
            "Infinity",
            "inf",
            "-inf",
            "1e309",
            "invalid",
            "",
            "observe",
            "stale",
            "rebuilding",
            "target_stale",
            "missing",
            "unadvertised",
            "disconnected",
            "recovery",
            "quarantine",
            "reconciliation",
        ] {
            let mut model = reference_model();
            let source = model.presentation.controls[0].target.clone();
            let mut input = "25.5";
            match scenario {
                "observe" => {}
                "stale" => model.observations.freshness = Freshness::Stale,
                "rebuilding" => model.observations.freshness = Freshness::Rebuilding,
                "target_stale" => {
                    model
                        .observations
                        .entities
                        .get_mut(&source)
                        .unwrap()
                        .freshness = Freshness::Stale
                }
                "missing" => {
                    model.observations.entities.remove(&source);
                }
                "unadvertised" => model.hello.as_mut().unwrap().operations.clear(),
                "disconnected" => model.connection = ConnectionState::Stale,
                "recovery" => model.recovery.mutations.push(ambiguous_record()),
                "quarantine" => model.recovery.quarantined.push(QuarantinedRecoveryRecord {
                    record: ambiguous_record(),
                    reason: RecoveryQuarantineReason::AttachedBootMismatch,
                }),
                "reconciliation" => model
                    .recovery
                    .reconciliation_required
                    .push(ambiguous_record().identity),
                _ => input = scenario,
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            input_control(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                0,
                input,
            );
            assert!(
                click_fixed(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &OperatorWorkflow::default(),
                    scenario == "observe",
                    0
                )
                .is_none(),
                "{scenario}"
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
            assert_eq!(*dispatcher, before);
        }
    }

    #[test]
    fn reference_revision_changes_block_confirm_without_retry_or_draft_cross_target_reuse() {
        let mut model = reference_model();
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut operator = OperatorWorkflow::default();
        input_control(&context, &mut renderer, &model, &operator, 0, "-1e308");
        let intent = click_fixed(&context, &mut renderer, &model, &operator, false, 0).unwrap();
        operator.begin(&model, intent).unwrap();
        model
            .observations
            .entities
            .get_mut(&RuntimeRef::Reference {
                reference: "1".into(),
            })
            .unwrap()
            .value["revision"] = json!("8");
        let client = FakeClient::default();
        assert_eq!(
            operator.confirm(&model, &client),
            Err(crate::model::OperatorIntentError::DraftStale)
        );
        assert_eq!(client.0.get(), 0);
        operator.cancel_or_acknowledge();
        // Even if a caller supplies a changed target without a revision change,
        // the control/target key prevents the previous Reference's draft leaking.
        model.presentation.controls[0].target = RuntimeRef::Reference {
            reference: "2".into(),
        };
        recorder_frame(
            &context,
            &mut renderer,
            &model,
            &operator,
            false,
            &mut String::new(),
            vec![],
        );
        assert!(
            renderer
                .editors
                .editor_mut(&model.presentation.controls[0])
                .unwrap()
                .text
                .is_empty()
        );
        assert!(click_fixed(&context, &mut renderer, &model, &operator, false, 0).is_none());
    }

    #[test]
    fn reference_api_replacement_clears_edits_and_keeps_the_original_active_workflow() {
        let mut dispatcher = WorkbenchDispatcher::new(reference_model(), FakeClient::default());
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        let mut operator = OperatorWorkflow::default();
        input_control(&context, &mut renderer, &dispatcher, &operator, 0, "25.5");
        operator
            .begin(
                &dispatcher,
                click_fixed(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &OperatorWorkflow::default(),
                    false,
                    0,
                )
                .unwrap(),
            )
            .unwrap();
        let original = operator.clone();
        let recovery = dispatcher.recovery.clone();
        let mut replacement = dispatcher.presentation.clone();
        replacement.controls[0].target = RuntimeRef::Reference {
            reference: "2".into(),
        };
        dispatcher
            .dispatch(
                CallOrigin {
                    caller_id: CallerId(1),
                    call_id: "replace-reference".into(),
                },
                WorkbenchRequest::UiReplaceDocument {
                    expected: dispatcher.presentation_expectation(),
                    document: serde_json::to_value(replacement).unwrap(),
                },
            )
            .unwrap();
        frame(
            &context,
            &mut renderer,
            &dispatcher,
            dispatcher.presentation_revision(),
            vec![],
        );
        assert_eq!(operator, original);
        assert_eq!(dispatcher.recovery, recovery);
        assert!(
            renderer
                .editors
                .editor_mut(&dispatcher.presentation.controls[0])
                .unwrap()
                .text
                .is_empty()
        );
        assert_eq!(dispatcher.client().unwrap().0.get(), 0);
        operator
            .confirm(&dispatcher, dispatcher.client().unwrap())
            .unwrap();
        assert_eq!(
            dispatcher.client().unwrap().1.borrow()[0].1["reference"],
            "1",
            "prepared intent retains its original target"
        );
    }

    fn recorder_frame(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        operator: &OperatorWorkflow,
        observe: bool,
        label: &mut String,
        events: Vec<egui::Event>,
    ) -> (PresentationResponse, Vec<(String, egui::Rect)>) {
        let mut response = None;
        // Follow the last explicit document epoch (initial fixtures start at 1).
        let revision = renderer.revision.unwrap_or(1);
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
                response = Some(renderer.show(
                    ui,
                    &model.presentation,
                    &model.observations,
                    revision,
                    &mut OperatorControlUi {
                        model,
                        operator,
                        observation_only: observe,
                        label,
                    },
                ));
            },
        );
        (response.unwrap(), painted_text(output))
    }

    fn click_recorder(
        context: &egui::Context,
        renderer: &mut PresentationRenderer,
        model: &WorkbenchModel,
        operator: &OperatorWorkflow,
        observe: bool,
        label: &mut String,
        button: &str,
    ) -> Option<OperatorIntent> {
        let (response, _) =
            recorder_frame(context, renderer, model, operator, observe, label, vec![]);
        assert!(response.intent.is_none());
        let (_, text) = recorder_frame(context, renderer, model, operator, observe, label, vec![]);
        let center = text
            .iter()
            .find(|(text, _)| text == button)
            .unwrap()
            .1
            .center();
        let mut intent = None;
        for pressed in [true, false] {
            let (response, _) = recorder_frame(
                context,
                renderer,
                model,
                operator,
                observe,
                label,
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
            if response.intent.is_some() {
                assert!(intent.is_none());
                intent = response.intent;
            }
        }
        intent
    }

    #[test]
    fn declarative_recorder_click_requires_separate_confirmation_even_when_confirm_is_false() {
        for confirm in [true, false] {
            for stopping in [false, true] {
                let mut model = recorder_model();
                model.presentation.controls[0].confirm = confirm;
                let run_id = json!({"boot_id":"0123456789abcdef0123456789abcdef","run_no":"4"});
                if stopping {
                    model.observations.observe(
                        RuntimeRef::Recorder,
                        json!({"state":"recording","active_run":run_id}),
                        None,
                    );
                }
                let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
                let before = (*dispatcher).clone();
                let context = egui::Context::default();
                let mut renderer = PresentationRenderer::default();
                let mut operator = OperatorWorkflow::default();
                let mut label = "Operator run".to_owned();
                let button = if stopping {
                    "Stop recording"
                } else {
                    "Start recording"
                };
                let (response, text) = recorder_frame(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut label,
                    vec![],
                );
                assert!(response.declarative && response.intent.is_none());
                assert!(labels(&text).contains(&"Dashboard recording"));
                assert!(super::super::controls::has_recorder_controls(
                    &dispatcher.presentation
                ));
                let intent = click_recorder(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut label,
                    button,
                )
                .unwrap();
                operator.begin(&dispatcher, intent).unwrap();
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::AwaitingConfirmation(_)
                ));
                assert_eq!(dispatcher.client().unwrap().0.get(), 0);
                assert!(
                    click_recorder(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        &mut label,
                        button
                    )
                    .is_none()
                );
                operator.cancel_or_acknowledge();
                assert_eq!(dispatcher.client().unwrap().0.get(), 0);
                let intent = click_recorder(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut label,
                    button,
                )
                .unwrap();
                operator.begin(&dispatcher, intent).unwrap();
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::Submitted { .. }
                ));
                let op = if stopping {
                    "recording_stop"
                } else {
                    "recording_start"
                };
                let args = if stopping {
                    json!({"run_id":run_id})
                } else {
                    json!({"label":"Operator run"})
                };
                assert_eq!(
                    *dispatcher.client().unwrap().1.borrow(),
                    vec![(op.to_owned(), args)]
                );
                assert!(
                    operator
                        .confirm(&dispatcher, dispatcher.client().unwrap())
                        .is_err()
                );
                assert!(
                    click_recorder(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        &mut label,
                        button
                    )
                    .is_none()
                );
                for (kind, accepted) in [
                    (ReplyKind::MutationAccepted, true),
                    (ReplyKind::MutationCompleted, false),
                ] {
                    operator.after_update(&ClientUpdate::Reply {
                        command_id: 1,
                        msg_id: "1".into(),
                        op: op.into(),
                        kind,
                        envelope: json!({}),
                        recovery: None,
                    });
                    assert!(if accepted {
                        matches!(operator.state, OperatorWorkflowState::Accepted { .. })
                    } else {
                        matches!(operator.state, OperatorWorkflowState::Completed { .. })
                    });
                }
                assert_eq!(dispatcher.client().unwrap().0.get(), 1);
                assert_eq!(*dispatcher, before);
                assert_eq!(dispatcher.presentation_revision(), 1);
            }
        }
    }

    fn ambiguous_record() -> RecoveryRecord {
        RecoveryRecord {
            boot_id: "boot".into(),
            identity: MutationIdentity {
                scope: "scope".into(),
                seq: 1,
            },
            op: "recording_start".into(),
            args: json!({"label":"Operator run"}),
            admission: KnownAdmission::Ambiguous,
        }
    }

    #[test]
    fn recorder_confirmation_revalidates_fences_and_active_run_before_submission() {
        for change in [
            "stale",
            "rebuilding",
            "unadvertised",
            "recovery",
            "changed_state",
            "changed_run",
        ] {
            let mut model = recorder_model();
            let stopping = change == "changed_run";
            if stopping {
                model.observations.observe(
                    RuntimeRef::Recorder,
                    json!({"state":"recording","active_run":{"boot_id":"boot","run_no":"4"}}),
                    None,
                );
            }
            let mut operator = OperatorWorkflow::default();
            let intent = click_recorder(
                &egui::Context::default(),
                &mut PresentationRenderer::default(),
                &model,
                &operator,
                false,
                &mut "Operator run".into(),
                if stopping {
                    "Stop recording"
                } else {
                    "Start recording"
                },
            )
            .unwrap();
            operator.begin(&model, intent).unwrap();
            match change {
                "stale" => model.observations.freshness = Freshness::Stale,
                "rebuilding" => model.observations.freshness = Freshness::Rebuilding,
                "unadvertised" => model.hello.as_mut().unwrap().operations.clear(),
                "recovery" => model.recovery.mutations.push(ambiguous_record()),
                "changed_state" => {
                    model
                        .observations
                        .entities
                        .get_mut(&RuntimeRef::Recorder)
                        .unwrap()
                        .value["state"] = json!("recording")
                }
                "changed_run" => {
                    model
                        .observations
                        .entities
                        .get_mut(&RuntimeRef::Recorder)
                        .unwrap()
                        .value["active_run"]["run_no"] = json!("5")
                }
                _ => unreachable!(),
            }
            let client = FakeClient::default();
            assert!(operator.confirm(&model, &client).is_err(), "{change}");
            assert_eq!(client.0.get(), 0);
            assert!(matches!(
                operator.state,
                OperatorWorkflowState::AwaitingConfirmation(_)
            ));
        }
    }

    #[test]
    fn recorder_control_references_suppress_legacy_buttons_and_share_the_bounded_label() {
        let mut model = recorder_model();
        let context = egui::Context::default();
        let mut label = "é".repeat(100);
        let before = model.clone();
        recorder_frame(
            &context,
            &mut PresentationRenderer::default(),
            &model,
            &OperatorWorkflow::default(),
            false,
            &mut label,
            vec![],
        );
        assert_eq!(label, "é".repeat(100));
        assert!(label.len() > RECORDING_LABEL_BYTES);
        assert!(label.len() <= (RECORDING_LABEL_BYTES + 1) * 4);
        assert!(
            click_recorder(
                &context,
                &mut PresentationRenderer::default(),
                &model,
                &OperatorWorkflow::default(),
                false,
                &mut label,
                "Start recording",
            )
            .is_none(),
            "overflowing label stays invalid instead of becoming a truncated candidate"
        );
        label = "é".repeat(64);
        assert_eq!(model, before);
        let controls = &super::super::controls::has_recorder_controls;
        assert!(controls(&model.presentation));
        model.presentation.windows[0].tabs.insert(
            0,
            crate::presentation::PresentationTab {
                id: "empty-tab".into(),
                title: "Empty tab".into(),
                panels: vec![],
            },
        );
        let (_, text) = recorder_frame(
            &context,
            &mut PresentationRenderer::default(),
            &model,
            &OperatorWorkflow::default(),
            false,
            &mut label,
            vec![],
        );
        assert!(!labels(&text).contains(&"Start recording"));
        assert!(
            controls(&model.presentation),
            "control on a hidden tab still replaces legacy buttons"
        );
        model.presentation.windows[0].tabs.pop();
        assert!(
            !controls(&model.presentation),
            "an unreferenced definition leaves legacy controls available"
        );
        model.presentation.windows.clear();
        assert!(!controls(&model.presentation));
        // The standard Recorder group uses this same widget and label, without
        // needing a declarative window or a selected signal.
        let output = context.run_ui(Default::default(), |ui| {
            assert!(
                OperatorControlUi {
                    model: &model,
                    operator: &OperatorWorkflow::default(),
                    observation_only: false,
                    label: &mut label
                }
                .show(ui)
                .is_none()
            );
        });
        assert!(labels(&painted_text(output)).contains(&"Start recording"));
        assert_eq!(label, "é".repeat(64));
    }

    #[test]
    fn oversized_recorder_label_stays_bounded_invalid_until_corrected() {
        for oversized in [
            "x".repeat(RECORDING_LABEL_BYTES + 1),
            "é".repeat(RECORDING_LABEL_BYTES / 2 + 1),
            "🌡".repeat(10_000),
        ] {
            let dispatcher = WorkbenchDispatcher::new(recorder_model(), FakeClient::default());
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut label = oversized;
            recorder_frame(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                false,
                &mut label,
                vec![],
            );
            assert!(label.len() > RECORDING_LABEL_BYTES);
            assert!(label.len() <= (RECORDING_LABEL_BYTES + 1) * 4);
            assert!(
                click_recorder(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &OperatorWorkflow::default(),
                    false,
                    &mut label,
                    "Start recording",
                )
                .is_none()
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);

            label = "Corrected label".into();
            let intent = click_recorder(
                &context,
                &mut renderer,
                &dispatcher,
                &OperatorWorkflow::default(),
                false,
                &mut label,
                "Start recording",
            )
            .expect("corrected label becomes a typed Recorder intent");
            assert_eq!(
                intent,
                OperatorIntent::StartRecording {
                    label: "Corrected label".into()
                }
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
        }
    }

    #[test]
    fn oversized_reference_draft_stays_bounded_invalid_until_explicitly_corrected() {
        const REFERENCE_BYTES: usize = 64;
        for oversized in [
            "1".repeat(REFERENCE_BYTES + 1),
            "é".repeat(REFERENCE_BYTES),
            "9".repeat(10_000),
        ] {
            let dispatcher = WorkbenchDispatcher::new(reference_model(), FakeClient::default());
            let context = egui::Context::default();
            let mut renderer = PresentationRenderer::default();
            let mut operator = OperatorWorkflow::default();
            input_control(
                &context,
                &mut renderer,
                &dispatcher,
                &operator,
                0,
                &oversized,
            );
            let editor = renderer
                .editors
                .editor_mut(&dispatcher.presentation.controls[0])
                .unwrap();
            assert!(editor.text.len() > REFERENCE_BYTES);
            assert!(editor.text.len() <= (REFERENCE_BYTES + 1) * 4);
            assert!(
                click_fixed(&context, &mut renderer, &dispatcher, &operator, false, 0).is_none()
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);

            renderer
                .editors
                .editor_mut(&dispatcher.presentation.controls[0])
                .unwrap()
                .text = "25.5".into();
            let intent = click_fixed(&context, &mut renderer, &dispatcher, &operator, false, 0)
                .expect("corrected draft becomes an ordinary typed intent");
            operator.begin(&dispatcher, intent.clone()).unwrap();
            operator.cancel_or_acknowledge();
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
            operator.begin(&dispatcher, intent).unwrap();
            operator
                .confirm(&dispatcher, dispatcher.client().unwrap())
                .unwrap();
            assert_eq!(dispatcher.client().unwrap().0.get(), 1);
        }
    }

    #[test]
    fn distinct_control_ids_keep_independent_drafts_for_the_same_full_target() {
        let mut model = reference_model();
        model.presentation.controls[1].target = model.presentation.controls[0].target.clone();
        model.presentation.validate().unwrap();
        let context = egui::Context::default();
        let mut renderer = PresentationRenderer::default();
        input_control(
            &context,
            &mut renderer,
            &model,
            &OperatorWorkflow::default(),
            0,
            "10.5",
        );
        input_control(
            &context,
            &mut renderer,
            &model,
            &OperatorWorkflow::default(),
            1,
            "20.5",
        );
        assert_eq!(
            renderer
                .editors
                .editor_mut(&model.presentation.controls[0])
                .unwrap()
                .text,
            "10.5"
        );
        assert_eq!(
            renderer
                .editors
                .editor_mut(&model.presentation.controls[1])
                .unwrap()
                .text,
            "20.5"
        );
    }

    #[test]
    fn recorder_buttons_block_observe_nonfresh_unavailable_and_existing_recovery_fences() {
        for scenario in [
            "observe",
            "stale",
            "rebuilding",
            "unknown",
            "stale_recorder",
            "missing_recorder",
            "unadvertised",
            "disconnected",
            "journal_failure",
            "reconciliation",
            "ambiguous",
            "quarantined",
            "blank_label",
            "capacity",
            "disabled_recorder",
            "rebuilding_recorder",
        ] {
            let mut model = recorder_model();
            let mut label = "Operator run".to_owned();
            match scenario {
                "observe" => {} // observation mode
                "stale" => model.observations.freshness = Freshness::Stale,
                "rebuilding" => model.observations.freshness = Freshness::Rebuilding,
                "unknown" => model.observations.freshness = Freshness::Unknown,
                "stale_recorder" => {
                    model
                        .observations
                        .entities
                        .get_mut(&RuntimeRef::Recorder)
                        .unwrap()
                        .freshness = Freshness::Stale
                }
                "missing_recorder" => {
                    model.observations.entities.remove(&RuntimeRef::Recorder);
                }
                "unadvertised" => model.hello.as_mut().unwrap().operations.clear(),
                "disconnected" => model.connection = ConnectionState::Stale,
                "journal_failure" => model.recovery_problem = Some("journal unavailable".into()),
                "reconciliation" => model
                    .recovery
                    .reconciliation_required
                    .push(ambiguous_record().identity),
                "ambiguous" => model.recovery.mutations.push(ambiguous_record()),
                "quarantined" => model.recovery.quarantined.push(QuarantinedRecoveryRecord {
                    record: ambiguous_record(),
                    reason: RecoveryQuarantineReason::AttachedBootMismatch,
                }),
                "blank_label" => label = "   ".into(),
                "capacity" => {
                    for id in 0..crate::model::MAX_OPERATOR_ACTIONS as u64 {
                        model.track_operator_intent(id).unwrap();
                    }
                }
                "disabled_recorder" => {
                    model
                        .observations
                        .entities
                        .get_mut(&RuntimeRef::Recorder)
                        .unwrap()
                        .value = json!({"state":"disabled","active_run":null})
                }
                "rebuilding_recorder" => {
                    model
                        .observations
                        .entities
                        .get_mut(&RuntimeRef::Recorder)
                        .unwrap()
                        .freshness = Freshness::Rebuilding
                }
                _ => unreachable!(),
            }
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            let before = (*dispatcher).clone();
            let button = if ["missing_recorder", "disabled_recorder"].contains(&scenario) {
                "Recorder unavailable"
            } else {
                "Start recording"
            };
            let intent = click_recorder(
                &egui::Context::default(),
                &mut PresentationRenderer::default(),
                &dispatcher,
                &OperatorWorkflow::default(),
                scenario == "observe",
                &mut label,
                button,
            );
            assert!(intent.is_none(), "scenario {scenario}");
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
            assert_eq!(*dispatcher, before);
        }
    }

    #[test]
    fn repeated_control_views_share_drafts_and_only_enter_the_single_confirmed_workflow() {
        for (mut model, button) in [
            (recorder_model(), "Start recording"),
            (reference_model(), "Configure fixed"),
            (controller_model(Some("ready")), "Start"),
            (property_model(), "Configure property"),
        ] {
            model.presentation.controls.truncate(1);
            model.presentation.controls[0].confirm = false;
            let panel = &mut model.presentation.windows[0].tabs[0].panels[0];
            panel.kind = PanelKind::Controls {
                control_ids: vec![model.presentation.controls[0].id.clone()],
            };
            let mut repeated = panel.clone();
            repeated.id = "repeated-panel".into();
            model.presentation.windows[0].tabs[0].panels.push(repeated);
            model.presentation.validate().unwrap();
            // Exercise either view as the source of the one operator gesture.
            for clicked_view in [0, 1] {
                let dispatcher = WorkbenchDispatcher::new(model.clone(), FakeClient::default());
                let before = serde_json::to_vec(&dispatcher.presentation).unwrap();
                let context = egui::Context::default();
                let mut renderer = PresentationRenderer::default();
                let mut operator = OperatorWorkflow::default();
                let mut label = "Repeated run".into();
                recorder_frame(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut label,
                    vec![],
                );
                if let Some(editor) = renderer
                    .editors
                    .editor_mut(&dispatcher.presentation.controls[0])
                {
                    assert_eq!(
                        editor.widget_ids.len(),
                        2,
                        "one draft, two bounded view identities"
                    );
                    editor.text = "5".into();
                }
                let (_, text) = recorder_frame(
                    &context,
                    &mut renderer,
                    &dispatcher,
                    &operator,
                    false,
                    &mut label,
                    vec![],
                );
                let center = text
                    .iter()
                    .filter(|(text, _)| text == button)
                    .nth(clicked_view)
                    .unwrap()
                    .1
                    .center();
                let mut intent = None;
                for pressed in [true, false] {
                    let (response, _) = recorder_frame(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        &mut label,
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
                    if let Some(clicked) = response.intent {
                        assert!(intent.replace(clicked).is_none());
                    }
                }
                let intent = intent.expect("either repeated view emits the same typed intent");
                operator.begin(&dispatcher, intent.clone()).unwrap();
                assert!(matches!(
                    operator.state,
                    OperatorWorkflowState::AwaitingConfirmation(_)
                ));
                assert!(
                    click_recorder(
                        &context,
                        &mut renderer,
                        &dispatcher,
                        &operator,
                        false,
                        &mut label,
                        button
                    )
                    .is_none()
                );
                operator.cancel_or_acknowledge();
                assert_eq!(dispatcher.client().unwrap().0.get(), 0);
                operator.begin(&dispatcher, intent).unwrap();
                operator
                    .confirm(&dispatcher, dispatcher.client().unwrap())
                    .unwrap();
                assert!(
                    operator
                        .confirm(&dispatcher, dispatcher.client().unwrap())
                        .is_err()
                );
                assert_eq!(dispatcher.client().unwrap().0.get(), 1);
                assert_eq!(
                    serde_json::to_vec(&dispatcher.presentation).unwrap(),
                    before
                );
                assert_eq!(dispatcher.presentation_revision(), 1);
            }
        }
    }

    #[test]
    fn unsupported_controls_and_mismatched_targets_remain_inactive() {
        for (kind, target) in [
            (ControlKind::SetReference, RuntimeRef::Recorder),
            (ControlKind::ControllerLifecycle, RuntimeRef::Recorder),
            (ControlKind::ApplyConfiguration, RuntimeRef::Recorder),
            (ControlKind::ReconnectResource, RuntimeRef::Recorder),
            (
                ControlKind::SetReference,
                RuntimeRef::Controller {
                    controller: "1".into(),
                },
            ),
            (
                ControlKind::RecordingLifecycle,
                RuntimeRef::Controller {
                    controller: "1".into(),
                },
            ),
            (
                ControlKind::ApplyConfiguration,
                RuntimeRef::Controller {
                    controller: "1".into(),
                },
            ),
            (
                ControlKind::ReconnectResource,
                RuntimeRef::Controller {
                    controller: "1".into(),
                },
            ),
            (
                ControlKind::ControllerLifecycle,
                RuntimeRef::Reference {
                    reference: "1".into(),
                },
            ),
            (
                ControlKind::RecordingLifecycle,
                RuntimeRef::Reference {
                    reference: "1".into(),
                },
            ),
        ] {
            let mut model = recorder_model();
            model.presentation.controls[0].kind = kind;
            model.presentation.controls[0].target = target;
            model.presentation.validate().unwrap();
            assert!(!super::super::controls::has_recorder_controls(
                &model.presentation
            ));
            let dispatcher = WorkbenchDispatcher::new(model, FakeClient::default());
            assert!(
                click_recorder(
                    &egui::Context::default(),
                    &mut PresentationRenderer::default(),
                    &dispatcher,
                    &OperatorWorkflow::default(),
                    false,
                    &mut "Operator run".into(),
                    "Dashboard recording"
                )
                .is_none()
            );
            assert_eq!(dispatcher.client().unwrap().0.get(), 0);
        }
    }

    #[test]
    fn presentation_replacement_preserves_ambiguous_recorder_workflow_and_recovery() {
        let mut model = recorder_model();
        let mut operator = OperatorWorkflow::default();
        operator
            .begin(
                &model,
                OperatorIntent::StartRecording {
                    label: "Operator run".into(),
                },
            )
            .unwrap();
        let client = FakeClient::default();
        operator.confirm(&model, &client).unwrap();
        let record = ambiguous_record();
        operator.after_update(&ClientUpdate::ReconciliationRequired {
            records: vec![record.clone()],
        });
        assert!(matches!(
            operator.state,
            OperatorWorkflowState::Ambiguous { .. }
        ));
        model.recovery.mutations.push(record.clone());
        model.recovery.reconciliation_required.push(record.identity);
        let mut dispatcher = WorkbenchDispatcher::new(model, client);
        let before_operator = operator.clone();
        let before_recovery = dispatcher.recovery.clone();
        let before_generation = dispatcher.recovery_generation();
        for document in [
            PresentationDocument::empty("removed-controls"),
            recorder_model().presentation,
        ] {
            dispatcher
                .dispatch(
                    CallOrigin {
                        caller_id: CallerId(1),
                        call_id: document.document_id.clone(),
                    },
                    WorkbenchRequest::UiReplaceDocument {
                        expected: dispatcher.presentation_expectation(),
                        document: serde_json::to_value(document).unwrap(),
                    },
                )
                .unwrap();
            let (response, _) = recorder_frame(
                &egui::Context::default(),
                &mut PresentationRenderer::default(),
                &dispatcher,
                &operator,
                false,
                &mut "Operator run".into(),
                vec![],
            );
            assert!(response.intent.is_none());
            assert_eq!(operator, before_operator);
            assert_eq!(dispatcher.recovery, before_recovery);
            assert_eq!(dispatcher.recovery_generation(), before_generation);
        }
        assert!(
            click_recorder(
                &egui::Context::default(),
                &mut PresentationRenderer::default(),
                &dispatcher,
                &operator,
                false,
                &mut "Operator run".into(),
                "Start recording"
            )
            .is_none()
        );
        assert!(
            operator
                .confirm(&dispatcher, dispatcher.client().unwrap())
                .is_err()
        );
        assert_eq!(dispatcher.client().unwrap().0.get(), 1);
        assert_eq!(operator, before_operator);
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

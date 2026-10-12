//! Read-only declarative layout over the committed document and observation cache.

use crate::{
    model::{Freshness, RuntimeObservations},
    presentation::{PanelKind, Plot, PresentationDocument, RuntimeRef, Trace},
};
use eframe::egui::{self, Color32};
use egui_plot::{Line, Plot as EguiPlot, PlotPoints};
use std::collections::{BTreeMap, HashSet};

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

fn trace_points(observations: &RuntimeObservations, trace: &Trace, seconds: f64) -> Vec<[f64; 2]> {
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
        .map(|point| [point.time_seconds, point.value])
        .collect()
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
    view.show(ui, |plot_ui| {
        for trace in plot.traces.iter().filter(|trace| trace.visible) {
            let points = trace_points(observations, trace, plot.time_window_seconds);
            if points.is_empty() {
                continue;
            }
            let color = match trace.style.color.as_str() {
                "cyan" => Color32::from_rgb(0, 200, 220),
                "red" => Color32::LIGHT_RED,
                "green" => Color32::LIGHT_GREEN,
                _ => Color32::from_hex(&trace.style.color).unwrap_or(Color32::LIGHT_BLUE),
            };
            plot_ui.line(
                Line::new(&trace.display_label, PlotPoints::from(points))
                    .id(egui::Id::new(&trace.id))
                    .width(trace.style.width as f32)
                    .color(color),
            );
        }
    });
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
                json!({"value":293.0 + offset,"unit":"K"}),
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
        (declared, text)
    }

    fn labels(text: &[(String, egui::Rect)]) -> Vec<&str> {
        text.iter().map(|(label, _)| label.as_str()).collect()
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
                vec![[40.0, 292.0 + offset], [60.0, 293.0 + offset]]
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

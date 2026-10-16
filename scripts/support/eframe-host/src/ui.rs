//! Read-only drawing over the admitted document and borrowed selected reads.
//! Fixed navigation only; no presentation model, parser, editor state or actions.
use crate::{
    consumer::observation_source::{ReadOnlyProvider, Scalar, SelectedRead},
    plot_ui,
    presentation::{RuntimeRef, v2},
    private_ipc::{SelectedSnapshot, protocol::Header},
    readonly_values::{Freshness, ReadBinding},
};
use eframe::egui;
use egui_plot::{Plot, PlotPoint};
use serde::Serialize;

#[derive(Default)]
pub(super) struct Navigation {
    epoch: Option<(u64, u64)>,
    window: usize,
    tab: usize,
    panel: Option<usize>,
}
#[derive(Default, Serialize)]
pub(super) struct Report {
    pointer: Option<[f32; 2]>,
    pub(super) controls: Vec<crate::controls::Report>,
    pub(super) control_state: Option<crate::private_ipc::controls::Feedback>,
    heading: String,
    panels: Vec<String>,
    scalars: Vec<ScalarReport>,
    plots: Vec<PlotReport>,
    widgets: Vec<WidgetReport>,
    views: Vec<ViewReport>,
}
#[derive(Serialize)]
struct WidgetReport {
    label: String,
    enabled: bool,
    rect: [f32; 4],
}
#[derive(Serialize)]
struct ViewReport {
    id: String,
    occurrence: u64,
    rect: [f32; 4],
    bounds: [[f64; 2]; 2],
    auto: [bool; 2],
    hidden: usize,
}
#[derive(Serialize)]
struct ScalarReport {
    label: String,
    value: String,
    unit: String,
    freshness: String,
    generation: Option<u64>,
}
#[derive(Serialize)]
struct PlotReport {
    time_mode: String,
    time_note: String,
    time_label: String,
    time_tooltip: String,
    label: String,
    points: usize,
    last: Option<[f64; 2]>,
    freshness: String,
    unit: String,
}
fn rect(rect: egui::Rect) -> [f32; 4] {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
}
pub(super) fn widget(report: &mut Report, label: &str, response: &egui::Response) {
    report.widgets.push(WidgetReport {
        label: label.into(),
        enabled: response.enabled(),
        rect: rect(response.rect),
    });
}
fn navigation(
    ui: &mut egui::Ui,
    value: &mut usize,
    titles: &[&str],
    label: &str,
    report: &mut Report,
) -> bool {
    *value = (*value).min(titles.len().saturating_sub(1));
    let previous = *value;
    ui.horizontal(|ui| {
        let response = ui.add_enabled(*value > 0, egui::Button::new(format!("Previous {label}")));
        widget(report, &format!("Previous {label}"), &response);
        if response.clicked() {
            *value -= 1;
        }
        egui::ComboBox::from_id_salt(label)
            .width(240.)
            .truncate()
            .selected_text(titles.get(*value).copied().unwrap_or("Empty"))
            .show_ui(ui, |ui| {
                for (index, title) in titles.iter().enumerate() {
                    ui.selectable_value(value, index, *title);
                }
            });
        ui.label(format!(
            "{} / {}",
            if titles.is_empty() { 0 } else { *value + 1 },
            titles.len()
        ));
        let response = ui.add_enabled(
            *value + 1 < titles.len(),
            egui::Button::new(format!("Next {label}")),
        );
        widget(report, &format!("Next {label}"), &response);
        if response.clicked() {
            *value += 1;
        }
    });
    previous != *value
}
pub(super) fn paint(
    ui: &mut egui::Ui,
    document: &v2::AdmittedV2Document,
    header: Header,
    reads: Option<&SelectedSnapshot<'_>>,
    state: &mut Navigation,
    controls: &mut crate::controls::Controls,
) -> Report {
    controls.begin_frame(header);
    let epoch = (header.identity.incarnation, header.epoch.revision);
    if state.epoch != Some(epoch) {
        *state = Navigation {
            epoch: Some(epoch),
            ..Default::default()
        };
        // Bound stock plot/widget memory to this document. Replaced revision IDs
        // cannot accumulate indefinitely; no retained point or editor model.
        ui.ctx().memory_mut(|memory| memory.data.clear());
    }
    let reads = reads.filter(|reads| {
        reads.epoch() == Some(header.epoch)
            && reads.incarnation() == Some(header.identity.incarnation)
    });
    let mut report = Report {
        pointer: ui
            .ctx()
            .input(|i| i.pointer.latest_pos().map(|p| [p.x, p.y])),
        ..Default::default()
    };
    controls.status(ui, &mut report);
    ui.label(format!(
        "Parent-approved revision {} · {:?}",
        header.epoch.revision,
        reads
            .map(ReadOnlyProvider::display_freshness)
            .unwrap_or(Freshness::Unknown)
    ));
    let titles: Vec<_> = document
        .dto()
        .windows
        .0
        .iter()
        .map(|w| w.title.as_str())
        .collect();
    if navigation(ui, &mut state.window, &titles, "window", &mut report) {
        state.tab = 0;
        state.panel = None;
    }
    let Some(window) = document.dto().windows.0.get(state.window) else {
        ui.heading("No presentation windows");
        return report;
    };
    ui.heading(window.title.as_str());
    report.heading = window.title.as_str().into();
    let titles: Vec<_> = window.tabs.0.iter().map(|t| t.title.as_str()).collect();
    if navigation(ui, &mut state.tab, &titles, "tab", &mut report) {
        state.panel = None;
    }
    let Some(tab) = window.tabs.0.get(state.tab) else {
        return report;
    };
    if state.panel.is_some_and(|p| p >= tab.panels.0.len()) {
        state.panel = None;
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("panel")
            .width(240.)
            .truncate()
            .selected_text(
                state
                    .panel
                    .map_or("All panels", |i| tab.panels.0[i].title.as_str()),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.panel, None, "All panels");
                for (index, panel) in tab.panels.0.iter().enumerate() {
                    ui.selectable_value(&mut state.panel, Some(index), panel.title.as_str());
                }
            });
        let next = ui.add_enabled(!tab.panels.0.is_empty(), egui::Button::new("Next panel"));
        widget(&mut report, "Next panel", &next);
        if next.clicked() {
            state.panel = Some(state.panel.map_or(0, |p| (p + 1) % tab.panels.0.len()));
        }
        let all = ui.button("All panels");
        widget(&mut report, "All panels", &all);
        if all.clicked() {
            state.panel = None;
        }
    });
    ui.small("Drag to pan · Ctrl+wheel/pinch to zoom · double-click to fit · click legend to hide");
    ui.push_id((epoch, window.id.as_str(), tab.id.as_str()), |ui| {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, panel) in tab.panels.0.iter().enumerate() {
                    if state.panel.is_some_and(|selected| selected != index) {
                        continue;
                    }
                    report.panels.push(panel.title.as_str().into());
                    ui.push_id(panel.id.as_str(), |ui| {
                        ui.group(|ui| {
                            ui.set_min_width(ui.available_width());
                            ui.heading(panel.title.as_str());
                            components(
                                ui,
                                &panel.components.0,
                                document,
                                reads,
                                &mut report,
                                controls,
                            );
                        });
                    });
                }
            });
    });
    report
}
// Admission already proves an acyclic graph, depth <= 4 and bounded expanded
// occurrences. Visit every admitted occurrence, with path-specific stock IDs.
fn components(
    ui: &mut egui::Ui,
    ids: &[v2::Id],
    document: &v2::AdmittedV2Document,
    reads: Option<&SelectedSnapshot<'_>>,
    report: &mut Report,
    controls: &mut crate::controls::Controls,
) {
    for (index, id) in ids.iter().enumerate() {
        let component = document
            .dto()
            .components
            .0
            .iter()
            .find(|c| component_id(c) == id)
            .expect("admitted component reference");
        ui.push_id((index, id.as_str()), |ui| {
            match component {
                v2::Component::Group {
                    label,
                    help,
                    layout,
                    children,
                    v1_control,
                    ..
                } => {
                    ui.group(|ui| {
                        ui.label(label.as_str())
                            .on_hover_text(help.as_ref().map_or("", |h| h.as_str()));
                        if let Some(origin) = v1_control
                            && origin.kind == v2::V1ControlKind::SetReference
                            && let v2::RuntimeRef::Reference { reference } = &origin.target
                        {
                            let target = RuntimeRef::Reference {
                                reference: reference.as_str().into(),
                            };
                            scalar(
                                ui,
                                "Observed Runtime Reference",
                                &target,
                                None,
                                None,
                                reads,
                                report,
                            );
                            let fresh = reads.is_some_and(|r| {
                                r.display_freshness() == Freshness::Fresh
                                    && r.selected(&target)
                                        .is_some_and(|r| r.display_freshness() == Freshness::Fresh)
                            });
                            controls.show(ui, id.as_str(), reference.as_str(), fresh, report);
                            return;
                        }
                        match layout {
                            v2::Layout::Row => {
                                ui.horizontal_wrapped(|ui| {
                                    components(ui, &children.0, document, reads, report, controls)
                                });
                            }
                            v2::Layout::Column => {
                                components(ui, &children.0, document, reads, report, controls)
                            }
                        }
                    });
                }
                v2::Component::Status {
                    label,
                    help,
                    source,
                    ..
                } => {
                    help_label(ui, help);
                    match source {
                        v2::StatusSource::Entity { target } => scalar(
                            ui,
                            label.as_str(),
                            &v2::read_target::target(target),
                            None,
                            None,
                            reads,
                            report,
                        ),
                        v2::StatusSource::Data { data_id } => {
                            data_scalar(ui, label.as_str(), data_id, None, document, reads, report)
                        }
                    }
                }
                v2::Component::Indicator {
                    label,
                    help,
                    data_id,
                    precision,
                    ..
                } => {
                    help_label(ui, help);
                    data_scalar(
                        ui,
                        label.as_str(),
                        data_id,
                        *precision,
                        document,
                        reads,
                        report,
                    );
                }
                v2::Component::Plot {
                    label,
                    help,
                    plot_id,
                    ..
                } => {
                    help_label(ui, help);
                    plot(ui, label.as_str(), plot_id, document, reads, report);
                }
                v2::Component::Button {
                    label,
                    help,
                    disabled_reason,
                    ..
                } => {
                    let response =
                        ui.add_enabled(false, egui::Button::new(label.as_str()))
                            .on_disabled_hover_text(
                                disabled_reason.as_ref().or(help.as_ref()).map_or(
                                    "Instrument actions are disabled in read-only mode",
                                    |h| h.as_str(),
                                ),
                            );
                    widget(report, label.as_str(), &response);
                }
                v2::Component::Toggle {
                    label,
                    help,
                    editor_id,
                    ..
                }
                | v2::Component::IntegerInput {
                    label,
                    help,
                    editor_id,
                    ..
                }
                | v2::Component::NumberInput {
                    label,
                    help,
                    editor_id,
                    ..
                }
                | v2::Component::TextInput {
                    label,
                    help,
                    editor_id,
                    ..
                }
                | v2::Component::Select {
                    label,
                    help,
                    editor_id,
                    ..
                } => {
                    // Display only. Temporary widget values are never candidates;
                    // there is no write path or action callback in this adapter.
                    let binding = document
                        .dto()
                        .editor_bindings
                        .0
                        .iter()
                        .find(|e| &e.id == editor_id)
                        .and_then(|e| data(document, &e.data_id));
                    let target = binding.map(|b| v2::read_target::target(&b.target));
                    let read = target
                        .as_ref()
                        .and_then(|t| reads.and_then(|r| r.selected(t)))
                        .filter(|r| {
                            binding
                                .unwrap()
                                .expect_kind
                                .is_none_or(|k| kind_matches(k, *r))
                        });
                    ui.label(label.as_str())
                        .on_hover_text(help.as_ref().map_or("", |h| h.as_str()));
                    let (mut text, unit) = scalar_value(read, None);
                    let response = match component {
                        v2::Component::Toggle { .. } => {
                            let mut checked = read
                                .is_some_and(|r| matches!(r.fields.scalar, Scalar::Boolean(true)));
                            ui.add_enabled(false, egui::Checkbox::new(&mut checked, &text))
                        }
                        v2::Component::IntegerInput { .. }
                            if read
                                .is_some_and(|r| matches!(r.fields.scalar, Scalar::Integer(_))) =>
                        {
                            let Scalar::Integer(mut value) = read.unwrap().fields.scalar else {
                                unreachable!()
                            };
                            ui.add_enabled(
                                false,
                                egui::DragValue::new(&mut value).suffix(format!(" {unit}")),
                            )
                        }
                        v2::Component::NumberInput { .. }
                            if read
                                .is_some_and(|r| matches!(r.fields.scalar, Scalar::Number(_))) =>
                        {
                            let Scalar::Number(mut value) = read.unwrap().fields.scalar else {
                                unreachable!()
                            };
                            ui.add_enabled(
                                false,
                                egui::DragValue::new(&mut value).suffix(format!(" {unit}")),
                            )
                        }
                        v2::Component::Select { .. } => {
                            ui.add_enabled_ui(false, |ui| {
                                egui::ComboBox::from_id_salt("readonly-choice")
                                    .selected_text(&text)
                                    .show_ui(ui, |ui| {
                                        ui.label(&text);
                                    })
                                    .response
                            })
                            .inner
                        }
                        _ => ui.add_enabled(false, egui::TextEdit::singleline(&mut text)),
                    };
                    widget(report, label.as_str(), &response);
                    response.on_disabled_hover_text(
                        "Instrument actions are disabled in read-only mode",
                    );
                    ui.label(freshness(reads, read));
                }
            }
        });
    }
}
fn help_label(ui: &mut egui::Ui, help: &Option<v2::Label>) {
    if let Some(help) = help {
        ui.label(help.as_str());
    }
}
fn component_id(component: &v2::Component) -> &v2::Id {
    match component {
        v2::Component::Group { id, .. }
        | v2::Component::Button { id, .. }
        | v2::Component::Toggle { id, .. }
        | v2::Component::IntegerInput { id, .. }
        | v2::Component::NumberInput { id, .. }
        | v2::Component::TextInput { id, .. }
        | v2::Component::Select { id, .. }
        | v2::Component::Indicator { id, .. }
        | v2::Component::Status { id, .. }
        | v2::Component::Plot { id, .. } => id,
    }
}
fn data<'a>(document: &'a v2::AdmittedV2Document, id: &v2::Id) -> Option<&'a v2::DataBinding> {
    let index = document
        .dto()
        .data_bindings
        .0
        .iter()
        .position(|binding| &binding.id == id)?;
    (!matches!(
        document.summary().data[index],
        v2::StaticAvailability::Disabled(_)
    ))
    .then_some(&document.dto().data_bindings.0[index])
}
fn kind_matches(expected: v2::ScalarKind, read: SelectedRead<'_>) -> bool {
    matches!(
        (expected, read.fields.kind),
        (
            v2::ScalarKind::Boolean,
            Some(crate::readonly_values::ScalarKind::Boolean)
        ) | (
            v2::ScalarKind::Integer,
            Some(crate::readonly_values::ScalarKind::Integer)
        ) | (
            v2::ScalarKind::Number,
            Some(crate::readonly_values::ScalarKind::Number)
        ) | (
            v2::ScalarKind::Text,
            Some(crate::readonly_values::ScalarKind::Text)
        ) | (
            v2::ScalarKind::Enum,
            Some(crate::readonly_values::ScalarKind::Enum)
        )
    )
}
fn data_scalar(
    ui: &mut egui::Ui,
    label: &str,
    id: &v2::Id,
    precision: Option<u8>,
    document: &v2::AdmittedV2Document,
    reads: Option<&SelectedSnapshot<'_>>,
    report: &mut Report,
) {
    let Some(binding) = data(document, id) else {
        ui.label(label);
        ui.label("Unavailable: incompatible data binding");
        report.scalars.push(ScalarReport {
            label: label.into(),
            value: "Unavailable".into(),
            unit: String::new(),
            freshness: "Unavailable — incompatible data binding".into(),
            generation: None,
        });
        return;
    };
    scalar(
        ui,
        label,
        &v2::read_target::target(&binding.target),
        binding.expect_kind,
        precision,
        reads,
        report,
    );
}
fn freshness(reads: Option<&SelectedSnapshot<'_>>, read: Option<SelectedRead<'_>>) -> String {
    let Some(read) = read else {
        return "Unavailable — no selected observation".into();
    };
    if let Scalar::Absent(reason) = read.fields.scalar {
        return format!("Unavailable — {}", reason.label());
    }
    let overall = reads
        .map(ReadOnlyProvider::display_freshness)
        .unwrap_or(Freshness::Unknown);
    let state = match overall {
        Freshness::Unknown | Freshness::Rebuilding => overall,
        Freshness::Stale if read.display_freshness() == Freshness::Fresh => Freshness::Stale,
        _ => read.display_freshness(),
    };
    format!("{state:?}")
}
fn scalar(
    ui: &mut egui::Ui,
    label: &str,
    target: &RuntimeRef,
    expect: Option<v2::ScalarKind>,
    precision: Option<u8>,
    reads: Option<&SelectedSnapshot<'_>>,
    report: &mut Report,
) {
    let read = reads
        .and_then(|reads| reads.selected(target))
        .filter(|read| expect.is_none_or(|kind| kind_matches(kind, *read)));
    let state = freshness(reads, read);
    let (value, unit) = scalar_value(read, precision);
    ui.label(label);
    ui.label(egui::RichText::new(format!("{value} {unit}")).size(25.));
    ui.colored_label(
        if state == "Fresh" {
            egui::Color32::LIGHT_GREEN
        } else {
            egui::Color32::YELLOW
        },
        &state,
    );
    report.scalars.push(ScalarReport {
        label: label.into(),
        value,
        unit,
        freshness: state,
        generation: read.and_then(|r| r.fields.origin.generation),
    });
}
fn scalar_value(read: Option<SelectedRead<'_>>, precision: Option<u8>) -> (String, String) {
    read.map_or_else(
        || ("Unavailable".into(), String::new()),
        |read| {
            let value = match read.fields.scalar {
                Scalar::Number(v) => format!(
                    "{v:.precision$}",
                    precision = usize::from(precision.unwrap_or(3))
                ),
                Scalar::Integer(v) => v.to_string(),
                Scalar::Boolean(v) => v.to_string(),
                Scalar::Text(v) | Scalar::Enum(v) => v.into(),
                Scalar::Absent(_) => "Unavailable".into(),
            };
            (value, read.fields.unit.map_or("", |u| u.symbol).into())
        },
    )
}
fn plot(
    ui: &mut egui::Ui,
    label: &str,
    id: &v2::Id,
    document: &v2::AdmittedV2Document,
    reads: Option<&SelectedSnapshot<'_>>,
    report: &mut Report,
) {
    let Some(plot) = document.dto().plots.0.iter().find(|plot| &plot.id == id) else {
        return;
    };
    ui.label(label);
    let anchor = reads.and_then(|r| r.time_anchor.as_ref()).filter(|a| {
        // Do not apply a new boot mapping to any retained old-boot trace.
        plot.traces.0.iter().filter(|t| t.visible).all(|t| {
            data(document, &t.data_id)
                .map(|b| v2::read_target::target(&b.target))
                .and_then(|target| reads.and_then(|r| r.selected(&target)))
                .is_none_or(|r| {
                    r.fields.trace.len() == 0 || r.fields.origin.boot == Some(a.boot_id.as_str())
                })
        })
    });
    let axis = crate::time_axis::Axis::new(plot.axes.time_mode, anchor);
    ui.label(axis.note);
    let mut owned = Vec::with_capacity(plot.traces.0.len());
    let mut states = Vec::with_capacity(plot.traces.0.len());
    let mut units = Vec::with_capacity(plot.traces.0.len());
    for trace in plot.traces.0.iter().filter(|trace| trace.visible) {
        let binding = data(document, &trace.data_id);
        let target = binding.map(|b| v2::read_target::target(&b.target));
        let read = target
            .as_ref()
            .and_then(|target| reads.and_then(|r| r.selected(target)))
            .filter(|read| {
                binding
                    .unwrap()
                    .expect_kind
                    .is_none_or(|kind| kind_matches(kind, *read))
            });
        let compatible = read.is_some_and(|read| {
            matches!(read.fields.scalar, Scalar::Number(_) | Scalar::Integer(_))
                && ReadBinding::for_target(read.target) == Some(ReadBinding::SignalSample)
                && trace.display_unit.as_ref().is_none_or(|requested| {
                    read.fields.unit.is_some_and(|u| {
                        requested.as_str() == u.symbol || requested.as_str() == u.id
                    })
                })
        });
        let points: Vec<PlotPoint> = read.filter(|_| compatible).map_or_else(Vec::new, |read| {
            let last = read
                .fields
                .trace
                .last()
                .map_or(0., |point| point.time_seconds);
            read.fields
                .trace
                .points()
                .filter(|p| p.time_seconds >= last - plot.time_window_seconds.0)
                .map(|p| PlotPoint::new(p.time_seconds, p.value))
                .collect()
        });
        let state = if compatible {
            freshness(reads, read)
        } else {
            "Unavailable — incompatible or absent numeric sample/unit".into()
        };
        let unit = read
            .and_then(|r| r.fields.unit)
            .map_or("", |u| u.symbol)
            .to_owned();
        report.plots.push(PlotReport {
            time_mode: format!("{:?}", axis.mode),
            time_note: axis.note.into(),
            time_label: points.last().map_or_else(String::new, |p| {
                axis.tick(p.x, &(p.x - plot.time_window_seconds.0..=p.x))
            }),
            time_tooltip: points
                .last()
                .map_or_else(String::new, |p| axis.tooltip(p.x)),
            label: trace.display_label.as_str().into(),
            points: points.len(),
            last: points.last().map(|p| [p.x, p.y]),
            freshness: state.clone(),
            unit: unit.clone(),
        });
        units.push(unit);
        states.push(state);
        owned.push((trace, points));
    }
    let curves: Vec<_> = owned
        .iter()
        .map(|(trace, points)| plot_ui::Curve {
            id: ui.id().with(trace.id.as_str()),
            label: trace.display_label.as_str(),
            points,
            width: trace.style.width.0 as f32,
            color: color(trace.style.color.as_str()),
        })
        .collect();
    ui.label(plot.title.as_str());
    let mut axis_units = Vec::new();
    for unit in &units {
        if !unit.is_empty() && !axis_units.contains(&unit.as_str()) {
            axis_units.push(unit.as_str());
        }
    }
    let y_label = if axis_units.is_empty() {
        "Value".into()
    } else {
        format!("Value ({})", axis_units.join(", "))
    };
    let plot_id = ui.id().with(id.as_str());
    let mut view = Plot::new(plot_id)
        .id(plot_id)
        .x_axis_label(axis.label())
        .x_axis_formatter(|mark, range| axis.tick(mark.value, range))
        .y_axis_label(y_label)
        .height(245.)
        .legend(egui_plot::Legend::default());
    if let Some(min) = plot.axes.y_min {
        view = view.include_y(min.0);
    }
    if let Some(max) = plot.axes.y_max {
        view = view.include_y(max.0);
    }
    plot_ui::show(ui, view, &curves, |name, index, point| {
        Some(format!(
            "{name}: {:.3} {} at {} · {}",
            point.y,
            units[index],
            axis.tooltip(point.x),
            states[index]
        ))
    });
    if let Some(memory) = egui_plot::PlotMemory::load(ui.ctx(), plot_id) {
        report.views.push(ViewReport {
            id: id.as_str().into(),
            occurrence: plot_id.value(),
            rect: rect(*memory.transform().frame()),
            bounds: [memory.bounds().min(), memory.bounds().max()],
            auto: [memory.auto_bounds.x, memory.auto_bounds.y],
            hidden: memory.hidden_items.len(),
        });
    }
    for (trace, state) in owned.iter().map(|(t, _)| *t).zip(states) {
        ui.label(trace.display_label.as_str());
        ui.label(state);
    }
}

fn color(value: &str) -> Option<egui::Color32> {
    match value {
        "red" => Some(egui::Color32::LIGHT_RED),
        "green" => Some(egui::Color32::LIGHT_GREEN),
        "blue" | "cyan" => Some(egui::Color32::LIGHT_BLUE),
        _ => egui::Color32::from_hex(value).ok(),
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;

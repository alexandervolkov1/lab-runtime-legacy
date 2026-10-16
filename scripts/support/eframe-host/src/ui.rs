//! Minimal drawing adapter over an admitted document and borrowed selected reads.
//! Fixed navigation/page state only; no presentation model, parser or actions.
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
    page: usize,
}
#[derive(Default, Serialize)]
pub(super) struct Report {
    heading: String,
    panels: Vec<String>,
    scalars: Vec<ScalarReport>,
    plots: Vec<PlotReport>,
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
    label: String,
    points: usize,
    last: Option<[f64; 2]>,
    freshness: String,
}

fn navigation(ui: &mut egui::Ui, value: &mut usize, count: usize, label: &str) {
    *value = (*value).min(count.saturating_sub(1));
    ui.horizontal(|ui| {
        if ui
            .add_enabled(*value > 0, egui::Button::new(format!("Previous {label}")))
            .clicked()
        {
            *value -= 1;
        }
        ui.label(format!(
            "{label} {} / {count}",
            if count == 0 { 0 } else { *value + 1 }
        ));
        if ui
            .add_enabled(
                *value + 1 < count,
                egui::Button::new(format!("Next {label}")),
            )
            .clicked()
        {
            *value += 1;
        }
    });
}
pub(super) fn paint(
    ui: &mut egui::Ui,
    document: &v2::AdmittedV2Document,
    header: Header,
    reads: Option<&SelectedSnapshot<'_>>,
    state: &mut Navigation,
) -> Report {
    let epoch = (header.identity.incarnation, header.epoch.revision);
    if state.epoch != Some(epoch) {
        *state = Navigation {
            epoch: Some(epoch),
            ..Default::default()
        };
        // Bound stock plot/widget memory to this document; IDs from replaced
        // documents cannot accumulate indefinitely in egui's temporary map.
        ui.ctx().memory_mut(|memory| memory.data.clear());
    }
    let reads = reads.filter(|reads| {
        reads.epoch() == Some(header.epoch)
            && reads.incarnation() == Some(header.identity.incarnation)
    });
    let mut report = Report::default();
    ui.label(format!(
        "Parent-approved revision {} · read-only · {:?}",
        header.epoch.revision,
        reads
            .map(ReadOnlyProvider::display_freshness)
            .unwrap_or(Freshness::Unknown)
    ));
    navigation(
        ui,
        &mut state.window,
        document.dto().windows.0.len(),
        "window",
    );
    let Some(window) = document.dto().windows.0.get(state.window) else {
        ui.heading("No presentation windows");
        return report;
    };
    ui.heading(window.title.as_str());
    report.heading = window.title.as_str().into();
    navigation(ui, &mut state.tab, window.tabs.0.len(), "tab");
    let Some(tab) = window.tabs.0.get(state.tab) else {
        return report;
    };
    ui.label(tab.title.as_str());
    navigation(
        ui,
        &mut state.page,
        tab.panels.0.len().div_ceil(2),
        "panel page",
    );
    egui::ScrollArea::vertical().show(ui, |ui| {
        // Two panels and at most four roots each. v1 conversion produces one
        // Status/Plot root per corresponding panel. Control groups stay inert.
        for panel in tab.panels.0.iter().skip(state.page * 2).take(2) {
            report.panels.push(panel.title.as_str().into());
            ui.group(|ui| {
                ui.heading(panel.title.as_str());
                for id in panel.components.0.iter().take(4) {
                    let Some(component) = document
                        .dto()
                        .components
                        .0
                        .iter()
                        .find(|c| component_id(c) == id)
                    else {
                        continue;
                    };
                    ui.push_id(
                        (
                            header.identity.incarnation,
                            header.epoch.revision,
                            panel.id.as_str(),
                            id.as_str(),
                        ),
                        |ui| match component {
                            v2::Component::Status { label, source, .. } => match source {
                                v2::StatusSource::Entity { target } => scalar(
                                    ui,
                                    label.as_str(),
                                    &v2::read_target::target(target),
                                    None,
                                    reads,
                                    &mut report,
                                ),
                                v2::StatusSource::Data { data_id } => data_scalar(
                                    ui,
                                    label.as_str(),
                                    data_id,
                                    document,
                                    reads,
                                    &mut report,
                                ),
                            },
                            v2::Component::Indicator { label, data_id, .. } => data_scalar(
                                ui,
                                label.as_str(),
                                data_id,
                                document,
                                reads,
                                &mut report,
                            ),
                            v2::Component::Plot { label, plot_id, .. } => {
                                plot(ui, label.as_str(), plot_id, document, reads, &mut report)
                            }
                            _ => {
                                ui.add_enabled(
                                    false,
                                    egui::Label::new("Controls unavailable in read-only host"),
                                );
                            }
                        },
                    );
                }
                if panel.components.0.len() > 4 {
                    ui.label("Additional roots are outside this minimal display adapter");
                }
            });
        }
    });
    report
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
    document: &v2::AdmittedV2Document,
    reads: Option<&SelectedSnapshot<'_>>,
    report: &mut Report,
) {
    let Some(binding) = data(document, id) else {
        ui.label("Unavailable: incompatible data binding");
        return;
    };
    scalar(
        ui,
        label,
        &v2::read_target::target(&binding.target),
        binding.expect_kind,
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
    reads: Option<&SelectedSnapshot<'_>>,
    report: &mut Report,
) {
    let read = reads
        .and_then(|reads| reads.selected(target))
        .filter(|read| expect.is_none_or(|kind| kind_matches(kind, *read)));
    let state = freshness(reads, read);
    let (value, unit) = read.map_or_else(
        || ("Unavailable".into(), String::new()),
        |read| {
            let value = match read.fields.scalar {
                Scalar::Number(v) => format!("{v:.3}"),
                Scalar::Integer(v) => v.to_string(),
                Scalar::Boolean(v) => v.to_string(),
                Scalar::Text(v) | Scalar::Enum(v) => v.into(),
                Scalar::Absent(_) => "Unavailable".into(),
            };
            (value, read.fields.unit.map_or("", |u| u.symbol).into())
        },
    );
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
    let mut owned = Vec::with_capacity(plot.traces.0.len());
    let mut states = Vec::with_capacity(plot.traces.0.len());
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
        report.plots.push(PlotReport {
            label: trace.display_label.as_str().into(),
            points: points.len(),
            last: points.last().map(|p| [p.x, p.y]),
            freshness: state.clone(),
        });
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
            color: Some(match trace.style.color.as_str() {
                "green" => egui::Color32::LIGHT_GREEN,
                "red" => egui::Color32::LIGHT_RED,
                _ => egui::Color32::LIGHT_BLUE,
            }),
        })
        .collect();
    let mut view = Plot::new(ui.id().with(id.as_str()))
        .height(245.)
        .legend(egui_plot::Legend::default());
    if let Some(min) = plot.axes.y_min {
        view = view.include_y(min.0);
    }
    if let Some(max) = plot.axes.y_max {
        view = view.include_y(max.0);
    }
    plot_ui::show(ui, view, &curves, |name, index, point| {
        Some(format!("{name}: {:.3} · {}", point.y, states[index]))
    });
    for state in states {
        ui.label(state);
    }
}

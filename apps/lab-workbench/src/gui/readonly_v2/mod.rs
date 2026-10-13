//! Isolated read-only rendering. No active presentation consumer or input drafts.
#![allow(
    dead_code,
    reason = "DGUI-4C7A is isolated pending activation qualification"
)]

#[cfg(test)]
mod fixtures;
mod read;
#[cfg(test)]
mod tests;

use crate::{model::RuntimeObservations, presentation::v2};
use eframe::egui;

/// Identity supplied by the presentation owner; never inferred from observations.
/// The owner must advance revision on replacement, even for the same document ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PresentationEpoch {
    pub(crate) owner: [u8; 16],
    pub(crate) revision: u64,
}

/// Borrowed semantic identity; hashing it for egui does not replace this address.
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

/// Finite frame counters only. No expanded tree, values, identities or drafts persist.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderReport {
    pub(crate) windows: usize,
    pub(crate) tabs: usize,
    pub(crate) panels: usize,
    pub(crate) occurrences: usize,
    pub(crate) placeholders: usize,
    pub(crate) statically_disabled: usize,
    pub(crate) maximum_path: usize,
}

/// Activation is blocked until egui-owned retained capacities are qualified or
/// guarded at runtime. An offline VisualReservation alone cannot establish that.
pub(crate) const ACTIVATION_QUALIFIED: bool = false;

/// Renders all logical windows/tabs in document order as a read-only preview.
/// The caller supplies the Ui (and its viewport/scroll policy). This foundation
/// keeps no tab selection, plot state, editor state or second observation cache.
/// Traversal visits at most the admitted 20,480 occurrences with four path slots.
pub(crate) fn show(
    ui: &mut egui::Ui,
    document: &v2::AdmittedV2Document,
    observations: &RuntimeObservations,
    epoch: PresentationEpoch,
) -> RenderReport {
    let mut frame = Frame {
        document,
        observations,
        epoch,
        report: RenderReport::default(),
    };
    ui.push_id(
        (
            "readonly-v2-arrangement",
            epoch,
            document.dto().document_id.as_str(),
        ),
        |ui| {
            for (w, window) in document.dto().windows.0.iter().enumerate() {
                frame.report.windows += 1;
                label(ui, window.title.as_str());
                for (t, tab) in window.tabs.0.iter().enumerate() {
                    frame.report.tabs += 1;
                    label(ui, tab.title.as_str());
                    for (p, panel) in tab.panels.0.iter().enumerate() {
                        frame.report.panels += 1;
                        label(ui, panel.title.as_str());
                        for root in 0..panel.components.0.len() {
                            frame.component(ui, [w, t, p, root], &mut [0; 4], 0);
                        }
                    }
                }
            }
        },
    );
    debug_assert_eq!(
        frame.report.occurrences,
        document.summary().expanded_components
    );
    frame.report
}

fn label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    // Labels must not create egui text-selection state, even if caller style
    // enables selectable labels. No focus, click or keyboard response is consumed.
    ui.add(
        egui::Label::new(text)
            .selectable(false)
            .sense(egui::Sense::hover()),
    );
}

struct Frame<'a> {
    document: &'a v2::AdmittedV2Document,
    observations: &'a RuntimeObservations,
    epoch: PresentationEpoch,
    report: RenderReport,
}
impl Frame<'_> {
    fn component(
        &mut self,
        ui: &mut egui::Ui,
        address: [usize; 4],
        path: &mut [usize; 4],
        depth: usize,
    ) {
        let [w, t, p, root] = address;
        let view = self
            .document
            .source()
            .occurrence(w, t, p, root, &path[..depth])
            .expect("admitted occurrence and bounded traversal");
        self.report.occurrences += 1;
        self.report.maximum_path = self.report.maximum_path.max(depth);
        let identity = OccurrenceIdentity {
            epoch: self.epoch,
            occurrence: view.identity,
        };
        if matches!(view.availability, v2::StaticAvailability::Disabled(_)) {
            self.report.statically_disabled += 1;
        }
        // Use the admitted occurrence's availability, including ancestor v1
        // ceilings. Never substitute definition-only action/editor availability.
        // Pass the hash as a numeric salt: egui debug IDs otherwise recursively
        // format their complete provenance into every descendant's debug string.
        // This reduces that overhead, but does not qualify its global ID tables.
        ui.push_id(identity.widget_id().value(), |ui| {
            ui.vertical(|ui| match view.component {
                v2::Component::Group {
                    label: caption,
                    help,
                    layout,
                    children,
                    ..
                } => {
                    label(ui, caption.as_str());
                    if let Some(help) = help {
                        label(ui, help.as_str());
                    }
                    let mut children_ui = |ui: &mut egui::Ui| {
                        for position in 0..children.0.len() {
                            path[depth] = position;
                            self.component(ui, address, path, depth + 1);
                        }
                    };
                    match layout {
                        v2::Layout::Row => {
                            ui.horizontal_top(children_ui);
                        }
                        v2::Layout::Column => {
                            ui.vertical(&mut children_ui);
                        }
                    }
                }
                v2::Component::Indicator {
                    label: caption,
                    help,
                    data_id,
                    precision,
                    ..
                } => {
                    label(ui, caption.as_str());
                    if let Some(help) = help {
                        label(ui, help.as_str());
                    }
                    read::data(
                        ui,
                        self.document,
                        self.observations,
                        data_id,
                        view.availability,
                        *precision,
                    );
                }
                v2::Component::Status {
                    label: caption,
                    help,
                    source,
                    ..
                } => {
                    label(ui, caption.as_str());
                    if let Some(help) = help {
                        label(ui, help.as_str());
                    }
                    match source {
                        v2::StatusSource::Data { data_id } => read::data(
                            ui,
                            self.document,
                            self.observations,
                            data_id,
                            view.availability,
                            None,
                        ),
                        v2::StatusSource::Entity { target } => {
                            read::entity(ui, self.observations, target)
                        }
                    }
                }
                unsupported => {
                    self.report.placeholders += 1;
                    let (caption, kind) = match unsupported {
                        v2::Component::Button { label, .. } => (label, "Button"),
                        v2::Component::Toggle { label, .. } => (label, "Toggle"),
                        v2::Component::Select { label, .. } => (label, "Select"),
                        v2::Component::IntegerInput { label, .. } => (label, "IntegerInput"),
                        v2::Component::NumberInput { label, .. } => (label, "NumberInput"),
                        v2::Component::TextInput { label, .. } => (label, "TextInput"),
                        v2::Component::Plot { label, .. } => (label, "Plot"),
                        _ => unreachable!("read-only components handled above"),
                    };
                    label(ui, caption.as_str());
                    label(
                        ui,
                        format!("{kind}: unsupported in read-only preview (inert)"),
                    );
                    if let v2::StaticAvailability::Disabled(reason) = view.availability {
                        label(ui, format!("Disabled: {reason:?}"));
                    }
                }
            });
        });
    }
}

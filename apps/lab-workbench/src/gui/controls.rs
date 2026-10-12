//! Read-only controls returning typed intent to the existing operator workflow.

use crate::{
    model::{
        ControllerLifecycleIntent, OperatorIntent, OperatorIntentError, OperatorWorkflow,
        RECORDING_LABEL_BYTES, WorkbenchModel, prepare_operator_intent,
    },
    presentation::{
        Control, ControlKind, MAX_CONTROLS, PanelKind, PresentationDocument, RuntimeRef,
    },
};
use eframe::egui;
use std::collections::{BTreeMap, HashSet};

// Match the ordinary fixed-value editor's text bound, without restricting f64 range.
const REFERENCE_VALUE_BYTES: usize = 64;

pub(super) fn reference_control_supported(control: &Control) -> bool {
    control.kind == ControlKind::SetReference
        && matches!(control.target, RuntimeRef::Reference { .. })
}

#[derive(Default)]
pub(super) struct ReferenceEditor {
    pub(super) text: String,
    // A control can be referenced by multiple panels. Track every current view
    // instance (bounded by the document's panel/reference counts), not just the
    // last rendered editor, so retirement also removes their undo/focus state.
    pub(super) widget_ids: HashSet<egui::Id>,
}

/// At most 64 current control/target drafts, each bounded to 64 UTF-8 bytes.
/// These are GUI input, not observations, document state or mutation recovery.
#[derive(Default)]
pub(super) struct ReferenceEditors {
    drafts: BTreeMap<(String, RuntimeRef), ReferenceEditor>,
}

impl ReferenceEditors {
    pub(super) fn reconcile(
        &mut self,
        context: &egui::Context,
        document: &PresentationDocument,
        reset: bool,
    ) {
        self.drafts.retain(|(id, target), editor| {
            let keep = !reset
                && document
                    .controls
                    .iter()
                    .filter(|control| reference_control_supported(control))
                    .take(MAX_CONTROLS)
                    .any(|control| &control.id == id && &control.target == target);
            if !keep {
                // Retire egui's undo/focus state with the draft so replacement
                // cannot retain an old edit or deliver it to a different target.
                for &id in &editor.widget_ids {
                    context.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(id));
                    context.memory_mut(|memory| memory.surrender_focus(id));
                }
            }
            keep
        });
        for control in document
            .controls
            .iter()
            .filter(|control| reference_control_supported(control))
            .take(MAX_CONTROLS)
        {
            self.drafts
                .entry((control.id.clone(), control.target.clone()))
                .or_default();
        }
    }

    pub(super) fn editor_mut(&mut self, control: &Control) -> Option<&mut ReferenceEditor> {
        self.drafts
            .get_mut(&(control.id.clone(), control.target.clone()))
    }
}

pub(super) fn recorder_control_supported(control: &Control) -> bool {
    control.kind == ControlKind::RecordingLifecycle && control.target == RuntimeRef::Recorder
}

pub(super) fn controller_control_supported(control: &Control) -> bool {
    control.kind == ControlKind::ControllerLifecycle
        && matches!(control.target, RuntimeRef::Controller { .. })
}

/// Locate a referenced lifecycle control for this target, including hidden tabs.
/// Ordinary buttons stay suppressed across tab switches; the location explains
/// where the operator can find the declarative action without duplicating it.
pub(super) fn controller_control_location(
    document: &PresentationDocument,
    controller: &str,
) -> Option<String> {
    for window in &document.windows {
        for tab in &window.tabs {
            for panel in &tab.panels {
                let PanelKind::Controls { control_ids } = &panel.kind else {
                    continue;
                };
                if document.controls.iter().any(|control| {
                    controller_control_supported(control)
                        && matches!(&control.target, RuntimeRef::Controller { controller: id } if id == controller)
                        && control_ids.contains(&control.id)
                }) {
                    return Some(format!("{} / {} / {}", window.title, tab.title, panel.title));
                }
            }
        }
    }
    None
}

/// Referenced controls in any tab replace the legacy Recorder buttons, even on a hidden tab.
pub(super) fn has_recorder_controls(document: &PresentationDocument) -> bool {
    document
        .windows
        .iter()
        .flat_map(|window| &window.tabs)
        .any(|tab| {
            tab.panels.iter().any(|panel| {
                let PanelKind::Controls { control_ids } = &panel.kind else {
                    return false;
                };
                document.controls.iter().any(|control| {
                    control_ids.contains(&control.id) && recorder_control_supported(control)
                })
            })
        })
}

/// Frame-local read-only authority view plus the shared, client-local label editor.
/// No client handle or submission/confirmation capability enters this renderer.
pub(super) struct OperatorControlUi<'a> {
    pub(super) model: &'a WorkbenchModel,
    pub(super) operator: &'a OperatorWorkflow,
    pub(super) observation_only: bool,
    pub(super) label: &'a mut String,
}

impl OperatorControlUi<'_> {
    pub(super) fn show_panel(
        &mut self,
        ui: &mut egui::Ui,
        controls: &[Control],
        control_ids: &[String],
        editors: &mut ReferenceEditors,
    ) -> Option<OperatorIntent> {
        if control_ids.is_empty() {
            ui.label("No controls configured.");
        }
        let mut intent = None;
        for control in control_ids
            .iter()
            .filter_map(|id| controls.iter().find(|control| &control.id == id))
        {
            ui.push_id((&control.id, &control.target), |ui| {
                if recorder_control_supported(control) {
                    ui.label(&control.label);
                    let clicked = self.show(ui);
                    if intent.is_none() {
                        intent = clicked;
                    }
                } else if reference_control_supported(control) {
                    ui.label(&control.label);
                    if let Some(editor) = editors.editor_mut(control) {
                        let clicked = self.show_reference(ui, &control.target, editor);
                        if intent.is_none() {
                            intent = clicked;
                        }
                    }
                } else if controller_control_supported(control) {
                    ui.label(&control.label);
                    let clicked = self.show_controller(ui, &control.target);
                    if intent.is_none() {
                        intent = clicked;
                    }
                } else {
                    ui.add_enabled(false, egui::Button::new(&control.label))
                        .on_disabled_hover_text("This control kind or target is not supported.");
                }
            });
        }
        intent
    }

    fn candidate(&self) -> Option<OperatorIntent> {
        let observation = self
            .model
            .observations
            .entities
            .get(&RuntimeRef::Recorder)?;
        if observation
            .value
            .get("active_run")
            .is_some_and(|run| !run.is_null())
        {
            Some(OperatorIntent::StopRecording)
        } else if observation
            .value
            .get("state")
            .and_then(serde_json::Value::as_str)
            == Some("idle")
        {
            Some(OperatorIntent::StartRecording {
                label: self.label.clone(),
            })
        } else {
            None
        }
    }

    fn eligibility(&self, intent: OperatorIntent) -> Result<OperatorIntent, OperatorIntentError> {
        if self.observation_only {
            return Err(OperatorIntentError::InvalidCandidate(
                "Runtime mutations are disabled in observation mode",
            ));
        }
        if self.operator.state.is_active() {
            return Err(OperatorIntentError::WorkflowBusy);
        }
        if !self.model.operator_action_capacity_available() {
            return Err(OperatorIntentError::ActionCapacityFull);
        }
        // Reuse the workflow's validation, without starting or confirming a workflow.
        prepare_operator_intent(self.model, intent.clone())?;
        Ok(intent)
    }

    fn show_reference(
        &self,
        ui: &mut egui::Ui,
        target: &RuntimeRef,
        editor: &mut ReferenceEditor,
    ) -> Option<OperatorIntent> {
        let RuntimeRef::Reference { reference } = target else {
            return None;
        };
        let observation = self
            .model
            .observations
            .entities
            .get(target)
            .map(|observation| &observation.value);
        let value = observation
            .and_then(|value| value.get("value"))
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite());
        let unit = observation
            .and_then(|value| value.pointer("/unit/symbol"))
            .and_then(serde_json::Value::as_str)
            .filter(|unit| !unit.is_empty());
        ui.label(match value {
            Some(value) => format!(
                "Current setpoint (cached): {value} {}",
                unit.unwrap_or("(unit unknown)")
            ),
            None => "Current setpoint: unavailable".into(),
        });
        ui.horizontal(|ui| {
            ui.label("New fixed value");
            let response = ui.add(
                egui::TextEdit::singleline(&mut editor.text).char_limit(REFERENCE_VALUE_BYTES),
            );
            editor.widget_ids.insert(response.id);
            super::app::truncate_utf8(&mut editor.text, REFERENCE_VALUE_BYTES);
            // Input is a raw Reference value in its Runtime unit. Neither the
            // display metadata nor this editor performs a unit conversion.
            let eligible = super::app::parse_finite(&editor.text)
                .map_err(OperatorIntentError::InvalidCandidate)
                .and_then(|value| {
                    self.eligibility(OperatorIntent::ConfigureReferenceFixed {
                        reference: reference.clone(),
                        value,
                    })
                });
            let mut response =
                ui.add_enabled(eligible.is_ok(), egui::Button::new("Configure fixed"));
            if let Err(error) = &eligible {
                response = response.on_disabled_hover_text(error.to_string());
            }
            if response.clicked() {
                eligible.ok()
            } else {
                None
            }
        })
        .inner
    }

    fn show_controller(&self, ui: &mut egui::Ui, target: &RuntimeRef) -> Option<OperatorIntent> {
        let RuntimeRef::Controller { controller } = target else {
            return None;
        };
        let state = self
            .model
            .observations
            .entities
            .get(target)
            .and_then(|observation| observation.value.get("state"))
            .and_then(serde_json::Value::as_str);
        ui.label(format!("Controller: {controller}"));
        ui.label(format!(
            "Controller state (cached): {}",
            state.unwrap_or("unavailable")
        ));
        let (label, action) = match state {
            Some("ready") => ("Start", ControllerLifecycleIntent::Start),
            Some("warming" | "running") => ("Pause", ControllerLifecycleIntent::Pause),
            Some("paused") => ("Resume", ControllerLifecycleIntent::Resume),
            Some("failed") => ("Reset failed", ControllerLifecycleIntent::ResetFailed),
            _ => {
                ui.add_enabled(false, egui::Button::new("No lifecycle action available"));
                return None;
            }
        };
        let eligible = self.eligibility(OperatorIntent::ControllerLifecycle {
            controller: controller.clone(),
            action,
        });
        let mut response = ui.add_enabled(eligible.is_ok(), egui::Button::new(label));
        if let Err(error) = &eligible {
            response = response.on_disabled_hover_text(error.to_string());
        }
        // Only the owner may begin the shared workflow, whose separate Confirm
        // rechecks the captured Runtime state even for Control.confirm = false.
        if response.clicked() {
            eligible.ok()
        } else {
            None
        }
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui) -> Option<OperatorIntent> {
        let state = self
            .model
            .observations
            .entities
            .get(&RuntimeRef::Recorder)
            .and_then(|observation| observation.value.get("state"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unavailable");
        ui.label(format!("Recorder state (cached): {state}"));
        ui.horizontal(|ui| {
            if matches!(
                self.candidate(),
                Some(OperatorIntent::StartRecording { .. })
            ) {
                ui.label("Recording label");
                ui.text_edit_singleline(self.label);
                super::app::truncate_utf8(self.label, RECORDING_LABEL_BYTES);
            }
            let candidate = self.candidate();
            let button = match candidate {
                Some(OperatorIntent::StartRecording { .. }) => "Start recording",
                Some(OperatorIntent::StopRecording) => "Stop recording",
                _ => "Recorder unavailable",
            };
            let eligible = candidate.map(|intent| self.eligibility(intent));
            let enabled = matches!(eligible, Some(Ok(_)));
            let mut response = ui.add_enabled(enabled, egui::Button::new(button));
            if let Some(Err(error)) = &eligible {
                response = response.on_disabled_hover_text(error.to_string());
            }
            // A click only returns intent. The owner begins the one workflow;
            // its separate Confirm remains mandatory regardless of Control.confirm.
            if response.clicked() {
                eligible.and_then(Result::ok)
            } else {
                None
            }
        })
        .inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_controller_lifecycle_with_controller_target_is_supported() {
        for kind in [
            ControlKind::ControllerLifecycle,
            ControlKind::SetReference,
            ControlKind::RecordingLifecycle,
            ControlKind::ApplyConfiguration,
            ControlKind::ReconnectResource,
        ] {
            for target in [
                RuntimeRef::Controller {
                    controller: "1".into(),
                },
                RuntimeRef::Recorder,
                RuntimeRef::Reference {
                    reference: "1".into(),
                },
            ] {
                let control = Control {
                    id: "control".into(),
                    label: "Action".into(),
                    kind: kind.clone(),
                    target: target.clone(),
                    confirm: false,
                };
                assert_eq!(
                    controller_control_supported(&control),
                    kind == ControlKind::ControllerLifecycle
                        && matches!(target, RuntimeRef::Controller { .. })
                );
            }
        }
    }

    #[test]
    fn lifecycle_location_suppresses_only_referenced_matching_controller_in_any_tab() {
        let mut document: PresentationDocument = serde_json::from_value(json!({
            "format_version":1,"document_id":"controller-location","plots":[],
            "controls":[{"id":"control","label":"Heater","kind":"controller_lifecycle",
                "target":{"kind":"controller","controller":"1"},"confirm":false}],
            "windows":[{"id":"window","title":"Workbench","tabs":[
                {"id":"signals","title":"Signals","panels":[]},
                {"id":"operator","title":"Operator","panels":[{"id":"panel","title":"Heater actions",
                    "kind":{"kind":"controls","control_ids":["control"]}}]}
            ]}]
        })).unwrap();
        document.validate().unwrap();
        assert_eq!(
            controller_control_location(&document, "1").as_deref(),
            Some("Workbench / Operator / Heater actions")
        );
        assert!(controller_control_location(&document, "2").is_none());
        document.controls[0].kind = ControlKind::SetReference;
        assert!(controller_control_location(&document, "1").is_none());
        document.controls[0].kind = ControlKind::ControllerLifecycle;
        document.windows[0].tabs[1].panels.clear();
        assert!(
            controller_control_location(&document, "1").is_none(),
            "unreferenced control must not hide ordinary buttons"
        );
        document.windows.clear();
        assert!(controller_control_location(&document, "1").is_none());
    }

    #[test]
    fn reference_editor_storage_is_bounded_and_retires_undo_and_focus_on_reset() {
        let context = egui::Context::default();
        let mut document = PresentationDocument::empty("editors");
        document.controls = (0..100)
            .map(|id| Control {
                id: id.to_string(),
                label: "Reference".into(),
                target: RuntimeRef::Reference {
                    reference: "1".into(),
                },
                kind: ControlKind::SetReference,
                confirm: true,
            })
            .collect();
        let mut editors = ReferenceEditors::default();
        editors.reconcile(&context, &document, false);
        assert_eq!(editors.drafts.len(), MAX_CONTROLS);
        let id = egui::Id::new("retired-editor");
        let editor = editors.editor_mut(&document.controls[0]).unwrap();
        editor.text = "20.5".into();
        editor.widget_ids.insert(id);
        let second_id = egui::Id::new("second-editor-instance");
        editor.widget_ids.insert(second_id);
        egui::text_edit::TextEditState::default().store(&context, second_id);
        egui::text_edit::TextEditState::default().store(&context, id);
        context.memory_mut(|memory| memory.request_focus(id));
        editors.reconcile(&context, &document, true);
        assert!(
            editors
                .editor_mut(&document.controls[0])
                .unwrap()
                .text
                .is_empty()
        );
        assert!(egui::text_edit::TextEditState::load(&context, id).is_none());
        assert!(egui::text_edit::TextEditState::load(&context, second_id).is_none());
        assert!(!context.memory(|memory| memory.has_focus(id)));
        // An oversized candidate cannot retain old keys past the first 64.
        document.controls.rotate_left(MAX_CONTROLS);
        editors.reconcile(&context, &document, false);
        assert_eq!(editors.drafts.len(), MAX_CONTROLS);
        document.controls.clear();
        editors.reconcile(&context, &document, false);
        assert!(editors.drafts.is_empty());
    }

    #[test]
    fn only_recording_lifecycle_on_recorder_is_supported() {
        for kind in [
            ControlKind::SetReference,
            ControlKind::ControllerLifecycle,
            ControlKind::ApplyConfiguration,
            ControlKind::ReconnectResource,
            ControlKind::RecordingLifecycle,
        ] {
            for target in [
                RuntimeRef::Recorder,
                RuntimeRef::Reference {
                    reference: "1".into(),
                },
            ] {
                let control = Control {
                    id: "record".into(),
                    label: "Recording".into(),
                    kind: kind.clone(),
                    target: target.clone(),
                    confirm: false,
                };
                assert_eq!(
                    recorder_control_supported(&control),
                    kind == ControlKind::RecordingLifecycle && target == RuntimeRef::Recorder
                );
            }
        }
    }
}

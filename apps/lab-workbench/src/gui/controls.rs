//! Read-only Recorder controls returning typed intent to the existing operator workflow.

use crate::{
    model::{
        OperatorIntent, OperatorIntentError, OperatorWorkflow, RECORDING_LABEL_BYTES,
        WorkbenchModel, prepare_operator_intent,
    },
    presentation::{Control, ControlKind, PanelKind, PresentationDocument, RuntimeRef},
};
use eframe::egui;

pub(super) fn recorder_control_supported(control: &Control) -> bool {
    control.kind == ControlKind::RecordingLifecycle && control.target == RuntimeRef::Recorder
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
pub(super) struct RecorderControlUi<'a> {
    pub(super) model: &'a WorkbenchModel,
    pub(super) operator: &'a OperatorWorkflow,
    pub(super) observation_only: bool,
    pub(super) label: &'a mut String,
}

impl RecorderControlUi<'_> {
    pub(super) fn show_panel(
        &mut self,
        ui: &mut egui::Ui,
        controls: &[Control],
        control_ids: &[String],
    ) -> Option<OperatorIntent> {
        if control_ids.is_empty() {
            ui.label("No controls configured.");
        }
        let mut intent = None;
        for control in control_ids
            .iter()
            .filter_map(|id| controls.iter().find(|control| &control.id == id))
        {
            ui.push_id(&control.id, |ui| {
                if recorder_control_supported(control) {
                    ui.label(&control.label);
                    let clicked = self.show(ui);
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

//! Read-only controls returning typed intent to the existing operator workflow.

use crate::{
    model::{
        ControllerLifecycleIntent, OperatorIntent, OperatorIntentError, OperatorWorkflow,
        PROPERTY_TEXT_BYTES, RECORDING_LABEL_BYTES, WorkbenchModel, prepare_operator_intent,
    },
    presentation::{
        ConfigurationOwner, Control, ControlKind, MAX_CONTROLS, PanelKind, PresentationDocument,
        RuntimeRef,
    },
};
use eframe::egui;
use std::collections::{BTreeMap, HashSet};

use visuals::{IndicatorView, InputKind, InputView, ObservedValue};

/// A frame-local derived view, never a document, draft owner or prepared action.
/// Only the trusted adapters below construct intents and consult eligibility.
struct ControlView<'a> {
    control: &'a Control,
    observation: Option<IndicatorView>,
    freshness: super::presentation::SourceState,
    metadata: Vec<String>,
    input: Option<InputView>,
    action_label: &'a str,
    action: Result<OperatorIntent, OperatorIntentError>,
}

impl ControlView<'_> {
    fn show_observation(&self, ui: &mut egui::Ui) {
        if self.observation.is_some() && !self.control.label.is_empty() {
            ui.label(&self.control.label);
        }
        for line in &self.metadata {
            ui.label(line);
        }
        if let Some(observation) = &self.observation {
            visuals::indicator(ui, observation);
            super::presentation::state_label(ui, self.freshness);
        }
    }

    fn show_action(self, ui: &mut egui::Ui) -> Option<OperatorIntent> {
        let reason = self.action.as_ref().err().map(ToString::to_string);
        // The visual button sees availability and a reason, never an intent or
        // an operation name. Only this adapter returns the already typed intent.
        visuals::button(ui, self.action_label, reason.as_deref())
            .then(|| self.action.ok())
            .flatten()
    }
}

/// Display/input primitives deliberately depend only on egui and plain view data.
/// They cannot resolve targets, validate operations or enter an operator workflow.
mod visuals {
    use eframe::egui;

    pub(super) enum ObservedValue {
        Number(f64),
        Text(String),
    }

    pub(super) struct IndicatorView {
        pub(super) caption: &'static str,
        pub(super) value: Option<ObservedValue>,
        pub(super) unit: Option<String>,
        pub(super) unavailable: &'static str,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum InputKind {
        Number,
        Integer,
        Text,
    }

    #[derive(Clone, Copy)]
    pub(super) struct InputView {
        pub(super) caption: &'static str,
        pub(super) kind: InputKind,
        pub(super) enabled: bool,
        pub(super) max_chars: usize,
        pub(super) max_bytes: usize,
    }

    pub(super) fn indicator(ui: &mut egui::Ui, view: &IndicatorView) {
        let Some(value) = &view.value else {
            ui.label(view.unavailable);
            return;
        };
        let value = match value {
            ObservedValue::Number(value) => value.to_string(),
            ObservedValue::Text(value) => value.clone(),
        };
        ui.label(format!(
            "{}: {value}{}",
            view.caption,
            view.unit
                .as_ref()
                .map_or(String::new(), |unit| format!(" {unit}"))
        ));
    }

    pub(super) fn input(ui: &mut egui::Ui, view: InputView, text: &mut String) -> egui::Response {
        ui.label(view.caption);
        let mut response = ui.add_enabled(
            view.enabled,
            egui::TextEdit::singleline(text).char_limit(view.max_chars),
        );
        if text.len() > view.max_bytes {
            let mut end = view.max_bytes;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            response.mark_changed();
        }
        response.on_hover_text(match view.kind {
            InputKind::Number => "Finite number in the Runtime unit",
            InputKind::Integer => "Exact decimal integer",
            InputKind::Text => "Text",
        })
    }

    pub(super) fn button(ui: &mut egui::Ui, caption: &str, blocked: Option<&str>) -> bool {
        let mut response = ui.add_enabled(blocked.is_none(), egui::Button::new(caption));
        if let Some(reason) = blocked {
            response = response.on_disabled_hover_text(reason);
        }
        response.clicked()
    }
}

// Match the ordinary fixed-value editor's text bound, without restricting f64 range.
const REFERENCE_VALUE_BYTES: usize = 64;

// Keep one excess scalar so an oversized paste remains invalid, rather than
// silently turning its prefix into an eligible text mutation. UTF-8 storage is
// bounded to four bytes per scalar; the existing workflow enforces 512 bytes.
const PROPERTY_INPUT_CHARS: usize = PROPERTY_TEXT_BYTES + 1;

pub(super) fn property_control_supported(control: &Control) -> bool {
    control.kind == ControlKind::ApplyConfiguration
        && matches!(control.target, RuntimeRef::ConfigurationProperty { .. })
}

fn edited_control_supported(control: &Control) -> bool {
    reference_control_supported(control) || property_control_supported(control)
}

pub(super) fn reference_control_supported(control: &Control) -> bool {
    control.kind == ControlKind::SetReference
        && matches!(control.target, RuntimeRef::Reference { .. })
}

#[derive(Default)]
pub(super) struct ControlEditor {
    pub(super) text: String,
    // A control can be referenced by multiple panels. Track every current view
    // instance (bounded by the document's panel/reference counts), not just the
    // last rendered editor, so retirement also removes their undo/focus state.
    pub(super) widget_ids: HashSet<egui::Id>,
}

/// At most 64 current control/target drafts: 64 bytes for References, at most
/// 2052 UTF-8 bytes for property input (valid text candidates stay <= 512 bytes).
/// These are GUI input, not observations, document state or mutation recovery.
#[derive(Default)]
pub(super) struct ControlEditors {
    drafts: BTreeMap<(String, RuntimeRef), ControlEditor>,
}

impl ControlEditors {
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
                    .filter(|control| edited_control_supported(control))
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
            .filter(|control| edited_control_supported(control))
            .take(MAX_CONTROLS)
        {
            self.drafts
                .entry((control.id.clone(), control.target.clone()))
                .or_default();
        }
    }

    pub(super) fn editor_mut(&mut self, control: &Control) -> Option<&mut ControlEditor> {
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
        editors: &mut ControlEditors,
    ) -> Option<OperatorIntent> {
        if control_ids.is_empty() {
            ui.label("No controls configured.");
        }
        let mut intent = None;
        for control in control_ids
            .iter()
            .filter_map(|id| controls.iter().find(|control| &control.id == id))
        {
            let clicked = ui
                .push_id((&control.id, &control.target), |ui| {
                    self.show_control(ui, control, editors.editor_mut(control))
                })
                .inner;
            if intent.is_none() {
                intent = clicked;
            }
        }
        intent
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

    /// Closed v1 adapter selection. A label, kind or descriptor cannot supply an
    /// operation name, substitute a target category or enable an extra action.
    fn view<'a>(&self, control: &'a Control, text: &str) -> ControlView<'a> {
        let mut view = ControlView {
            control,
            observation: None,
            freshness: super::presentation::source_state(&self.model.observations, &control.target),
            metadata: Vec::new(),
            input: None,
            action_label: &control.label,
            action: Err(OperatorIntentError::InvalidCandidate(
                "This control kind or target is not supported.",
            )),
        };
        if recorder_control_supported(control) {
            self.recorder_view(&mut view, text);
        } else if reference_control_supported(control) {
            self.reference_view(&mut view, text);
        } else if controller_control_supported(control) {
            self.controller_view(&mut view);
        } else if property_control_supported(control) {
            self.property_view(&mut view, text);
        }
        view.action = view.action.and_then(|intent| self.eligibility(intent));
        view
    }

    fn recorder_view(&self, view: &mut ControlView<'_>, text: &str) {
        let observation = self
            .model
            .observations
            .entities
            .get(&view.control.target)
            .map(|item| &item.value);
        let state = observation
            .and_then(|item| item.get("state"))
            .and_then(serde_json::Value::as_str);
        view.observation = Some(IndicatorView {
            caption: "Recorder state (cached)",
            value: state.map(|state| ObservedValue::Text(state.into())),
            unit: None,
            unavailable: "Recorder state (cached): unavailable",
        });
        if observation
            .and_then(|item| item.get("active_run"))
            .is_some_and(|run| !run.is_null())
        {
            view.action_label = "Stop recording";
            view.action = Ok(OperatorIntent::StopRecording);
        } else if state == Some("idle") {
            view.input = Some(InputView {
                caption: "Recording label",
                kind: InputKind::Text,
                enabled: true,
                max_chars: usize::MAX,
                max_bytes: RECORDING_LABEL_BYTES,
            });
            view.action_label = "Start recording";
            view.action = Ok(OperatorIntent::StartRecording { label: text.into() });
        } else {
            view.action_label = "Recorder unavailable";
            view.action = Err(OperatorIntentError::InvalidCandidate(
                "Recorder unavailable",
            ));
        }
    }

    fn reference_view(&self, view: &mut ControlView<'_>, text: &str) {
        let RuntimeRef::Reference { reference } = &view.control.target else {
            return;
        };
        let observation = self
            .model
            .observations
            .entities
            .get(&view.control.target)
            .map(|item| &item.value);
        let value = observation
            .and_then(|item| item.get("value"))
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite());
        let unit = observation
            .and_then(|item| item.pointer("/unit/symbol"))
            .and_then(serde_json::Value::as_str)
            .filter(|unit| !unit.is_empty());
        view.observation = Some(IndicatorView {
            caption: "Current setpoint (cached)",
            value: value.map(ObservedValue::Number),
            unit: Some(unit.unwrap_or("(unit unknown)").into()),
            unavailable: "Current setpoint: unavailable",
        });
        view.input = Some(InputView {
            caption: "New fixed value",
            kind: InputKind::Number,
            enabled: true,
            max_chars: REFERENCE_VALUE_BYTES,
            max_bytes: REFERENCE_VALUE_BYTES,
        });
        view.action_label = "Configure fixed";
        // This candidate is the raw Reference value in its Runtime unit. Display
        // metadata and input components perform no conversion or mutation parsing.
        view.action = super::app::parse_finite(text)
            .map_err(OperatorIntentError::InvalidCandidate)
            .map(|value| OperatorIntent::ConfigureReferenceFixed {
                reference: reference.clone(),
                value,
            });
    }

    fn controller_view(&self, view: &mut ControlView<'_>) {
        let RuntimeRef::Controller { controller } = &view.control.target else {
            return;
        };
        let state = self
            .model
            .observations
            .entities
            .get(&view.control.target)
            .and_then(|item| item.value.get("state"))
            .and_then(serde_json::Value::as_str);
        view.metadata.push(format!("Controller: {controller}"));
        view.observation = Some(IndicatorView {
            caption: "Controller state (cached)",
            value: state.map(|state| ObservedValue::Text(state.into())),
            unit: None,
            unavailable: "Controller state (cached): unavailable",
        });
        let (label, action) = match state {
            Some("ready") => ("Start", ControllerLifecycleIntent::Start),
            Some("warming" | "running") => ("Pause", ControllerLifecycleIntent::Pause),
            Some("paused") => ("Resume", ControllerLifecycleIntent::Resume),
            Some("failed") => ("Reset failed", ControllerLifecycleIntent::ResetFailed),
            _ => {
                view.action_label = "No lifecycle action available";
                view.action = Err(OperatorIntentError::InvalidCandidate(
                    "No lifecycle action available",
                ));
                return;
            }
        };
        view.action_label = label;
        view.action = Ok(OperatorIntent::ControllerLifecycle {
            controller: controller.clone(),
            action,
        });
    }

    fn property_view(&self, view: &mut ControlView<'_>, text: &str) {
        let RuntimeRef::ConfigurationProperty { owner, property } = &view.control.target else {
            return;
        };
        let owner = match owner {
            ConfigurationOwner::Instrument { instrument } => format!("instrument {instrument}"),
            ConfigurationOwner::Component { component } => format!("component {component}"),
            ConfigurationOwner::Resource { resource } => format!("resource {resource}"),
        };
        view.metadata
            .push(format!("Owner: {owner}; property: {property}"));
        let observation = self
            .model
            .observations
            .entities
            .get(&view.control.target)
            .map(|item| &item.value);
        let value_type = observation
            .and_then(|item| item.get("value_type"))
            .and_then(serde_json::Value::as_str);
        let access = observation
            .and_then(|item| item.get("access"))
            .and_then(serde_json::Value::as_str);
        view.metadata.push(format!(
            "type: {}; access: {}",
            value_type.unwrap_or("unknown"),
            access.unwrap_or("unknown")
        ));
        let current = observation
            .and_then(|item| item.get("current"))
            .map(|value| {
                ObservedValue::Text(
                    value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned),
                )
            });
        view.observation = Some(IndicatorView {
            caption: "Current value (cached)",
            value: current,
            unit: None,
            unavailable: "Current value (cached): unavailable",
        });
        if let Some(unit) = observation
            .and_then(|item| item.get("unit"))
            .and_then(|unit| unit.get("symbol").or_else(|| unit.get("id")))
            .and_then(serde_json::Value::as_str)
            .filter(|unit| !unit.is_empty())
        {
            view.metadata.push(format!("Unit: {unit}"));
        }
        view.input = Some(InputView {
            caption: "New value",
            kind: if value_type == Some("integer") {
                InputKind::Integer
            } else {
                InputKind::Text
            },
            enabled: access == Some("read_write") && matches!(value_type, Some("integer" | "text")),
            max_chars: PROPERTY_INPUT_CHARS,
            max_bytes: PROPERTY_INPUT_CHARS * 4,
        });
        view.action_label = "Configure property";
        // The existing prepare path enforces access, type, constraints, revision
        // and warnings. Unsupported observed scalar types never become mutations.
        view.action = super::app::property_candidate(value_type, text)
            .map_err(OperatorIntentError::InvalidCandidate)
            .map(|value| OperatorIntent::ConfigureProperty {
                target: view.control.target.clone(),
                value,
            });
    }

    fn show_control(
        &mut self,
        ui: &mut egui::Ui,
        control: &Control,
        mut editor: Option<&mut ControlEditor>,
    ) -> Option<OperatorIntent> {
        let text = if recorder_control_supported(control) {
            self.label.as_str()
        } else {
            editor.as_ref().map_or("", |editor| editor.text.as_str())
        };
        let mut view = self.view(control, text);
        view.show_observation(ui);
        ui.horizontal(|ui| {
            if let Some(input) = view.input {
                let response = if recorder_control_supported(control) {
                    visuals::input(ui, input, self.label)
                } else if let Some(editor) = editor.as_mut() {
                    let response = visuals::input(ui, input, &mut editor.text);
                    editor.widget_ids.insert(response.id);
                    response
                } else {
                    return None;
                };
                if response.changed() {
                    // Re-resolve after this frame's edit, including bounded paste.
                    // The prepared action is owned elsewhere and never borrows it.
                    let text = if recorder_control_supported(control) {
                        self.label.as_str()
                    } else {
                        &editor.as_ref().unwrap().text
                    };
                    view = self.view(control, text);
                }
            }
            // A click returns at most one typed intent. The owner still requires
            // separate Confirm and rechecks guards, even with control.confirm=false.
            view.show_action(ui)
        })
        .inner
    }

    /// The ordinary Recorder group consumes the same adapter and primitives.
    pub(super) fn show(&mut self, ui: &mut egui::Ui) -> Option<OperatorIntent> {
        self.show_control(
            ui,
            &Control {
                id: "ordinary-recorder".into(),
                label: String::new(),
                kind: ControlKind::RecordingLifecycle,
                target: RuntimeRef::Recorder,
                confirm: true,
            },
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::types::{ConnectionState, EventCursor, HelloState};
    use crate::model::PropertyMutationCandidate;
    use serde_json::json;

    #[test]
    fn shared_views_keep_typed_adapters_identity_metadata_and_input_kinds() {
        let property = RuntimeRef::ConfigurationProperty {
            owner: ConfigurationOwner::Resource {
                resource: "7".into(),
            },
            property: "title".into(),
        };
        for (kind, target, observation, input, expected_input, expected_intent) in [
            (
                ControlKind::RecordingLifecycle,
                RuntimeRef::Recorder,
                json!({"state":"idle","active_run":null}),
                "Run",
                Some(InputKind::Text),
                OperatorIntent::StartRecording {
                    label: "Run".into(),
                },
            ),
            (
                ControlKind::SetReference,
                RuntimeRef::Reference {
                    reference: "7".into(),
                },
                json!({"value":12.5,"revision":"9","unit":{"symbol":"K"}}),
                "25.5",
                Some(InputKind::Number),
                OperatorIntent::ConfigureReferenceFixed {
                    reference: "7".into(),
                    value: 25.5,
                },
            ),
            (
                ControlKind::ControllerLifecycle,
                RuntimeRef::Controller {
                    controller: "7".into(),
                },
                json!({"state":"paused"}),
                "",
                None,
                OperatorIntent::ControllerLifecycle {
                    controller: "7".into(),
                    action: ControllerLifecycleIntent::Resume,
                },
            ),
            (
                ControlKind::ApplyConfiguration,
                property.clone(),
                json!({"current":"Old","value_type":"text","access":"read_write","revision":"9","mutation_class":"live_safe"}),
                "New",
                Some(InputKind::Text),
                OperatorIntent::ConfigureProperty {
                    target: property.clone(),
                    value: PropertyMutationCandidate::Text("New".into()),
                },
            ),
            (
                ControlKind::ApplyConfiguration,
                property.clone(),
                json!({"current":1,"value_type":"integer","access":"read_write","revision":"9","mutation_class":"ordinary_live"}),
                "9223372036854775807",
                Some(InputKind::Integer),
                OperatorIntent::ConfigureProperty {
                    target: property,
                    value: PropertyMutationCandidate::Integer(i64::MAX),
                },
            ),
        ] {
            let control = Control {
                id: "control-7".into(),
                label: "Operator label".into(),
                kind,
                target,
                confirm: false,
            };
            let mut model = WorkbenchModel::new(PresentationDocument::empty("views"));
            model.connection = ConnectionState::Ready;
            model.hello = Some(HelloState {
                boot_id: "boot".into(),
                scope: "scope".into(),
                next_seq: 1,
                operations: [
                    "recording_start",
                    "reference_configure",
                    "controller_resume",
                    "property_configure",
                ]
                .map(str::to_owned)
                .into(),
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
            model
                .observations
                .observe(control.target.clone(), observation, None);
            model.observations.complete_rebuild();
            let before = model.clone();
            let operator = OperatorWorkflow::default();
            let mut label = String::new();
            let adapter = OperatorControlUi {
                model: &model,
                operator: &operator,
                observation_only: false,
                label: &mut label,
            };
            let view = adapter.view(&control, input);
            assert!(std::ptr::eq(view.control, &control));
            assert_eq!(view.control.id, "control-7");
            assert_eq!(view.control.label, "Operator label");
            assert_eq!(
                view.freshness,
                super::super::presentation::SourceState::Fresh
            );
            assert_eq!(view.input.map(|input| input.kind), expected_input);
            assert_eq!(view.action, Ok(expected_intent));
            if control.kind == ControlKind::SetReference {
                let observed = view.observation.unwrap();
                assert_eq!(observed.unit.as_deref(), Some("K"));
                assert!(matches!(observed.value, Some(ObservedValue::Number(12.5))));
            }
            if control.kind == ControlKind::ApplyConfiguration {
                assert!(
                    view.metadata
                        .iter()
                        .any(|line| line == "Owner: resource 7; property: title")
                );
            }
            assert_eq!(model, before, "resolving a view cannot mutate the owner");
            model.observations.freshness = crate::model::Freshness::Rebuilding;
            let adapter = OperatorControlUi {
                model: &model,
                operator: &operator,
                observation_only: false,
                label: &mut label,
            };
            let view = adapter.view(&control, input);
            assert_eq!(
                view.freshness,
                super::super::presentation::SourceState::Rebuilding
            );
            assert_eq!(view.action, Err(OperatorIntentError::RebuildNotFresh));
        }
    }

    #[test]
    fn only_apply_configuration_with_property_target_is_supported() {
        for kind in [
            ControlKind::ApplyConfiguration,
            ControlKind::SetReference,
            ControlKind::RecordingLifecycle,
            ControlKind::ControllerLifecycle,
            ControlKind::ReconnectResource,
        ] {
            for target in [
                RuntimeRef::ConfigurationProperty {
                    owner: ConfigurationOwner::Instrument {
                        instrument: "1".into(),
                    },
                    property: "limit".into(),
                },
                RuntimeRef::Recorder,
                RuntimeRef::Reference {
                    reference: "1".into(),
                },
                RuntimeRef::Controller {
                    controller: "1".into(),
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
                    property_control_supported(&control),
                    kind == ControlKind::ApplyConfiguration
                        && matches!(target, RuntimeRef::ConfigurationProperty { .. })
                );
            }
        }
    }

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
    fn control_editor_storage_is_bounded_and_retires_undo_and_focus_on_reset() {
        let context = egui::Context::default();
        let mut document = PresentationDocument::empty("editors");
        document.controls = (0..100)
            .map(|id| Control {
                id: id.to_string(),
                label: "Reference".into(),
                target: if id % 2 == 0 {
                    RuntimeRef::Reference {
                        reference: "1".into(),
                    }
                } else {
                    RuntimeRef::ConfigurationProperty {
                        owner: ConfigurationOwner::Instrument {
                            instrument: "1".into(),
                        },
                        property: "limit".into(),
                    }
                },
                kind: if id % 2 == 0 {
                    ControlKind::SetReference
                } else {
                    ControlKind::ApplyConfiguration
                },
                confirm: true,
            })
            .collect();
        let mut editors = ControlEditors::default();
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

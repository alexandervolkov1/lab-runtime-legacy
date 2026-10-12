//! Static reference and binding checks. No Runtime observations or eligibility.

use super::{dto::*, graph};
use std::collections::BTreeMap;

/// A static reason to keep a binding or an occurrence inert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StaticBlocker {
    /// The document explicitly supplies no action for this button.
    InertButton,
    /// The exact Runtime target has a category incompatible with its selector/family.
    IncompatibleTarget,
    /// Parameter observation and action identify different full Runtime targets.
    ParameterTargetMismatch,
    /// An ancestor records a v1 kind/target pair that was never executable.
    InertV1Origin,
    /// An action under a supported v1 origin changes its family or exact target.
    V1ActionMismatch,
}

/// Passing structural checks never makes an action eligible or metadata authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StaticAvailability {
    /// Requires future target, metadata, freshness and trusted adapter resolution.
    NeedsRuntimeResolution,
    /// Already known to be incompatible, without consulting Runtime.
    Disabled(StaticBlocker),
}

/// Finite document-local results. No executable action or authority is retained.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StructuralSummary {
    pub(crate) expanded_components: usize,
    pub(crate) expanded_plots: usize,
    /// In definition order, including unused definitions.
    pub(crate) data: Vec<StaticAvailability>,
    /// In definition order, including unused definitions.
    pub(crate) editors: Vec<StaticAvailability>,
    /// In definition order. Occurrence-local origin ceilings apply in addition.
    pub(crate) actions: Vec<StaticAvailability>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Registry {
    Document,
    Window,
    Tab,
    Panel,
    Component,
    Data,
    Editor,
    Action,
    Plot,
    Trace,
}

pub(super) struct Index<'a>(BTreeMap<&'a Id, (Registry, usize)>);
impl<'a> Index<'a> {
    fn insert(&mut self, id: &'a Id, registry: Registry, position: usize) -> Result<(), String> {
        if self.0.insert(id, (registry, position)).is_some() {
            return Err(format!("duplicate definition ID: {}", id.0));
        }
        Ok(())
    }
    pub(super) fn resolve(&self, id: &Id, expected: Registry) -> Result<usize, String> {
        match self.0.get(id) {
            Some((actual, position)) if *actual == expected => Ok(*position),
            Some(_) => Err(format!(
                "wrong registry for {:?} reference: {}",
                expected, id.0
            )),
            None => Err(format!("dangling {:?} reference: {}", expected, id.0)),
        }
    }
    fn build(doc: &'a DocumentDto) -> Result<Self, String> {
        let mut index = Self(BTreeMap::new());
        index.insert(&doc.document_id, Registry::Document, 0)?;
        for window in &doc.windows.0 {
            index.insert(&window.id, Registry::Window, 0)?;
            for tab in &window.tabs.0 {
                index.insert(&tab.id, Registry::Tab, 0)?;
                for panel in &tab.panels.0 {
                    index.insert(&panel.id, Registry::Panel, 0)?;
                }
            }
        }
        for (i, item) in doc.components.0.iter().enumerate() {
            index.insert(item.id(), Registry::Component, i)?;
        }
        for (i, item) in doc.data_bindings.0.iter().enumerate() {
            index.insert(&item.id, Registry::Data, i)?;
        }
        for (i, item) in doc.editor_bindings.0.iter().enumerate() {
            index.insert(&item.id, Registry::Editor, i)?;
        }
        for (i, item) in doc.action_bindings.0.iter().enumerate() {
            index.insert(item.id(), Registry::Action, i)?;
        }
        for (i, item) in doc.plots.0.iter().enumerate() {
            index.insert(&item.id, Registry::Plot, i)?;
            for trace in &item.traces.0 {
                index.insert(&trace.id, Registry::Trace, 0)?;
            }
        }
        Ok(index)
    }
}

pub(super) fn check(doc: &DocumentDto) -> Result<StructuralSummary, String> {
    let index = Index::build(doc)?;
    let mut summary = StructuralSummary {
        expanded_components: 0,
        expanded_plots: 0,
        data: doc
            .data_bindings
            .0
            .iter()
            .map(|data| {
                if compatible_target(data.selector, &data.target) {
                    StaticAvailability::NeedsRuntimeResolution
                } else {
                    StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
                }
            })
            .collect(),
        editors: Vec::new(),
        actions: Vec::new(),
    };
    let mut recorder_editor_seen = false;
    for editor in &doc.editor_bindings.0 {
        let data_index = index.resolve(&editor.data_id, Registry::Data)?;
        let expected = match editor.purpose {
            Purpose::RecordingLabel => {
                if recorder_editor_seen {
                    return Err("only one recording_label editor is allowed".into());
                }
                recorder_editor_seen = true;
                Some(Selector::RecorderState)
            }
            Purpose::ReferenceFixed => Some(Selector::ReferenceValue),
            Purpose::PropertyValue => Some(Selector::ConfigurationPropertyCurrent),
            Purpose::LocalOnly => None,
        };
        if expected.is_some_and(|selector| selector != doc.data_bindings.0[data_index].selector) {
            return Err("editor purpose/data selector mismatch".into());
        }
        summary.editors.push(summary.data[data_index]);
    }
    for action in &doc.action_bindings.0 {
        let mut state = if compatible_target(action_selector(action), action.target()) {
            StaticAvailability::NeedsRuntimeResolution
        } else {
            StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
        };
        if let Some(Parameter::Editor { editor_id }) = action.parameter() {
            let i = index.resolve(editor_id, Registry::Editor)?;
            let editor = &doc.editor_bindings.0[i];
            let expected = match action {
                ActionBinding::RecorderLifecycle { .. } => Purpose::RecordingLabel,
                ActionBinding::ReferenceFixed { .. } => Purpose::ReferenceFixed,
                ActionBinding::PropertyConfigure { .. } => Purpose::PropertyValue,
                ActionBinding::ControllerLifecycle { .. } => {
                    unreachable!("closed DTO has no parameter")
                }
            };
            if editor.purpose != expected {
                return Err("action family/editor purpose mismatch".into());
            }
            let data = &doc.data_bindings.0[index.resolve(&editor.data_id, Registry::Data)?];
            if &data.target != action.target() {
                state = StaticAvailability::Disabled(StaticBlocker::ParameterTargetMismatch);
            } else if let StaticAvailability::Disabled(reason) = summary.editors[i] {
                state = StaticAvailability::Disabled(reason);
            }
        }
        summary.actions.push(state);
    }
    // A property_scalar definition may be unused. If used by inputs, both closed
    // matching_type alternatives must exist; repeated views don't create editors.
    let mut property_inputs = vec![0u8; doc.editor_bindings.0.len()];
    for component in &doc.components.0 {
        match component {
            Component::Group { children, .. } => {
                for child in &children.0 {
                    index.resolve(child, Registry::Component)?;
                }
            }
            Component::Button {
                action_id: Some(id),
                ..
            } => {
                index.resolve(id, Registry::Action)?;
            }
            Component::Indicator { data_id, .. }
            | Component::Status {
                source: StatusSource::Data { data_id },
                ..
            } => {
                index.resolve(data_id, Registry::Data)?;
            }
            Component::Plot { plot_id, .. } => {
                index.resolve(plot_id, Registry::Plot)?;
            }
            _ => {}
        }
        if let Some((editor_id, kind, applicability)) = input(component) {
            let i = index.resolve(editor_id, Registry::Editor)?;
            let candidate = doc.editor_bindings.0[i].candidate_kind;
            if candidate == CandidateKind::PropertyScalar {
                if applicability != Applicability::MatchingType {
                    return Err("property_scalar input requires matching_type".into());
                }
                property_inputs[i] |= match kind {
                    CandidateKind::Integer => 1,
                    CandidateKind::Text => 2,
                    _ => return Err("property_scalar requires IntegerInput/TextInput".into()),
                };
            } else if kind != candidate {
                return Err("widget/editor candidate kind mismatch".into());
            }
        }
    }
    if property_inputs
        .iter()
        .any(|&kinds| kinds == 1 || kinds == 2)
    {
        return Err(
            "property_scalar inputs require both IntegerInput and TextInput definitions".into(),
        );
    }
    for plot in &doc.plots.0 {
        for trace in &plot.traces.0 {
            let data = &doc.data_bindings.0[index.resolve(&trace.data_id, Registry::Data)?];
            if data.selector != Selector::SignalSample {
                return Err("trace requires signal_sample".into());
            }
        }
    }
    for window in &doc.windows.0 {
        for tab in &window.tabs.0 {
            for panel in &tab.panels.0 {
                for root in &panel.components.0 {
                    index.resolve(root, Registry::Component)?;
                }
                check_panel_marker(doc, &index, panel)?;
            }
        }
    }
    let (components, plots) = graph::check(doc, &index)?;
    summary.expanded_components = components;
    summary.expanded_plots = plots;
    Ok(summary)
}

fn check_panel_marker(doc: &DocumentDto, index: &Index<'_>, panel: &Panel) -> Result<(), String> {
    let Some(kind) = panel.v1_kind else {
        return Ok(());
    };
    if kind != V1PanelKind::Controls && panel.components.0.len() != 1 {
        return Err("v1 plot/status panel requires exactly one root".into());
    }
    for root in &panel.components.0 {
        let component = &doc.components.0[index.resolve(root, Registry::Component)?];
        let compatible = matches!(
            (kind, component),
            (
                V1PanelKind::Controls,
                Component::Group {
                    v1_control: Some(_),
                    ..
                }
            ) | (V1PanelKind::Plot, Component::Plot { .. })
                | (
                    V1PanelKind::Status,
                    Component::Status {
                        source: StatusSource::Entity { .. },
                        ..
                    }
                )
        );
        if !compatible {
            return Err("v1_kind root shape mismatch".into());
        }
    }
    Ok(())
}

pub(super) fn input(component: &Component) -> Option<(&Id, CandidateKind, Applicability)> {
    let (id, kind, applicability) = match component {
        Component::Toggle {
            editor_id,
            applicability,
            ..
        } => (editor_id, CandidateKind::Boolean, applicability),
        Component::IntegerInput {
            editor_id,
            applicability,
            ..
        } => (editor_id, CandidateKind::Integer, applicability),
        Component::NumberInput {
            editor_id,
            applicability,
            ..
        } => (editor_id, CandidateKind::Number, applicability),
        Component::TextInput {
            editor_id,
            applicability,
            ..
        } => (editor_id, CandidateKind::Text, applicability),
        Component::Select {
            editor_id,
            applicability,
            ..
        } => (editor_id, CandidateKind::Enum, applicability),
        _ => return None,
    };
    Some((id, kind, *applicability))
}

fn compatible_target(selector: Selector, target: &RuntimeRef) -> bool {
    matches!(
        (selector, target),
        (Selector::SignalSample, RuntimeRef::Signal { .. })
            | (
                Selector::ConfigurationPropertyCurrent,
                RuntimeRef::ConfigurationProperty { .. }
            )
            | (Selector::ReferenceValue, RuntimeRef::Reference { .. })
            | (Selector::ControllerState, RuntimeRef::Controller { .. })
            | (Selector::RecorderState, RuntimeRef::Recorder { .. })
    )
}

fn action_selector(action: &ActionBinding) -> Selector {
    match action {
        ActionBinding::RecorderLifecycle { .. } => Selector::RecorderState,
        ActionBinding::ReferenceFixed { .. } => Selector::ReferenceValue,
        ActionBinding::ControllerLifecycle { .. } => Selector::ControllerState,
        ActionBinding::PropertyConfigure { .. } => Selector::ConfigurationPropertyCurrent,
    }
}

pub(super) fn origin_selector(origin: &V1ControlOrigin) -> Option<Selector> {
    let selector = match origin.kind {
        V1ControlKind::RecordingLifecycle => Selector::RecorderState,
        V1ControlKind::SetReference => Selector::ReferenceValue,
        V1ControlKind::ControllerLifecycle => Selector::ControllerState,
        V1ControlKind::ApplyConfiguration => Selector::ConfigurationPropertyCurrent,
        V1ControlKind::ReconnectResource => return None,
    };
    compatible_target(selector, &origin.target).then_some(selector)
}

pub(super) fn origin_action_matches(origin: &V1ControlOrigin, action: &ActionBinding) -> bool {
    origin_selector(origin) == Some(action_selector(action)) && &origin.target == action.target()
}

//! Deterministic one-way conversion of validated v1 syntax, without observations.

use super::{
    AdmittedV2Document, ParsedDocument,
    bounded::{List, Number, Text},
    dto::*,
    limits, source_v1 as v1,
};
use std::{borrow::Cow, collections::BTreeSet, io};

/// Conversion fails atomically; a source error is distinct from amplification.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConversionError {
    /// Source violates v1 structure, numeric/string constraints or canonical size.
    InvalidV1(v1::DocumentError),
    /// A valid source does not fit all v2 admission limits, or reservation failed.
    ConversionLimit,
}
impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidV1(error) => write!(f, "invalid_v1: {error}"),
            Self::ConversionLimit => f.write_str("conversion_limit"),
        }
    }
}
impl std::error::Error for ConversionError {}
type Result<T> = std::result::Result<T, ConversionError>;

/// Borrowed proof of complete v1 validation. No mutable access or unchecked entry.
#[derive(Debug)]
pub(crate) struct ValidatedV1Document<'a>(&'a v1::PresentationDocument);
impl<'a> ValidatedV1Document<'a> {
    /// Checks the unchanged v1 invariants and its canonical pretty byte ceiling.
    pub(crate) fn check(source: &'a v1::PresentationDocument) -> Result<Self> {
        source.validate().map_err(ConversionError::InvalidV1)?;
        // Same v1 serializer as validate_serialized_size, but no output buffer.
        let mut count = SizeCounter(0);
        serde_json::to_writer_pretty(&mut count, source)
            .map_err(|_| ConversionError::InvalidV1(v1::DocumentError::Limit("file_bytes")))?;
        Ok(Self(source))
    }
}

struct SizeCounter(usize);
impl io::Write for SizeCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|&size| size <= limits::PRESENTATION_FILE_BYTES)
            .ok_or_else(|| io::Error::other("v1 canonical size limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Category {
    Data,
    Editor,
    Action,
    Component,
}
impl Category {
    fn name(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Editor => "editor",
            Self::Action => "action",
            Self::Component => "component",
        }
    }
}
struct Ids<'a> {
    used: BTreeSet<Cow<'a, str>>,
    next: [usize; 4],
}
impl<'a> Ids<'a> {
    fn new(source: &'a v1::PresentationDocument) -> Self {
        let mut used = BTreeSet::new();
        used.insert(Cow::Borrowed(source.document_id.as_str()));
        for window in &source.windows {
            used.insert(Cow::Borrowed(window.id.as_str()));
            for tab in &window.tabs {
                used.insert(Cow::Borrowed(tab.id.as_str()));
                for panel in &tab.panels {
                    used.insert(Cow::Borrowed(panel.id.as_str()));
                }
            }
        }
        for control in &source.controls {
            used.insert(Cow::Borrowed(control.id.as_str()));
        }
        for plot in &source.plots {
            used.insert(Cow::Borrowed(plot.id.as_str()));
            for trace in &plot.traces {
                used.insert(Cow::Borrowed(trace.id.as_str()));
            }
        }
        Self { used, next: [0; 4] }
    }
    fn allocate(&mut self, category: Category) -> Result<Id> {
        // All earlier indices were occupied or allocated; nothing is ever removed.
        // At most used.len() occupied candidates can precede the first free one.
        for _ in 0..=self.used.len() {
            let next = &mut self.next[category as usize];
            let candidate = format!("v2:{}:{}", category.name(), next);
            *next = next
                .checked_add(1)
                .ok_or(ConversionError::ConversionLimit)?;
            if self.used.insert(Cow::Owned(candidate.clone())) {
                return Ok(Text(candidate));
            }
        }
        Err(ConversionError::ConversionLimit)
    }
}

// Only validated source text or fixed, nonempty template labels enter this helper.
fn text(value: &str) -> Id {
    Text(value.to_owned())
}
fn push<T, const N: usize>(list: &mut List<T, N>, item: T) -> Result<()> {
    if list.0.len() >= N {
        return Err(ConversionError::ConversionLimit);
    }
    list.0
        .try_reserve(1)
        .map_err(|_| ConversionError::ConversionLimit)?;
    list.0.push(item);
    Ok(())
}

fn target(source: &v1::RuntimeRef) -> RuntimeRef {
    match source {
        v1::RuntimeRef::Instrument { instrument } => RuntimeRef::Instrument {
            instrument: text(instrument),
        },
        v1::RuntimeRef::Signal {
            instrument,
            parameter,
        } => RuntimeRef::Signal {
            instrument: text(instrument),
            parameter: text(parameter),
        },
        v1::RuntimeRef::Reference { reference } => RuntimeRef::Reference {
            reference: text(reference),
        },
        v1::RuntimeRef::Controller { controller } => RuntimeRef::Controller {
            controller: text(controller),
        },
        v1::RuntimeRef::Resource { resource } => RuntimeRef::Resource {
            resource: text(resource),
        },
        v1::RuntimeRef::Component { component } => RuntimeRef::Component {
            component: text(component),
        },
        v1::RuntimeRef::Recorder => RuntimeRef::Recorder {},
        v1::RuntimeRef::ConfigurationProperty { owner, property } => {
            RuntimeRef::ConfigurationProperty {
                owner: match owner {
                    v1::ConfigurationOwner::Instrument { instrument } => {
                        ConfigurationOwner::Instrument {
                            instrument: text(instrument),
                        }
                    }
                    v1::ConfigurationOwner::Component { component } => {
                        ConfigurationOwner::Component {
                            component: text(component),
                        }
                    }
                    v1::ConfigurationOwner::Resource { resource } => ConfigurationOwner::Resource {
                        resource: text(resource),
                    },
                },
                property: text(property),
            }
        }
    }
}
fn origin(control: &v1::Control) -> V1ControlOrigin {
    V1ControlOrigin {
        kind: match control.kind {
            v1::ControlKind::RecordingLifecycle => V1ControlKind::RecordingLifecycle,
            v1::ControlKind::SetReference => V1ControlKind::SetReference,
            v1::ControlKind::ControllerLifecycle => V1ControlKind::ControllerLifecycle,
            v1::ControlKind::ApplyConfiguration => V1ControlKind::ApplyConfiguration,
            v1::ControlKind::ReconnectResource => V1ControlKind::ReconnectResource,
        },
        target: target(&control.target),
        confirm: control.confirm,
    }
}

struct Builder<'a> {
    ids: Ids<'a>,
    dto: DocumentDto,
    recorder: Option<(Id, Id)>,
}
impl<'a> Builder<'a> {
    fn new(source: &'a v1::PresentationDocument) -> Self {
        Self {
            ids: Ids::new(source),
            recorder: None,
            dto: DocumentDto {
                format_version: 2,
                binding_version: 1,
                document_id: text(&source.document_id),
                windows: List(Vec::new()),
                components: List(Vec::new()),
                data_bindings: List(Vec::new()),
                editor_bindings: List(Vec::new()),
                action_bindings: List(Vec::new()),
                plots: List(Vec::new()),
                settings: Settings::default(),
            },
        }
    }
    fn data(&mut self, selector: Selector, source: &v1::RuntimeRef) -> Result<Id> {
        let id = self.ids.allocate(Category::Data)?;
        push(
            &mut self.dto.data_bindings,
            DataBinding {
                id: id.clone(),
                selector,
                target: target(source),
                expect_kind: None,
            },
        )?;
        Ok(id)
    }
    fn control(&mut self, control: &v1::Control) -> Result<()> {
        let origin = origin(control);
        let selector = super::structure::origin_selector(&origin);
        let mut children = List(Vec::new());
        // At most four generated children; the parent precedes them in the registry.
        let mut generated = Vec::new();
        if let Some(selector) = selector {
            let (data, editor) = if let Some((data, editor)) = &self.recorder
                && selector == Selector::RecorderState
            {
                (data.clone(), Some(editor.clone()))
            } else {
                let data = self.data(selector, &control.target)?;
                let editor_spec = match selector {
                    Selector::RecorderState => Some((Purpose::RecordingLabel, CandidateKind::Text)),
                    Selector::ReferenceValue => {
                        Some((Purpose::ReferenceFixed, CandidateKind::Number))
                    }
                    Selector::ConfigurationPropertyCurrent => {
                        Some((Purpose::PropertyValue, CandidateKind::PropertyScalar))
                    }
                    Selector::ControllerState => None,
                    Selector::SignalSample => {
                        unreachable!("control origin cannot be a signal action")
                    }
                };
                let editor = if let Some((purpose, candidate_kind)) = editor_spec {
                    let id = self.ids.allocate(Category::Editor)?;
                    push(
                        &mut self.dto.editor_bindings,
                        EditorBinding {
                            id: id.clone(),
                            data_id: data.clone(),
                            purpose,
                            candidate_kind,
                            constraints: Constraints::default(),
                        },
                    )?;
                    if selector == Selector::RecorderState {
                        self.recorder = Some((data.clone(), id.clone()));
                    }
                    Some(id)
                } else {
                    None
                };
                (data, editor)
            };
            let action_id = self.ids.allocate(Category::Action)?;
            let parameter = || Parameter::Editor {
                editor_id: editor.as_ref().expect("template editor").clone(),
            };
            let action = match selector {
                Selector::RecorderState => ActionBinding::RecorderLifecycle {
                    id: action_id.clone(),
                    target: target(&control.target),
                    label: parameter(),
                },
                Selector::ReferenceValue => ActionBinding::ReferenceFixed {
                    id: action_id.clone(),
                    target: target(&control.target),
                    value: parameter(),
                },
                Selector::ControllerState => ActionBinding::ControllerLifecycle {
                    id: action_id.clone(),
                    target: target(&control.target),
                },
                Selector::ConfigurationPropertyCurrent => ActionBinding::PropertyConfigure {
                    id: action_id.clone(),
                    target: target(&control.target),
                    value: parameter(),
                },
                Selector::SignalSample => unreachable!("control origin cannot be a signal action"),
            };
            push(&mut self.dto.action_bindings, action)?;
            let id = self.ids.allocate(Category::Component)?;
            generated.push(match selector {
                Selector::RecorderState | Selector::ControllerState => Component::Status {
                    id,
                    label: text(if selector == Selector::RecorderState {
                        "Recorder state"
                    } else {
                        "Controller state"
                    }),
                    help: None,
                    source: StatusSource::Data { data_id: data },
                },
                _ => Component::Indicator {
                    id,
                    label: text("Current"),
                    help: None,
                    data_id: data,
                    precision: None,
                },
            });
            if let Some(editor_id) = editor {
                let id = self.ids.allocate(Category::Component)?;
                match selector {
                    Selector::RecorderState => generated.push(Component::TextInput {
                        id,
                        label: text("Recording label"),
                        help: None,
                        editor_id,
                        applicability: Applicability::Always,
                    }),
                    Selector::ReferenceValue => generated.push(Component::NumberInput {
                        id,
                        label: text("New value"),
                        help: None,
                        editor_id,
                        applicability: Applicability::Always,
                    }),
                    Selector::ConfigurationPropertyCurrent => {
                        generated.push(Component::IntegerInput {
                            id,
                            label: text("Integer candidate"),
                            help: None,
                            editor_id: editor_id.clone(),
                            applicability: Applicability::MatchingType,
                        });
                        generated.push(Component::TextInput {
                            id: self.ids.allocate(Category::Component)?,
                            label: text("Text candidate"),
                            help: None,
                            editor_id,
                            applicability: Applicability::MatchingType,
                        });
                    }
                    _ => unreachable!("only editable templates have editors"),
                }
            }
            generated.push(Component::Button {
                id: self.ids.allocate(Category::Component)?,
                label: text("Action"),
                help: None,
                action_id: Some(action_id),
                disabled_reason: None,
            });
        } else {
            generated.push(Component::Status {
                id: self.ids.allocate(Category::Component)?,
                label: text("Target"),
                help: None,
                source: StatusSource::Entity {
                    target: target(&control.target),
                },
            });
            generated.push(Component::Button {
                id: self.ids.allocate(Category::Component)?,
                label: text("Unavailable"),
                help: None,
                action_id: None,
                disabled_reason: Some(text("Unsupported v1 control kind/target")),
            });
        }
        for child in &generated {
            push(&mut children, child.id().clone())?;
        }
        push(
            &mut self.dto.components,
            Component::Group {
                id: text(&control.id),
                label: text(&control.label),
                help: None,
                layout: Layout::Column,
                children,
                v1_control: Some(origin),
            },
        )?;
        for child in generated {
            push(&mut self.dto.components, child)?;
        }
        Ok(())
    }
    fn plots(&mut self, source: &v1::PresentationDocument) -> Result<()> {
        for plot in &source.plots {
            let mut traces = List(Vec::new());
            for trace in &plot.traces {
                let data_id = self.data(Selector::SignalSample, &trace.source)?;
                push(
                    &mut traces,
                    Trace {
                        id: text(&trace.id),
                        data_id,
                        display_label: text(&trace.display_label),
                        visible: trace.visible,
                        style: TraceStyle {
                            color: text(&trace.style.color),
                            width: Number(trace.style.width),
                        },
                        display_unit: trace.display_unit.as_deref().map(text),
                    },
                )?;
            }
            push(
                &mut self.dto.plots,
                Plot {
                    id: text(&plot.id),
                    title: text(&plot.title),
                    time_window_seconds: Number(plot.time_window_seconds),
                    axes: Axes {
                        time_mode: plot.axes.time_mode,
                        y_min: plot.axes.y_min.map(Number),
                        y_max: plot.axes.y_max.map(Number),
                    },
                    traces,
                },
            )?;
        }
        Ok(())
    }
    fn windows(&mut self, source: &v1::PresentationDocument) -> Result<()> {
        for window in &source.windows {
            let mut tabs = List(Vec::new());
            for tab in &window.tabs {
                let mut panels = List(Vec::new());
                for panel in &tab.panels {
                    let mut components = List(Vec::new());
                    let kind = match &panel.kind {
                        v1::PanelKind::Controls { control_ids } => {
                            for id in control_ids {
                                push(&mut components, text(id))?;
                            }
                            V1PanelKind::Controls
                        }
                        v1::PanelKind::Plot { plot_id } => {
                            let plot = source
                                .plots
                                .iter()
                                .find(|plot| &plot.id == plot_id)
                                .expect("validated v1 plot reference");
                            let id = self.ids.allocate(Category::Component)?;
                            push(&mut components, id.clone())?;
                            push(
                                &mut self.dto.components,
                                Component::Plot {
                                    id,
                                    label: text(&plot.title),
                                    help: None,
                                    plot_id: text(plot_id),
                                },
                            )?;
                            V1PanelKind::Plot
                        }
                        v1::PanelKind::Status { source } => {
                            let id = self.ids.allocate(Category::Component)?;
                            push(&mut components, id.clone())?;
                            push(
                                &mut self.dto.components,
                                Component::Status {
                                    id,
                                    label: text(&panel.title),
                                    help: None,
                                    source: StatusSource::Entity {
                                        target: target(source),
                                    },
                                },
                            )?;
                            V1PanelKind::Status
                        }
                    };
                    push(
                        &mut panels,
                        Panel {
                            id: text(&panel.id),
                            title: text(&panel.title),
                            components,
                            v1_kind: Some(kind),
                        },
                    )?;
                }
                push(
                    &mut tabs,
                    Tab {
                        id: text(&tab.id),
                        title: text(&tab.title),
                        panels,
                    },
                )?;
            }
            push(
                &mut self.dto.windows,
                Window {
                    id: text(&window.id),
                    title: text(&window.title),
                    tabs,
                },
            )?;
        }
        Ok(())
    }
}

/// Implements contract 7.1 only. The borrowed v1 source cannot be changed, and no
/// original-document copy, converter index or partial result survives admission.
pub(crate) fn convert_v1(source: ValidatedV1Document<'_>) -> Result<AdmittedV2Document> {
    let source = source.0;
    let mut builder = Builder::new(source);
    for control in &source.controls {
        builder.control(control)?;
    }
    builder.plots(source)?;
    builder.windows(source)?;
    let Builder { dto, ids, recorder } = builder;
    drop(ids);
    drop(recorder);
    // The closed DTO is constructed only from validated v1 fields and fixed
    // templates. Every List insertion is bounded; no arbitrary DTO entry exists.
    // Full JSON scanner limits are checked by both wire probes during admission.
    // Do not serialize/reparse floats or retain a second decoded document.
    ParsedDocument(dto)
        .check_local()
        .and_then(|local| local.check_structure())
        .and_then(|structured| structured.check_admission())
        .map_err(|_| ConversionError::ConversionLimit)
}

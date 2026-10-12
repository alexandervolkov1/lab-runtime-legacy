//! Closed syntactic DTOs. Deserialization is not admission validation.

use super::bounded::{I64Decimal, List, Number, Text, nullable, optional};
use serde::Deserialize;

pub(crate) type Id = Text<true>;
pub(crate) type Label = Text<true>;

macro_rules! closed_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
        #[serde(remote = "Self", rename_all = "snake_case")]
        pub(crate) enum $name { $($variant),+ }
        string_only!($name);
    };
}
// Unit enums also have an externally tagged object representation in serde.
// The contract allows only a JSON string, never {"compact":null}, for example.
macro_rules! string_only {
    ($name:ident) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct StringVisitor;
                impl<'de> serde::de::Visitor<'de> for StringVisitor {
                    type Value = $name;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("a closed enum string")
                    }
                    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<$name, E> {
                        $name::deserialize(serde::de::value::StrDeserializer::<E>::new(value))
                    }
                }
                d.deserialize_str(StringVisitor)
            }
        }
    };
}
closed_enum!(ScalarKind {
    Boolean,
    Integer,
    Number,
    Text,
    Enum
});
closed_enum!(CandidateKind {
    Boolean,
    Integer,
    Number,
    Text,
    Enum,
    PropertyScalar
});
closed_enum!(Purpose {
    RecordingLabel,
    ReferenceFixed,
    PropertyValue,
    LocalOnly
});
closed_enum!(Selector {
    SignalSample,
    ConfigurationPropertyCurrent,
    ReferenceValue,
    ControllerState,
    RecorderState
});
closed_enum!(Density {
    Comfortable,
    Compact
});
closed_enum!(Layout { Row, Column });
closed_enum!(V1PanelKind {
    Controls,
    Plot,
    Status
});
closed_enum!(V1ControlKind {
    RecordingLifecycle,
    SetReference,
    ControllerLifecycle,
    ApplyConfiguration,
    ReconnectResource
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(remote = "Self", rename_all = "snake_case")]
pub(crate) enum Applicability {
    #[default]
    Always,
    MatchingType,
}
string_only!(Applicability);

#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct DocumentDto {
    #[serde(deserialize_with = "format_version")]
    pub(crate) format_version: u8,
    #[serde(deserialize_with = "binding_version")]
    pub(crate) binding_version: u8,
    pub(crate) document_id: Id,
    pub(crate) windows: List<Window, 64>,
    pub(crate) components: List<Component, 512>,
    pub(crate) data_bindings: List<DataBinding, 1152>,
    pub(crate) editor_bindings: List<EditorBinding, 64>,
    pub(crate) action_bindings: List<ActionBinding, 64>,
    pub(crate) plots: List<Plot, 32>,
    #[serde(default)]
    pub(crate) settings: Settings,
}

fn version<'de, D: serde::Deserializer<'de>, const EXPECTED: u8>(d: D) -> Result<u8, D::Error> {
    let value = u8::deserialize(d)?;
    if value == EXPECTED {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "unsupported document/binding version",
        ))
    }
}
fn format_version<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    version::<D, 2>(d)
}
fn binding_version<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    version::<D, 1>(d)
}

#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) density: Density,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            density: Density::Comfortable,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Window {
    pub(crate) id: Id,
    pub(crate) title: Label,
    pub(crate) tabs: List<Tab, 64>,
}
#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Tab {
    pub(crate) id: Id,
    pub(crate) title: Label,
    pub(crate) panels: List<Panel, 64>,
}
#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Panel {
    pub(crate) id: Id,
    pub(crate) title: Label,
    pub(crate) components: List<Id, 64>,
    #[serde(default, deserialize_with = "optional")]
    pub(crate) v1_kind: Option<V1PanelKind>,
}

// Separate v2 reference DTOs enforce bounded nonempty strings during decoding,
// without changing the acceptance or serde implementation of v1 references.
#[derive(Debug, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum RuntimeRef {
    Instrument {
        instrument: Id,
    },
    Signal {
        instrument: Id,
        parameter: Id,
    },
    Reference {
        reference: Id,
    },
    Controller {
        controller: Id,
    },
    Resource {
        resource: Id,
    },
    Component {
        component: Id,
    },
    Recorder {},
    ConfigurationProperty {
        owner: ConfigurationOwner,
        property: Id,
    },
}
#[derive(Debug, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum ConfigurationOwner {
    Instrument { instrument: Id },
    Component { component: Id },
    Resource { resource: Id },
}

#[derive(Debug, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Component {
    Group {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        layout: Layout,
        children: List<Id, 64>,
        #[serde(default, deserialize_with = "optional")]
        v1_control: Option<V1ControlOrigin>,
    },
    Button {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        #[serde(deserialize_with = "nullable")]
        action_id: Option<Id>,
        #[serde(default, deserialize_with = "optional")]
        disabled_reason: Option<Label>,
    },
    Toggle {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        editor_id: Id,
        #[serde(default)]
        applicability: Applicability,
    },
    IntegerInput {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        editor_id: Id,
        #[serde(default)]
        applicability: Applicability,
    },
    NumberInput {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        editor_id: Id,
        #[serde(default)]
        applicability: Applicability,
    },
    TextInput {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        editor_id: Id,
        #[serde(default)]
        applicability: Applicability,
    },
    Select {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        editor_id: Id,
        #[serde(default)]
        applicability: Applicability,
    },
    Indicator {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        data_id: Id,
        #[serde(default, deserialize_with = "optional")]
        precision: Option<u8>,
    },
    Status {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        source: StatusSource,
    },
    Plot {
        id: Id,
        label: Label,
        #[serde(default, deserialize_with = "optional")]
        help: Option<Label>,
        plot_id: Id,
    },
}

#[derive(Debug, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum StatusSource {
    Data { data_id: Id },
    Entity { target: RuntimeRef },
}

#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct V1ControlOrigin {
    pub(crate) kind: V1ControlKind,
    pub(crate) target: RuntimeRef,
    pub(crate) confirm: bool,
}

#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Plot {
    pub(crate) id: Id,
    pub(crate) title: Label,
    pub(crate) time_window_seconds: Number,
    pub(crate) axes: Axes,
    pub(crate) traces: List<Trace, 32>,
}
#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Axes {
    #[serde(deserialize_with = "nullable")]
    pub(crate) y_min: Option<Number>,
    #[serde(deserialize_with = "nullable")]
    pub(crate) y_max: Option<Number>,
}
#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Trace {
    pub(crate) id: Id,
    pub(crate) data_id: Id,
    pub(crate) display_label: Label,
    pub(crate) visible: bool,
    pub(crate) style: TraceStyle,
    #[serde(deserialize_with = "nullable")]
    pub(crate) display_unit: Option<Label>,
}
#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct TraceStyle {
    pub(crate) color: Label,
    pub(crate) width: Number,
}

#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct DataBinding {
    pub(crate) id: Id,
    pub(crate) selector: Selector,
    pub(crate) target: RuntimeRef,
    pub(crate) expect_kind: Option<ScalarKind>,
}
#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct EditorBinding {
    pub(crate) id: Id,
    pub(crate) data_id: Id,
    pub(crate) purpose: Purpose,
    pub(crate) candidate_kind: CandidateKind,
    #[serde(default)]
    pub(crate) constraints: Constraints,
}
#[derive(Debug, Default, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Constraints {
    #[serde(default, deserialize_with = "optional")]
    pub(crate) integer_min: Option<I64Decimal>,
    #[serde(default, deserialize_with = "optional")]
    pub(crate) integer_max: Option<I64Decimal>,
    #[serde(default, deserialize_with = "optional")]
    pub(crate) number_min: Option<Number>,
    #[serde(default, deserialize_with = "optional")]
    pub(crate) number_max: Option<Number>,
    #[serde(default, deserialize_with = "optional")]
    pub(crate) max_utf8_bytes: Option<u16>,
    #[serde(default, deserialize_with = "optional")]
    pub(crate) choices: Option<List<Choice, 64>>,
}
#[derive(Debug, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub(crate) struct Choice {
    pub(crate) token: Text<false>,
    pub(crate) label: Label,
}

#[derive(Debug, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum TypedScalar {
    Boolean { value: bool },
    Integer { value: I64Decimal },
    Number { value: Number },
    Text { value: Text<false> },
    Enum { value: Text<false> },
}
#[derive(Debug, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Parameter {
    Editor { editor_id: Id },
    Literal { value: TypedScalar },
}

#[derive(Debug, Deserialize)]
#[serde(
    remote = "Self",
    tag = "family",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum ActionBinding {
    RecorderLifecycle {
        id: Id,
        target: RuntimeRef,
        label: Parameter,
    },
    ReferenceFixed {
        id: Id,
        target: RuntimeRef,
        value: Parameter,
    },
    ControllerLifecycle {
        id: Id,
        target: RuntimeRef,
    },
    PropertyConfigure {
        id: Id,
        target: RuntimeRef,
        value: Parameter,
    },
}

// Serde's struct/adjacent-enum derives also support positional sequences. JSON
// DTOs require objects, including all-default Constraints. Force map entry before
// delegating field/tag decoding to the remote derive, preserving strict shapes.
macro_rules! map_only {
    ($($name:ident),+ $(,)?) => {$(
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct ObjectVisitor;
                impl<'de> serde::de::Visitor<'de> for ObjectVisitor {
                    type Value = $name;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str("a JSON object")
                    }
                    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<$name, A::Error> {
                        $name::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    }
                }
                d.deserialize_map(ObjectVisitor)
            }
        }
    )+};
}
map_only!(
    DocumentDto,
    Settings,
    Window,
    Tab,
    Panel,
    RuntimeRef,
    ConfigurationOwner,
    Component,
    StatusSource,
    V1ControlOrigin,
    Plot,
    Axes,
    Trace,
    TraceStyle,
    DataBinding,
    EditorBinding,
    Constraints,
    Choice,
    TypedScalar,
    Parameter,
    ActionBinding
);

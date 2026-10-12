//! Explicit schema order and optional/default normalization, shared by both encodings.
//! No intermediate Value/map tree and no copies of document-owned strings or arrays.

use super::{
    bounded::{I64Decimal, List, Number, Text, U64Decimal},
    dto::*,
};
use serde::{
    Serialize, Serializer,
    ser::{Error, SerializeMap},
};

impl<const N: bool> Serialize for Text<N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}
impl<T: Serialize, const N: usize> Serialize for List<T, N> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}
impl Serialize for I64Decimal {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}
impl Serialize for U64Decimal {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}
impl Serialize for Number {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if !self.0.is_finite() {
            return Err(S::Error::custom("nonfinite Number"));
        }
        s.serialize_f64(self.0)
    }
}

macro_rules! strings {
    ($ty:ident { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok,S::Error> {
                s.serialize_str(match self { $(Self::$variant => $name),+ })
            }
        }
    };
}
strings!(ScalarKind { Boolean=>"boolean",Integer=>"integer",Number=>"number",Text=>"text",Enum=>"enum" });
strings!(CandidateKind { Boolean=>"boolean",Integer=>"integer",Number=>"number",Text=>"text",Enum=>"enum",PropertyScalar=>"property_scalar" });
strings!(Purpose { RecordingLabel=>"recording_label",ReferenceFixed=>"reference_fixed",PropertyValue=>"property_value",LocalOnly=>"local_only" });
strings!(Selector { SignalSample=>"signal_sample",ConfigurationPropertyCurrent=>"configuration_property_current",ReferenceValue=>"reference_value",ControllerState=>"controller_state",RecorderState=>"recorder_state" });
strings!(Density { Comfortable=>"comfortable",Compact=>"compact" });
strings!(Layout { Row=>"row",Column=>"column" });
strings!(V1PanelKind { Controls=>"controls",Plot=>"plot",Status=>"status" });
strings!(V1ControlKind { RecordingLifecycle=>"recording_lifecycle",SetReference=>"set_reference",ControllerLifecycle=>"controller_lifecycle",ApplyConfiguration=>"apply_configuration",ReconnectResource=>"reconnect_resource" });
strings!(Applicability { Always=>"always",MatchingType=>"matching_type" });

// Required nullable fields are in the first list, so absent payloads emit null.
// Optional absence is omitted. Concrete defaults are emitted, including {}, null
// expect_kind, comfortable settings, and always applicability.
macro_rules! object {
    ($ty:ident; $($required:ident),*; $($optional:ident),*) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok,S::Error> {
                let mut map = s.serialize_map(None)?;
                $(map.serialize_entry(stringify!($required), &self.$required)?;)*
                $(if let Some(value) = &self.$optional { map.serialize_entry(stringify!($optional),value)?; })*
                map.end()
            }
        }
    };
}
object!(DocumentDto; format_version,binding_version,document_id,windows,components,data_bindings,editor_bindings,action_bindings,plots,settings;);
object!(Settings; density;);
object!(Window; id,title,tabs;);
object!(Tab; id,title,panels;);
object!(Panel; id,title,components; v1_kind);
object!(V1ControlOrigin; kind,target,confirm;);
object!(Plot; id,title,time_window_seconds,axes,traces;);
object!(Axes; y_min,y_max;);
object!(Trace; id,data_id,display_label,visible,style,display_unit;);
object!(TraceStyle; color,width;);
object!(DataBinding; id,selector,target,expect_kind;);
object!(EditorBinding; id,data_id,purpose,candidate_kind,constraints;);
object!(Constraints; ; integer_min,integer_max,number_min,number_max,max_utf8_bytes,choices);
object!(Choice; token,label;);

macro_rules! fields {
    ($s:expr; $($key:literal => $value:expr),* $(,)?) => {{
        let mut map = $s.serialize_map(None)?;
        $(map.serialize_entry($key, $value)?;)*
        map.end()
    }};
}
impl Serialize for RuntimeRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Instrument { instrument } => {
                fields!(s; "kind"=>"instrument","instrument"=>instrument)
            }
            Self::Signal {
                instrument,
                parameter,
            } => fields!(s; "kind"=>"signal","instrument"=>instrument,"parameter"=>parameter),
            Self::Reference { reference } => fields!(s; "kind"=>"reference","reference"=>reference),
            Self::Controller { controller } => {
                fields!(s; "kind"=>"controller","controller"=>controller)
            }
            Self::Resource { resource } => fields!(s; "kind"=>"resource","resource"=>resource),
            Self::Component { component } => fields!(s; "kind"=>"component","component"=>component),
            Self::Recorder {} => fields!(s; "kind"=>"recorder"),
            Self::ConfigurationProperty { owner, property } => {
                fields!(s; "kind"=>"configuration_property","owner"=>owner,"property"=>property)
            }
        }
    }
}
impl Serialize for ConfigurationOwner {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Instrument { instrument } => {
                fields!(s; "kind"=>"instrument","instrument"=>instrument)
            }
            Self::Component { component } => fields!(s; "kind"=>"component","component"=>component),
            Self::Resource { resource } => fields!(s; "kind"=>"resource","resource"=>resource),
        }
    }
}
impl Serialize for StatusSource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Data { data_id } => fields!(s; "kind"=>"data","data_id"=>data_id),
            Self::Entity { target } => fields!(s; "kind"=>"entity","target"=>target),
        }
    }
}
impl Serialize for TypedScalar {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Boolean { value } => fields!(s; "kind"=>"boolean","value"=>value),
            Self::Integer { value } => fields!(s; "kind"=>"integer","value"=>value),
            Self::Number { value } => fields!(s; "kind"=>"number","value"=>value),
            Self::Text { value } => fields!(s; "kind"=>"text","value"=>value),
            Self::Enum { value } => fields!(s; "kind"=>"enum","value"=>value),
        }
    }
}
impl Serialize for Parameter {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Editor { editor_id } => fields!(s; "kind"=>"editor","editor_id"=>editor_id),
            Self::Literal { value } => fields!(s; "kind"=>"literal","value"=>value),
        }
    }
}
impl Serialize for ActionBinding {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::RecorderLifecycle { id, target, label } => {
                fields!(s; "id"=>id,"family"=>"recorder_lifecycle","target"=>target,"label"=>label)
            }
            Self::ReferenceFixed { id, target, value } => {
                fields!(s; "id"=>id,"family"=>"reference_fixed","target"=>target,"value"=>value)
            }
            Self::ControllerLifecycle { id, target } => {
                fields!(s; "id"=>id,"family"=>"controller_lifecycle","target"=>target)
            }
            Self::PropertyConfigure { id, target, value } => {
                fields!(s; "id"=>id,"family"=>"property_configure","target"=>target,"value"=>value)
            }
        }
    }
}

macro_rules! component {
    ($s:expr,$kind:literal,$id:ident,$label:ident,$help:ident; $($required:ident),*; $($optional:ident),*) => {{
        let mut map = $s.serialize_map(None)?;
        map.serialize_entry("id",$id)?;
        map.serialize_entry("kind",$kind)?;
        map.serialize_entry("label",$label)?;
        if let Some(help) = $help { map.serialize_entry("help",help)?; }
        $(map.serialize_entry(stringify!($required),$required)?;)*
        $(if let Some(value) = $optional { map.serialize_entry(stringify!($optional),value)?; })*
        map.end()
    }};
}
impl Serialize for Component {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Group {
                id,
                label,
                help,
                layout,
                children,
                v1_control,
            } => component!(s,"group",id,label,help;layout,children;v1_control),
            Self::Button {
                id,
                label,
                help,
                action_id,
                disabled_reason,
            } => component!(s,"button",id,label,help;action_id;disabled_reason),
            Self::Toggle {
                id,
                label,
                help,
                editor_id,
                applicability,
            } => component!(s,"toggle",id,label,help;editor_id,applicability;),
            Self::IntegerInput {
                id,
                label,
                help,
                editor_id,
                applicability,
            } => component!(s,"integer_input",id,label,help;editor_id,applicability;),
            Self::NumberInput {
                id,
                label,
                help,
                editor_id,
                applicability,
            } => component!(s,"number_input",id,label,help;editor_id,applicability;),
            Self::TextInput {
                id,
                label,
                help,
                editor_id,
                applicability,
            } => component!(s,"text_input",id,label,help;editor_id,applicability;),
            Self::Select {
                id,
                label,
                help,
                editor_id,
                applicability,
            } => component!(s,"select",id,label,help;editor_id,applicability;),
            Self::Indicator {
                id,
                label,
                help,
                data_id,
                precision,
            } => component!(s,"indicator",id,label,help;data_id;precision),
            Self::Status {
                id,
                label,
                help,
                source,
            } => component!(s,"status",id,label,help;source;),
            Self::Plot {
                id,
                label,
                help,
                plot_id,
            } => component!(s,"plot",id,label,help;plot_id;),
        }
    }
}

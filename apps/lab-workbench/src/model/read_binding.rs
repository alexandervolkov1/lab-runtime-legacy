//! Closed, renderer-neutral decoding of read-only Runtime scalar projections.

use super::{Freshness, RuntimeObservations};
use crate::presentation::RuntimeRef;
use serde_json::Value;

const MAX_UNIT_ID_BYTES: usize = 32;
const MAX_UNIT_SYMBOL_BYTES: usize = 16;

/// A finite semantic selector understood by Workbench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadBinding {
    SignalSample,
    ConfigurationPropertyCurrent,
    ReferenceValue,
    ControllerState,
    RecorderState,
}

impl ReadBinding {
    /// Selects only meanings fixed by an existing Runtime target category.
    pub(crate) fn for_target(target: &RuntimeRef) -> Option<Self> {
        match target {
            RuntimeRef::Signal { .. } => Some(Self::SignalSample),
            RuntimeRef::ConfigurationProperty { .. } => Some(Self::ConfigurationPropertyCurrent),
            RuntimeRef::Reference { .. } => Some(Self::ReferenceValue),
            RuntimeRef::Controller { .. } => Some(Self::ControllerState),
            RuntimeRef::Recorder => Some(Self::RecorderState),
            RuntimeRef::Instrument { .. }
            | RuntimeRef::Resource { .. }
            | RuntimeRef::Component { .. } => None,
        }
    }
}

/// Exact scalar type, independent of its JSON representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarKind {
    Boolean,
    Integer,
    Number,
    Text,
    Enum,
}

impl ScalarKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Text => "text",
            Self::Enum => "enum",
        }
    }
}

/// Decoded scalar with no implicit conversions between variants.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScalarValue {
    Boolean(bool),
    Integer(i64),
    Number(f64),
    Text(String),
    Enum(String),
}

/// Sample quality is independent of observation-cache freshness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadQuality {
    Good,
    Unavailable,
    NotObserved,
}

/// Explicit absence or incompatibility; none of these carries a default value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadAbsence {
    MissingObservation,
    MissingValue,
    Unavailable,
    NotObserved,
    MissingMetadata,
    IncompatibleMetadata,
    Unsupported,
}

impl ReadAbsence {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::MissingObservation => "no Runtime observation",
            Self::MissingValue => "value absent",
            Self::Unavailable => "unavailable",
            Self::NotObserved => "not observed",
            Self::MissingMetadata => "typed metadata absent",
            Self::IncompatibleMetadata => "value and metadata are incompatible",
            Self::Unsupported => "unsupported typed value",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReadUnit {
    pub(crate) id: String,
    pub(crate) symbol: String,
}

impl ReadUnit {
    pub(crate) fn display(&self) -> Option<&str> {
        if !self.symbol.is_empty() {
            Some(&self.symbol)
        } else if !self.id.is_empty() {
            Some(&self.id)
        } else {
            None
        }
    }

    fn retained_bytes(&self) -> usize {
        self.id.len() + self.symbol.len()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ReadPayload {
    Value(ScalarValue),
    Absent(ReadAbsence),
}

/// Frame-local result. The cache retains raw projections and bounded descriptors,
/// never a second history or a renderer-owned authoritative value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TypedRead {
    pub(crate) binding: ReadBinding,
    pub(crate) kind: Option<ScalarKind>,
    pub(crate) payload: ReadPayload,
    pub(crate) unit: Option<ReadUnit>,
    pub(crate) quality: Option<ReadQuality>,
    pub(crate) freshness: Freshness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DescriptorKind {
    Typed(ScalarKind),
    Missing,
    Incompatible,
    Unsupported,
}

/// Minimal decoded discovery metadata retained separately from current samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReadDescriptor {
    kind: DescriptorKind,
    unit: Result<Option<ReadUnit>, ReadAbsence>,
}

impl ReadDescriptor {
    pub(super) fn from_signal_discovery(value: &Value) -> Self {
        Self {
            kind: descriptor_kind(value.get("value_type"), "float"),
            unit: decode_unit(value.get("unit")),
        }
    }

    pub(super) fn retained_bytes(&self) -> usize {
        1 + self
            .unit
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .map_or(0, ReadUnit::retained_bytes)
    }
}

/// Resolves one known source from the existing observation cache only.
pub(crate) fn resolve_read(
    observations: &RuntimeObservations,
    target: &RuntimeRef,
    binding: ReadBinding,
) -> TypedRead {
    let freshness = effective_freshness(observations, target);
    if !binding_matches(binding, target) {
        return absent(
            binding,
            None,
            ReadAbsence::IncompatibleMetadata,
            None,
            None,
            freshness,
        );
    }
    match binding {
        ReadBinding::SignalSample => resolve_signal(observations, target, freshness),
        ReadBinding::ConfigurationPropertyCurrent => {
            resolve_property(observations, target, freshness)
        }
        ReadBinding::ReferenceValue => resolve_known_field(
            observations,
            target,
            binding,
            ScalarKind::Number,
            "value",
            freshness,
        ),
        ReadBinding::ControllerState | ReadBinding::RecorderState => resolve_known_field(
            observations,
            target,
            binding,
            ScalarKind::Enum,
            "state",
            freshness,
        ),
    }
}

fn resolve_signal(
    observations: &RuntimeObservations,
    target: &RuntimeRef,
    freshness: Freshness,
) -> TypedRead {
    let descriptor = observations.read_descriptors.get(target);
    let observation = observations.entities.get(target);
    if descriptor.is_none() && observation.is_none() {
        return absent(
            ReadBinding::SignalSample,
            None,
            ReadAbsence::MissingObservation,
            None,
            None,
            freshness,
        );
    }
    let (kind, unit) = match descriptor {
        Some(descriptor) => {
            let kind = match descriptor.descriptor.kind {
                DescriptorKind::Typed(kind) => kind,
                DescriptorKind::Missing => {
                    return absent(
                        ReadBinding::SignalSample,
                        None,
                        ReadAbsence::MissingMetadata,
                        None,
                        None,
                        freshness,
                    );
                }
                DescriptorKind::Incompatible => {
                    return absent(
                        ReadBinding::SignalSample,
                        None,
                        ReadAbsence::IncompatibleMetadata,
                        None,
                        None,
                        freshness,
                    );
                }
                DescriptorKind::Unsupported => {
                    return absent(
                        ReadBinding::SignalSample,
                        None,
                        ReadAbsence::Unsupported,
                        None,
                        None,
                        freshness,
                    );
                }
            };
            let unit = match &descriptor.descriptor.unit {
                Ok(unit) => unit.clone(),
                Err(reason) => {
                    return absent(
                        ReadBinding::SignalSample,
                        Some(kind),
                        *reason,
                        None,
                        None,
                        freshness,
                    );
                }
            };
            (kind, unit)
        }
        None => {
            return absent(
                ReadBinding::SignalSample,
                None,
                ReadAbsence::MissingMetadata,
                None,
                None,
                freshness,
            );
        }
    };
    let Some(observation) = observation else {
        return absent(
            ReadBinding::SignalSample,
            Some(kind),
            ReadAbsence::MissingObservation,
            unit,
            None,
            freshness,
        );
    };
    // Discovery describes a signal but is not a measurement attempt.
    if observation.value.get("signal").is_none() {
        return absent(
            ReadBinding::SignalSample,
            Some(kind),
            ReadAbsence::MissingObservation,
            unit,
            None,
            freshness,
        );
    }
    let RuntimeRef::Signal {
        instrument,
        parameter,
    } = target
    else {
        unreachable!("binding/target compatibility checked before signal decoding")
    };
    if observation
        .value
        .pointer("/signal/instrument")
        .and_then(Value::as_str)
        != Some(instrument.as_str())
        || observation
            .value
            .pointer("/signal/parameter")
            .and_then(Value::as_str)
            != Some(parameter.as_str())
    {
        return absent(
            ReadBinding::SignalSample,
            Some(kind),
            ReadAbsence::IncompatibleMetadata,
            unit,
            None,
            freshness,
        );
    }
    let sample_unit = match decode_unit(observation.value.get("unit")) {
        Ok(unit) => unit,
        Err(reason) => {
            return absent(
                ReadBinding::SignalSample,
                Some(kind),
                reason,
                unit,
                None,
                freshness,
            );
        }
    };
    if unit.is_some() && sample_unit.is_some() && unit != sample_unit {
        return absent(
            ReadBinding::SignalSample,
            Some(kind),
            ReadAbsence::IncompatibleMetadata,
            sample_unit,
            None,
            freshness,
        );
    }
    let unit = sample_unit.or(unit);
    match observation.value.get("quality").and_then(Value::as_str) {
        Some("good") => {
            if observation.value.get("status").and_then(Value::as_str) != Some("available") {
                return absent(
                    ReadBinding::SignalSample,
                    Some(kind),
                    ReadAbsence::IncompatibleMetadata,
                    unit,
                    Some(ReadQuality::Good),
                    freshness,
                );
            }
            let Some(value) = observation
                .value
                .get("value")
                .filter(|value| !value.is_null())
            else {
                return absent(
                    ReadBinding::SignalSample,
                    Some(kind),
                    ReadAbsence::MissingValue,
                    unit,
                    Some(ReadQuality::Good),
                    freshness,
                );
            };
            match decode_scalar(kind, value, IntegerEncoding::DecimalString) {
                Some(value) => present(
                    ReadBinding::SignalSample,
                    value,
                    unit,
                    Some(ReadQuality::Good),
                    freshness,
                ),
                None => absent(
                    ReadBinding::SignalSample,
                    Some(kind),
                    ReadAbsence::IncompatibleMetadata,
                    unit,
                    Some(ReadQuality::Good),
                    freshness,
                ),
            }
        }
        Some("unavailable") => {
            if observation
                .value
                .get("value")
                .is_some_and(|value| !value.is_null())
            {
                return absent(
                    ReadBinding::SignalSample,
                    Some(kind),
                    ReadAbsence::IncompatibleMetadata,
                    unit,
                    Some(ReadQuality::Unavailable),
                    freshness,
                );
            }
            let not_observed = match observation.value.get("status").and_then(Value::as_str) {
                Some("not_observed") => true,
                Some("unavailable") => false,
                _ => {
                    return absent(
                        ReadBinding::SignalSample,
                        Some(kind),
                        ReadAbsence::IncompatibleMetadata,
                        unit,
                        Some(ReadQuality::Unavailable),
                        freshness,
                    );
                }
            };
            absent(
                ReadBinding::SignalSample,
                Some(kind),
                if not_observed {
                    ReadAbsence::NotObserved
                } else {
                    ReadAbsence::Unavailable
                },
                unit,
                Some(if not_observed {
                    ReadQuality::NotObserved
                } else {
                    ReadQuality::Unavailable
                }),
                freshness,
            )
        }
        _ => absent(
            ReadBinding::SignalSample,
            Some(kind),
            ReadAbsence::IncompatibleMetadata,
            unit,
            None,
            freshness,
        ),
    }
}

fn resolve_property(
    observations: &RuntimeObservations,
    target: &RuntimeRef,
    freshness: Freshness,
) -> TypedRead {
    let Some(observation) = observations.entities.get(target) else {
        return absent(
            ReadBinding::ConfigurationPropertyCurrent,
            None,
            ReadAbsence::MissingObservation,
            None,
            None,
            freshness,
        );
    };
    let kind = match descriptor_kind(observation.value.get("value_type"), "number") {
        DescriptorKind::Typed(kind) => kind,
        DescriptorKind::Missing => {
            return absent(
                ReadBinding::ConfigurationPropertyCurrent,
                None,
                ReadAbsence::MissingMetadata,
                None,
                None,
                freshness,
            );
        }
        DescriptorKind::Incompatible => {
            return absent(
                ReadBinding::ConfigurationPropertyCurrent,
                None,
                ReadAbsence::IncompatibleMetadata,
                None,
                None,
                freshness,
            );
        }
        DescriptorKind::Unsupported => {
            return absent(
                ReadBinding::ConfigurationPropertyCurrent,
                None,
                ReadAbsence::Unsupported,
                None,
                None,
                freshness,
            );
        }
    };
    let unit = match decode_unit(observation.value.get("unit")) {
        Ok(unit) => unit,
        Err(reason) => {
            return absent(
                ReadBinding::ConfigurationPropertyCurrent,
                Some(kind),
                reason,
                None,
                None,
                freshness,
            );
        }
    };
    let Some(value) = observation
        .value
        .get("current")
        .filter(|value| !value.is_null())
    else {
        return absent(
            ReadBinding::ConfigurationPropertyCurrent,
            Some(kind),
            ReadAbsence::MissingValue,
            unit,
            None,
            freshness,
        );
    };
    match decode_scalar(kind, value, IntegerEncoding::JsonInteger) {
        Some(value) => present(
            ReadBinding::ConfigurationPropertyCurrent,
            value,
            unit,
            None,
            freshness,
        ),
        None => absent(
            ReadBinding::ConfigurationPropertyCurrent,
            Some(kind),
            ReadAbsence::IncompatibleMetadata,
            unit,
            None,
            freshness,
        ),
    }
}

fn resolve_known_field(
    observations: &RuntimeObservations,
    target: &RuntimeRef,
    binding: ReadBinding,
    kind: ScalarKind,
    field: &str,
    freshness: Freshness,
) -> TypedRead {
    let Some(observation) = observations.entities.get(target) else {
        return absent(
            binding,
            Some(kind),
            ReadAbsence::MissingObservation,
            None,
            None,
            freshness,
        );
    };
    let unit = match decode_unit(observation.value.get("unit")) {
        Ok(unit) => unit,
        Err(reason) => {
            return absent(binding, Some(kind), reason, None, None, freshness);
        }
    };
    let Some(value) = observation
        .value
        .get(field)
        .filter(|value| !value.is_null())
    else {
        return absent(
            binding,
            Some(kind),
            ReadAbsence::MissingValue,
            unit,
            None,
            freshness,
        );
    };
    match decode_scalar(kind, value, IntegerEncoding::JsonInteger) {
        Some(value) => present(binding, value, unit, None, freshness),
        None => absent(
            binding,
            Some(kind),
            ReadAbsence::IncompatibleMetadata,
            unit,
            None,
            freshness,
        ),
    }
}

#[derive(Clone, Copy)]
enum IntegerEncoding {
    JsonInteger,
    DecimalString,
}

fn decode_scalar(kind: ScalarKind, value: &Value, integer: IntegerEncoding) -> Option<ScalarValue> {
    match kind {
        ScalarKind::Boolean => value.as_bool().map(ScalarValue::Boolean),
        ScalarKind::Integer => match integer {
            IntegerEncoding::JsonInteger => value.as_i64().map(ScalarValue::Integer),
            IntegerEncoding::DecimalString => value
                .as_str()
                .and_then(|value| value.parse::<i64>().ok())
                .map(ScalarValue::Integer),
        },
        ScalarKind::Number => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(ScalarValue::Number),
        ScalarKind::Text => value
            .as_str()
            .map(|value| ScalarValue::Text(value.to_owned())),
        ScalarKind::Enum => value
            .as_str()
            .map(|value| ScalarValue::Enum(value.to_owned())),
    }
}

fn descriptor_kind(value: Option<&Value>, number_name: &str) -> DescriptorKind {
    match value {
        Some(Value::String(value)) => match value.as_str() {
            "boolean" => DescriptorKind::Typed(ScalarKind::Boolean),
            "integer" => DescriptorKind::Typed(ScalarKind::Integer),
            value if value == number_name => DescriptorKind::Typed(ScalarKind::Number),
            "text" => DescriptorKind::Typed(ScalarKind::Text),
            "enum" => DescriptorKind::Typed(ScalarKind::Enum),
            _ => DescriptorKind::Unsupported,
        },
        None => DescriptorKind::Missing,
        Some(_) => DescriptorKind::Incompatible,
    }
}

fn decode_unit(value: Option<&Value>) -> Result<Option<ReadUnit>, ReadAbsence> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let Some(object) = value.as_object() else {
        return Err(ReadAbsence::IncompatibleMetadata);
    };
    let text = |field: &str, limit: usize| -> Result<String, ReadAbsence> {
        match object.get(field) {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(value)) if value.len() <= limit => Ok(value.clone()),
            Some(_) => Err(ReadAbsence::IncompatibleMetadata),
        }
    };
    let unit = ReadUnit {
        id: text("id", MAX_UNIT_ID_BYTES)?,
        symbol: text("symbol", MAX_UNIT_SYMBOL_BYTES)?,
    };
    Ok((unit.display().is_some()).then_some(unit))
}

fn binding_matches(binding: ReadBinding, target: &RuntimeRef) -> bool {
    matches!(
        (binding, target),
        (ReadBinding::SignalSample, RuntimeRef::Signal { .. })
            | (
                ReadBinding::ConfigurationPropertyCurrent,
                RuntimeRef::ConfigurationProperty { .. }
            )
            | (ReadBinding::ReferenceValue, RuntimeRef::Reference { .. })
            | (ReadBinding::ControllerState, RuntimeRef::Controller { .. })
            | (ReadBinding::RecorderState, RuntimeRef::Recorder)
    )
}

fn effective_freshness(observations: &RuntimeObservations, target: &RuntimeRef) -> Freshness {
    observations
        .entities
        .get(target)
        .map_or(observations.freshness, |observation| {
            if observation.freshness == Freshness::Fresh {
                observations.freshness
            } else {
                observation.freshness
            }
        })
}

fn present(
    binding: ReadBinding,
    value: ScalarValue,
    unit: Option<ReadUnit>,
    quality: Option<ReadQuality>,
    freshness: Freshness,
) -> TypedRead {
    let kind = Some(match &value {
        ScalarValue::Boolean(_) => ScalarKind::Boolean,
        ScalarValue::Integer(_) => ScalarKind::Integer,
        ScalarValue::Number(_) => ScalarKind::Number,
        ScalarValue::Text(_) => ScalarKind::Text,
        ScalarValue::Enum(_) => ScalarKind::Enum,
    });
    TypedRead {
        binding,
        kind,
        payload: ReadPayload::Value(value),
        unit,
        quality,
        freshness,
    }
}

fn absent(
    binding: ReadBinding,
    kind: Option<ScalarKind>,
    reason: ReadAbsence,
    unit: Option<ReadUnit>,
    quality: Option<ReadQuality>,
    freshness: Freshness,
) -> TypedRead {
    TypedRead {
        binding,
        kind,
        payload: ReadPayload::Absent(reason),
        unit,
        quality,
        freshness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::projections::MAX_READ_DESCRIPTORS,
        presentation::{ConfigurationOwner, RuntimeRef},
    };
    use serde_json::json;

    fn property() -> RuntimeRef {
        RuntimeRef::ConfigurationProperty {
            owner: ConfigurationOwner::Instrument {
                instrument: "1".into(),
            },
            property: "value".into(),
        }
    }

    fn signal(parameter: &str) -> RuntimeRef {
        RuntimeRef::Signal {
            instrument: "1".into(),
            parameter: parameter.into(),
        }
    }

    fn resolved_property(kind: &str, current: Value) -> TypedRead {
        let target = property();
        let mut observations = RuntimeObservations::default();
        observations.observe(
            target.clone(),
            json!({"value_type":kind,"current":current,"unit":null}),
            None,
        );
        observations.complete_rebuild();
        resolve_read(
            &observations,
            &target,
            ReadBinding::ConfigurationPropertyCurrent,
        )
    }

    #[test]
    fn all_scalar_kinds_are_distinct_and_integer_is_exact() {
        for (kind, current, expected) in [
            ("boolean", json!(false), ScalarValue::Boolean(false)),
            ("integer", json!(i64::MIN), ScalarValue::Integer(i64::MIN)),
            ("integer", json!(i64::MAX), ScalarValue::Integer(i64::MAX)),
            ("number", json!(0.0), ScalarValue::Number(0.0)),
            ("text", json!(""), ScalarValue::Text(String::new())),
            ("enum", json!("idle"), ScalarValue::Enum("idle".into())),
        ] {
            let reading = resolved_property(kind, current);
            assert_eq!(reading.payload, ReadPayload::Value(expected));
            assert_eq!(reading.kind.unwrap().name(), kind);
        }

        assert_eq!(
            resolved_property("integer", json!(1.0)).payload,
            ReadPayload::Absent(ReadAbsence::IncompatibleMetadata)
        );
        assert_eq!(
            resolved_property("number", json!(1)).payload,
            ReadPayload::Value(ScalarValue::Number(1.0))
        );
    }

    #[test]
    fn signal_uses_discovery_type_and_runtime_measurement_encoding() {
        for (parameter, kind, value, expected) in [
            ("1", "boolean", json!(false), ScalarValue::Boolean(false)),
            (
                "2",
                "integer",
                json!(i64::MIN.to_string()),
                ScalarValue::Integer(i64::MIN),
            ),
            (
                "3",
                "integer",
                json!(i64::MAX.to_string()),
                ScalarValue::Integer(i64::MAX),
            ),
            ("4", "float", json!(0.0), ScalarValue::Number(0.0)),
            ("5", "text", json!(""), ScalarValue::Text(String::new())),
            (
                "6",
                "enum",
                json!("ready"),
                ScalarValue::Enum("ready".into()),
            ),
        ] {
            let target = signal(parameter);
            let mut observations = RuntimeObservations::default();
            observations.observe_read_descriptor(
                target.clone(),
                &json!({"value_type":kind,"unit":{"id":"1","symbol":""}}),
            );
            observations.observe(
                target.clone(),
                json!({"signal":{"instrument":"1","parameter":parameter},"quality":"good",
                    "status":"available","value":value,"unit":{"id":"1","symbol":""}}),
                None,
            );
            observations.complete_rebuild();
            let reading = resolve_read(&observations, &target, ReadBinding::SignalSample);
            assert_eq!(reading.payload, ReadPayload::Value(expected));
            assert_eq!(reading.quality, Some(ReadQuality::Good));
            assert_eq!(reading.unit.unwrap().display(), Some("1"));
        }

        let target = signal("7");
        let mut observations = RuntimeObservations::default();
        observations
            .observe_read_descriptor(target.clone(), &json!({"value_type":"integer","unit":null}));
        observations.observe(
            target.clone(),
            json!({"signal":{"instrument":"1","parameter":"7"},"quality":"good",
                "value":i64::MAX,"unit":null}),
            None,
        );
        observations.complete_rebuild();
        assert_eq!(
            resolve_read(&observations, &target, ReadBinding::SignalSample).payload,
            ReadPayload::Absent(ReadAbsence::IncompatibleMetadata),
            "measurement integers are decimal strings, never f64 or JSON integers"
        );
    }

    #[test]
    fn absence_quality_freshness_and_metadata_failures_remain_distinct() {
        let target = signal("1");
        let mut observations = RuntimeObservations::default();
        observations.observe_read_descriptor(
            target.clone(),
            &json!({"value_type":"boolean","unit":{"id":"ok","symbol":"u"}}),
        );
        assert_eq!(
            resolve_read(&observations, &target, ReadBinding::SignalSample).payload,
            ReadPayload::Absent(ReadAbsence::MissingObservation),
            "a descriptor does not establish a current sample"
        );
        observations.observe(
            target.clone(),
            json!({"signal":{"instrument":"1","parameter":"1"},"quality":"unavailable",
                "status":"not_observed","failure":"not_observed","value":null,
                "unit":{"id":"ok","symbol":"u"}}),
            None,
        );
        observations.complete_rebuild();
        let not_observed = resolve_read(&observations, &target, ReadBinding::SignalSample);
        assert_eq!(
            not_observed.payload,
            ReadPayload::Absent(ReadAbsence::NotObserved)
        );
        assert_eq!(not_observed.quality, Some(ReadQuality::NotObserved));
        assert_eq!(not_observed.freshness, Freshness::Fresh);

        observations.entities.get_mut(&target).unwrap().value["status"] = json!("unavailable");
        observations.entities.get_mut(&target).unwrap().value["failure"] = json!("sensor_fault");
        let unavailable = resolve_read(&observations, &target, ReadBinding::SignalSample);
        assert_eq!(
            unavailable.payload,
            ReadPayload::Absent(ReadAbsence::Unavailable)
        );
        assert_eq!(unavailable.quality, Some(ReadQuality::Unavailable));

        observations.begin_rebuild();
        let rebuilding = resolve_read(&observations, &target, ReadBinding::SignalSample);
        assert_eq!(
            rebuilding.payload,
            ReadPayload::Absent(ReadAbsence::Unavailable)
        );
        assert_eq!(rebuilding.freshness, Freshness::Stale);
        observations.freshness = Freshness::Rebuilding;
        observations.entities.get_mut(&target).unwrap().freshness = Freshness::Fresh;
        assert_eq!(
            resolve_read(&observations, &target, ReadBinding::SignalSample).freshness,
            Freshness::Rebuilding
        );

        let missing_target = signal("missing");
        assert_eq!(
            resolve_read(&observations, &missing_target, ReadBinding::SignalSample).payload,
            ReadPayload::Absent(ReadAbsence::MissingObservation)
        );
        observations.observe_read_descriptor(
            missing_target.clone(),
            &json!({"value_type":"bytes","unit":null}),
        );
        assert_eq!(
            resolve_read(&observations, &missing_target, ReadBinding::SignalSample).payload,
            ReadPayload::Absent(ReadAbsence::Unsupported)
        );
    }

    #[test]
    fn false_zero_empty_and_missing_values_are_not_conflated() {
        assert_eq!(
            resolved_property("boolean", json!(false)).payload,
            ReadPayload::Value(ScalarValue::Boolean(false))
        );
        assert_eq!(
            resolved_property("integer", json!(0)).payload,
            ReadPayload::Value(ScalarValue::Integer(0))
        );
        assert_eq!(
            resolved_property("text", json!("")).payload,
            ReadPayload::Value(ScalarValue::Text(String::new()))
        );
        assert_eq!(
            resolved_property("text", Value::Null).payload,
            ReadPayload::Absent(ReadAbsence::MissingValue)
        );

        let target = property();
        let observations = RuntimeObservations::default();
        assert_eq!(
            resolve_read(
                &observations,
                &target,
                ReadBinding::ConfigurationPropertyCurrent
            )
            .payload,
            ReadPayload::Absent(ReadAbsence::MissingObservation)
        );

        for (metadata, expected) in [
            (json!({"current":1}), ReadAbsence::MissingMetadata),
            (
                json!({"value_type":true,"current":1}),
                ReadAbsence::IncompatibleMetadata,
            ),
            (
                json!({"value_type":"bytes","current":"00"}),
                ReadAbsence::Unsupported,
            ),
            (
                json!({"value_type":"float","current":1.0}),
                ReadAbsence::Unsupported,
            ),
        ] {
            let mut observations = RuntimeObservations::default();
            observations.observe(target.clone(), metadata, None);
            observations.complete_rebuild();
            assert_eq!(
                resolve_read(
                    &observations,
                    &target,
                    ReadBinding::ConfigurationPropertyCurrent
                )
                .payload,
                ReadPayload::Absent(expected)
            );
        }
    }

    #[test]
    fn semantic_domain_sources_have_closed_types_and_target_matching() {
        let mut observations = RuntimeObservations::default();
        let reference = RuntimeRef::Reference {
            reference: "1".into(),
        };
        let controller = RuntimeRef::Controller {
            controller: "2".into(),
        };
        observations.observe(
            reference.clone(),
            json!({"value":1.5,"unit":{"id":"V","symbol":"V"}}),
            None,
        );
        observations.observe(controller.clone(), json!({"state":"paused"}), None);
        observations.observe(RuntimeRef::Recorder, json!({"state":"idle"}), None);
        observations.complete_rebuild();
        assert_eq!(
            resolve_read(&observations, &reference, ReadBinding::ReferenceValue).payload,
            ReadPayload::Value(ScalarValue::Number(1.5))
        );
        assert_eq!(
            resolve_read(&observations, &controller, ReadBinding::ControllerState).payload,
            ReadPayload::Value(ScalarValue::Enum("paused".into()))
        );
        assert_eq!(
            resolve_read(
                &observations,
                &RuntimeRef::Recorder,
                ReadBinding::RecorderState
            )
            .payload,
            ReadPayload::Value(ScalarValue::Enum("idle".into()))
        );
        assert_eq!(
            resolve_read(&observations, &controller, ReadBinding::ReferenceValue).payload,
            ReadPayload::Absent(ReadAbsence::IncompatibleMetadata)
        );
    }

    #[test]
    fn descriptor_cache_is_bounded_and_rebuild_retires_unseen_metadata() {
        let mut observations = RuntimeObservations::default();
        for index in 0..=MAX_READ_DESCRIPTORS {
            observations.observe_read_descriptor(
                signal(&index.to_string()),
                &json!({"value_type":"float","unit":{"id":"V","symbol":"V"}}),
            );
        }
        assert_eq!(observations.read_descriptors.len(), MAX_READ_DESCRIPTORS);
        assert!(observations.read_descriptor_overflowed);

        observations.begin_rebuild();
        assert!(!observations.read_descriptor_overflowed);
        let replacement = signal("replacement");
        observations.observe_read_descriptor(
            replacement.clone(),
            &json!({"value_type":"boolean","unit":null}),
        );
        observations.complete_rebuild();
        assert_eq!(observations.read_descriptors.len(), 1);
        assert!(observations.read_descriptors.contains_key(&replacement));

        observations.clear_read_descriptors();
        assert!(observations.read_descriptors.is_empty());
        assert!(!observations.read_descriptor_overflowed);
    }
}

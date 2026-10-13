//! One frame-local typed read at a time; no metadata refresh or observation copy.

use super::{TextBuffer, label};
use crate::{
    model::{
        Freshness, ReadBinding, ReadPayload, ReadQuality, RuntimeObservations, ScalarKind,
        ScalarValue, TypedRead, resolve_read,
    },
    presentation::{ConfigurationOwner, RuntimeRef, v2},
};
use std::io;

// Runtime text is not bounded by the presentation document's string validator.
// Refuse oversized selected scalar strings BEFORE resolve_read clones them.
const SCALAR_BYTES: usize = 512;
const ENTITY_BYTES: usize = 1024;

fn target(source: &v2::RuntimeRef) -> RuntimeRef {
    let text = |s: &v2::Id| s.as_str().to_owned();
    match source {
        v2::RuntimeRef::Instrument { instrument } => RuntimeRef::Instrument {
            instrument: text(instrument),
        },
        v2::RuntimeRef::Signal {
            instrument,
            parameter,
        } => RuntimeRef::Signal {
            instrument: text(instrument),
            parameter: text(parameter),
        },
        v2::RuntimeRef::Reference { reference } => RuntimeRef::Reference {
            reference: text(reference),
        },
        v2::RuntimeRef::Controller { controller } => RuntimeRef::Controller {
            controller: text(controller),
        },
        v2::RuntimeRef::Resource { resource } => RuntimeRef::Resource {
            resource: text(resource),
        },
        v2::RuntimeRef::Component { component } => RuntimeRef::Component {
            component: text(component),
        },
        v2::RuntimeRef::Recorder {} => RuntimeRef::Recorder,
        v2::RuntimeRef::ConfigurationProperty { owner, property } => {
            RuntimeRef::ConfigurationProperty {
                owner: match owner {
                    v2::ConfigurationOwner::Instrument { instrument } => {
                        ConfigurationOwner::Instrument {
                            instrument: text(instrument),
                        }
                    }
                    v2::ConfigurationOwner::Component { component } => {
                        ConfigurationOwner::Component {
                            component: text(component),
                        }
                    }
                    v2::ConfigurationOwner::Resource { resource } => ConfigurationOwner::Resource {
                        resource: text(resource),
                    },
                },
                property: text(property),
            }
        }
    }
}

fn binding(selector: v2::Selector) -> ReadBinding {
    match selector {
        v2::Selector::SignalSample => ReadBinding::SignalSample,
        v2::Selector::ConfigurationPropertyCurrent => ReadBinding::ConfigurationPropertyCurrent,
        v2::Selector::ReferenceValue => ReadBinding::ReferenceValue,
        v2::Selector::ControllerState => ReadBinding::ControllerState,
        v2::Selector::RecorderState => ReadBinding::RecorderState,
    }
}
pub(super) fn kind(kind: v2::ScalarKind) -> ScalarKind {
    match kind {
        v2::ScalarKind::Boolean => ScalarKind::Boolean,
        v2::ScalarKind::Integer => ScalarKind::Integer,
        v2::ScalarKind::Number => ScalarKind::Number,
        v2::ScalarKind::Text => ScalarKind::Text,
        v2::ScalarKind::Enum => ScalarKind::Enum,
    }
}

pub(super) fn data(
    ui: &mut TextBuffer,
    document: &v2::AdmittedV2Document,
    observations: &RuntimeObservations,
    id: &v2::Id,
    availability: v2::StaticAvailability,
    precision: Option<u8>,
) {
    let data = document
        .dto()
        .data_bindings
        .0
        .iter()
        .find(|data| &data.id == id)
        .expect("admitted data reference");
    let target = target(&data.target);
    if let v2::StaticAvailability::Disabled(reason) = availability {
        label(ui, format!("Disabled: {reason:?}"));
        freshness(ui, observations, &target, None);
        return;
    }
    typed(
        ui,
        observations,
        &target,
        binding(data.selector),
        data.expect_kind.map(kind),
        precision,
    );
}

pub(super) fn entity(
    ui: &mut TextBuffer,
    observations: &RuntimeObservations,
    source: &v2::RuntimeRef,
) {
    let target = target(source);
    if let Some(binding) = ReadBinding::for_target(&target) {
        typed(ui, observations, &target, binding, None, None);
    } else {
        // The existing v1 entity-status meaning: bounded cached projection text
        // for Instrument/Resource/Component, with no field selector or query.
        if let Some(observation) = observations.entities.get(&target) {
            label(
                ui,
                format!("Value (cached): {}", entity_text(&observation.value)),
            );
        } else {
            label(ui, "Value (cached): no Runtime observation");
        }
        freshness(ui, observations, &target, None);
    }
}

fn typed(
    ui: &mut TextBuffer,
    observations: &RuntimeObservations,
    target: &RuntimeRef,
    binding: ReadBinding,
    expect: Option<ScalarKind>,
    precision: Option<u8>,
) {
    let field = match binding {
        ReadBinding::SignalSample | ReadBinding::ReferenceValue => "value",
        ReadBinding::ConfigurationPropertyCurrent => "current",
        ReadBinding::ControllerState | ReadBinding::RecorderState => "state",
    };
    if observations
        .entities
        .get(target)
        .and_then(|o| o.value.get(field))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| s.len() > SCALAR_BYTES)
    {
        label(
            ui,
            "Unavailable: Runtime scalar exceeds read-only display limit (512 UTF-8 bytes)",
        );
        freshness(ui, observations, target, None);
        return;
    }
    let reading = resolve_read(observations, target, binding);
    let mismatch = expect
        .zip(reading.kind)
        .is_some_and(|(expected, actual)| expected != actual);
    if mismatch {
        label(ui, "Unavailable: expect_kind mismatch");
        if matches!(reading.payload, ReadPayload::Absent(_)) {
            show_payload(ui, &reading, precision);
        }
    } else {
        show_payload(ui, &reading, precision);
    }
    if let Some(unit) = reading.unit.as_ref().and_then(|u| u.display()) {
        label(ui, format!("Runtime unit: {unit}"));
    }
    if let Some(quality) = reading.quality {
        label(
            ui,
            match quality {
                ReadQuality::Good => "Quality: good",
                ReadQuality::Unavailable => "Quality: unavailable",
                ReadQuality::NotObserved => "Quality: not_observed",
            },
        );
    }
    freshness(ui, observations, target, Some(reading.freshness));
}

fn show_payload(ui: &mut TextBuffer, reading: &TypedRead, precision: Option<u8>) {
    match &reading.payload {
        ReadPayload::Absent(reason) => label(ui, format!("Value (cached): {}", reason.label())),
        ReadPayload::Value(value) => {
            let exact = match value {
                ScalarValue::Boolean(v) => v.to_string(),
                ScalarValue::Integer(v) => v.to_string(),
                ScalarValue::Number(v) => format!("{v:?}"),
                ScalarValue::Text(v) if v.is_empty() => "\"\" (empty Text)".into(),
                ScalarValue::Enum(v) if v.is_empty() => "\"\" (empty Enum)".into(),
                ScalarValue::Text(v) | ScalarValue::Enum(v) => v.clone(),
            };
            label(ui, format!("Value (cached): {exact}"));
            // Precision only supplies a secondary rounded Number caption. The
            // primary value stays round-trippable, including negative zero.
            if let (ScalarValue::Number(value), Some(precision)) = (value, precision) {
                label(
                    ui,
                    format!(
                        "Rounded: {value:.precision$}",
                        precision = usize::from(precision)
                    ),
                );
            }
        }
    }
}

fn freshness(
    ui: &mut TextBuffer,
    observations: &RuntimeObservations,
    target: &RuntimeRef,
    resolved: Option<Freshness>,
) {
    if !observations.entities.contains_key(target) {
        label(ui, "Unresolved — no Runtime observation");
        label(ui, format!("Connection: {:?}", observations.freshness));
        return;
    }
    // Display the global rebuild fence independently: even an old Stale entity
    // remains explicitly Rebuilding while the authoritative barrier is pending.
    let state = if observations.freshness == Freshness::Rebuilding {
        Freshness::Rebuilding
    } else {
        resolved.unwrap_or_else(|| {
            observations
                .entities
                .get(target)
                .map_or(observations.freshness, |o| {
                    if o.freshness == Freshness::Fresh {
                        observations.freshness
                    } else {
                        o.freshness
                    }
                })
        })
    };
    label(
        ui,
        match state {
            Freshness::Fresh => "Fresh",
            Freshness::Stale => "Stale — cached observation",
            Freshness::Unknown => "Unknown — freshness not established",
            Freshness::Rebuilding => "Rebuilding — freshness not established",
        },
    );
}

// Same visible prefix policy as v1 entity status, without serializing the entire
// cached projection first. JSON writes stop at the cap; no intermediate tree.
pub(super) fn entity_text(value: &serde_json::Value) -> String {
    struct Prefix {
        bytes: [u8; ENTITY_BYTES],
        len: usize,
    }
    impl io::Write for Prefix {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let remaining = ENTITY_BYTES - self.len;
            let keep = remaining.min(bytes.len());
            self.bytes[self.len..self.len + keep].copy_from_slice(&bytes[..keep]);
            self.len += keep;
            if bytes.len() > remaining {
                Err(io::Error::other("display prefix full"))
            } else {
                Ok(keep)
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut prefix = Prefix {
        bytes: [0; ENTITY_BYTES],
        len: 0,
    };
    let truncated = serde_json::to_writer(&mut prefix, value).is_err();
    let len =
        std::str::from_utf8(&prefix.bytes[..prefix.len]).map_or_else(|e| e.valid_up_to(), str::len);
    let mut text = String::from_utf8(prefix.bytes[..len].to_vec()).expect("complete UTF-8 prefix");
    if truncated {
        text.push('…');
    }
    text
}

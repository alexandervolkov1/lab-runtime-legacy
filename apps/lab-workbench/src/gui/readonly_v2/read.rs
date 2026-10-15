//! One frame-local borrowed typed read at a time; no cache reconstruction.
use super::{
    TextBuffer, label,
    observation_source::{ReadOnlyProvider, Scalar, SelectedRead},
};
use crate::{
    presentation::{ConfigurationOwner, RuntimeRef, v2},
    readonly_values::{Freshness, ReadAbsence, ReadBinding, ReadQuality, ScalarKind},
};
use std::io;
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
    observations: &dyn ReadOnlyProvider,
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
        observations.with_selected(&target, binding(data.selector), &mut |read| {
            freshness(ui, observations, read)
        });
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
    observations: &dyn ReadOnlyProvider,
    source: &v2::RuntimeRef,
) {
    let target = target(source);
    if let Some(binding) = ReadBinding::for_target(&target) {
        typed(ui, observations, &target, binding, None, None);
    } else {
        if let Some(value) = observations.raw_entity(&target) {
            label(ui, format!("Value (cached): {}", entity_text(value.value)));
            let state = match observations.display_freshness() {
                Freshness::Unknown | Freshness::Rebuilding => observations.display_freshness(),
                Freshness::Stale if value.freshness == Freshness::Fresh => Freshness::Stale,
                _ => value.freshness,
            };
            freshness_state(ui, state);
        } else if observations.supports_raw_entities() {
            label(ui, "Value (cached): no Runtime observation");
            freshness(ui, observations, None);
        } else {
            label(
                ui,
                "Unavailable: raw entity presentation is not supplied by this read-only provider",
            );
            freshness(ui, observations, None);
        }
    }
}

fn typed(
    ui: &mut TextBuffer,
    observations: &dyn ReadOnlyProvider,
    target: &RuntimeRef,
    binding: ReadBinding,
    expect: Option<ScalarKind>,
    precision: Option<u8>,
) {
    observations.with_selected(target, binding, &mut |read| {
        let Some(reading) = read else {
            label(ui, "Value (cached): no Runtime observation");
            freshness(ui, observations, None);
            return;
        };
        if matches!(reading.fields.scalar, Scalar::Text(s) | Scalar::Enum(s) if s.len() > SCALAR_BYTES) {
            label(ui, "Unavailable: Runtime scalar exceeds read-only display limit (512 UTF-8 bytes)");
            freshness(ui, observations, Some(reading));
            return;
        }
        let mismatch = expect.zip(reading.fields.kind).is_some_and(|(expected, actual)| expected != actual);
        if mismatch {
            label(ui, "Unavailable: expect_kind mismatch");
            if matches!(reading.fields.scalar, Scalar::Absent(_)) { show_payload(ui, reading.fields.scalar, precision); }
        } else { show_payload(ui, reading.fields.scalar, precision); }
        if let Some(unit) = reading.fields.unit {
            let text = if unit.symbol.is_empty() { unit.id } else { unit.symbol };
            if !text.is_empty() { label(ui, format!("Runtime unit: {text}")); }
        }
        if let Some(quality) = reading.fields.quality {
            label(ui, match quality {
                ReadQuality::Good => "Quality: good",
                ReadQuality::Unavailable => "Quality: unavailable",
                ReadQuality::NotObserved => "Quality: not_observed",
            });
        }
        freshness(ui, observations, Some(reading));
    });
}
fn show_payload(ui: &mut TextBuffer, scalar: Scalar<'_>, precision: Option<u8>) {
    if let Scalar::Absent(reason) = scalar {
        label(ui, format!("Value (cached): {}", reason.label()));
        return;
    }
    let exact = match scalar {
        Scalar::Boolean(v) => v.to_string(),
        Scalar::Integer(v) => v.to_string(),
        Scalar::Number(v) => format!("{v:?}"),
        Scalar::Text("") => "\"\" (empty Text)".into(),
        Scalar::Enum("") => "\"\" (empty Enum)".into(),
        Scalar::Text(v) | Scalar::Enum(v) => v.to_owned(),
        Scalar::Absent(_) => unreachable!(),
    };
    label(ui, format!("Value (cached): {exact}"));
    if let (Scalar::Number(value), Some(precision)) = (scalar, precision) {
        label(
            ui,
            format!(
                "Rounded: {value:.precision$}",
                precision = usize::from(precision)
            ),
        );
    }
}
fn freshness(
    ui: &mut TextBuffer,
    observations: &dyn ReadOnlyProvider,
    resolved: Option<SelectedRead<'_>>,
) {
    let missing = resolved.is_none_or(|read| {
        matches!(
            read.fields.scalar,
            Scalar::Absent(ReadAbsence::MissingObservation)
        )
    });
    if missing {
        label(ui, "Unresolved — no Runtime observation");
        label(
            ui,
            format!("Connection: {:?}", observations.display_freshness()),
        );
        return;
    }
    let state = match observations.display_freshness() {
        Freshness::Unknown | Freshness::Rebuilding => observations.display_freshness(),
        Freshness::Stale if resolved.unwrap().display_freshness() == Freshness::Fresh => {
            Freshness::Stale
        }
        _ => resolved.unwrap().display_freshness(),
    };
    freshness_state(ui, state);
}
fn freshness_state(ui: &mut TextBuffer, state: Freshness) {
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

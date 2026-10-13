//! Necessary structural/guard-floor models, NOT an executable v2 adapter or an
//! end-to-end proof of Runtime eligibility. Neither predicate constructs actions.

use super::{conversion_fixtures as fixtures, *};
use crate::presentation as v1;

#[derive(Clone, Copy)]
enum Candidate<'a> {
    None,
    Text(&'a str),
    Integer(i64),
    Number(f64),
    Boolean,
}
#[derive(Clone, Copy)]
enum PropertyType {
    Missing,
    Integer,
    Text,
    Boolean,
    Number,
    Enum,
}
#[derive(Clone, Copy)]
enum Freshness {
    Fresh,
    Stale,
    Rebuilding,
}
struct State {
    // observation-only, unready, operation unavailable, recovery, quarantine,
    // unresolved target, invalid runtime constraints, stale revision.
    blockers: u8,
    freshness: Freshness,
    property_type: PropertyType,
    writable: bool,
    lifecycle_available: bool,
}
impl State {
    fn common(&self) -> bool {
        self.blockers == 0 && matches!(self.freshness, Freshness::Fresh)
    }
    fn property(&self, candidate: Candidate<'_>) -> bool {
        self.writable
            && match (self.property_type, candidate) {
                (PropertyType::Integer, Candidate::Integer(value)) => (-10..=10).contains(&value),
                (PropertyType::Text, Candidate::Text(value)) => value.len() <= 512,
                _ => false,
            }
    }
}

fn eligible_v1(control: &v1::Control, state: &State, candidate: Candidate<'_>) -> bool {
    if !state.common() {
        return false;
    }
    match (&control.kind, &control.target, candidate) {
        (v1::ControlKind::RecordingLifecycle, v1::RuntimeRef::Recorder, Candidate::Text(label)) => {
            state.lifecycle_available && !label.trim().is_empty() && label.len() <= 128
        }
        (
            v1::ControlKind::SetReference,
            v1::RuntimeRef::Reference { .. },
            Candidate::Number(value),
        ) => value.is_finite(),
        (
            v1::ControlKind::ControllerLifecycle,
            v1::RuntimeRef::Controller { .. },
            Candidate::None,
        ) => state.lifecycle_available,
        (
            v1::ControlKind::ApplyConfiguration,
            v1::RuntimeRef::ConfigurationProperty { .. },
            candidate,
        ) => state.property(candidate),
        _ => false,
    }
}
fn eligible_v2(
    doc: &AdmittedV2Document,
    group_id: &str,
    state: &State,
    candidate: Candidate<'_>,
) -> bool {
    if !state.common() {
        return false;
    }
    let Component::Group {
        children,
        v1_control: Some(origin),
        ..
    } = doc
        .dto()
        .components
        .0
        .iter()
        .find(|c| c.id().0 == group_id)
        .unwrap()
    else {
        return false;
    };
    let button = doc
        .dto()
        .components
        .0
        .iter()
        .find(|c| Some(c.id()) == children.0.last())
        .unwrap();
    let Component::Button {
        action_id: Some(id),
        ..
    } = button
    else {
        return false;
    };
    let index = doc
        .dto()
        .action_bindings
        .0
        .iter()
        .position(|a| a.id() == id)
        .unwrap();
    let action = &doc.dto().action_bindings.0[index];
    if doc.summary().actions[index] != StaticAvailability::NeedsRuntimeResolution
        || !structure::origin_action_matches(origin, action)
    {
        return false;
    }
    if let Some(Parameter::Editor { editor_id }) = action.parameter() {
        let editor = doc
            .dto()
            .editor_bindings
            .0
            .iter()
            .find(|e| &e.id == editor_id)
            .unwrap();
        let data = doc
            .dto()
            .data_bindings
            .0
            .iter()
            .find(|d| d.id == editor.data_id)
            .unwrap();
        if &data.target != action.target() {
            return false;
        }
        let accepted = matches!(
            (editor.purpose, editor.candidate_kind, candidate),
            (
                Purpose::RecordingLabel,
                CandidateKind::Text,
                Candidate::Text(_)
            ) | (
                Purpose::ReferenceFixed,
                CandidateKind::Number,
                Candidate::Number(_)
            ) | (
                Purpose::PropertyValue,
                CandidateKind::PropertyScalar,
                Candidate::Integer(_) | Candidate::Text(_)
            )
        );
        if !accepted {
            return false;
        }
    }
    match (action, candidate) {
        (
            ActionBinding::RecorderLifecycle {
                target: RuntimeRef::Recorder {},
                ..
            },
            Candidate::Text(label),
        ) => {
            state.lifecycle_available
                && label.len() <= 128
                && label.chars().any(|c| !c.is_whitespace())
        }
        (
            ActionBinding::ReferenceFixed {
                target: RuntimeRef::Reference { .. },
                ..
            },
            Candidate::Number(value),
        ) => value.is_finite(),
        (
            ActionBinding::ControllerLifecycle {
                target: RuntimeRef::Controller { .. },
                ..
            },
            Candidate::None,
        ) => state.lifecycle_available,
        (
            ActionBinding::PropertyConfigure {
                target: RuntimeRef::ConfigurationProperty { .. },
                ..
            },
            candidate,
        ) => state.property(candidate),
        _ => false,
    }
}

#[test]
fn converted_guard_floor_implies_v1_for_deterministic_state_and_candidate_matrix() {
    let source = fixtures::mixed();
    let converted = convert_v1(ValidatedV1Document::check(&source).unwrap()).unwrap();
    let oversized = "Ж".repeat(257);
    let candidates = [
        Candidate::None,
        Candidate::Text(" "),
        Candidate::Text("sample"),
        Candidate::Text(&oversized),
        Candidate::Integer(-10),
        Candidate::Integer(10),
        Candidate::Integer(11),
        Candidate::Integer(i64::MIN),
        Candidate::Integer(i64::MAX),
        Candidate::Number(1.0),
        Candidate::Number(f64::NAN),
        Candidate::Boolean,
    ];
    let mut witnessed = [false; 6];
    for blockers in 0..=255 {
        for freshness in [Freshness::Fresh, Freshness::Stale, Freshness::Rebuilding] {
            for property_type in [
                PropertyType::Missing,
                PropertyType::Integer,
                PropertyType::Text,
                PropertyType::Boolean,
                PropertyType::Number,
                PropertyType::Enum,
            ] {
                for writable in [false, true] {
                    for lifecycle_available in [false, true] {
                        let state = State {
                            blockers,
                            freshness,
                            property_type,
                            writable,
                            lifecycle_available,
                        };
                        for (i, control) in source.controls.iter().enumerate() {
                            for candidate in candidates {
                                let permitted =
                                    eligible_v2(&converted, &control.id, &state, candidate);
                                assert!(!permitted || eligible_v1(control, &state, candidate));
                                witnessed[i] |= permitted;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(witnessed, [true, true, true, true, false, true]);
    // False confirmation is kept only on provenance, never on an action.
    for action in &converted.dto().action_bindings.0 {
        assert!(
            serde_json::to_value(action)
                .unwrap()
                .get("confirm")
                .is_none()
        );
    }
}

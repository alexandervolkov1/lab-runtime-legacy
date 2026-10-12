//! DTO fixtures are presentation examples, never experimental observations.

use super::*;
use serde_json::{Value, json};

fn empty() -> Value {
    json!({"format_version":2,"binding_version":1,"document_id":"test",
        "windows":[],"components":[],"data_bindings":[],"editor_bindings":[],
        "action_bindings":[],"plots":[]})
}

fn check(value: &Value) -> Result<LocallyCheckedDocument, String> {
    parse(&serde_json::to_vec(value).unwrap())?.check_local()
}

pub(super) fn examples() -> Vec<Value> {
    let contract = include_str!("../../../../../docs/generic-controls-v2-contract.md");
    let section = contract
        .split("## 11. Neutral complete document examples")
        .nth(1)
        .unwrap()
        .split("## 12.")
        .next()
        .unwrap();
    section
        .split("```json")
        .skip(1)
        .map(|part| serde_json::from_str(part.split("```").next().unwrap()).unwrap())
        .collect()
}

fn editor(purpose: &str, kind: &str, constraints: Value) -> Value {
    json!({"id":"e","data_id":"d","purpose":purpose,"candidate_kind":kind,"constraints":constraints})
}

// Contains every nested object shape, variant, nullable and non-null optional.
// Deliberately unresolved: a local check is not document admission.
fn vocabulary() -> Value {
    let mut doc = examples().pop().unwrap();
    doc["settings"] = json!({"density":"compact"});
    doc["windows"][0]["tabs"][0]["panels"][0]["v1_kind"] = json!("controls");
    let components = doc["components"].as_array_mut().unwrap();
    for kind in [
        "toggle",
        "integer_input",
        "number_input",
        "text_input",
        "select",
    ] {
        components.push(json!({"kind":kind,"id":kind,"label":"Input","help":"Help","editor_id":"e","applicability":"matching_type"}));
    }
    components.extend([
        json!({"kind":"indicator","id":"i","label":"Value","data_id":"d","precision":17}),
        json!({"kind":"status","id":"s","label":"State","source":{"kind":"data","data_id":"d"}}),
        json!({"kind":"status","id":"s2","label":"State","source":{"kind":"entity","target":{"kind":"recorder"}}}),
        json!({"kind":"button","id":"b","label":"Disabled","action_id":null,"disabled_reason":"Inert"}),
    ]);
    for kind in [
        "recording_lifecycle",
        "set_reference",
        "controller_lifecycle",
        "apply_configuration",
        "reconnect_resource",
    ] {
        components.push(
            json!({"kind":"group","id":kind,"label":"Group","layout":"column","children":[],
            "v1_control":{"kind":kind,"target":{"kind":"recorder"},"confirm":false}}),
        );
    }
    let bindings = doc["data_bindings"].as_array_mut().unwrap();
    for selector in [
        "signal_sample",
        "configuration_property_current",
        "reference_value",
        "controller_state",
        "recorder_state",
    ] {
        bindings.push(json!({"id":selector,"selector":selector,"target":{"kind":"recorder"},"expect_kind":null}));
    }
    for target in [
        json!({"kind":"instrument","instrument":"opaque"}),
        json!({"kind":"signal","instrument":"1","parameter":"2"}),
        json!({"kind":"reference","reference":"1"}),
        json!({"kind":"controller","controller":"1"}),
        json!({"kind":"resource","resource":"1"}),
        json!({"kind":"component","component":"1"}),
    ] {
        bindings.push(json!({"id":"target","selector":"signal_sample","target":target}));
    }
    for owner in [
        json!({"kind":"instrument","instrument":"1"}),
        json!({"kind":"resource","resource":"1"}),
        json!({"kind":"component","component":"1"}),
    ] {
        bindings.push(json!({"id":"owned","selector":"configuration_property_current","target":{"kind":"configuration_property","owner":owner,"property":"p"}}));
    }
    let editors = doc["editor_bindings"].as_array_mut().unwrap();
    for (purpose, kind, constraints) in [
        ("recording_label", "text", json!({"max_utf8_bytes":128})),
        (
            "reference_fixed",
            "number",
            json!({"number_min":-1.5,"number_max":2}),
        ),
        (
            "property_value",
            "integer",
            json!({"integer_min":"-9223372036854775808","integer_max":"9223372036854775807"}),
        ),
        ("property_value", "text", json!({"max_utf8_bytes":512})),
        (
            "property_value",
            "property_scalar",
            json!({"integer_min":"0","max_utf8_bytes":1}),
        ),
        ("local_only", "boolean", json!({})),
        ("local_only", "integer", json!({})),
        ("local_only", "number", json!({})),
        ("local_only", "text", json!({})),
        (
            "local_only",
            "enum",
            json!({"choices":[{"token":"a","label":"A"}]}),
        ),
    ] {
        editors.push(editor(purpose, kind, constraints));
    }
    doc["action_bindings"] = json!([
        {"id":"a","family":"recorder_lifecycle","target":{"kind":"recorder"},"label":{"kind":"literal","value":{"kind":"text","value":""}}},
        {"id":"a","family":"reference_fixed","target":{"kind":"reference","reference":"1"},"value":{"kind":"literal","value":{"kind":"number","value":1.25}}},
        {"id":"a","family":"controller_lifecycle","target":{"kind":"controller","controller":"1"}},
        {"id":"a","family":"property_configure","target":{"kind":"recorder"},"value":{"kind":"literal","value":{"kind":"integer","value":"9223372036854775807"}}},
        {"id":"a","family":"property_configure","target":{"kind":"recorder"},"value":{"kind":"editor","editor_id":"e"}}
    ]);
    doc
}

#[test]
fn all_three_contract_examples_and_closed_vocabulary_pass_local_checks() {
    let examples = examples();
    assert_eq!(examples.len(), 3);
    for example in examples {
        check(&example).unwrap();
    }
    check(&vocabulary()).unwrap();
    let result = check(&empty()).unwrap();
    assert_eq!(result.dto().settings.density, Density::Comfortable);
}

fn object_paths(value: &Value, path: String, output: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            output.push(path.clone());
            for (key, value) in map {
                object_paths(value, format!("{path}/{key}"), output);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                object_paths(value, format!("{path}/{index}"), output);
            }
        }
        _ => {}
    }
}

#[test]
fn unknown_fields_and_tags_are_rejected_at_every_object_level() {
    let fixture = vocabulary();
    let mut paths = Vec::new();
    object_paths(&fixture, String::new(), &mut paths);
    assert!(paths.len() > 60);
    for path in paths {
        let mut doc = fixture.clone();
        doc.pointer_mut(&path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(false));
        assert!(check(&doc).is_err(), "unknown field at {path}");
        for tag in [
            "kind",
            "family",
            "selector",
            "purpose",
            "candidate_kind",
            "density",
            "layout",
            "applicability",
            "v1_kind",
            "expect_kind",
        ] {
            if fixture.pointer(&path).unwrap().get(tag).is_some() {
                let mut doc = fixture.clone();
                doc.pointer_mut(&path).unwrap()[tag] = json!("unknown");
                assert!(check(&doc).is_err(), "unknown {tag} at {path}");
            }
        }
    }
}

fn optional_field(object: &Value, key: &str) -> bool {
    matches!(
        key,
        "settings"
            | "help"
            | "v1_control"
            | "v1_kind"
            | "applicability"
            | "precision"
            | "expect_kind"
            | "constraints"
            | "integer_min"
            | "integer_max"
            | "number_min"
            | "number_max"
            | "max_utf8_bytes"
            | "choices"
    ) || (key == "disabled_reason" && !object["action_id"].is_null())
}

#[test]
fn required_fields_wrong_types_and_nulls_are_checked_at_every_level() {
    let fixture = vocabulary();
    let mut paths = Vec::new();
    object_paths(&fixture, String::new(), &mut paths);
    for path in paths {
        let object = fixture.pointer(&path).unwrap();
        for (key, value) in object.as_object().unwrap() {
            if !optional_field(object, key) {
                let mut doc = fixture.clone();
                doc.pointer_mut(&path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(check(&doc).is_err(), "missing {path}/{key}");
            }
            // Required nullable fields and expect_kind are the only nulls.
            if !matches!(
                key.as_str(),
                "action_id" | "y_min" | "y_max" | "display_unit" | "expect_kind"
            ) {
                let mut doc = fixture.clone();
                doc.pointer_mut(&path).unwrap()[key] = Value::Null;
                assert!(check(&doc).is_err(), "null at {path}/{key}");
            }
            let wrong = if value.is_array() {
                json!({})
            } else {
                json!([])
            };
            let mut doc = fixture.clone();
            doc.pointer_mut(&path).unwrap()[key] = wrong;
            assert!(check(&doc).is_err(), "wrong type at {path}/{key}");
        }
    }
}

#[test]
fn duplicate_keys_are_detected_before_any_value_is_discarded() {
    // Walk each actual shape, including identical keys and escape-equivalent keys.
    let fixture = vocabulary();
    let mut paths = Vec::new();
    object_paths(&fixture, String::new(), &mut paths);
    for path in paths {
        let object = fixture.pointer(&path).unwrap();
        let original = serde_json::to_string(object).unwrap();
        let tail = if original == "{}" {
            "}"
        } else {
            &original[1..]
        };
        let separator = if original == "{}" { "" } else { "," };
        let duplicate = format!("{{\"duplicate\":0,\"duplicate\":1{separator}{tail}");
        let encoded = serde_json::to_string(&fixture)
            .unwrap()
            .replacen(&original, &duplicate, 1);
        assert!(
            parse(encoded.as_bytes())
                .unwrap_err()
                .contains("duplicate JSON key"),
            "{path}"
        );
    }
    assert!(
        parse(br#"{"format_version":2,"\u0066ormat_version":2}"#)
            .unwrap_err()
            .contains("duplicate JSON key")
    );
}

#[test]
fn exact_decimal_extremes_canonical_spelling_and_scalar_types() {
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        let encoded = format!("\"{value}\"");
        assert_eq!(
            serde_json::from_str::<I64Decimal>(&encoded).unwrap().0,
            value
        );
        let scalar: TypedScalar =
            serde_json::from_value(json!({"kind":"integer","value":value.to_string()})).unwrap();
        assert!(matches!(scalar,TypedScalar::Integer { value: I64Decimal(v) } if v == value));
    }
    for value in [
        "-9223372036854775809",
        "9223372036854775808",
        "00",
        "01",
        "-01",
        "-0",
        "+1",
        "1e2",
        "1.0",
        " 1",
        "",
        "１２",
    ] {
        assert!(
            serde_json::from_value::<I64Decimal>(json!(value)).is_err(),
            "{value}"
        );
    }
    for value in [0, u64::MAX] {
        assert_eq!(
            serde_json::from_value::<U64Decimal>(json!(value.to_string()))
                .unwrap()
                .0,
            value
        );
    }
    for value in [
        "18446744073709551616",
        "-1",
        "00",
        "+0",
        "-0",
        "1e2",
        "1.0",
        " 0",
        "",
    ] {
        assert!(
            serde_json::from_value::<U64Decimal>(json!(value)).is_err(),
            "{value}"
        );
    }
    for (kind, value) in [
        ("boolean", json!(false)),
        ("number", json!(0)),
        ("text", json!("")),
        ("enum", json!("token")),
    ] {
        serde_json::from_value::<TypedScalar>(json!({"kind":kind,"value":value})).unwrap();
    }
    for kind in ["integer", "boolean", "number", "text", "enum"] {
        for value in [Value::Null, json!([]), json!({})] {
            assert!(
                serde_json::from_value::<TypedScalar>(json!({"kind":kind,"value":value})).is_err()
            );
        }
    }
    assert!(serde_json::from_str::<I64Decimal>("42").is_err());
    assert!(serde_json::from_str::<U64Decimal>("0").is_err());
    assert!(serde_json::from_str::<TypedScalar>(r#"{"kind":"number","value":1e400}"#).is_err());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(super::bounded::Number::try_from(value).is_err());
    }
}

#[test]
fn unicode_byte_boundaries_and_opaque_targets_are_preserved() {
    let mut doc = empty();
    doc["document_id"] = json!("é".repeat(256));
    check(&doc).unwrap();
    doc["document_id"] = json!("é".repeat(257));
    assert!(check(&doc).is_err());
    doc["document_id"] = json!("🦀".repeat(128));
    check(&doc).unwrap();
    doc["document_id"] = json!("🦀".repeat(129));
    assert!(check(&doc).is_err());
    doc["document_id"] = json!("");
    assert!(check(&doc).is_err());
    let escaped = format!("\"{}\"", "\\u0061".repeat(512));
    super::bounded::preflight(escaped.as_bytes()).unwrap();
    assert!(
        super::bounded::preflight(format!("\"{}\"", "\\u0061".repeat(513)).as_bytes()).is_err()
    );
    let reference: RuntimeRef =
        serde_json::from_value(json!({"kind":"signal","instrument":"opaque","parameter":"001"}))
            .unwrap();
    assert!(
        matches!(reference,RuntimeRef::Signal { instrument,parameter } if instrument.0 == "opaque" && parameter.0 == "001")
    );
}

#[test]
fn local_numeric_limits_and_constraints_are_not_admission() {
    let mut doc = empty();
    for (purpose, kind, constraints) in [
        ("recording_label", "integer", json!({})),
        ("reference_fixed", "integer", json!({})),
        ("property_value", "number", json!({})),
        ("local_only", "property_scalar", json!({})),
        ("local_only", "text", json!({"integer_min":"0"})),
        ("local_only", "integer", json!({"number_max":1})),
        ("local_only", "number", json!({"max_utf8_bytes":1})),
        (
            "local_only",
            "text",
            json!({"choices":[{"token":"a","label":"A"}]}),
        ),
        (
            "local_only",
            "integer",
            json!({"integer_min":"9223372036854775807","integer_max":"-9223372036854775808"}),
        ),
        (
            "local_only",
            "number",
            json!({"number_min":2,"number_max":1}),
        ),
        ("recording_label", "text", json!({"max_utf8_bytes":129})),
        ("property_value", "text", json!({"max_utf8_bytes":0})),
        ("property_value", "text", json!({"max_utf8_bytes":513})),
    ] {
        doc["editor_bindings"] = json!([editor(purpose, kind, constraints)]);
        assert!(check(&doc).is_err(), "{doc}");
    }
    for (kind, constraints) in [
        (
            "integer",
            json!({"integer_min":"9223372036854775807","integer_max":"9223372036854775807"}),
        ),
        ("number", json!({"number_min":-0.0,"number_max":0.0})),
    ] {
        doc["editor_bindings"] = json!([editor("local_only", kind, constraints)]);
        check(&doc).unwrap();
    }
    let mut doc = examples().pop().unwrap();
    for window in [0.0, -1.0, 604800.1] {
        doc["plots"][0]["time_window_seconds"] = json!(window);
        assert!(check(&doc).is_err());
    }
    doc["plots"][0]["time_window_seconds"] = json!(604800);
    check(&doc).unwrap();
    for width in [0.0, -1.0, 32.1] {
        doc["plots"][0]["traces"][0]["style"]["width"] = json!(width);
        assert!(check(&doc).is_err());
    }
    doc["plots"][0]["traces"][0]["style"]["width"] = json!(32);
    check(&doc).unwrap();
    doc["plots"][0]["axes"] = json!({"y_min":1,"y_max":1});
    assert!(check(&doc).is_err());
    doc["plots"][0]["axes"] = json!({"y_min":-1,"y_max":1});
    check(&doc).unwrap();
}

#[test]
fn enum_choices_are_bounded_nonempty_and_unique() {
    let mut doc = empty();
    for count in [0, 1, 64, 65] {
        let choices: Vec<_> = (0..count)
            .map(|i| json!({"token":i.to_string(),"label":"Choice"}))
            .collect();
        doc["editor_bindings"] = json!([editor("local_only", "enum", json!({"choices":choices}))]);
        assert_eq!(check(&doc).is_ok(), (1..=64).contains(&count));
    }
    for choices in [
        json!([{"token":"","label":"Empty"}]),
        json!([{"token":"a","label":"A"},{"token":"a","label":"Other"}]),
    ] {
        doc["editor_bindings"] = json!([editor("local_only", "enum", json!({"choices":choices}))]);
        assert!(check(&doc).is_err());
    }
}

#[test]
fn input_depth_count_and_array_allocations_are_bounded() {
    let mut encoded = serde_json::to_vec(&empty()).unwrap();
    encoded.resize(super::bounded::INPUT_BYTES, b' ');
    parse(&encoded).unwrap();
    encoded.push(b' ');
    assert!(parse(&encoded).unwrap_err().contains("body byte limit"));
    assert!(
        super::bounded::preflight(format!("{}0{}", "[".repeat(17), "]".repeat(17)).as_bytes())
            .is_err()
    );
    assert!(
        super::bounded::preflight(format!("[{}]", vec!["0"; 16_384].join(",")).as_bytes()).is_err()
    );
    assert!(super::bounded::preflight(format!("\"{}\"", "x".repeat(100_000)).as_bytes()).is_err());
    let mut doc = empty();
    for (key, max, item) in [
        ("windows", 64, json!({"id":"w","title":"W","tabs":[]})),
        (
            "components",
            512,
            json!({"id":"c","kind":"plot","label":"P","plot_id":"p"}),
        ),
        (
            "data_bindings",
            1152,
            json!({"id":"d","selector":"recorder_state","target":{"kind":"recorder"}}),
        ),
        (
            "editor_bindings",
            64,
            editor("local_only", "boolean", json!({})),
        ),
        (
            "action_bindings",
            64,
            json!({"id":"a","family":"controller_lifecycle","target":{"kind":"controller","controller":"1"}}),
        ),
        (
            "plots",
            32,
            json!({"id":"p","title":"P","time_window_seconds":1,"axes":{"y_min":null,"y_max":null},"traces":[]}),
        ),
    ] {
        doc[key] = json!(vec![item.clone(); max]);
        check(&doc).unwrap();
        doc[key] = json!(vec![item; max + 1]);
        assert!(check(&doc).is_err(), "{key}");
        doc[key] = json!([]);
    }
}

#[test]
fn unresolved_references_and_cycles_are_only_locally_checked() {
    let mut doc = empty();
    doc["components"] = json!([
        {"id":"g","kind":"group","label":"Cycle","layout":"row","children":["g","missing"]},
        {"id":"b","kind":"button","label":"Unresolved","action_id":"missing-action"}
    ]);
    let local = check(&doc).unwrap();
    assert_eq!(local.dto().components.0.len(), 2);
    assert!(std::any::type_name_of_val(&local).ends_with("LocallyCheckedDocument"));
    // There is deliberately no FullyValidatedDocument constructor or conversion
    // into the v1 owner document. The structural stage must reject this fixture.
    assert!(local.check_structure().is_err());
}

#[test]
fn architecture_guard_parser_has_no_execution_or_publication_dependency() {
    for source in [
        include_str!("mod.rs"),
        include_str!("bounded.rs"),
        include_str!("dto.rs"),
        include_str!("validation.rs"),
        include_str!("structure.rs"),
        include_str!("graph.rs"),
    ] {
        for forbidden in [
            "ClientHandle",
            "WorkbenchDispatcher",
            "OperatorWorkflow",
            "OperatorIntent",
            "crate::",
            "super::super",
            "std::fs",
            "std::net",
            "egui",
            ".dispatch(",
            ".query(",
            ".mutation(",
        ] {
            assert!(!source.contains(forbidden), "parser acquired {forbidden}");
        }
    }
    for consumer in [
        include_str!("../../dispatcher.rs"),
        include_str!("../../external.rs"),
        include_str!("../../gui/app.rs"),
    ] {
        assert!(!consumer.contains("presentation::v2"));
        assert!(!consumer.contains("LocallyCheckedDocument"));
        assert!(!consumer.contains("ParsedDocument"));
        assert!(!consumer.contains("StructurallyCheckedDocument"));
    }
}

#[test]
fn map_only_decoding_rejects_positional_structs_and_tagged_unions() {
    for encoded in [
        r#"["text","x"]"#,
        r#"["boolean",false]"#,
        r#"["integer","42"]"#,
    ] {
        assert!(serde_json::from_str::<TypedScalar>(encoded).is_err());
    }
    assert!(serde_json::from_str::<Constraints>("[]").is_err());
    assert!(serde_json::from_str::<Settings>(r#"["compact"]"#).is_err());
    assert!(serde_json::from_str::<Axes>("[null,null]").is_err());
    let fixture = vocabulary();
    let mut paths = Vec::new();
    object_paths(&fixture, String::new(), &mut paths);
    for path in paths {
        for (field, value) in fixture.pointer(&path).unwrap().as_object().unwrap() {
            if let Value::String(tag) = value {
                let mut doc = fixture.clone();
                doc.pointer_mut(&path).unwrap()[field] = json!({tag: null});
                assert!(
                    check(&doc).is_err(),
                    "externally tagged enum/object at {path}/{field}"
                );
            }
        }
    }
    for kind in ["boolean", "integer", "number", "text", "enum"] {
        let value = match kind {
            "boolean" => json!(false),
            "integer" => json!("0"),
            "number" => json!(0),
            _ => json!(""),
        };
        for invalid in [
            json!({"kind":kind}),
            json!({"kind":kind,"value":value,"extra":0}),
            json!({"kind":"unknown","value":value}),
        ] {
            assert!(serde_json::from_value::<TypedScalar>(invalid).is_err());
        }
    }
}

#[test]
fn optional_defaults_nullable_fields_versions_and_inert_buttons_are_exact() {
    let mut doc = empty();
    for key in ["format_version", "binding_version"] {
        let saved = doc[key].clone();
        for invalid in [json!(0), json!(3), json!(2.0), json!("2"), Value::Null] {
            doc[key] = invalid;
            assert!(parse(&serde_json::to_vec(&doc).unwrap()).is_err());
        }
        doc[key] = saved;
    }
    doc["components"] = json!([{"id":"b","kind":"button","label":"B","action_id":null}]);
    assert!(check(&doc).is_err());
    doc["components"][0]["disabled_reason"] = json!("Unavailable");
    check(&doc).unwrap();
    doc["components"][0]["action_id"] = json!("a");
    assert!(check(&doc).is_err());
    doc["components"][0]
        .as_object_mut()
        .unwrap()
        .remove("disabled_reason");
    check(&doc).unwrap();
    doc["components"] =
        json!([{"id":"i","kind":"indicator","label":"I","data_id":"missing","precision":18}]);
    assert!(check(&doc).is_err());
    for precision in [0, 17] {
        doc["components"][0]["precision"] = json!(precision);
        check(&doc).unwrap();
    }
    doc["components"] = json!([{"id":"i","kind":"text_input","label":"I","editor_id":"missing"}]);
    assert!(matches!(
        &check(&doc).unwrap().dto().components.0[0],
        Component::TextInput {
            applicability: Applicability::Always,
            ..
        }
    ));
    doc["editor_bindings"] =
        json!([{"id":"e","data_id":"missing","purpose":"local_only","candidate_kind":"text"}]);
    let local = check(&doc).unwrap();
    assert!(
        local.dto().editor_bindings.0[0]
            .constraints
            .max_utf8_bytes
            .is_none()
    );
    doc["data_bindings"] =
        json!([{"id":"d","selector":"recorder_state","target":{"kind":"recorder"}}]);
    assert!(
        check(&doc).unwrap().dto().data_bindings.0[0]
            .expect_kind
            .is_none()
    );
}

#[test]
fn action_literal_types_are_restricted_without_resolving_targets() {
    for (family, field, allowed) in [
        ("recorder_lifecycle", "label", vec!["text"]),
        ("reference_fixed", "value", vec!["number"]),
        ("property_configure", "value", vec!["integer", "text"]),
    ] {
        for (kind, value) in [
            ("boolean", json!(false)),
            ("integer", json!("0")),
            ("number", json!(0)),
            ("text", json!("")),
            ("enum", json!("a")),
        ] {
            let mut doc = empty();
            let mut action = json!({"id":"a","family":family,"target":{"kind":"recorder"}});
            action[field] = json!({"kind":"literal","value":{"kind":kind,"value":value}});
            doc["action_bindings"] = json!([action]);
            assert_eq!(
                check(&doc).is_ok(),
                allowed.contains(&kind),
                "{family}/{kind}"
            );
        }
    }
}

#[test]
fn nested_array_and_aggregate_definition_limits() {
    let base = examples().pop().unwrap();
    for (path, max, item) in [
        (
            "/windows/0/tabs",
            64,
            json!({"id":"t","title":"T","panels":[]}),
        ),
        (
            "/windows/0/tabs/0/panels",
            64,
            json!({"id":"p","title":"P","components":[]}),
        ),
        (
            "/windows/0/tabs/0/panels/0/components",
            64,
            json!("component"),
        ),
        ("/plots/0/traces", 32, base["plots"][0]["traces"][0].clone()),
    ] {
        let mut doc = base.clone();
        *doc.pointer_mut(path).unwrap() = json!(vec![item.clone(); max]);
        check(&doc).unwrap();
        *doc.pointer_mut(path).unwrap() = json!(vec![item; max + 1]);
        assert!(check(&doc).is_err(), "{path}");
    }
    let mut doc = empty();
    let group = json!({"id":"g","kind":"group","label":"G","layout":"row","children":[]});
    doc["components"] = json!(vec![group.clone(); 128]);
    check(&doc).unwrap();
    doc["components"] = json!(vec![group.clone(); 129]);
    assert!(check(&doc).is_err());
    doc["components"] = json!([group]);
    doc["components"][0]["children"] = json!(vec!["missing"; 64]);
    check(&doc).unwrap();
    doc["components"][0]["children"] = json!(vec!["missing"; 65]);
    assert!(check(&doc).is_err());
    let mut doc = base;
    let panel = doc["windows"][0]["tabs"][0]["panels"][0].clone();
    doc["windows"][0]["tabs"] = json!([
        {"id":"a","title":"A","panels":vec![panel.clone();32]},
        {"id":"b","title":"B","panels":vec![panel.clone();32]}
    ]);
    check(&doc).unwrap();
    doc["windows"][0]["tabs"][1]["panels"]
        .as_array_mut()
        .unwrap()
        .push(panel);
    assert!(check(&doc).is_err());
    // The depth definition and value/member charges match the existing v1 wire
    // scanner: root container is level 1; its scalar children add no container.
    super::bounded::preflight(format!("{}0{}", "[".repeat(16), "]".repeat(16)).as_bytes()).unwrap();
    super::bounded::preflight(format!("[{}]", vec!["0"; 16_383].join(",")).as_bytes()).unwrap();
}

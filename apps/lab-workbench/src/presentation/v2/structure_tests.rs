//! Deterministic structural fixtures. No Runtime evidence is fabricated.

use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn empty() -> Value {
    json!({"format_version":2,"binding_version":1,"document_id":"document",
        "windows":[],"components":[],"data_bindings":[],"editor_bindings":[],"action_bindings":[],"plots":[]})
}
fn check(doc: &Value) -> Result<StructurallyCheckedDocument, String> {
    parse(&serde_json::to_vec(doc).unwrap())?
        .check_local()?
        .check_structure()
}
fn rejects(doc: &Value, reason: &str) {
    // Prove the defect belongs to the new stage, not an earlier shape rejection.
    let local = parse(&serde_json::to_vec(doc).unwrap())
        .unwrap()
        .check_local()
        .unwrap();
    let error = local.check_structure().unwrap_err();
    assert!(error.contains(reason), "expected {reason}: {error}");
}
fn group(id: &str, children: Value) -> Value {
    json!({"id":id,"kind":"group","label":"Group","layout":"column","children":children})
}
fn leaf(id: &str) -> Value {
    json!({"id":id,"kind":"status","label":"Status","source":{"kind":"entity","target":{"kind":"recorder"}}})
}
fn arrange(doc: &mut Value, roots: Value) {
    doc["windows"] = json!([{"id":"window","title":"W","tabs":[{"id":"tab","title":"T",
        "panels":[{"id":"panel","title":"P","components":roots}]}]}]);
}
fn graph_fixture(components: Value, roots: Value) -> Value {
    let mut doc = empty();
    doc["components"] = components;
    arrange(&mut doc, roots);
    doc
}
fn property(owner_kind: &str, owner_id: &str, property: &str) -> Value {
    let mut owner = json!({"kind":owner_kind});
    owner[owner_kind] = json!(owner_id);
    json!({"kind":"configuration_property","owner":owner,"property":property})
}

#[test]
fn complete_contract_examples_pass_the_structural_stage() {
    for doc in tests::examples() {
        let checked = check(&doc).unwrap();
        assert!(checked.summary().expanded_components > 0);
        assert!(
            checked
                .summary()
                .actions
                .iter()
                .all(|state| *state == StaticAvailability::NeedsRuntimeResolution)
        );
    }
    assert_eq!(check(&empty()).unwrap().summary().expanded_components, 0);
}

#[test]
fn ids_share_one_namespace_including_document_and_nested_definitions() {
    let doc = tests::examples().pop().unwrap();
    let paths = [
        "/document_id",
        "/windows/0/id",
        "/windows/0/tabs/0/id",
        "/windows/0/tabs/0/panels/0/id",
        "/components/0/id",
        "/data_bindings/0/id",
        "/editor_bindings/0/id",
        "/action_bindings/0/id",
        "/plots/0/id",
        "/plots/0/traces/0/id",
    ];
    for (i, left) in paths.iter().enumerate() {
        for right in &paths[i + 1..] {
            let mut bad = doc.clone();
            *bad.pointer_mut(left).unwrap() = doc.pointer(right).unwrap().clone();
            rejects(&bad, "duplicate definition ID");
        }
    }
    for path in [
        "/windows",
        "/windows/0/tabs",
        "/windows/0/tabs/0/panels",
        "/components",
        "/data_bindings",
        "/editor_bindings",
        "/action_bindings",
        "/plots",
        "/plots/0/traces",
    ] {
        let mut bad = doc.clone();
        let array = bad.pointer_mut(path).unwrap().as_array_mut().unwrap();
        array.push(array[0].clone());
        rejects(&bad, "duplicate definition ID");
    }
}

#[test]
fn every_reference_category_rejects_dangling_and_wrong_registry_ids_even_when_unused() {
    let mut doc = tests::examples().pop().unwrap();
    doc["components"].as_array_mut().unwrap().extend([
        group("group",json!(["integer"])),
        json!({"id":"indicator","kind":"indicator","label":"I","data_id":"property-data"}),
        json!({"id":"status","kind":"status","label":"S","source":{"kind":"data","data_id":"property-data"}}),
    ]);
    check(&doc).unwrap();
    for path in [
        "/windows/0/tabs/0/panels/0/components/0",
        "/components/3/children/0",
        "/components/4/data_id",
        "/components/5/source/data_id",
        "/components/0/editor_id",
        "/components/1/action_id",
        "/editor_bindings/0/data_id",
        "/action_bindings/0/value/editor_id",
        "/components/2/plot_id",
        "/plots/0/traces/0/data_id",
    ] {
        for (target, reason) in [("missing", "dangling"), ("window", "wrong registry")] {
            let mut bad = doc.clone();
            *bad.pointer_mut(path).unwrap() = json!(target);
            rejects(&bad, reason);
        }
    }
}

#[test]
fn cycles_self_cycles_and_unused_cycles_are_rejected() {
    for roots in [json!([]), json!(["a"])] {
        rejects(
            &graph_fixture(
                json!([group("a", json!(["b"])), group("b", json!(["a"]))]),
                roots,
            ),
            "cycle",
        );
    }
    rejects(
        &graph_fixture(json!([group("a", json!(["a"]))]), json!([])),
        "cycle",
    );
}

#[test]
fn group_depth_four_passes_five_fails_including_unused_chains() {
    for levels in [4, 5] {
        let mut components = vec![leaf("leaf")];
        for i in 0..levels {
            components.push(group(
                &format!("g{i}"),
                json!([if i + 1 == levels {
                    "leaf".into()
                } else {
                    format!("g{}", i + 1)
                }]),
            ));
        }
        for roots in [json!([]), json!(["g0"])] {
            let doc = graph_fixture(json!(components), roots);
            if levels == 4 {
                check(&doc).unwrap();
            } else {
                rejects(&doc, "depth");
            }
        }
    }
}

#[test]
fn diamond_and_repeated_edges_count_instances_not_definitions() {
    let doc = graph_fixture(
        json!([
            group("root", json!(["left", "right"])),
            group("left", json!(["shared"])),
            group("right", json!(["shared"])),
            group("shared", json!(["value", "value"])),
            leaf("value")
        ]),
        json!(["root", "root"]),
    );
    let checked = check(&doc).unwrap();
    assert_eq!(checked.dto().components.0.len(), 5);
    assert_eq!(checked.summary().expanded_components, 18);
    let mut identities = BTreeSet::new();
    for root in 0..2 {
        for side in 0..2 {
            for repeated in 0..2 {
                let view = checked
                    .occurrence(0, 0, 0, root, &[side, 0, repeated])
                    .unwrap();
                assert_eq!(view.component.id().0, "value");
                assert!(identities.insert(view.identity));
            }
        }
    }
    assert_eq!(identities.len(), 8);
    assert!(checked.occurrence(0, 0, 0, 0, &[0, 0, 0, 0]).is_none());
    assert!(checked.occurrence(0, 0, 0, 2, &[]).is_none());
    assert!(checked.occurrence(0, 0, 0, 0, &[0; 5]).is_none());
}

#[test]
fn exact_expansion_ceiling_hidden_tabs_and_checked_overflow() {
    // 4096 roots * (one group + four leaves) = 20,480, with only two definitions.
    let mut doc = graph_fixture(
        json!([
            group("g", json!(["leaf", "leaf", "leaf", "leaf"])),
            leaf("leaf")
        ]),
        json!([]),
    );
    let tabs: Vec<_> = (0..64)
        .map(|i| {
            json!({"id":format!("t{i}"),"title":"Hidden or visible",
        "panels":[{"id":format!("p{i}"),"title":"P","components":vec!["g";64]}]})
        })
        .collect();
    doc["windows"][0]["tabs"] = json!(tabs);
    let checked = check(&doc).unwrap();
    assert_eq!(checked.summary().expanded_components, 20_480);
    assert_ne!(
        checked.occurrence(0, 0, 0, 0, &[0]).unwrap().identity,
        checked.occurrence(0, 63, 0, 0, &[0]).unwrap().identity
    );
    doc["components"].as_array_mut().unwrap().push(group(
        "extra",
        json!(["leaf", "leaf", "leaf", "leaf", "leaf"]),
    ));
    doc["windows"][0]["tabs"][0]["panels"][0]["components"][0] = json!("extra");
    // Only this root grows from five to six occurrences: exactly 20,481.
    rejects(&doc, "expanded occurrence limit");
    assert_eq!(graph::bounded_add(20_479, 1, 20_480).unwrap(), 20_480);
    assert!(graph::bounded_add(20_480, 1, 20_480).is_err());
    assert!(graph::bounded_add(usize::MAX, 1, usize::MAX).is_err());
}

#[test]
fn exponential_shared_graph_rejects_without_expansion_and_unused_graph_has_no_instances() {
    let mut components = vec![leaf("leaf")];
    for i in 0..4 {
        let child = if i == 3 {
            "leaf".into()
        } else {
            format!("g{}", i + 1)
        };
        components.push(group(&format!("g{i}"), json!(vec![child; 64])));
    }
    let mut doc = graph_fixture(json!(components), json!([]));
    assert_eq!(check(&doc).unwrap().summary().expanded_components, 0);
    doc["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["g0"]);
    rejects(&doc, "expanded occurrence limit");
}

#[test]
fn expanded_plot_limit_counts_repeated_views_in_all_windows() {
    let mut doc = tests::examples().pop().unwrap();
    arrange(&mut doc, json!(vec!["chart"; 64]));
    assert_eq!(check(&doc).unwrap().summary().expanded_plots, 64);
    doc["windows"].as_array_mut().unwrap().push(json!({"id":"other-window","title":"W","tabs":[
        {"id":"other-tab","title":"T","panels":[{"id":"other-panel","title":"P","components":["chart"]}]}]}));
    rejects(&doc, "expanded occurrence limit");
}

#[test]
fn all_widget_editor_pairs_are_closed_and_property_scalar_requires_both_alternatives() {
    for (widget, expected) in [
        ("toggle", "boolean"),
        ("integer_input", "integer"),
        ("number_input", "number"),
        ("text_input", "text"),
        ("select", "enum"),
    ] {
        for kind in ["boolean", "integer", "number", "text", "enum"] {
            let mut doc = empty();
            doc["data_bindings"] = json!([{"id":"data","selector":"signal_sample","target":{"kind":"signal","instrument":"opaque","parameter":"p"}}]);
            doc["editor_bindings"] = json!([{"id":"editor","data_id":"data","purpose":"local_only","candidate_kind":kind}]);
            for applicability in ["always", "matching_type"] {
                doc["components"] = json!([{"id":"input","kind":widget,"label":"Input","editor_id":"editor","applicability":applicability}]);
                if kind == expected {
                    check(&doc).unwrap();
                } else {
                    rejects(&doc, "candidate kind mismatch");
                }
            }
        }
    }
    let mut doc = tests::examples().pop().unwrap();
    doc["editor_bindings"][0]["candidate_kind"] = json!("property_scalar");
    doc["components"][0]["applicability"] = json!("matching_type");
    rejects(&doc, "require both");
    doc["components"].as_array_mut().unwrap().push(json!({"id":"text","kind":"text_input","label":"Text","editor_id":"property-draft","applicability":"matching_type"}));
    check(&doc).unwrap();
    doc["components"][0]["applicability"] = json!("always");
    rejects(&doc, "requires matching_type");
    doc["components"][0]["applicability"] = json!("matching_type");
    for widget in ["toggle", "number_input", "select"] {
        doc["components"][0]["kind"] = json!(widget);
        rejects(&doc, "requires IntegerInput/TextInput");
    }
}

#[test]
fn editor_purpose_selector_and_action_family_are_structural_contracts() {
    for (family, purpose, kind, selector, field) in [
        (
            "recorder_lifecycle",
            "recording_label",
            "text",
            "recorder_state",
            "label",
        ),
        (
            "reference_fixed",
            "reference_fixed",
            "number",
            "reference_value",
            "value",
        ),
        (
            "property_configure",
            "property_value",
            "integer",
            "configuration_property_current",
            "value",
        ),
    ] {
        let mut doc = empty();
        doc["data_bindings"] =
            json!([{"id":"data","selector":selector,"target":{"kind":"recorder"}}]);
        doc["editor_bindings"] =
            json!([{"id":"editor","purpose":purpose,"candidate_kind":kind,"data_id":"data"}]);
        let mut action = json!({"id":"action","family":family,"target":{"kind":"recorder"}});
        action[field] = json!({"kind":"editor","editor_id":"editor"});
        doc["action_bindings"] = json!([action]);
        check(&doc).unwrap();
        for wrong_selector in ["signal_sample", "controller_state"] {
            doc["data_bindings"][0]["selector"] = json!(wrong_selector);
            rejects(&doc, "purpose/data selector mismatch");
        }
        doc["data_bindings"][0]["selector"] = json!(selector);
        doc["editor_bindings"][0]["purpose"] = json!("local_only");
        rejects(&doc, "family/editor purpose mismatch");
    }
    let mut doc = tests::examples().pop().unwrap();
    doc["data_bindings"][1]["selector"] = json!("recorder_state");
    rejects(&doc, "trace requires signal_sample");
}

#[test]
fn only_one_recorder_editor_definition_but_repeated_views_are_allowed() {
    let mut doc = empty();
    doc["data_bindings"] =
        json!([{"id":"data","selector":"recorder_state","target":{"kind":"recorder"}}]);
    doc["editor_bindings"] = json!([{"id":"editor","purpose":"recording_label","candidate_kind":"text","data_id":"data"}]);
    doc["components"] =
        json!([{"id":"input","kind":"text_input","label":"Label","editor_id":"editor"}]);
    arrange(&mut doc, json!(["input", "input"]));
    assert_eq!(check(&doc).unwrap().summary().expanded_components, 2);
    doc["editor_bindings"].as_array_mut().unwrap().push(
        json!({"id":"second","purpose":"recording_label","candidate_kind":"text","data_id":"data"}),
    );
    rejects(&doc, "only one recording_label");
}

#[test]
fn target_mismatch_is_disabled_without_retargeting_or_guessed_metadata() {
    let mut doc = tests::examples().pop().unwrap();
    let original = doc["data_bindings"][0]["target"].clone();
    for target in [
        property("component", "1", "sample_count"),
        property("instrument", "2", "sample_count"),
        property("instrument", "1", "different"),
    ] {
        doc["action_bindings"][0]["target"] = target.clone();
        let checked = check(&doc).unwrap();
        assert_eq!(
            checked.summary().actions[0],
            StaticAvailability::Disabled(StaticBlocker::ParameterTargetMismatch)
        );
        let typed: RuntimeRef = serde_json::from_value(target).unwrap();
        assert_eq!(checked.dto().action_bindings.0[0].target(), &typed);
        let original: RuntimeRef = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(checked.dto().data_bindings.0[0].target, original);
    }
    doc["data_bindings"][0]["target"] = json!({"kind":"reference","reference":"unknown"});
    doc["action_bindings"][0]["target"] = doc["data_bindings"][0]["target"].clone();
    let checked = check(&doc).unwrap();
    assert_eq!(
        checked.summary().data[0],
        StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
    );
    assert_eq!(
        checked.summary().editors[0],
        StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
    );
    assert_eq!(
        checked.summary().actions[0],
        StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
    );
    doc["data_bindings"][0]["target"] = property("resource", "not-a-number", "sample_count");
    doc["action_bindings"][0]["target"] = doc["data_bindings"][0]["target"].clone();
    doc["data_bindings"][0]["expect_kind"] = json!("boolean");
    let checked = check(&doc).unwrap();
    assert_eq!(
        checked.summary().actions[0],
        StaticAvailability::NeedsRuntimeResolution
    );
    assert_eq!(
        checked.dto().data_bindings.0[0].expect_kind,
        Some(ScalarKind::Boolean)
    );
    assert_eq!(
        checked.dto().data_bindings.0[0].target,
        serde_json::from_value::<RuntimeRef>(property("resource", "not-a-number", "sample_count"))
            .unwrap()
    );
}

#[test]
fn two_property_owners_and_two_editor_ids_keep_distinct_identity() {
    let mut doc = tests::examples().pop().unwrap();
    let mut data = doc["data_bindings"][0].clone();
    data["id"] = json!("second-data");
    data["target"] = property("component", "1", "sample_count");
    let mut editor = doc["editor_bindings"][0].clone();
    editor["id"] = json!("second-editor");
    editor["data_id"] = json!("second-data");
    let mut action = doc["action_bindings"][0].clone();
    action["id"] = json!("second-action");
    action["target"] = data["target"].clone();
    action["value"]["editor_id"] = json!("second-editor");
    doc["data_bindings"].as_array_mut().unwrap().push(data);
    doc["editor_bindings"].as_array_mut().unwrap().push(editor);
    doc["action_bindings"].as_array_mut().unwrap().push(action);
    let checked = check(&doc).unwrap();
    assert!(
        checked
            .summary()
            .actions
            .iter()
            .all(|state| *state == StaticAvailability::NeedsRuntimeResolution)
    );
    assert_ne!(
        checked.dto().data_bindings.0[0].target,
        checked.dto().data_bindings.0[2].target
    );
    doc["data_bindings"][2]["target"] = doc["data_bindings"][0]["target"].clone();
    doc["action_bindings"][1]["target"] = doc["data_bindings"][0]["target"].clone();
    assert_eq!(check(&doc).unwrap().dto().editor_bindings.0.len(), 2);
}

#[test]
fn v1_panel_markers_require_their_static_root_shapes() {
    let mut doc = graph_fixture(json!([leaf("leaf")]), json!(["leaf"]));
    doc["windows"][0]["tabs"][0]["panels"][0]["v1_kind"] = json!("status");
    check(&doc).unwrap();
    for kind in ["controls", "plot"] {
        doc["windows"][0]["tabs"][0]["panels"][0]["v1_kind"] = json!(kind);
        rejects(&doc, "root shape");
    }
    doc["windows"][0]["tabs"][0]["panels"][0]["v1_kind"] = json!("status");
    doc["windows"][0]["tabs"][0]["panels"][0]["components"] = json!([]);
    rejects(&doc, "exactly one");
    doc["windows"][0]["tabs"][0]["panels"][0]["v1_kind"] = json!("controls");
    check(&doc).unwrap();
    let mut doc = tests::examples().pop().unwrap();
    doc["windows"][0]["tabs"][0]["panels"][1]["v1_kind"] = json!("plot");
    check(&doc).unwrap();
}

#[test]
fn v1_inert_origin_ceiling_is_per_occurrence_and_cannot_be_removed_by_nested_groups() {
    let mut doc = tests::examples().pop().unwrap();
    let mut origin = group("origin", json!(["nested"]));
    origin["v1_control"] = json!({"kind":"reconnect_resource","target":{"kind":"resource","resource":"1"},"confirm":false});
    let mut nested = group("nested", json!(["apply"]));
    nested["v1_control"] = json!({"kind":"apply_configuration","target":doc["action_bindings"][0]["target"],"confirm":false});
    doc["components"]
        .as_array_mut()
        .unwrap()
        .extend([origin, nested]);
    arrange(&mut doc, json!(["origin", "apply", "nested"]));
    let checked = check(&doc).unwrap();
    assert_eq!(
        checked
            .occurrence(0, 0, 0, 0, &[0, 0])
            .unwrap()
            .availability,
        StaticAvailability::Disabled(StaticBlocker::InertV1Origin)
    );
    assert_eq!(
        checked.occurrence(0, 0, 0, 1, &[]).unwrap().availability,
        StaticAvailability::NeedsRuntimeResolution
    );
    assert_eq!(
        checked.occurrence(0, 0, 0, 2, &[0]).unwrap().availability,
        StaticAvailability::NeedsRuntimeResolution
    );
    for kind in [
        "recording_lifecycle",
        "set_reference",
        "controller_lifecycle",
        "apply_configuration",
    ] {
        doc["components"][3]["v1_control"]["kind"] = json!(kind);
        let checked = check(&doc).unwrap();
        assert_eq!(
            checked
                .occurrence(0, 0, 0, 0, &[0, 0])
                .unwrap()
                .availability,
            StaticAvailability::Disabled(StaticBlocker::InertV1Origin)
        );
    }
    doc["components"][3]["v1_control"] = json!({"kind":"set_reference","target":{"kind":"reference","reference":"1"},"confirm":true});
    assert_eq!(
        check(&doc)
            .unwrap()
            .occurrence(0, 0, 0, 0, &[0, 0])
            .unwrap()
            .availability,
        StaticAvailability::Disabled(StaticBlocker::V1ActionMismatch)
    );
    doc["components"][3]["v1_control"] = json!({"kind":"apply_configuration","target":property("component","1","sample_count"),"confirm":false});
    assert_eq!(
        check(&doc)
            .unwrap()
            .occurrence(0, 0, 0, 0, &[0, 0])
            .unwrap()
            .availability,
        StaticAvailability::Disabled(StaticBlocker::V1ActionMismatch)
    );
    doc["components"][1]["action_id"] = Value::Null;
    doc["components"][1]["disabled_reason"] = json!("Unsupported v1 control kind/target");
    assert_eq!(
        check(&doc)
            .unwrap()
            .occurrence(0, 0, 0, 0, &[0, 0])
            .unwrap()
            .availability,
        StaticAvailability::Disabled(StaticBlocker::InertButton)
    );
}

#[test]
fn authority_fields_and_unknown_action_families_are_still_closed_shape_errors() {
    let doc = tests::examples().pop().unwrap();
    for field in [
        "expected_revision",
        "generation",
        "boot",
        "scope",
        "sequence",
        "output_lease",
        "confirm",
    ] {
        let mut bad = doc.clone();
        bad["action_bindings"][0][field] = json!("1");
        assert!(check(&bad).is_err());
    }
    let mut bad = doc;
    bad["action_bindings"][0]["family"] = json!("output_write");
    assert!(check(&bad).is_err());
}

#[test]
fn every_known_source_and_action_target_category_is_checked_without_runtime_resolution() {
    let targets = [
        json!({"kind":"signal","instrument":"opaque","parameter":"001"}),
        property("instrument", "opaque", "p"),
        json!({"kind":"reference","reference":"opaque"}),
        json!({"kind":"controller","controller":"opaque"}),
        json!({"kind":"recorder"}),
        json!({"kind":"instrument","instrument":"opaque"}),
        json!({"kind":"resource","resource":"opaque"}),
        json!({"kind":"component","component":"opaque"}),
    ];
    for (expected, selector) in [
        "signal_sample",
        "configuration_property_current",
        "reference_value",
        "controller_state",
        "recorder_state",
    ]
    .iter()
    .enumerate()
    {
        for (actual, target) in targets.iter().enumerate() {
            let mut doc = empty();
            doc["data_bindings"] = json!([{"id":"data","selector":selector,"target":target}]);
            let state = check(&doc).unwrap().summary().data[0];
            assert_eq!(
                state,
                if expected == actual {
                    StaticAvailability::NeedsRuntimeResolution
                } else {
                    StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
                }
            );
        }
    }
    for (family, origin_kind, expected, field, literal) in [
        (
            "recorder_lifecycle",
            "recording_lifecycle",
            4,
            Some("label"),
            json!({"kind":"text","value":"run"}),
        ),
        (
            "reference_fixed",
            "set_reference",
            2,
            Some("value"),
            json!({"kind":"number","value":1}),
        ),
        (
            "controller_lifecycle",
            "controller_lifecycle",
            3,
            None,
            Value::Null,
        ),
        (
            "property_configure",
            "apply_configuration",
            1,
            Some("value"),
            json!({"kind":"integer","value":"1"}),
        ),
    ] {
        for (actual, target) in targets.iter().enumerate() {
            let mut doc = empty();
            let mut action = json!({"id":"action","family":family,"target":target});
            if let Some(field) = field {
                action[field] = json!({"kind":"literal","value":literal});
            }
            doc["action_bindings"] = json!([action]);
            let mut origin = group("origin", json!(["button"]));
            origin["v1_control"] = json!({"kind":origin_kind,"target":target,"confirm":false});
            doc["components"] = json!([origin,{"id":"button","kind":"button","label":"Action","action_id":"action"}]);
            arrange(&mut doc, json!(["origin"]));
            doc["windows"][0]["tabs"][0]["panels"][0]["v1_kind"] = json!("controls");
            let checked = check(&doc).unwrap();
            if expected == actual {
                assert_eq!(
                    checked.summary().actions[0],
                    StaticAvailability::NeedsRuntimeResolution
                );
                assert_eq!(
                    checked.occurrence(0, 0, 0, 0, &[0]).unwrap().availability,
                    StaticAvailability::NeedsRuntimeResolution
                );
            } else {
                assert_eq!(
                    checked.summary().actions[0],
                    StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
                );
                assert_eq!(
                    checked.occurrence(0, 0, 0, 0, &[0]).unwrap().availability,
                    StaticAvailability::Disabled(StaticBlocker::InertV1Origin)
                );
            }
        }
    }
}

#[test]
fn action_families_cannot_borrow_another_purposes_editor() {
    let mut doc = empty();
    doc["data_bindings"] = json!([
        {"id":"recorder","selector":"recorder_state","target":{"kind":"recorder"}},
        {"id":"reference","selector":"reference_value","target":{"kind":"reference","reference":"1"}},
        {"id":"property","selector":"configuration_property_current","target":property("instrument","1","p")}
    ]);
    doc["editor_bindings"] = json!([
        {"id":"recording-editor","purpose":"recording_label","candidate_kind":"text","data_id":"recorder"},
        {"id":"reference-editor","purpose":"reference_fixed","candidate_kind":"number","data_id":"reference"},
        {"id":"property-editor","purpose":"property_value","candidate_kind":"text","data_id":"property"}
    ]);
    for (family, field, correct) in [
        ("recorder_lifecycle", "label", "recording-editor"),
        ("reference_fixed", "value", "reference-editor"),
        ("property_configure", "value", "property-editor"),
    ] {
        for editor in ["recording-editor", "reference-editor", "property-editor"] {
            let mut action = json!({"id":"action","family":family,"target":{"kind":"recorder"}});
            action[field] = json!({"kind":"editor","editor_id":editor});
            doc["action_bindings"] = json!([action]);
            if editor == correct {
                check(&doc).unwrap();
            } else {
                rejects(&doc, "family/editor purpose mismatch");
            }
        }
    }
}

//! Bounded deterministic inverse/normal-form tests, with no running owner.

use super::{conversion_fixtures as fixtures, *};
use crate::presentation as v1;
use serde_json::{Value, json};

fn convert(source: &v1::PresentationDocument) -> AdmittedV2Document {
    convert_v1(ValidatedV1Document::check(source).unwrap()).unwrap()
}
fn value() -> Value {
    serde_json::to_value(convert(&fixtures::mixed()).dto()).unwrap()
}
fn component<'a>(doc: &'a mut Value, id: &str) -> &'a mut Value {
    doc["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == id)
        .unwrap()
}
fn reject(doc: Value, name: &str) -> ExportError {
    let source = admit(&serde_json::to_vec(&doc).unwrap())
        .unwrap_or_else(|e| panic!("{name} must reach export: {e}"));
    let bytes = source.canonical_bytes().to_vec();
    let summary = format!("{:?}", source.summary());
    let allocation = source.allocation();
    let error = export_v1(&source).unwrap_err();
    assert!(
        matches!(error, ExportError::UnrepresentableV1(_)),
        "{name}: {error}"
    );
    assert!(error.to_string().len() < 128);
    assert_eq!(bytes, source.canonical_bytes(), "{name}");
    assert_eq!(summary, format!("{:?}", source.summary()), "{name}");
    assert_eq!(allocation, source.allocation(), "{name}");
    error
}
fn round_trip(source: &v1::PresentationDocument) {
    let original = serde_json::to_vec_pretty(source).unwrap();
    let converted = convert(source);
    let restored = export_v1(&converted).unwrap();
    restored.validate().unwrap();
    restored.validate_serialized_size().unwrap();
    // Bytes distinguish signed zero; PartialEq alone does not.
    assert_eq!(original, serde_json::to_vec_pretty(&restored).unwrap());
    assert_eq!(*source, restored);
    let rebuilt = convert(&restored);
    assert_eq!(converted.canonical_bytes(), rebuilt.canonical_bytes());
    assert_eq!(converted.summary(), rebuilt.summary());
    assert_eq!(original, serde_json::to_vec_pretty(source).unwrap());
}

#[test]
fn export_round_trip_empty_arrangement_repetition_unused_and_shared_recorder() {
    round_trip(&v1::PresentationDocument::empty("empty"));
    let mut source = fixtures::mixed();
    let mut unused = source.controls[0].clone();
    unused.id = "unused_control".into();
    source.controls.insert(2, unused);
    // Several nonempty windows/tabs, with repeated references preserving order.
    let mut tab = source.windows[0].tabs[1].clone();
    tab.id = "extra_tab".into();
    for panel in &mut tab.panels {
        panel.id.push_str("_extra");
    }
    source.windows[1].tabs.push(tab);
    source.windows.reverse();
    source.controls.reverse();
    source.plots.reverse();
    for plot in &mut source.plots {
        plot.traces.reverse();
    }
    for window in &mut source.windows {
        window.tabs.reverse();
        for tab in &mut window.tabs {
            tab.panels.reverse();
        }
    }
    round_trip(&source);
    round_trip(&fixtures::expansion());
    round_trip(&export_fixtures::long_normal_source());
}

#[test]
fn export_all_five_kinds_all_targets_confirm_and_inert_action_floor() {
    let targets: Vec<v1::RuntimeRef> = serde_json::from_value(json!([
        {"kind":"instrument","instrument":"001"}, {"kind":"signal","instrument":"001","parameter":"p"},
        {"kind":"reference","reference":"r"}, {"kind":"controller","controller":"c"},
        {"kind":"resource","resource":"res"}, {"kind":"component","component":"component"}, {"kind":"recorder"},
        {"kind":"configuration_property","owner":{"kind":"instrument","instrument":"same"},"property":"gain"},
        {"kind":"configuration_property","owner":{"kind":"component","component":"same"},"property":"gain"},
        {"kind":"configuration_property","owner":{"kind":"resource","resource":"same"},"property":"gain"}
    ])).unwrap();
    for confirm in [false, true] {
        let mut source = v1::PresentationDocument::empty("matrix");
        for kind in [
            v1::ControlKind::RecordingLifecycle,
            v1::ControlKind::SetReference,
            v1::ControlKind::ControllerLifecycle,
            v1::ControlKind::ApplyConfiguration,
            v1::ControlKind::ReconnectResource,
        ] {
            for target in &targets {
                source.controls.push(v1::Control {
                    id: format!("c{}", source.controls.len()),
                    label: "Control".into(),
                    kind: kind.clone(),
                    target: target.clone(),
                    confirm,
                });
            }
        }
        // Every source category also survives as an inactive or active trace.
        let mut plot = fixtures::mixed().plots.remove(0);
        let trace = plot.traces[0].clone();
        plot.traces = targets
            .iter()
            .enumerate()
            .map(|(i, target)| v1::Trace {
                id: format!("t{i}"),
                source: target.clone(),
                ..trace.clone()
            })
            .collect();
        source.plots.push(plot);
        round_trip(&source);
        let converted = convert(&source);
        assert_eq!(converted.dto().action_bindings.0.len(), 6);
        // Only the original four semantic families exist; confirm is provenance.
        let families: Vec<_> = converted
            .dto()
            .action_bindings
            .0
            .iter()
            .map(|a| {
                serde_json::to_value(a).unwrap()["family"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            families,
            [
                "recorder_lifecycle",
                "reference_fixed",
                "controller_lifecycle",
                "property_configure",
                "property_configure",
                "property_configure"
            ]
        );
    }
}

#[test]
fn export_collision_algorithm_all_source_namespaces_and_exact_strings() {
    let mut source = fixtures::mixed();
    source.document_id = "v2:data:0".into();
    source.windows[0].id = "v2:editor:0".into();
    source.windows[0].tabs[0].id = "v2:action:0".into();
    source.windows[0].tabs[1].panels[0].id = "v2:component:0".into();
    source.plots[0].traces[0].id = "v2:data:2".into();
    source.plots[1].id = "v2:editor:2".into();
    source.controls[5].id = "v2:component:2".into();
    if let v1::PanelKind::Controls { control_ids } = &mut source.windows[0].tabs[1].panels[0].kind {
        control_ids[4] = "v2:component:2".into();
    }
    for control in &mut source.controls {
        control.label = "\0\r\n\t\\\" e\u{301}é Ж".repeat(20);
    }
    source.plots[0].traces[0].display_label = "Ж".repeat(256);
    round_trip(&source);
}

#[test]
fn export_exact_binary64_bits_and_numeric_boundaries() {
    let mut source = fixtures::mixed();
    // No parser detour: test exact original f64 bits, not the JSON parser's rounding.
    for number in [
        0.0,
        -0.0,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        -f64::MAX,
        9007199254740991.0,
        9007199254740992.0,
        9007199254740994.0,
        0.1,
        f64::from_bits(0x3fd5_5555_5555_5555),
    ] {
        for lower in [false, true] {
            source.plots[0].axes.y_min = lower.then_some(number);
            source.plots[0].axes.y_max = (!lower).then_some(number);
            round_trip(&source);
            let restored = export_v1(&convert(&source)).unwrap();
            let axes = &restored.plots[0].axes;
            assert_eq!(
                if lower { axes.y_min } else { axes.y_max }
                    .unwrap()
                    .to_bits(),
                number.to_bits()
            );
        }
    }
    source.plots[0].axes = v1::AxisOptions::default();
    for window in [f64::from_bits(1), f64::MIN_POSITIVE, 604800.0] {
        for width in [f64::from_bits(1), f64::MIN_POSITIVE, 32.0] {
            source.plots[0].time_window_seconds = window;
            source.plots[0].traces[0].style.width = width;
            round_trip(&source);
        }
    }
}

#[test]
fn export_default_normalization_and_representable_edits() {
    let mut doc = value();
    doc.as_object_mut().unwrap().remove("settings");
    for data in doc["data_bindings"].as_array_mut().unwrap() {
        data.as_object_mut().unwrap().remove("expect_kind");
    }
    for editor in doc["editor_bindings"].as_array_mut().unwrap() {
        editor.as_object_mut().unwrap().remove("constraints");
    }
    for c in doc["components"].as_array_mut().unwrap() {
        if c["applicability"] == "always" {
            c.as_object_mut().unwrap().remove("applicability");
        }
    }
    // These fields have a v1 inverse; changing them is not a lossy template edit.
    component(&mut doc, "record")["label"] = json!("Changed control label");
    component(&mut doc, "record")["v1_control"]["confirm"] = json!(true);
    doc["plots"][0]["traces"][0]["visible"] = json!(false);
    doc["data_bindings"][4]["target"]["parameter"] = json!("another-source");
    let source = admit(&serde_json::to_vec(&doc).unwrap()).unwrap();
    let restored = export_v1(&source).unwrap();
    assert!(restored.controls[0].confirm);
    assert_eq!(restored.controls[0].label, "Changed control label");
    assert_eq!(
        source.canonical_bytes(),
        convert(&restored).canonical_bytes()
    );
}

#[test]
fn export_refuses_every_template_field_edit_and_preserves_source() {
    let edits = [
        ("/settings/density", json!("compact")),
        ("/components/0/layout", json!("row")),
        ("/components/1/label", json!("Edited state")),
        (
            "/components/0/children",
            json!(["v2:component:1", "v2:component:0", "v2:component:2"]),
        ),
        ("/editor_bindings/1/constraints", json!({"number_min":-0.0})),
        ("/data_bindings/1/expect_kind", json!("number")),
        (
            "/action_bindings/1/value",
            json!({"kind":"literal","value":{"kind":"number","value":1.0}}),
        ),
        ("/components/2/applicability", json!("matching_type")),
    ];
    for (path, replacement) in edits {
        let mut doc = value();
        *doc.pointer_mut(path)
            .unwrap_or_else(|| panic!("missing {path}")) = replacement;
        assert_eq!(
            reject(doc, path),
            ExportError::UnrepresentableV1(match path.split('/').nth(1).unwrap() {
                "settings" => "/settings",
                "editor_bindings" => "/editor_bindings",
                "data_bindings" => "/data_bindings",
                "action_bindings" => "/action_bindings",
                _ => "/components",
            })
        );
    }
    for (id, field, replacement) in [
        ("record", "help", json!("Extra help")),
        ("v2:component:3", "precision", json!(2)),
        ("v2:component:13", "disabled_reason", json!("Other reason")),
    ] {
        let mut doc = value();
        component(&mut doc, id)[field] = replacement;
        reject(doc, field);
    }
}

#[test]
fn export_refuses_missing_markers_arbitrary_groups_toggle_select_and_extra_definitions() {
    let mut doc = value();
    doc["windows"] = json!([]);
    component(&mut doc, "record")
        .as_object_mut()
        .unwrap()
        .remove("v1_control");
    reject(doc, "missing origin");
    let mut doc = value();
    doc["windows"][0]["tabs"][1]["panels"][0]
        .as_object_mut()
        .unwrap()
        .remove("v1_kind");
    reject(doc, "missing panel annotation");
    let mut doc = value();
    component(&mut doc, "record")["children"] = json!(["v2:component:0", "v2:component:3"]);
    reject(doc, "multi-source group");
    for (widget, scalar) in [("toggle", "boolean"), ("select", "enum")] {
        let mut doc = value();
        doc["editor_bindings"].as_array_mut().unwrap().push(json!({"id":"extra-editor","data_id":"v2:data:4","purpose":"local_only","candidate_kind":scalar}));
        doc["components"].as_array_mut().unwrap().push(
            json!({"id":"extra-widget","kind":widget,"label":"Extra","editor_id":"extra-editor"}),
        );
        reject(doc, widget);
    }
    for (registry, extra) in [
        (
            "components",
            json!({"id":"extra","kind":"status","label":"Unused","source":{"kind":"entity","target":{"kind":"recorder"}}}),
        ),
        (
            "data_bindings",
            json!({"id":"extra","selector":"recorder_state","target":{"kind":"recorder"}}),
        ),
        (
            "editor_bindings",
            json!({"id":"extra","data_id":"v2:data:1","purpose":"local_only","candidate_kind":"number"}),
        ),
        (
            "action_bindings",
            json!({"id":"extra","family":"controller_lifecycle","target":{"kind":"controller","controller":"c"}}),
        ),
    ] {
        let mut doc = value();
        doc[registry].as_array_mut().unwrap().push(extra);
        reject(doc, registry);
    }
}

#[test]
fn export_refuses_noncanonical_ids_registry_order_and_trace_binding_changes() {
    for (from, to) in [
        ("v2:component:0", "renamed"),
        ("v2:data:4", "v2:data:04"),
        ("v2:editor:1", "v2:editor:99"),
        ("v2:action:1", "custom-action"),
    ] {
        let bytes = serde_json::to_string(&value())
            .unwrap()
            .replace(&format!("\"{from}\""), &format!("\"{to}\""));
        reject(serde_json::from_str(&bytes).unwrap(), from);
    }
    for registry in [
        "components",
        "data_bindings",
        "editor_bindings",
        "action_bindings",
    ] {
        let mut doc = value();
        doc[registry].as_array_mut().unwrap().swap(0, 1);
        reject(doc, registry);
    }
    let mut doc = value();
    doc["plots"][0]["traces"][1]["data_id"] = json!("v2:data:4");
    reject(doc, "shared trace data with leftover definition");
    let mut doc = value();
    doc["plots"][0]["traces"][0]["data_id"] = json!("v2:data:5");
    doc["plots"][0]["traces"][1]["data_id"] = json!("v2:data:4");
    reject(doc, "swapped independent trace bindings");
}

#[test]
fn export_refuses_action_substitution_inert_activation_and_shared_control_bindings() {
    let mut doc = value();
    doc["action_bindings"][1] = json!({"id":"v2:action:1","family":"controller_lifecycle","target":{"kind":"controller","controller":"c"}});
    reject(doc, "different family");
    let mut doc = value();
    let button = component(&mut doc, "v2:component:13");
    button["action_id"] = json!("v2:action:2");
    button.as_object_mut().unwrap().remove("disabled_reason");
    reject(doc, "inert action injection");
    let mut source = fixtures::mixed();
    source.windows.clear();
    let mut reference = source.controls[1].clone();
    reference.id = "reference2".into();
    source.controls.push(reference);
    let converted = convert(&source);
    let doc = serde_json::to_value(converted.dto()).unwrap();
    for (from, to) in [
        ("v2:data:4", "v2:data:1"),
        ("v2:editor:3", "v2:editor:1"),
        ("v2:action:5", "v2:action:1"),
    ] {
        // Replace references, then remove the obsolete definition: still coupled.
        let mut edited = doc.clone();
        for registry in ["data_bindings", "editor_bindings", "action_bindings"] {
            edited[registry]
                .as_array_mut()
                .unwrap()
                .retain(|d| d["id"] != from);
        }
        let bytes = serde_json::to_string(&edited)
            .unwrap()
            .replace(&format!("\"{from}\""), &format!("\"{to}\""));
        reject(serde_json::from_str(&bytes).unwrap(), from);
    }
}

#[test]
fn export_refuses_broken_recorder_topology_and_property_pair() {
    let mut doc = value();
    let mut editor = doc["editor_bindings"][0].clone();
    editor["id"] = json!("separate-recorder-editor");
    doc["editor_bindings"].as_array_mut().unwrap().push(editor);
    component(&mut doc, "v2:component:15")["editor_id"] = json!("separate-recorder-editor");
    doc["action_bindings"][4]["label"]["editor_id"] = json!("separate-recorder-editor");
    // The v2 validator itself forbids two recording_label editors. Use local_only
    // to keep a structurally admitted edited proposal with no new action grant.
    doc["editor_bindings"][3]["purpose"] = json!("local_only");
    doc["action_bindings"][4]["label"]["editor_id"] = json!("v2:editor:0");
    reject(doc, "separate recorder input");
    let mut doc = value();
    let mut data = doc["data_bindings"][0].clone();
    data["id"] = json!("separate-recorder-data");
    doc["data_bindings"].as_array_mut().unwrap().push(data);
    component(&mut doc, "v2:component:14")["source"]["data_id"] = json!("separate-recorder-data");
    reject(doc, "separate recorder read");
    let mut doc = value();
    component(&mut doc, "property")["children"] =
        json!(["v2:component:8", "v2:component:9", "v2:component:11"]);
    reject(doc, "missing property text input");
    let mut doc = value();
    doc["editor_bindings"][2]["candidate_kind"] = json!("integer");
    component(&mut doc, "v2:component:10")["kind"] = json!("integer_input");
    reject(doc, "property editor loses text capability");
}

#[test]
fn export_limits_v1_count_size_and_amplified_normal_form_are_atomic() {
    let mut groups = value();
    groups["windows"] = json!([]);
    groups["components"] = json!([]);
    groups["data_bindings"] = json!([]);
    groups["editor_bindings"] = json!([]);
    groups["action_bindings"] = json!([]);
    groups["plots"] = json!([]);
    for i in 0..65 {
        groups["components"].as_array_mut().unwrap().push(json!({
            "id":format!("c{i}"),"kind":"group","label":"C","layout":"column","children":[],
            "v1_control":{"kind":"reconnect_resource","target":{"kind":"resource","resource":"r"},"confirm":false}
        }));
    }
    for (name, doc) in [
        ("65 controls", groups),
        (
            "v1 serialized-size amplification",
            export_fixtures::shared_traces(480, true),
        ),
        (
            "repeat conversion wire amplification",
            export_fixtures::shared_traces(600, false),
        ),
    ] {
        let source =
            admit(&serde_json::to_vec(&doc).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let before = source.canonical_bytes().to_vec();
        assert_eq!(
            export_v1(&source).unwrap_err(),
            ExportError::ConversionLimit,
            "{name}"
        );
        assert_eq!(before, source.canonical_bytes());
    }
    assert_eq!(ExportError::ConversionLimit.to_string(), "conversion_limit");
}

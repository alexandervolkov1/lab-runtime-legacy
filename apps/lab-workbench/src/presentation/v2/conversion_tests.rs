use super::{conversion_fixtures as fixtures, *};
use crate::presentation as v1;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn convert(source: &v1::PresentationDocument) -> AdmittedV2Document {
    convert_v1(ValidatedV1Document::check(source).unwrap()).unwrap()
}
fn value(doc: &AdmittedV2Document) -> Value {
    serde_json::to_value(doc.dto()).unwrap()
}
fn targets() -> Vec<v1::RuntimeRef> {
    serde_json::from_value(json!([
        {"kind":"instrument","instrument":"001"}, {"kind":"signal","instrument":"001","parameter":"p"},
        {"kind":"reference","reference":"r"}, {"kind":"controller","controller":"c"},
        {"kind":"resource","resource":"res"}, {"kind":"component","component":"component"}, {"kind":"recorder"},
        {"kind":"configuration_property","owner":{"kind":"instrument","instrument":"same"},"property":"gain"},
        {"kind":"configuration_property","owner":{"kind":"component","component":"same"},"property":"gain"},
        {"kind":"configuration_property","owner":{"kind":"resource","resource":"same"},"property":"gain"}
    ])).unwrap()
}
fn kinds() -> Vec<v1::ControlKind> {
    vec![
        v1::ControlKind::RecordingLifecycle,
        v1::ControlKind::SetReference,
        v1::ControlKind::ControllerLifecycle,
        v1::ControlKind::ApplyConfiguration,
        v1::ControlKind::ReconnectResource,
    ]
}
fn supported(control: &v1::Control) -> bool {
    matches!(
        (&control.kind, &control.target),
        (
            v1::ControlKind::RecordingLifecycle,
            v1::RuntimeRef::Recorder
        ) | (
            v1::ControlKind::SetReference,
            v1::RuntimeRef::Reference { .. }
        ) | (
            v1::ControlKind::ControllerLifecycle,
            v1::RuntimeRef::Controller { .. }
        ) | (
            v1::ControlKind::ApplyConfiguration,
            v1::RuntimeRef::ConfigurationProperty { .. }
        )
    )
}
fn matrix() -> v1::PresentationDocument {
    let mut doc = v1::PresentationDocument::empty("matrix");
    for kind in kinds() {
        for target in targets() {
            doc.controls.push(v1::Control {
                id: format!("c{}", doc.controls.len()),
                label: "Control".into(),
                kind: kind.clone(),
                target,
                confirm: false,
            });
        }
    }
    doc
}

#[test]
fn empty_source_and_every_source_validation_gate() {
    let source = v1::PresentationDocument::empty("empty");
    let converted = value(&convert(&source));
    assert_eq!(
        converted,
        json!({"format_version":2,"binding_version":1,"document_id":"empty","windows":[],"components":[],"data_bindings":[],"editor_bindings":[],"action_bindings":[],"plots":[],"settings":{"density":"comfortable"}})
    );
    let mut bad = source.clone();
    bad.format_version = 2;
    assert!(matches!(
        ValidatedV1Document::check(&bad),
        Err(ConversionError::InvalidV1(
            v1::DocumentError::UnsupportedVersion(2)
        ))
    ));
    bad = fixtures::mixed();
    bad.controls[0].id = bad.document_id.clone();
    assert!(ValidatedV1Document::check(&bad).is_err());
    bad = fixtures::mixed();
    bad.plots[0].time_window_seconds = f64::NAN;
    assert!(ValidatedV1Document::check(&bad).is_err());
    bad = fixtures::mixed();
    bad.controls[0].label = "Ж".repeat(257);
    assert!(ValidatedV1Document::check(&bad).is_err());
    bad = fixtures::mixed();
    bad.windows[0].tabs[1].panels[0].kind = v1::PanelKind::Controls {
        control_ids: vec!["absent".into()],
    };
    assert!(ValidatedV1Document::check(&bad).is_err());
    let mut oversized = fixtures::near_limit();
    for plot in &mut oversized.plots {
        for trace in &mut plot.traces {
            trace.style.color = "x".repeat(512);
        }
    }
    oversized.validate().unwrap();
    assert!(oversized.validate_serialized_size().is_err());
    assert_eq!(
        ValidatedV1Document::check(&oversized).unwrap_err(),
        ConversionError::InvalidV1(v1::DocumentError::Limit("file_bytes"))
    );
}

#[test]
fn exact_normal_form_templates_and_allocation_order() {
    let source = fixtures::mixed();
    let doc = value(&convert(&source));
    let components = doc["components"].as_array().unwrap();
    let groups: Vec<_> = components.iter().filter(|c| c["kind"] == "group").collect();
    assert_eq!(groups.len(), source.controls.len());
    assert_eq!(components.len(), 25);
    assert_eq!(doc["data_bindings"].as_array().unwrap().len(), 6);
    assert_eq!(doc["editor_bindings"].as_array().unwrap().len(), 3);
    assert_eq!(doc["action_bindings"].as_array().unwrap().len(), 5);
    for (i, control) in source.controls.iter().enumerate() {
        let group = groups[i];
        assert_eq!(group["id"], control.id);
        assert_eq!(group["label"], control.label);
        assert_eq!(group["layout"], "column");
        assert_eq!(
            group["v1_control"],
            json!({"kind":control.kind,"target":control.target,"confirm":control.confirm})
        );
    }
    let specifications = [
        (
            "record",
            vec![
                ("status", "Recorder state"),
                ("text_input", "Recording label"),
                ("button", "Action"),
            ],
        ),
        (
            "reference",
            vec![
                ("indicator", "Current"),
                ("number_input", "New value"),
                ("button", "Action"),
            ],
        ),
        (
            "controller",
            vec![("status", "Controller state"), ("button", "Action")],
        ),
        (
            "property",
            vec![
                ("indicator", "Current"),
                ("integer_input", "Integer candidate"),
                ("text_input", "Text candidate"),
                ("button", "Action"),
            ],
        ),
        (
            "reconnect",
            vec![("status", "Target"), ("button", "Unavailable")],
        ),
        (
            "record2",
            vec![
                ("status", "Recorder state"),
                ("text_input", "Recording label"),
                ("button", "Action"),
            ],
        ),
    ];
    let mut next = 0;
    for (id, specs) in specifications {
        let group = components.iter().find(|c| c["id"] == id).unwrap();
        for (i, (kind, label)) in specs.iter().enumerate() {
            let child_id = format!("v2:component:{next}");
            assert_eq!(group["children"][i], child_id);
            let child = components.iter().find(|c| c["id"] == child_id).unwrap();
            assert_eq!(child["kind"], *kind);
            assert_eq!(child["label"], *label);
            assert!(child.get("help").is_none());
            next += 1;
        }
    }
    assert_eq!(doc["data_bindings"][0]["selector"], "recorder_state");
    assert_eq!(doc["data_bindings"][1]["selector"], "reference_value");
    assert_eq!(doc["data_bindings"][2]["selector"], "controller_state");
    assert_eq!(
        doc["data_bindings"][3]["selector"],
        "configuration_property_current"
    );
    for (i, editor) in doc["editor_bindings"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(editor["id"], format!("v2:editor:{i}"));
        assert_eq!(editor["constraints"], json!({}));
    }
    assert_eq!(
        doc["editor_bindings"][2]["candidate_kind"],
        "property_scalar"
    );
    for i in [9, 10] {
        let input = components
            .iter()
            .find(|c| c["id"] == format!("v2:component:{i}"))
            .unwrap();
        assert_eq!(input["applicability"], "matching_type");
        assert_eq!(input["editor_id"], "v2:editor:2");
    }
    let families = [
        "recorder_lifecycle",
        "reference_fixed",
        "controller_lifecycle",
        "property_configure",
        "recorder_lifecycle",
    ];
    for (i, family) in families.iter().enumerate() {
        assert_eq!(doc["action_bindings"][i]["id"], format!("v2:action:{i}"));
        assert_eq!(doc["action_bindings"][i]["family"], *family);
    }
    assert_eq!(
        doc["action_bindings"][0]["label"],
        doc["action_bindings"][4]["label"]
    );
    assert_eq!(
        doc["action_bindings"][0]["label"],
        json!({"kind":"editor","editor_id":"v2:editor:0"})
    );
    assert_eq!(
        doc["windows"][0]["tabs"][1]["panels"][1]["components"],
        json!(["v2:component:17"])
    );
    assert_eq!(
        doc["windows"][0]["tabs"][1]["panels"][2]["components"],
        json!(["v2:component:18"])
    );
}

#[test]
fn all_kind_target_pairs_keep_full_origins_and_only_trusted_action_families() {
    let source = matrix();
    let admitted = convert(&source);
    let doc = value(&admitted);
    for control in &source.controls {
        let group = doc["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == control.id)
            .unwrap();
        assert_eq!(
            group["v1_control"]["target"],
            serde_json::to_value(&control.target).unwrap()
        );
        assert_eq!(group["v1_control"]["confirm"], false);
        let last = group["children"].as_array().unwrap().last().unwrap();
        let button = doc["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| &c["id"] == last)
            .unwrap();
        if supported(control) {
            let action = doc["action_bindings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["id"] == button["action_id"])
                .unwrap();
            assert_eq!(
                action["target"],
                serde_json::to_value(&control.target).unwrap()
            );
        } else {
            assert_eq!(group["children"].as_array().unwrap().len(), 2);
            assert!(button["action_id"].is_null());
            assert_eq!(
                button["disabled_reason"],
                "Unsupported v1 control kind/target"
            );
        }
    }
    assert_eq!(admitted.dto().action_bindings.0.len(), 6);
    assert_eq!(admitted.dto().editor_bindings.0.len(), 5);
    assert!(
        admitted
            .summary()
            .actions
            .iter()
            .all(|a| *a == StaticAvailability::NeedsRuntimeResolution)
    );
}

#[test]
fn arrangement_unused_definitions_trace_independence_and_every_display_field() {
    let mut source = fixtures::mixed();
    source.controls.push(v1::Control {
        id: "unused".into(),
        label: "Unused control".into(),
        target: v1::RuntimeRef::Recorder,
        kind: v1::ControlKind::RecordingLifecycle,
        confirm: true,
    });
    let admitted = convert(&source);
    let doc = value(&admitted);
    for (w, window) in source.windows.iter().enumerate() {
        assert_eq!(doc["windows"][w]["id"], window.id);
        assert_eq!(doc["windows"][w]["title"], window.title);
        assert_eq!(
            doc["windows"][w]["tabs"].as_array().unwrap().len(),
            window.tabs.len()
        );
        for (t, tab) in window.tabs.iter().enumerate() {
            let out = &doc["windows"][w]["tabs"][t];
            assert_eq!(out["id"], tab.id);
            assert_eq!(out["title"], tab.title);
            assert_eq!(out["panels"].as_array().unwrap().len(), tab.panels.len());
            for (p, panel) in tab.panels.iter().enumerate() {
                let out = &out["panels"][p];
                assert_eq!(out["id"], panel.id);
                assert_eq!(out["title"], panel.title);
                match &panel.kind {
                    v1::PanelKind::Controls { control_ids } => {
                        assert_eq!(out["v1_kind"], "controls");
                        assert_eq!(out["components"], json!(control_ids));
                    }
                    v1::PanelKind::Plot { plot_id } => {
                        assert_eq!(out["v1_kind"], "plot");
                        let leaf = doc["components"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|c| c["id"] == out["components"][0])
                            .unwrap();
                        assert_eq!(leaf["plot_id"], *plot_id);
                        assert_eq!(
                            leaf["label"],
                            source
                                .plots
                                .iter()
                                .find(|p| &p.id == plot_id)
                                .unwrap()
                                .title
                        );
                    }
                    v1::PanelKind::Status { source } => {
                        assert_eq!(out["v1_kind"], "status");
                        let leaf = doc["components"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|c| c["id"] == out["components"][0])
                            .unwrap();
                        assert_eq!(leaf["source"], json!({"kind":"entity","target":source}));
                        assert_eq!(leaf["label"], panel.title);
                    }
                }
            }
        }
    }
    for (i, plot) in source.plots.iter().enumerate() {
        let output = &doc["plots"][i];
        assert_eq!(output["id"], plot.id);
        assert_eq!(output["title"], plot.title);
        assert_eq!(output["time_window_seconds"], plot.time_window_seconds);
        assert_eq!(output["axes"], serde_json::to_value(&plot.axes).unwrap());
        assert_eq!(
            output["traces"].as_array().unwrap().len(),
            plot.traces.len()
        );
        for (j, trace) in plot.traces.iter().enumerate() {
            let out = &output["traces"][j];
            let data = doc["data_bindings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["id"] == out["data_id"])
                .unwrap();
            assert_eq!(data["target"], serde_json::to_value(&trace.source).unwrap());
            assert_eq!(data["selector"], "signal_sample");
            for field in ["id", "display_label", "visible", "style", "display_unit"] {
                assert_eq!(out[field], serde_json::to_value(trace).unwrap()[field]);
            }
        }
    }
    assert_ne!(
        doc["plots"][0]["traces"][0]["data_id"],
        doc["plots"][0]["traces"][1]["data_id"]
    );
    assert!(
        doc["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "unused")
    );
    assert_eq!(
        admitted.dto().plots.0[0].axes.y_min.unwrap().0.to_bits(),
        (-0.0f64).to_bits()
    );
}

#[test]
fn collision_search_reserves_every_source_registry_before_generating_any_ids() {
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
    let doc = value(&convert(&source));
    assert_eq!(doc["document_id"], "v2:data:0");
    assert_eq!(doc["data_bindings"][0]["id"], "v2:data:1");
    assert_eq!(doc["data_bindings"][1]["id"], "v2:data:3");
    assert_eq!(doc["editor_bindings"][0]["id"], "v2:editor:1");
    assert_eq!(doc["editor_bindings"][1]["id"], "v2:editor:3");
    assert_eq!(doc["action_bindings"][0]["id"], "v2:action:1");
    assert_eq!(
        doc["components"][0]["children"],
        json!(["v2:component:1", "v2:component:3", "v2:component:4"])
    );
    assert!(
        doc["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "v2:component:2" && c["kind"] == "group")
    );
}

#[test]
fn source_unchanged_deterministic_bytes_and_exact_binary64_display_values() {
    let mut source = fixtures::mixed();
    let before = serde_json::to_vec(&source).unwrap();
    let first = convert(&source);
    let second = convert(&source);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.summary(), second.summary());
    assert_eq!(serde_json::to_vec(&source).unwrap(), before);
    for number in [
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        -f64::MAX,
        -0.0,
        9007199254740994.0,
    ] {
        source.plots[0].axes.y_min = Some(number);
        source.plots[0].axes.y_max = None;
        let converted = convert(&source);
        assert_eq!(
            converted.dto().plots.0[0].axes.y_min.unwrap().0.to_bits(),
            number.to_bits()
        );
    }
    // Independently re-enter the normal JSON admission path for the neutral case.
    let reparsed = admit(first.canonical_bytes()).unwrap();
    assert_eq!(reparsed.canonical_bytes(), first.canonical_bytes());
}

#[test]
fn finite_expansion_and_definition_budgets_and_atomic_conversion_limit() {
    let source = fixtures::expansion();
    let doc = convert(&source);
    assert_eq!(doc.summary().expanded_components, 20_480);
    assert_eq!(doc.dto().components.0.len(), 5);
    assert_eq!(doc.dto().editor_bindings.0.len(), 1);
    let mut definitions = fixtures::mixed();
    definitions.windows.clear();
    definitions.plots.clear();
    let property = definitions.controls[3].clone();
    definitions.controls = (0..64)
        .map(|i| {
            let mut c = property.clone();
            c.id = format!("p{i}");
            c
        })
        .collect();
    let doc = convert(&definitions);
    assert_eq!(doc.dto().components.0.len(), 320);
    assert_eq!(doc.dto().editor_bindings.0.len(), 64);
    assert_eq!(doc.dto().action_bindings.0.len(), 64);
    let too_large = fixtures::near_limit();
    too_large.validate().unwrap();
    too_large.validate_serialized_size().unwrap();
    let before = serde_json::to_vec(&too_large).unwrap();
    assert_eq!(
        convert_v1(ValidatedV1Document::check(&too_large).unwrap()).unwrap_err(),
        ConversionError::ConversionLimit
    );
    assert_eq!(serde_json::to_vec(&too_large).unwrap(), before);
    assert_eq!(
        ConversionError::ConversionLimit.to_string(),
        "conversion_limit"
    );
}

#[test]
fn incompatible_trace_sources_remain_disabled_without_retargeting() {
    let mut source = fixtures::mixed();
    source.plots[0].traces[0].source = v1::RuntimeRef::Resource {
        resource: "not-a-signal".into(),
    };
    let doc = convert(&source);
    let id = &doc.dto().plots.0[0].traces.0[0].data_id;
    let index = doc
        .dto()
        .data_bindings
        .0
        .iter()
        .position(|d| &d.id == id)
        .unwrap();
    assert_eq!(
        doc.summary().data[index],
        StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
    );
    assert_eq!(
        serde_json::to_value(&doc.dto().data_bindings.0[index].target).unwrap(),
        json!({"kind":"resource","resource":"not-a-signal"})
    );
}

#[test]
fn reused_nested_groups_cannot_escape_disabled_origin_ceilings() {
    let original = convert(&fixtures::mixed());
    let mut edited = value(&original);
    let group = edited["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "reconnect")
        .unwrap();
    group["children"] = json!(["record", "record"]);
    let admitted = admit(&serde_json::to_vec(&edited).unwrap()).unwrap();
    for child in [0, 1] {
        assert_eq!(
            admitted
                .source()
                .occurrence(0, 1, 0, 3, &[child, 2])
                .unwrap()
                .availability,
            StaticAvailability::Disabled(StaticBlocker::InertV1Origin)
        );
    }
    assert_eq!(
        admitted
            .source()
            .occurrence(0, 1, 0, 0, &[2])
            .unwrap()
            .availability,
        StaticAvailability::NeedsRuntimeResolution
    );
    assert_eq!(
        admitted
            .source()
            .occurrence(0, 1, 0, 2, &[2])
            .unwrap()
            .availability,
        StaticAvailability::NeedsRuntimeResolution
    );
}

fn v1_suppressed(source: &v1::PresentationDocument) -> BTreeSet<(String, String)> {
    let mut result = BTreeSet::new();
    for panel in source
        .windows
        .iter()
        .flat_map(|w| &w.tabs)
        .flat_map(|t| &t.panels)
    {
        if let v1::PanelKind::Controls { control_ids } = &panel.kind {
            for control in &source.controls {
                if control_ids.contains(&control.id) && supported(control) {
                    result.insert((
                        serde_json::to_string(&control.kind).unwrap(),
                        serde_json::to_string(&control.target).unwrap(),
                    ));
                }
            }
        }
    }
    result
}
fn v2_suppressed(doc: &AdmittedV2Document) -> BTreeSet<(String, String)> {
    let mut result = BTreeSet::new();
    for (w, window) in doc.dto().windows.0.iter().enumerate() {
        for (t, tab) in window.tabs.0.iter().enumerate() {
            for (p, panel) in tab.panels.0.iter().enumerate() {
                for root in 0..panel.components.0.len() {
                    let group = doc.source().occurrence(w, t, p, root, &[]).unwrap();
                    if let Component::Group {
                        children,
                        v1_control: Some(origin),
                        ..
                    } = group.component
                    {
                        for child in 0..children.0.len() {
                            let occurrence =
                                doc.source().occurrence(w, t, p, root, &[child]).unwrap();
                            if matches!(
                                occurrence.component,
                                Component::Button {
                                    action_id: Some(_),
                                    ..
                                }
                            ) && occurrence.availability
                                == StaticAvailability::NeedsRuntimeResolution
                            {
                                result.insert((
                                    serde_json::to_string(&origin.kind).unwrap(),
                                    serde_json::to_string(&origin.target).unwrap(),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    result
}
#[test]
fn shared_recorder_draft_and_referenced_only_suppression_include_hidden_tabs() {
    let mut source = fixtures::mixed();
    for controls in [
        vec![],
        vec!["reconnect"],
        vec!["record", "record2", "record"],
        vec!["controller"],
    ] {
        source.windows[0].tabs[1].panels[0].kind = v1::PanelKind::Controls {
            control_ids: controls.into_iter().map(str::to_owned).collect(),
        };
        let converted = convert(&source);
        assert_eq!(v2_suppressed(&converted), v1_suppressed(&source));
        assert_eq!(
            converted
                .dto()
                .editor_bindings
                .0
                .iter()
                .filter(|e| e.purpose == Purpose::RecordingLabel)
                .count(),
            1
        );
        assert_eq!(
            converted
                .dto()
                .data_bindings
                .0
                .iter()
                .filter(|d| d.selector == Selector::RecorderState)
                .count(),
            1
        );
    }
}

#[test]
fn exact_v1_canonical_size_gate_matches_existing_serializer_without_a_buffer() {
    let mut source = fixtures::near_limit();
    let baseline = serde_json::to_vec_pretty(&source).unwrap().len();
    let mut remaining = 1_048_576usize.checked_sub(baseline).unwrap();
    for plot in &mut source.plots {
        for trace in &mut plot.traces {
            let count = remaining.min(512 - trace.style.color.len());
            trace.style.color.push_str(&"x".repeat(count));
            remaining -= count;
        }
    }
    assert_eq!(remaining, 0);
    assert_eq!(serde_json::to_vec_pretty(&source).unwrap().len(), 1_048_576);
    source.validate_serialized_size().unwrap();
    assert_eq!(
        convert_v1(ValidatedV1Document::check(&source).unwrap()).unwrap_err(),
        ConversionError::ConversionLimit
    );
    source
        .plots
        .last_mut()
        .unwrap()
        .traces
        .last_mut()
        .unwrap()
        .style
        .color
        .push('x');
    assert_eq!(serde_json::to_vec_pretty(&source).unwrap().len(), 1_048_577);
    source.validate().unwrap();
    assert!(source.validate_serialized_size().is_err());
    assert!(matches!(
        ValidatedV1Document::check(&source),
        Err(ConversionError::InvalidV1(_))
    ));
}

#[test]
fn property_drafts_remain_independent_for_equal_names_and_repeated_targets() {
    let mut source = fixtures::mixed();
    source.windows.clear();
    source.plots.clear();
    let property = source.controls[3].clone();
    source.controls = (0..3)
        .map(|i| {
            let mut control = property.clone();
            control.id = format!("property{i}");
            if i == 1 {
                control.target = v1::RuntimeRef::ConfigurationProperty {
                    owner: v1::ConfigurationOwner::Component {
                        component: "another-owner".into(),
                    },
                    property: "gain".into(),
                };
            }
            control
        })
        .collect();
    let doc = convert(&source);
    assert_eq!(doc.dto().editor_bindings.0.len(), 3);
    assert_eq!(doc.dto().data_bindings.0.len(), 3);
    assert_eq!(doc.dto().action_bindings.0.len(), 3);
    let unique: BTreeSet<_> = doc.dto().editor_bindings.0.iter().map(|e| &e.id).collect();
    assert_eq!(unique.len(), 3);
    for (i, control) in source.controls.iter().enumerate() {
        assert_eq!(
            serde_json::to_value(doc.dto().action_bindings.0[i].target()).unwrap(),
            serde_json::to_value(&control.target).unwrap()
        );
        assert_eq!(
            doc.dto().action_bindings.0[i].target(),
            &doc.dto().data_bindings.0[i].target
        );
    }
}

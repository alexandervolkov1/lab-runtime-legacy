//! Real headless egui frames over deterministic, admitted syntax and observations.

use super::*;
use crate::{
    model::{Freshness, WorkbenchModel},
    presentation::{PresentationDocument, RuntimeRef},
};
use serde_json::{Value, json};

const EPOCH: PresentationEpoch = PresentationEpoch {
    owner: [1; 16],
    revision: 1,
};

fn syntax() -> Value {
    json!({
        "format_version":2,"binding_version":1,"document_id":"reads",
        "windows":[{"id":"w","title":"Window","tabs":[{"id":"t","title":"Tab","panels":[{"id":"p","title":"Panel","components":["group"]}]}]}],
        "components":[
            {"id":"group","kind":"group","label":"Pair","layout":"row","children":["a","b"]},
            {"id":"a","kind":"indicator","label":"First","data_id":"d1"},
            {"id":"b","kind":"status","label":"Second","source":{"kind":"data","data_id":"d2"}}
        ],
        "data_bindings":[
            {"id":"d1","selector":"signal_sample","target":{"kind":"signal","instrument":"i","parameter":"a"}},
            {"id":"d2","selector":"signal_sample","target":{"kind":"signal","instrument":"i","parameter":"b"}}
        ], "editor_bindings":[],"action_bindings":[],"plots":[]
    })
}
fn admit(doc: &Value) -> v2::AdmittedV2Document {
    v2::admit(&serde_json::to_vec(doc).unwrap()).unwrap()
}
fn signal(parameter: &str) -> RuntimeRef {
    RuntimeRef::Signal {
        instrument: "i".into(),
        parameter: parameter.into(),
    }
}
fn observe(observations: &mut RuntimeObservations, parameter: &str, kind: &str, value: Value) {
    observations.observe_read_descriptor(
        signal(parameter),
        &json!({"value_type":kind,"generation":"1","unit":{"id":"count","symbol":"ct"}}),
    );
    observations.observe(signal(parameter), json!({"signal":{"instrument":"i","parameter":parameter},"generation":"1","quality":"good","status":"available","value":value,"unit":{"id":"count","symbol":"ct"}}), None);
}
fn observations() -> RuntimeObservations {
    let mut result = RuntimeObservations::default();
    observe(&mut result, "a", "integer", json!(i64::MIN.to_string()));
    observe(&mut result, "b", "boolean", json!(false));
    result.complete_rebuild();
    result
}
fn frame(
    context: &egui::Context,
    document: &v2::AdmittedV2Document,
    observations: &RuntimeObservations,
    epoch: PresentationEpoch,
    events: Vec<egui::Event>,
) -> (RenderReport, Vec<(String, egui::Rect)>) {
    let mut report = RenderReport::default();
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            report = show(ui, document, observations, epoch);
        },
    );
    fn texts(shape: &egui::epaint::Shape, result: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::epaint::Shape::Text(text) => {
                result.push((text.galley.text().to_owned(), text.visual_bounding_rect()))
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    texts(shape, result);
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    for shape in &output.shapes {
        texts(&shape.shape, &mut text);
    }
    output.drop_without_applying_deltas();
    (report, text)
}
fn labels(doc: &Value, observations: &RuntimeObservations) -> Vec<String> {
    frame(
        &egui::Context::default(),
        &admit(doc),
        observations,
        EPOCH,
        vec![],
    )
    .1
    .into_iter()
    .map(|(s, _)| s)
    .collect()
}
fn has(text: &[String], needle: &str) {
    assert!(
        text.iter().any(|s| s == needle),
        "missing {needle:?} in {text:?}"
    );
}

#[test]
fn two_independent_reads_and_nested_row_column_use_real_layout() {
    let mut doc = syntax();
    doc["components"][2] = json!({"id":"b","kind":"indicator","label":"Second","data_id":"d2"});
    let observations = observations();
    let (report, painted) = frame(
        &egui::Context::default(),
        &admit(&doc),
        &observations,
        EPOCH,
        vec![],
    );
    assert_eq!(report.occurrences, 3);
    let rect = |name: &str| painted.iter().find(|(text, _)| text == name).unwrap().1;
    assert!(rect("First").min.x < rect("Second").min.x);
    // Painted ink bounds differ with glyph ascenders; the two captions occupy
    // the same row, with no vertical stacking.
    assert!((rect("First").min.y - rect("Second").min.y).abs() <= 2.0);
    let text: Vec<_> = painted.into_iter().map(|(s, _)| s).collect();
    has(&text, "Value (cached): -9223372036854775808");
    has(&text, "Value (cached): false");
    doc["components"][0]["layout"] = json!("column");
    doc["components"].as_array_mut().unwrap().push(
        json!({"id":"nested","kind":"group","label":"Nested","layout":"row","children":["a","b"]}),
    );
    doc["components"][0]["children"] = json!(["a", "nested", "b"]);
    let (report, painted) = frame(
        &egui::Context::default(),
        &admit(&doc),
        &observations,
        EPOCH,
        vec![],
    );
    assert_eq!(report.occurrences, 6);
    assert_eq!(report.maximum_path, 2);
    let first = painted.iter().find(|(s, _)| s == "First").unwrap().1;
    let nested = painted.iter().find(|(s, _)| s == "Nested").unwrap().1;
    let last = painted.iter().rfind(|(s, _)| s == "Second").unwrap().1;
    assert!(first.min.y < nested.min.y && nested.min.y < last.min.y);
}

#[test]
fn all_scalars_exact_integers_empty_text_enum_and_number_precision() {
    let mut doc = syntax();
    doc["components"][1]["precision"] = json!(2);
    for (kind, value, expected) in [
        ("boolean", json!(false), "false"),
        (
            "integer",
            json!(i64::MIN.to_string()),
            "-9223372036854775808",
        ),
        (
            "integer",
            json!(i64::MAX.to_string()),
            "9223372036854775807",
        ),
        ("integer", json!("0"), "0"),
        ("float", json!(0.0), "0.0"),
        ("float", json!(-0.0), "-0.0"),
        ("float", json!(f64::from_bits(1)), "5e-324"),
        ("float", json!(f64::MAX), "1.7976931348623157e308"),
        ("float", json!(1.23456789), "1.23456789"),
        ("text", json!(""), "\"\" (empty Text)"),
        ("text", json!("hello"), "hello"),
        ("enum", json!("ready"), "ready"),
    ] {
        let mut observations = observations();
        observe(&mut observations, "a", kind, value);
        let text = labels(&doc, &observations);
        has(&text, &format!("Value (cached): {expected}"));
        has(&text, "Runtime unit: ct");
        has(&text, "Quality: good");
        if kind != "float" {
            assert!(!text.iter().any(|s| s.starts_with("Rounded:")));
        }
        if expected == "1.23456789" {
            has(&text, "Rounded: 1.23");
        }
    }
}

#[test]
fn absence_metadata_quality_expectation_and_generation_remain_explicit() {
    let mut doc = syntax();
    doc["components"][0]["children"] = json!(["a"]);
    for (field, value, expected) in [
        ("value", Value::Null, "Value (cached): value absent"),
        (
            "generation",
            json!("2"),
            "Value (cached): value and metadata are incompatible",
        ),
    ] {
        let mut observations = observations();
        observations.entities.get_mut(&signal("a")).unwrap().value[field] = value;
        has(&labels(&doc, &observations), expected);
    }
    for (status, quality) in [
        ("unavailable", "Quality: unavailable"),
        ("not_observed", "Quality: not_observed"),
    ] {
        let mut observations = observations();
        let sample = &mut observations.entities.get_mut(&signal("a")).unwrap().value;
        sample["quality"] = json!("unavailable");
        sample["status"] = json!(status);
        sample["value"] = Value::Null;
        let text = labels(&doc, &observations);
        has(&text, quality);
        assert!(!text.iter().any(|s| s.contains("-9223372036854775808")));
    }
    for (metadata, expected) in [
        (json!({"generation":"1"}), "typed metadata absent"),
        (
            json!({"value_type":12,"generation":"1"}),
            "value and metadata are incompatible",
        ),
        (
            json!({"value_type":"unknown","generation":"1"}),
            "unsupported typed value",
        ),
    ] {
        let mut observations = observations();
        observations.observe_read_descriptor(signal("a"), &metadata);
        has(
            &labels(&doc, &observations),
            &format!("Value (cached): {expected}"),
        );
    }
    doc["data_bindings"][0]["expect_kind"] = json!("number");
    let text = labels(&doc, &observations());
    has(&text, "Unavailable: expect_kind mismatch");
    assert!(!text.iter().any(|s| s.contains("-9223372036854775808")));
    let mut missing = RuntimeObservations::default();
    missing.observe(signal("a"), json!({"signal":{"instrument":"i","parameter":"a"},"value":1,"quality":"good","status":"available","generation":"1"}), None);
    has(
        &labels(&doc, &missing),
        "Value (cached): typed metadata absent",
    );
    has(
        &labels(&doc, &RuntimeObservations::default()),
        "Value (cached): no Runtime observation",
    );
}

#[test]
fn freshness_rebuild_and_missing_observation_never_fabricate_fresh_value() {
    let doc = syntax();
    for state in [
        Freshness::Fresh,
        Freshness::Stale,
        Freshness::Rebuilding,
        Freshness::Unknown,
    ] {
        let mut observations = observations();
        observations.freshness = state;
        let text = labels(&doc, &observations);
        assert!(text.iter().any(|s| s.starts_with(&format!("{state:?}"))));
        if state != Freshness::Fresh {
            assert!(!text.iter().any(|s| s == "Fresh"));
        }
    }
    let mut observations = observations();
    observations.begin_rebuild();
    let text = labels(&doc, &observations);
    has(&text, "Rebuilding — freshness not established");
    assert!(!text.iter().any(|s| s == "Fresh"));
    observations = RuntimeObservations::default();
    observations.freshness = Freshness::Fresh;
    let text = labels(&doc, &observations);
    has(&text, "Unresolved — no Runtime observation");
    assert!(!text.iter().any(|s| s == "Fresh"));
}

#[test]
fn repeated_definitions_multiple_arrangements_and_epoch_identity() {
    let mut doc = syntax();
    doc["components"][0]["children"] = json!(["a", "a"]);
    let mut window = doc["windows"][0].clone();
    window["id"] = json!("w2");
    window["title"] = json!("Second window");
    window["tabs"][0]["id"] = json!("t2");
    window["tabs"][0]["panels"][0]["id"] = json!("p2");
    window["tabs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"empty","title":"Empty tab","panels":[]}));
    doc["windows"].as_array_mut().unwrap().push(window);
    let admitted = admit(&doc);
    let identities: Vec<_> = [(0, 0), (0, 1), (1, 0)]
        .map(|(w, child)| OccurrenceIdentity {
            epoch: EPOCH,
            occurrence: admitted
                .source()
                .occurrence(w, 0, 0, 0, &[child])
                .unwrap()
                .identity,
        })
        .into();
    assert_ne!(identities[0], identities[1]);
    assert_ne!(identities[0], identities[2]);
    assert_ne!(identities[0].widget_id(), identities[1].widget_id());
    let changed = OccurrenceIdentity {
        epoch: PresentationEpoch {
            revision: 2,
            ..EPOCH
        },
        ..identities[0].clone()
    };
    let other_owner = OccurrenceIdentity {
        epoch: PresentationEpoch {
            owner: [2; 16],
            ..EPOCH
        },
        ..identities[0].clone()
    };
    assert_ne!(identities[0].widget_id(), changed.widget_id());
    assert_ne!(identities[0].widget_id(), other_owner.widget_id());
    let context = egui::Context::default();
    // Deliberate foreign state under the old ID cannot be addressed by a new epoch.
    context.data_mut(|data| data.insert_temp(identities[0].widget_id(), "old".to_owned()));
    let (report, _) = frame(&context, &admitted, &observations(), changed.epoch, vec![]);
    assert_eq!(
        (
            report.windows,
            report.tabs,
            report.panels,
            report.occurrences
        ),
        (2, 3, 2, 6)
    );
    assert!(context.data(|data| data.get_temp::<String>(changed.widget_id()).is_none()));
}

#[test]
fn compiled_egui_does_not_format_or_retain_diagnostic_id_sources() {
    // This generic is instantiated in the real application test target, whose
    // dev assertions stay enabled. The cfg must come from egui's own artifact.
    #[derive(Hash)]
    struct Source(u64);
    impl std::fmt::Debug for Source {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            panic!("egui attempted to retain ID provenance");
        }
    }
    let root = egui::Id::new(Source(123));
    let child = root.with(Source(456));
    let salt = egui::IdSalt::new(Source(789));
    assert_ne!(root, child);
    assert!(format!("{root:?}").starts_with("id_"));
    assert!(format!("{salt:?}").starts_with("salt_"));
}

#[test]
fn document_owner_revision_and_repeated_occurrences_never_restore_foreign_state() {
    let mut syntax = syntax();
    syntax["components"][0]["children"] = json!(["a", "a"]);
    let first = admit(&syntax);
    syntax["document_id"] = json!("replacement");
    let second = admit(&syntax);
    let address = |doc: &v2::AdmittedV2Document, epoch, child| {
        OccurrenceIdentity {
            epoch,
            occurrence: doc
                .source()
                .occurrence(0, 0, 0, 0, &[child])
                .unwrap()
                .identity,
        }
        .widget_id()
    };
    let old = address(&first, EPOCH, 0);
    let mut ids = std::collections::BTreeSet::new();
    for document in [&first, &second] {
        for owner in [[1; 16], [2; 16]] {
            for revision in 1..=16 {
                for child in 0..2 {
                    assert!(ids.insert(
                        address(document, PresentationEpoch { owner, revision }, child).value()
                    ));
                }
            }
        }
    }
    assert_eq!(ids.len(), 128);
    assert_eq!(old, address(&first, EPOCH, 0));
    let observations = RuntimeObservations::default();
    let before = observations.clone();
    for _ in 0..3 {
        let context = egui::Context::default();
        assert!(egui::text_edit::TextEditState::load(&context, old).is_none());
        // Deliberate foreign state tests the namespace; the renderer creates no
        // editor. Its future children must also remain scoped to this identity.
        egui::text_edit::TextEditState::default().store(&context, old);
        context.data_mut(|data| data.insert_temp(old, "foreign view/draft".to_owned()));
        context.memory_mut(|memory| memory.request_focus(old));
        for revision in 2..=4 {
            let epoch = PresentationEpoch { revision, ..EPOCH };
            let current = address(&second, epoch, 0);
            frame(&context, &second, &observations, epoch, vec![]);
            assert_ne!(context.memory(|memory| memory.focused()), Some(current));
            assert!(context.data(|data| data.get_temp::<String>(current).is_none()));
            assert!(egui::text_edit::TextEditState::load(&context, current).is_none());
        }
        assert_eq!(observations, before);
        assert_eq!(
            context.data(|data| data.count::<egui::text_edit::TextEditState>()),
            1
        );
    }
}

#[test]
fn entity_status_all_targets_and_static_disabled_binding() {
    let mut doc = syntax();
    for target in [
        json!({"kind":"instrument","instrument":"i"}),
        json!({"kind":"resource","resource":"r"}),
        json!({"kind":"component","component":"c"}),
        json!({"kind":"reference","reference":"r"}),
        json!({"kind":"controller","controller":"c"}),
        json!({"kind":"recorder"}),
        json!({"kind":"configuration_property","owner":{"kind":"resource","resource":"r"},"property":"p"}),
    ] {
        let mut observations = RuntimeObservations::default();
        let reference: RuntimeRef = serde_json::from_value(target.clone()).unwrap();
        observations.observe(reference, json!({"state":"running","value":12.5,"value_type":"integer","current":i64::MAX,"diagnostic":"cached"}), None);
        observations.complete_rebuild();
        doc["components"][2]["source"] = json!({"kind":"entity","target":target});
        let text = labels(&doc, &observations);
        assert!(text.iter().any(|s| s.contains("running")
            || s.contains("12.5")
            || s.contains("9223372036854775807")));
    }
    doc["data_bindings"][0]["target"] = json!({"kind":"resource","resource":"r"});
    let text = labels(&doc, &observations());
    has(&text, "Disabled: IncompatibleTarget");
    assert!(!text.iter().any(|s| s.contains("-9223372036854775808")));
}

#[test]
fn oversized_runtime_text_is_refused_and_entity_text_is_a_bounded_prefix() {
    let mut observations = observations();
    observe(&mut observations, "a", "text", json!("Ж".repeat(257)));
    let text = labels(&syntax(), &observations);
    has(
        &text,
        "Unavailable: Runtime scalar exceeds read-only display limit (512 UTF-8 bytes)",
    );
    let value = json!({"state":"Ж".repeat(10000)});
    let mut expected = value.to_string();
    let mut end = 1024;
    while !expected.is_char_boundary(end) {
        end -= 1;
    }
    expected.truncate(end);
    expected.push('…');
    assert_eq!(read::entity_text(&value), expected);
}

#[test]
fn read_only_boundary_and_live_owner_remain_isolated() {
    for source in [include_str!("mod.rs"), include_str!("read.rs")] {
        for forbidden in [
            "ClientHandle",
            "WorkbenchDispatcher",
            "WorkbenchModel",
            "OperatorIntent",
            "OperatorWorkflow",
            "RecoveryJournal",
            "super::app",
            "super::controls",
            "std::fs",
            "std::net",
            ".query(",
            ".mutation(",
            ".dispatch(",
            "TextEdit",
            ".clicked(",
            ".data_mut(",
            ".memory_mut(",
        ] {
            assert!(!source.contains(forbidden), "renderer acquired {forbidden}");
        }
    }
    for consumer in [
        include_str!("../app.rs"),
        include_str!("../../dispatcher.rs"),
        include_str!("../../external.rs"),
    ] {
        assert!(!consumer.contains("readonly_v2"));
    }
    let source = include_str!("mod.rs");
    assert!(!source.contains("Vec<"));
    assert!(!source.contains("HashMap"));
    assert!(!std::hint::black_box(ACTIVATION_QUALIFIED));
    let admitted = admit(&syntax());
    let bytes = admitted.canonical_bytes().to_vec();
    let mut model = WorkbenchModel::new(PresentationDocument::empty("active-v1"));
    model.observations = observations();
    model.recovery.scope = Some("retained-scope".into());
    model.recovery.next_seq = Some(42);
    let before = model.clone();
    let dispatcher = crate::dispatcher::WorkbenchDispatcher::new(model, SilentClient::default());
    let workflow = crate::model::OperatorWorkflow::default();
    let before_workflow = workflow.clone();
    let recovery_generation = dispatcher.recovery_generation();
    let presentation_revision = dispatcher.presentation_revision();
    let context = egui::Context::default();
    for revision in 1..=3 {
        frame(
            &context,
            &admitted,
            &dispatcher.observations,
            PresentationEpoch { revision, ..EPOCH },
            vec![egui::Event::Text("input cannot mutate".into())],
        );
        assert_eq!(&*dispatcher, &before);
        assert_eq!(workflow, before_workflow);
        assert_eq!(dispatcher.recovery_generation(), recovery_generation);
        assert_eq!(dispatcher.presentation_revision(), presentation_revision);
        assert_eq!(dispatcher.client().unwrap().0.get(), 0);
        assert_eq!(bytes, admitted.canonical_bytes());
        assert_eq!(context.data(|data| data.len()), 0);
    }
}

#[derive(Default)]
struct SilentClient(std::cell::Cell<usize>);
impl crate::dispatcher::LabClient for SilentClient {
    fn query(&self, _: &str, _: Value) -> Result<u64, crate::client::types::CommandSendError> {
        self.0.set(self.0.get() + 1);
        Ok(1)
    }
    fn mutation(&self, _: &str, _: Value) -> Result<u64, crate::client::types::CommandSendError> {
        self.0.set(self.0.get() + 1);
        Ok(1)
    }
    fn operation_status(
        &self,
        _: crate::client::types::MutationIdentity,
    ) -> Result<u64, crate::client::types::CommandSendError> {
        self.0.set(self.0.get() + 1);
        Ok(1)
    }
}

#[test]
fn all_data_selectors_and_complete_property_owner_identities_use_the_read_layer() {
    let mut doc = syntax();
    doc["components"][0]["children"] = json!(["a"]);
    let mut cases = vec![
        (
            "reference_value",
            json!({"kind":"reference","reference":"r"}),
            json!({"value":-0.0}),
            "Value (cached): -0.0".to_owned(),
        ),
        (
            "controller_state",
            json!({"kind":"controller","controller":"c"}),
            json!({"state":"paused"}),
            "Value (cached): paused".into(),
        ),
        (
            "recorder_state",
            json!({"kind":"recorder"}),
            json!({"state":"recording"}),
            "Value (cached): recording".into(),
        ),
    ];
    for (i, owner) in [
        json!({"kind":"instrument","instrument":"same"}),
        json!({"kind":"component","component":"same"}),
        json!({"kind":"resource","resource":"same"}),
    ]
    .into_iter()
    .enumerate()
    {
        cases.push((
            "configuration_property_current",
            json!({"kind":"configuration_property","owner":owner,"property":"gain"}),
            json!({"value_type":"integer","current":i as i64}),
            format!("Value (cached): {i}"),
        ));
    }
    let mut observations = RuntimeObservations::default();
    for (_, target, observation, _) in &cases {
        observations.observe(
            serde_json::from_value(target.clone()).unwrap(),
            observation.clone(),
            None,
        );
    }
    observations.complete_rebuild();
    for (selector, target, _, expected) in cases {
        doc["data_bindings"][0]["selector"] = json!(selector);
        doc["data_bindings"][0]["target"] = target;
        has(&labels(&doc, &observations), &expected);
    }
}

#[test]
fn maximum_graph_streams_all_occurrences_without_retained_widget_state() {
    let admitted = fixtures::maximum_graph();
    assert_eq!(admitted.summary().expanded_components, 20_480);
    assert_eq!(admitted.dto().components.0.len(), 3);
    let context = egui::Context::default();
    let observations = RuntimeObservations::default();
    let before = admitted.canonical_bytes().to_vec();
    for revision in 1..=2 {
        // Retain no test-side expansion/painted-text list for the large fixture.
        context
            .run_ui(egui::RawInput::default(), |ui| {
                let report = show(
                    ui,
                    &admitted,
                    &observations,
                    PresentationEpoch { revision, ..EPOCH },
                );
                assert_eq!(report.occurrences, 20_480);
                assert_eq!(report.panels, 64);
                assert_eq!(report.maximum_path, 1);
            })
            .drop_without_applying_deltas();
        assert_eq!(context.data(|data| data.len()), 0);
    }
    assert_eq!(before, admitted.canonical_bytes());
    assert!(std::mem::size_of::<Frame<'_>>() < 4096);
    assert!(admitted.visual_reservation().accounted_bytes() <= 8 * 1024 * 1024);
}

#[test]
fn four_group_levels_and_inert_placeholders_preserve_occurrence_origin_ceiling() {
    let source: PresentationDocument = serde_json::from_value(json!({
        "format_version":1,"document_id":"inert","windows":[],"plots":[],
        "controls":[
            {"id":"ref","label":"Reference","kind":"set_reference","target":{"kind":"reference","reference":"r"},"confirm":false},
            {"id":"inert","label":"Reconnect","kind":"reconnect_resource","target":{"kind":"resource","resource":"r"},"confirm":false}
        ]
    })).unwrap();
    // Keep document/global identities unique.
    let mut source = source;
    source.document_id = "document".into();
    let converted = v2::convert_v1(v2::ValidatedV1Document::check(&source).unwrap()).unwrap();
    let mut doc = serde_json::to_value(converted.dto()).unwrap();
    doc["windows"] = syntax()["windows"].clone();
    doc["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["inert"]);
    let inert = doc["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "inert")
        .unwrap();
    inert["children"] = json!(["g2"]);
    for (id, child) in [("g2", "g3"), ("g3", "ref")] {
        doc["components"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"kind":"group","label":id,"layout":"column","children":[child]}));
    }
    let admitted = admit(&doc);
    let (report, text) = frame(
        &egui::Context::default(),
        &admitted,
        &RuntimeObservations::default(),
        EPOCH,
        vec![],
    );
    assert_eq!(report.maximum_path, 4);
    assert_eq!(report.placeholders, 2);
    let text: Vec<_> = text.into_iter().map(|(s, _)| s).collect();
    has(&text, "Disabled: InertV1Origin");
    has(
        &text,
        "NumberInput: unsupported in read-only preview (inert)",
    );
    has(&text, "Button: unsupported in read-only preview (inert)");
    assert!(!text.iter().any(|s| s.contains("Confirm")));
    // A supported ancestor with a different full target is also a ceiling,
    // even when a reused descendant has its own supported v1 origin.
    let root = doc["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "inert")
        .unwrap();
    root["v1_control"] = json!({"kind":"set_reference","target":{"kind":"reference","reference":"another"},"confirm":false});
    has(
        &labels(&doc, &RuntimeObservations::default()),
        "Disabled: V1ActionMismatch",
    );
}

#[test]
fn every_unimplemented_component_is_a_noninteractive_placeholder() {
    let mut doc = syntax();
    for (widget, kind) in [
        ("toggle", "boolean"),
        ("select", "enum"),
        ("integer_input", "integer"),
        ("number_input", "number"),
        ("text_input", "text"),
    ] {
        doc["editor_bindings"].as_array_mut().unwrap().push(json!({"id":format!("editor-{widget}"),"data_id":"d1","purpose":"local_only","candidate_kind":kind}));
        doc["components"].as_array_mut().unwrap().push(json!({"id":widget,"kind":widget,"label":widget,"editor_id":format!("editor-{widget}")}));
        doc["components"][0]["children"]
            .as_array_mut()
            .unwrap()
            .push(json!(widget));
    }
    doc["components"].as_array_mut().unwrap().extend([
        json!({"id":"button","kind":"button","label":"Do not execute","action_id":null,"disabled_reason":"No action"}),
        json!({"id":"chart","kind":"plot","label":"No plot implementation","plot_id":"plot"})
    ]);
    doc["plots"] = json!([{"id":"plot","title":"Plot","time_window_seconds":1.0,"axes":{"y_min":null,"y_max":null},"traces":[]}]);
    doc["components"][0]["children"]
        .as_array_mut()
        .unwrap()
        .extend([json!("button"), json!("chart")]);
    doc["components"][0]["layout"] = json!("column");
    let admitted = admit(&doc);
    let context = egui::Context::default();
    let (report, painted) = frame(&context, &admitted, &observations(), EPOCH, vec![]);
    assert_eq!(report.placeholders, 7);
    let text: Vec<_> = painted.iter().map(|(s, _)| s.clone()).collect();
    for kind in [
        "Button",
        "Toggle",
        "Select",
        "IntegerInput",
        "NumberInput",
        "TextInput",
        "Plot",
    ] {
        has(
            &text,
            &format!("{kind}: unsupported in read-only preview (inert)"),
        );
    }
    let at = painted
        .iter()
        .find(|(s, _)| s == "Do not execute")
        .unwrap()
        .1
        .center();
    let before = admitted.canonical_bytes().to_vec();
    frame(
        &context,
        &admitted,
        &observations(),
        EPOCH,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
            egui::Event::Text("forbidden candidate".into()),
        ],
    );
    assert_eq!(before, admitted.canonical_bytes());
    assert_eq!(
        context.data(|data| data.count::<egui::text_edit::TextEditState>()),
        0
    );
    assert!(context.memory(|memory| memory.focused()).is_none());
}

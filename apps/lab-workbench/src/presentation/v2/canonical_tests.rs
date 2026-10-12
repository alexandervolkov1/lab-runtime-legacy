//! Explicit byte goldens and bounded offline envelope-capacity fixtures.

use super::{
    bounded::{Number, Text},
    canonical::CappedBuffer,
    wire::Exchange,
    *,
};
use serde_json::{Value, json};

fn empty() -> Value {
    json!({"format_version":2,"binding_version":1,"document_id":"doc","windows":[],"components":[],
        "data_bindings":[],"editor_bindings":[],"action_bindings":[],"plots":[]})
}
fn checked(value: &Value) -> StructurallyCheckedDocument {
    parse(&serde_json::to_vec(value).unwrap())
        .unwrap()
        .check_local()
        .unwrap()
        .check_structure()
        .unwrap()
}

#[test]
fn numeric_and_escaping_golden_bytes() {
    for (value, expected) in [
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (42.0, "42.0"),
        (1e30, "1e+30"),
        (1e-6, "1e-6"),
        (1e-5, "0.00001"),
        (1e15, "1000000000000000.0"),
        (1e16, "1e+16"),
        (f64::from_bits(1), "5e-324"),
        (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
        (f64::MAX, "1.7976931348623157e+308"),
        (-f64::MAX, "-1.7976931348623157e+308"),
        (9007199254740991.0, "9007199254740991.0"),
        (9007199254740992.0, "9007199254740992.0"),
        (9007199254740994.0, "9007199254740994.0"),
    ] {
        assert_eq!(
            serde_json::to_vec(&Number(value)).unwrap(),
            expected.as_bytes(),
            "{value:?}"
        );
        assert_eq!(expected.parse::<f64>().unwrap().to_bits(), value.to_bits());
    }
    for (value, expected) in [
        (i64::MIN, "\"-9223372036854775808\""),
        (i64::MAX, "\"9223372036854775807\""),
        (0, "\"0\""),
        (-1, "\"-1\""),
        (9007199254740991, "\"9007199254740991\""),
        (9007199254740992, "\"9007199254740992\""),
        (9007199254740993, "\"9007199254740993\""),
    ] {
        assert_eq!(
            serde_json::to_vec(&I64Decimal(value)).unwrap(),
            expected.as_bytes()
        );
        assert_eq!(
            serde_json::from_str::<I64Decimal>(expected).unwrap().0,
            value
        );
    }
    for (value, expected) in [(0, "\"0\""), (u64::MAX, "\"18446744073709551615\"")] {
        assert_eq!(
            serde_json::to_vec(&U64Decimal(value)).unwrap(),
            expected.as_bytes()
        );
        assert_eq!(
            serde_json::from_str::<U64Decimal>(expected).unwrap().0,
            value
        );
    }
    let text = Text::<false>::try_from("é🦀\"\\\0\u{1}\n\r\t\u{8}\u{c}".to_string()).unwrap();
    assert_eq!(
        serde_json::to_vec(&text).unwrap(),
        "\"é🦀\\\"\\\\\\u0000\\u0001\\n\\r\\t\\b\\f\"".as_bytes()
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(serde_json::to_vec(&Number(value)).is_err());
    }
}

#[test]
fn decimal_encoding_covers_all_bit_widths_and_number_spellings_normalize() {
    for bit in 0..64 {
        let unsigned = 1u64 << bit;
        for value in [
            unsigned - 1,
            unsigned,
            unsigned.saturating_add(1),
            u64::MAX - unsigned,
        ] {
            let encoded = serde_json::to_vec(&U64Decimal(value)).unwrap();
            assert_eq!(encoded, format!("\"{value}\"").as_bytes());
            assert_eq!(
                serde_json::from_slice::<U64Decimal>(&encoded).unwrap().0,
                value
            );
            let signed = i64::from_ne_bytes(value.to_ne_bytes());
            let encoded = serde_json::to_vec(&I64Decimal(signed)).unwrap();
            assert_eq!(encoded, format!("\"{signed}\"").as_bytes());
            assert_eq!(
                serde_json::from_slice::<I64Decimal>(&encoded).unwrap().0,
                signed
            );
        }
    }
    for (input, expected) in [
        ("0", "0.0"),
        ("-0", "-0.0"),
        ("3e2", "300.0"),
        ("300.00", "300.0"),
        ("9007199254740993", "9007199254740992.0"),
    ] {
        let number: Number = serde_json::from_str(input).unwrap();
        assert_eq!(serde_json::to_vec(&number).unwrap(), expected.as_bytes());
    }
    let controls = Text::<false>::try_from("\u{1f}/".to_owned()).unwrap();
    assert_eq!(serde_json::to_vec(&controls).unwrap(), br#""\u001f/""#);
}

// Test-only normalization is independent of the serializer's field selection.
fn normalize(mut value: Value) -> Value {
    fn number(value: &mut Value) {
        if !value.is_null() {
            *value = json!(value.as_f64().unwrap());
        }
    }
    value
        .as_object_mut()
        .unwrap()
        .entry("settings")
        .or_insert(json!({"density":"comfortable"}));
    for data in value["data_bindings"].as_array_mut().unwrap() {
        data.as_object_mut()
            .unwrap()
            .entry("expect_kind")
            .or_insert(Value::Null);
    }
    for editor in value["editor_bindings"].as_array_mut().unwrap() {
        editor
            .as_object_mut()
            .unwrap()
            .entry("constraints")
            .or_insert(json!({}));
        // These two fields alone are contract Number, never Integer bounds.
        for key in ["number_min", "number_max"] {
            if let Some(bound) = editor["constraints"].get_mut(key) {
                number(bound);
            }
        }
    }
    for plot in value["plots"].as_array_mut().unwrap() {
        number(&mut plot["time_window_seconds"]);
        number(&mut plot["axes"]["y_min"]);
        number(&mut plot["axes"]["y_max"]);
        for trace in plot["traces"].as_array_mut().unwrap() {
            number(&mut trace["style"]["width"]);
        }
    }
    for component in value["components"].as_array_mut().unwrap() {
        if matches!(
            component["kind"].as_str(),
            Some("toggle" | "integer_input" | "number_input" | "text_input" | "select")
        ) {
            component
                .as_object_mut()
                .unwrap()
                .entry("applicability")
                .or_insert(json!("always"));
        }
    }
    value
}

// Write keys in reverse order at every level without altering array order.
fn reversed(value: &Value) -> String {
    match value {
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .rev()
                .map(|(key, value)| format!(
                    "{}:{}",
                    serde_json::to_string(key).unwrap(),
                    reversed(value)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Array(array) => format!(
            "[{}]",
            array.iter().map(reversed).collect::<Vec<_>>().join(",")
        ),
        scalar => serde_json::to_string(scalar).unwrap(),
    }
}

#[test]
fn three_complete_examples_normalize_order_and_defaults_without_mutation() {
    for value in tests::examples() {
        let source = checked(&value);
        let original = format!("{source:?}");
        let candidate = source.check_canonical_wire().unwrap();
        assert!(std::ptr::eq(candidate.source(), &source));
        assert_eq!(format!("{source:?}"), original);
        let canonical: Value = serde_json::from_slice(candidate.bytes()).unwrap();
        assert_eq!(canonical, normalize(value.clone()));
        let reordered = parse(reversed(&value).as_bytes())
            .unwrap()
            .check_local()
            .unwrap()
            .check_structure()
            .unwrap();
        assert_eq!(
            candidate.bytes(),
            reordered.check_canonical_wire().unwrap().bytes()
        );
        let explicit_defaults = checked(&normalize(value));
        assert_eq!(
            candidate.bytes(),
            explicit_defaults.check_canonical_wire().unwrap().bytes()
        );
        let reparsed = parse(candidate.bytes())
            .unwrap()
            .check_local()
            .unwrap()
            .check_structure()
            .unwrap();
        assert_eq!(source.summary(), reparsed.summary());
        assert_eq!(
            candidate.bytes(),
            reparsed.check_canonical_wire().unwrap().bytes()
        );
        let document_usage = bounded::inspect(candidate.bytes()).unwrap();
        let compact_length = serde_json::to_vec(source.dto()).unwrap().len();
        let request = candidate.request_usage();
        let response = candidate.response_usage();
        assert_eq!(
            request.json.values_members,
            document_usage.values_members + 18
        );
        assert_eq!(
            response.json.values_members,
            document_usage.values_members + 14
        );
        assert_eq!(request.json.max_depth, document_usage.max_depth + 2);
        assert_eq!(response.json.max_depth, document_usage.max_depth + 2);
        // Worst legal call_id: 64 NUL bytes -> 384 encoded bytes, plus quotes.
        assert_eq!(request.json.body_bytes, compact_length + 566);
        assert_eq!(response.json.body_bytes, compact_length + 540);
        assert_eq!(request.lf_frame_bytes, request.json.body_bytes + 1);
        assert_eq!(request.crlf_frame_bytes, request.json.body_bytes + 2);
        let mut wire_buffer = CappedBuffer::new(limits::JSON_BODY_BYTES).unwrap();
        for exchange in [Exchange::ReplaceRequest, Exchange::PresentationResult] {
            wire::probe(source.dto(), exchange, &mut wire_buffer).unwrap();
            assert!(!wire_buffer.bytes.contains(&b'\n'));
            let wire: Value = serde_json::from_slice(&wire_buffer.bytes).unwrap();
            assert_eq!(wire["call_id"].as_str().unwrap().len(), 64);
            assert_eq!(wire["v"], 2);
            match exchange {
                Exchange::ReplaceRequest => {
                    assert_eq!(wire["type"], "request");
                    assert_eq!(wire["op"], "ui_replace_document");
                    assert_eq!(
                        wire["args"]["expected"],
                        json!({"workbench_id":"ffffffffffffffffffffffffffffffff","revision":"18446744073709551615"})
                    );
                }
                Exchange::PresentationResult => {
                    assert_eq!(wire["type"], "result");
                    assert_eq!(
                        wire["result"]["workbench_id"],
                        "ffffffffffffffffffffffffffffffff"
                    );
                    assert_eq!(
                        wire["result"]["presentation_revision"],
                        "18446744073709551615"
                    );
                }
            }
            assert_eq!(
                match exchange {
                    Exchange::ReplaceRequest => &wire["args"]["document"],
                    Exchange::PresentationResult => &wire["result"]["document"],
                },
                &canonical
            );
        }
    }
}

#[test]
fn every_dto_variant_and_runtime_ref_survives_encoding_with_schema_field_order() {
    // This vocabulary fixture is intentionally only local (duplicate IDs). It
    // exercises the serializer, but cannot produce a canonical/wire proof.
    let fixture = tests::vocabulary();
    let local = parse(&serde_json::to_vec(&fixture).unwrap())
        .unwrap()
        .check_local()
        .unwrap();
    let bytes = serde_json::to_vec_pretty(local.dto()).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value, normalize(fixture));
    parse(&bytes).unwrap().check_local().unwrap();
    let group: Component = serde_json::from_value(json!({"id":"g","kind":"group","label":"G","help":"H","layout":"row","children":[],
        "v1_control":{"kind":"reconnect_resource","target":{"kind":"resource","resource":"opaque"},"confirm":false}})).unwrap();
    assert_eq!(serde_json::to_vec(&group).unwrap(),br#"{"id":"g","kind":"group","label":"G","help":"H","layout":"row","children":[],"v1_control":{"kind":"reconnect_resource","target":{"kind":"resource","resource":"opaque"},"confirm":false}}"#);
    let action: ActionBinding = serde_json::from_value(json!({"id":"a","family":"property_configure","target":{"kind":"configuration_property","owner":{"kind":"component","component":"001"},"property":"p"},"value":{"kind":"literal","value":{"kind":"integer","value":"-9223372036854775808"}}})).unwrap();
    assert_eq!(serde_json::to_vec(&action).unwrap(),br#"{"id":"a","family":"property_configure","target":{"kind":"configuration_property","owner":{"kind":"component","component":"001"},"property":"p"},"value":{"kind":"literal","value":{"kind":"integer","value":"-9223372036854775808"}}}"#);
}

fn byte_sized_document(size: usize) -> Value {
    // Fixed 700 entries, then one bounded pass over three strings per entry.
    // Each ASCII padding byte increases both JSON encodings by exactly one byte.
    let mut doc = empty();
    doc["data_bindings"] = json!((0..700).map(|i|json!({"id":format!("d{i}"),"selector":"signal_sample","target":{"kind":"signal","instrument":"i","parameter":"p"}})).collect::<Vec<_>>());
    let base = checked(&doc);
    let baseline = serde_json::to_vec_pretty(base.dto()).unwrap().len();
    let mut remaining = size.checked_sub(baseline).unwrap();
    for data in doc["data_bindings"].as_array_mut().unwrap() {
        for path in ["/id", "/target/instrument", "/target/parameter"] {
            let field = data.pointer_mut(path).unwrap();
            let old = field.as_str().unwrap();
            let count = remaining.min(512 - old.len());
            *field = json!(format!("{old}{}", "x".repeat(count)));
            remaining -= count;
        }
    }
    assert_eq!(remaining, 0);
    doc
}

#[test]
fn exact_canonical_byte_limit_and_one_byte_over_fail_without_a_proof() {
    let source = checked(&byte_sized_document(limits::PRESENTATION_FILE_BYTES));
    let proof = source.check_canonical_wire().unwrap();
    assert_eq!(proof.bytes().len(), 1_048_576);
    assert!(proof.request_usage().crlf_frame_bytes <= limits::FRAME_BYTES);
    drop(proof);
    let too_large = checked(&byte_sized_document(limits::PRESENTATION_FILE_BYTES + 1));
    let before = format!("{:?}", too_large.summary());
    let error = too_large.check_canonical_wire().unwrap_err();
    assert!(error.contains("canonical document") && error.contains("byte limit"));
    assert_eq!(format!("{:?}", too_large.summary()), before);
}

fn value_sized_document(size: usize) -> StructurallyCheckedDocument {
    // 1,000 unused bindings + 80 shallow groups + one leaf. Each repeated child
    // adds one value; the flattened schema stays well below the byte/depth limits.
    let mut doc = empty();
    doc["data_bindings"] = json!((0..1000).map(|i|json!({"id":format!("d{i}"),"selector":"recorder_state","target":{"kind":"recorder"}})).collect::<Vec<_>>());
    doc["components"] = json!([{"id":"leaf","kind":"indicator","label":"Value","data_id":"d0"}]);
    for i in 0..80 {
        doc["components"].as_array_mut().unwrap().push(
            json!({"id":format!("g{i}"),"kind":"group","label":"G","layout":"row","children":[]}),
        );
    }
    let base = checked(&doc);
    let baseline = bounded::inspect(&serde_json::to_vec(base.dto()).unwrap())
        .unwrap()
        .values_members;
    let mut remaining = size.checked_sub(baseline).unwrap();
    for i in 1..=80 {
        let count = remaining.min(64);
        doc["components"][i]["children"] = json!(vec!["leaf"; count]);
        remaining -= count;
    }
    assert_eq!(remaining, 0);
    let source = checked(&doc);
    assert_eq!(
        bounded::inspect(&serde_json::to_vec(source.dto()).unwrap())
            .unwrap()
            .values_members,
        size
    );
    source
}

#[test]
fn real_dto_request_and_response_value_headroom_is_checked_separately() {
    let exact = value_sized_document(limits::JSON_VALUES - 18);
    let proof = exact.check_canonical_wire().unwrap();
    assert_eq!(proof.request_usage().json.values_members, 16_384);
    assert_eq!(proof.response_usage().json.values_members, 16_380);
    let request_over = value_sized_document(limits::JSON_VALUES - 17);
    assert!(
        request_over
            .check_canonical_wire()
            .unwrap_err()
            .contains("request wire headroom")
    );
    let mut buffer = CappedBuffer::new(limits::JSON_BODY_BYTES).unwrap();
    wire::probe(
        request_over.dto(),
        Exchange::PresentationResult,
        &mut buffer,
    )
    .unwrap();
    let response_exact = value_sized_document(limits::JSON_VALUES - 14);
    assert_eq!(
        wire::probe(
            response_exact.dto(),
            Exchange::PresentationResult,
            &mut buffer
        )
        .unwrap()
        .json
        .values_members,
        16_384
    );
    let response_over = value_sized_document(limits::JSON_VALUES - 13);
    assert!(
        wire::probe(
            response_over.dto(),
            Exchange::PresentationResult,
            &mut buffer
        )
        .unwrap_err()
        .contains("value/member limit")
    );
    // Request has four more values, so it rejects first when both exceed the bound.
    assert!(response_over.check_canonical_wire().is_err());
}

#[test]
fn scanner_depth_string_and_physical_frame_limits_include_the_envelope() {
    let mut buffer = CappedBuffer::new(limits::JSON_BODY_BYTES).unwrap();
    for levels in [14, 15] {
        // Scanner-only JSON fixture, not a representable v2 DTO: flattened v2
        // arrangement reaches only depth 8 bare / 10 inside these envelopes.
        let mut nested = json!(0);
        for _ in 0..levels {
            nested = json!([nested]);
        }
        assert_eq!(
            bounded::inspect(&serde_json::to_vec(&nested).unwrap())
                .unwrap()
                .max_depth,
            levels
        );
        for exchange in [Exchange::ReplaceRequest, Exchange::PresentationResult] {
            let result = wire::probe(&nested, exchange, &mut buffer);
            if levels == 14 {
                assert_eq!(result.unwrap().json.max_depth, 16);
            } else {
                assert!(result.unwrap_err().contains("depth limit"));
            }
        }
    }
    let valid = json!("x".repeat(512));
    wire::probe(&valid, Exchange::ReplaceRequest, &mut buffer).unwrap();
    let invalid = json!("x".repeat(513));
    assert!(wire::probe(&invalid, Exchange::PresentationResult, &mut buffer).is_err());
    let mut frame = b"0".to_vec();
    frame.resize(limits::JSON_BODY_BYTES - 1, b' ');
    let usage = wire::inspect_frame(&frame).unwrap();
    assert_eq!(usage.crlf_frame_bytes, 2_097_153);
    frame.push(b' ');
    assert!(
        wire::inspect_frame(&frame)
            .unwrap_err()
            .contains("frame limit")
    );
    assert!(
        wire::inspect_frame(b"{\n}")
            .unwrap_err()
            .contains("frame delimiter")
    );
    let mut tiny = CappedBuffer::new(3).unwrap();
    use std::io::Write;
    tiny.write_all(b"abc").unwrap();
    let capacity = tiny.bytes.capacity();
    assert!(tiny.write_all(b"d").is_err());
    assert_eq!(tiny.bytes, b"abc");
    assert_eq!(tiny.bytes.capacity(), capacity);
}

#[test]
fn disabled_targets_origin_ceilings_and_repeated_references_survive_serialization() {
    let mut doc = tests::examples().pop().unwrap();
    doc["action_bindings"][0]["target"] = json!({"kind":"configuration_property","owner":{"kind":"component","component":"1"},"property":"sample_count"});
    doc["components"].as_array_mut().unwrap().push(json!({"id":"origin","kind":"group","label":"Inert","layout":"column","children":["apply","chart","chart"],
        "v1_control":{"kind":"reconnect_resource","target":{"kind":"resource","resource":"unknown"},"confirm":false}}));
    doc["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["origin", "origin"]);
    let source = checked(&doc);
    let proof = source.check_canonical_wire().unwrap();
    let restored = parse(proof.bytes())
        .unwrap()
        .check_local()
        .unwrap()
        .check_structure()
        .unwrap();
    assert_eq!(source.summary(), restored.summary());
    assert_eq!(
        source.summary().actions[0],
        StaticAvailability::Disabled(StaticBlocker::ParameterTargetMismatch)
    );
    assert_eq!(source.summary().expanded_plots, 5);
    for root in 0..2 {
        let before = source.occurrence(0, 0, 0, root, &[0]).unwrap();
        let after = restored.occurrence(0, 0, 0, root, &[0]).unwrap();
        assert_eq!(
            before.availability,
            StaticAvailability::Disabled(StaticBlocker::InertV1Origin)
        );
        assert_eq!(before.availability, after.availability);
        assert_eq!(before.identity, after.identity);
    }
    assert_eq!(
        serde_json::from_slice::<Value>(proof.bytes()).unwrap(),
        normalize(doc)
    );
}

#[test]
fn empty_document_has_explicit_order_defaults_spacing_and_no_final_newline() {
    let doc = checked(&empty());
    let candidate = doc.check_canonical_wire().unwrap();
    assert_eq!(
        candidate.bytes(),
        br#"{
  "format_version": 2,
  "binding_version": 1,
  "document_id": "doc",
  "windows": [],
  "components": [],
  "data_bindings": [],
  "editor_bindings": [],
  "action_bindings": [],
  "plots": [],
  "settings": {
    "density": "comfortable"
  }
}"#
    );
    assert_eq!(
        candidate.request_usage().json.values_members,
        candidate.response_usage().json.values_members + 4
    );
}

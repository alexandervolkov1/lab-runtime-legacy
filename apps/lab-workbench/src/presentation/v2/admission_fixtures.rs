//! Deterministic test/probe inputs, never observations or application consumers.

use serde_json::{Value, json};

pub(super) fn empty() -> Value {
    json!({"format_version":2,"binding_version":1,"document_id":"doc","windows":[],
        "components":[],"data_bindings":[],"editor_bindings":[],"action_bindings":[],"plots":[]})
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

pub(super) fn expanded() -> Value {
    let mut doc = empty();
    doc["data_bindings"] = json!([{"id":"d","selector":"signal_sample","target":{"kind":"signal","instrument":"1","parameter":"1"}}]);
    doc["components"] = json!([
        {"id":"g","kind":"group","label":"G","layout":"row","children":["leaf","leaf","leaf","leaf"]},
        {"id":"gp","kind":"group","label":"G","layout":"row","children":["leaf","leaf","leaf","plot"]},
        {"id":"leaf","kind":"indicator","label":"Value","data_id":"d"},
        {"id":"plot","kind":"plot","label":"Plot","plot_id":"p"}
    ]);
    doc["plots"] = json!([{"id":"p","title":"Plot","time_window_seconds":60,
        "axes":{"y_min":null,"y_max":null},"traces":[{"id":"trace","data_id":"d","display_label":"Value","visible":true,"style":{"color":"blue","width":1},"display_unit":null}]}]);
    doc["editor_bindings"] = json!((0..64).map(|i|json!({"id":format!("e{i}"),"data_id":"d","purpose":"local_only","candidate_kind":"text"})).collect::<Vec<_>>());
    doc["windows"] = json!([{"id":"w","title":"W","tabs":(0..64).map(|i| json!({"id":format!("t{i}"),"title":"T","panels":[{"id":format!("panel{i}"),"title":"P","components":vec![if i == 0 {"gp"} else {"g"};64]}]})).collect::<Vec<_>>()}]);
    doc
}

pub(super) fn collections() -> Vec<Value> {
    let mut cases = Vec::new();
    let mut doc = empty();
    doc["data_bindings"] = json!((0..1152).map(|i|json!({"id":format!("d{i}"),"selector":"recorder_state","target":{"kind":"recorder"}})).collect::<Vec<_>>());
    cases.push(doc);
    let mut doc = empty();
    doc["components"] = json!((0..512).map(|i|json!({"id":format!("c{i}"),"kind":"status","label":"Status","source":{"kind":"entity","target":{"kind":"recorder"}}})).collect::<Vec<_>>());
    cases.push(doc);
    let mut doc = empty();
    doc["windows"] = json!(
        (0..64)
            .map(|i| json!({"id":format!("w{i}"),"title":"W","tabs":[]}))
            .collect::<Vec<_>>()
    );
    cases.push(doc);
    let mut doc = empty();
    doc["action_bindings"] = json!((0..64).map(|i|json!({"id":format!("a{i}"),"family":"controller_lifecycle","target":{"kind":"controller","controller":format!("opaque{i}")}})).collect::<Vec<_>>());
    cases.push(doc);
    let mut doc = empty();
    doc["plots"] = json!((0..32).map(|i|json!({"id":format!("p{i}"),"title":"P","time_window_seconds":1,"axes":{"y_min":null,"y_max":null},"traces":[]})).collect::<Vec<_>>());
    cases.push(doc);
    let mut doc = empty();
    doc["data_bindings"] =
        json!([{"id":"d","selector":"recorder_state","target":{"kind":"recorder"}}]);
    doc["editor_bindings"] = json!([{"id":"e","data_id":"d","purpose":"local_only","candidate_kind":"enum","constraints":{"choices":(0..64).map(|i|json!({"token":format!("token{i}"),"label":"Choice"})).collect::<Vec<_>>()}}]);
    cases.push(doc);
    let mut doc = expanded();
    doc["windows"] = json!([]);
    doc["editor_bindings"] = json!([]);
    doc["components"] = json!([{"id":"leaf","kind":"indicator","label":"Value","data_id":"d"}]);
    for i in 0..128 {
        doc["components"].as_array_mut().unwrap().push(json!({"id":format!("g{i}"),"kind":"group","label":"x".repeat(512),"layout":"row","children":vec!["leaf";64]}));
    }
    doc["plots"][0]["traces"] = json!((0..32).map(|i|json!({"id":format!("trace{i}"),"data_id":"d","display_label":"Value","visible":true,"style":{"color":"blue","width":1},"display_unit":null})).collect::<Vec<_>>());
    cases.push(doc);
    cases
}

pub(crate) fn large() -> Value {
    let mut doc = empty();
    doc["settings"] = json!({"density":"comfortable"});
    doc["data_bindings"] = json!((0..700).map(|i|json!({"id":format!("d{i}"),"selector":"signal_sample","target":{"kind":"signal","instrument":"i","parameter":"p"},"expect_kind":null})).collect::<Vec<_>>());
    let mut remaining = 1_048_576 - serde_json::to_vec_pretty(&doc).unwrap().len();
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

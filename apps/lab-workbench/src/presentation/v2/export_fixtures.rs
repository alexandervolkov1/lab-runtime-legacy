//! Compact admitted v2 inputs whose attempted v1 inverse amplifies bounded data.

use serde_json::{Value, json};

pub(super) fn long_normal_source() -> crate::presentation::PresentationDocument {
    let mut dto = shared_traces(320, true);
    let target = dto["data_bindings"][0]["target"].clone();
    for plot in dto["plots"].as_array_mut().unwrap() {
        for trace in plot["traces"].as_array_mut().unwrap() {
            trace.as_object_mut().unwrap().remove("data_id");
            trace["source"] = target.clone();
        }
    }
    serde_json::from_value(json!({
        "format_version":1,"document_id":"long-export","windows":[],"controls":[],
        "plots":dto["plots"].take()
    }))
    .unwrap()
}

pub(super) fn shared_traces(count: usize, long: bool) -> Value {
    assert!(count <= 1024);
    let text = if long { "x".repeat(512) } else { "x".into() };
    let mut doc = json!({
        "format_version":2,"binding_version":1,"document_id":"export-limits",
        "windows":[],"components":[],"editor_bindings":[],"action_bindings":[],
        "data_bindings":[{"id":"shared","selector":"signal_sample","target":{"kind":"signal","instrument":text,"parameter":text}}],
        "plots":[]
    });
    for start in (0..count).step_by(32) {
        let traces: Vec<_> = (start..count.min(start + 32))
            .map(|i| {
                json!({
                    "id":format!("trace{i}"),"data_id":"shared","display_label":text,
                    "visible":true,"style":{"color":text,"width":1.0},"display_unit":null
                })
            })
            .collect();
        doc["plots"].as_array_mut().unwrap().push(json!({
            "id":format!("plot{start}"),"title":"Plot","time_window_seconds":60.0,
            "axes":{"y_min":null,"y_max":null},"traces":traces
        }));
    }
    doc
}

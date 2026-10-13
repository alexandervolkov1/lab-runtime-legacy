//! Bounded deterministic inputs shared with the native allocation probe.
use crate::{
    model::{LivePoint, RuntimeObservations},
    presentation::{RuntimeRef, v2},
};
use serde_json::{Value, json};

pub(crate) fn syntax(count: usize, shared: bool) -> Value {
    assert!(count <= 32);
    let data: Vec<_> = (0..count).map(|i| json!({"id":format!("d{i}"),"selector":"signal_sample",
        "target":{"kind":"signal","instrument":"instrument","parameter":format!("s{}",if shared { 0 } else { i })}})).collect();
    let traces: Vec<_> = (0..count).map(|i| json!({"id":format!("trace{i}"),"data_id":format!("d{i}"),
        "display_label":"Same label","visible":true,"style":{"color":"cyan","width":1.5},"display_unit":"cosmetic"})).collect();
    json!({"format_version":2,"binding_version":1,"document_id":"plot-document",
        "windows":[{"id":"w","title":"Window","tabs":[{"id":"t","title":"Tab","panels":[{"id":"p","title":"Panel","components":["chart","chart"]}]}]}],
        "components":[{"id":"chart","kind":"plot","label":"Plot component","plot_id":"plot"}],
        "data_bindings":data,"editor_bindings":[],"action_bindings":[],
        "plots":[{"id":"plot","title":"Plot title","time_window_seconds":10000.0,"axes":{"y_min":-10.0,"y_max":10.0},"traces":traces}]})
}
pub(crate) fn admit(value: &Value) -> v2::AdmittedV2Document {
    v2::admit(&serde_json::to_vec(value).unwrap()).unwrap()
}
pub(crate) fn target(index: usize) -> RuntimeRef {
    RuntimeRef::Signal {
        instrument: "instrument".into(),
        parameter: format!("s{index}"),
    }
}
pub(crate) fn observe(obs: &mut RuntimeObservations, index: usize, kind: &str, value: Value) {
    obs.observe_read_descriptor(
        target(index),
        &json!({"value_type":kind,"generation":"1","unit":{"id":"volt","symbol":"V"}}),
    );
    obs.observe(target(index), json!({"signal":{"instrument":"instrument","parameter":format!("s{index}")},
        "generation":"1","quality":"good","status":"available","value":value,"unit":{"id":"volt","symbol":"V"}}), None);
}
pub(crate) fn observations(sources: usize, points: usize) -> RuntimeObservations {
    assert!(sources <= 32 && points <= 4096);
    observations_with_lengths(&[points; 32][..sources])
}
pub(crate) fn observations_with_lengths(lengths: &[usize]) -> RuntimeObservations {
    assert!(lengths.len() <= 32 && lengths.iter().all(|&points| points <= 4096));
    let mut obs = RuntimeObservations::default();
    for (source, &points) in lengths.iter().enumerate() {
        observe(&mut obs, source, "float", json!(1.0));
        for i in 0..points {
            obs.live
                .entry(target(source))
                .or_default()
                .push(LivePoint {
                    time_seconds: i as f64,
                    value: if i == 123 {
                        1000.0
                    } else if i == 234 {
                        -1000.0
                    } else {
                        (i % 7) as f64
                    },
                })
                .unwrap();
        }
    }
    obs.complete_rebuild();
    obs
}

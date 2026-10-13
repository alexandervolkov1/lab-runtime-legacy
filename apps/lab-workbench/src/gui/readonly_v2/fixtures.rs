//! Bounded graph fixtures shared with the isolated native egui allocation probe.

use crate::{
    gui::readonly_v2::{Location, PAGE_SIZE},
    presentation::v2,
};
use serde_json::json;

pub(super) fn maximum_graph() -> v2::AdmittedV2Document {
    graph(64, 64)
}

pub(super) fn repeated_small_graph() -> v2::AdmittedV2Document {
    graph(1, 2)
}

fn graph(panels: usize, roots: usize) -> v2::AdmittedV2Document {
    let doc = json!({
        "format_version":2,"binding_version":1,"document_id":"large",
        "windows":[{"id":"w","title":"Window","tabs":[{"id":"t","title":"Tab","panels":
            (0..panels).map(|p| json!({"id":format!("p{p}"),"title":"Panel","components":vec!["group";roots]})).collect::<Vec<_>>()
        }]}],
        "components":[
            {"id":"group","kind":"group","label":"Group","layout":"row","children":["a","b","a","b"]},
            {"id":"a","kind":"indicator","label":"One","data_id":"d"},
            {"id":"b","kind":"status","label":"Two","source":{"kind":"data","data_id":"d"}}
        ],
        "data_bindings":[{"id":"d","selector":"signal_sample","target":{"kind":"signal","instrument":"i","parameter":"p"}}],
        "editor_bindings":[],"action_bindings":[],"plots":[]
    });
    v2::admit(&serde_json::to_vec(&doc).unwrap()).unwrap()
}

/// Test/probe-only exhaustive navigation, using a depth-four call stack. This is
/// deliberately not a production frame operation and materializes no expansion.
pub(super) fn visit_pages(doc: &v2::AdmittedV2Document, mut visit: impl FnMut(Location)) {
    fn list(
        doc: &v2::AdmittedV2Document,
        location: Location,
        count: usize,
        visit: &mut impl FnMut(Location),
    ) {
        for page in 0..count.div_ceil(PAGE_SIZE).max(1) {
            visit(Location { page, ..location });
            for index in page * PAGE_SIZE..((page + 1) * PAGE_SIZE).min(count) {
                let (root, path) = location.occurrence(index);
                let view = doc
                    .source()
                    .occurrence(
                        location.window,
                        location.tab,
                        location.panel,
                        root,
                        &path[..location.depth],
                    )
                    .unwrap();
                if let v2::Component::Group { children, .. } = view.component {
                    list(doc, location.enter(index), children.0.len(), visit);
                }
            }
        }
    }
    if doc.dto().windows.0.is_empty() {
        visit(Location::default());
    }
    for (window, w) in doc.dto().windows.0.iter().enumerate() {
        if w.tabs.0.is_empty() {
            visit(Location {
                window,
                ..Default::default()
            });
        }
        for (tab, t) in w.tabs.0.iter().enumerate() {
            if t.panels.0.is_empty() {
                visit(Location {
                    window,
                    tab,
                    ..Default::default()
                });
            }
            for (panel, p) in t.panels.0.iter().enumerate() {
                list(
                    doc,
                    Location {
                        window,
                        tab,
                        panel,
                        ..Default::default()
                    },
                    p.components.0.len(),
                    &mut visit,
                );
            }
        }
    }
}

pub(super) fn one_indicator() -> v2::AdmittedV2Document {
    let doc = repeated_small_graph();
    let mut syntax = serde_json::to_value(doc.dto()).unwrap();
    syntax["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["a"]);
    v2::admit(&serde_json::to_vec(&syntax).unwrap()).unwrap()
}

pub(super) fn empty() -> v2::AdmittedV2Document {
    let doc = one_indicator();
    let mut syntax = serde_json::to_value(doc.dto()).unwrap();
    syntax["windows"] = json!([]);
    v2::admit(&serde_json::to_vec(&syntax).unwrap()).unwrap()
}

pub(super) fn nested() -> v2::AdmittedV2Document {
    let doc = repeated_small_graph();
    let mut syntax = serde_json::to_value(doc.dto()).unwrap();
    syntax["components"][0]["children"] = json!(["g2", "g2"]);
    for (id, children, layout) in [
        ("g2", json!(["g3", "g3"]), "column"),
        ("g3", json!(["g4", "g4"]), "row"),
        ("g4", json!(["a", "b", "a", "b"]), "column"),
    ] {
        syntax["components"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"kind":"group","label":id,"layout":layout,"children":children}));
    }
    v2::admit(&serde_json::to_vec(&syntax).unwrap()).unwrap()
}

/// Maximum byte-length labels/help with diverse Unicode and entity projections.
pub(super) fn text_stress() -> (v2::AdmittedV2Document, crate::model::RuntimeObservations) {
    let doc = repeated_small_graph();
    let mut syntax = serde_json::to_value(doc.dto()).unwrap();
    syntax["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["a", "b", "c", "e"]);
    syntax["components"] = json!([]);
    syntax["windows"][0]["title"] = json!("W".repeat(512));
    syntax["windows"][0]["tabs"][0]["title"] = json!("T".repeat(512));
    syntax["windows"][0]["tabs"][0]["panels"][0]["title"] = json!("P".repeat(512));
    let mut observations = crate::model::RuntimeObservations::default();
    for (index, id) in ["a", "b", "c", "e"].into_iter().enumerate() {
        let text = |part: usize| {
            let text: String = (0..512)
                .map(|n| char::from_u32(0x100 + (index * 1536 + part * 512 + n) as u32).unwrap())
                .collect();
            text[..text.floor_char_boundary(512)].to_owned()
        };
        syntax["components"].as_array_mut().unwrap().push(json!({"id":id,"kind":"status","label":text(0),"help":text(1),"source":{"kind":"entity","target":{"kind":"instrument","instrument":id}}}));
        observations.observe(
            crate::presentation::RuntimeRef::Instrument {
                instrument: id.into(),
            },
            json!({"value":text(2).repeat(2)}),
            None,
        );
    }
    (
        v2::admit(&serde_json::to_vec(&syntax).unwrap()).unwrap(),
        observations,
    )
}

pub(super) fn line_stress() -> (v2::AdmittedV2Document, crate::model::RuntimeObservations) {
    let (doc, obs) = text_stress();
    let mut syntax = serde_json::to_value(doc.dto()).unwrap();
    for component in syntax["components"].as_array_mut().unwrap() {
        component["label"] = json!("\n".repeat(512));
        component["help"] = json!("\n".repeat(512));
    }
    syntax["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["g"]);
    syntax["components"].as_array_mut().unwrap().push(json!({"id":"g","kind":"group","label":"H".repeat(512),"help":"h".repeat(512),"layout":"row","children":["a","b","c","e"]}));
    (
        v2::admit(&serde_json::to_vec(&syntax).unwrap()).unwrap(),
        obs,
    )
}

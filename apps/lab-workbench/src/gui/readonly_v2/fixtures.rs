//! Bounded graph fixtures shared with the isolated native egui allocation probe.

use crate::presentation::v2;
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

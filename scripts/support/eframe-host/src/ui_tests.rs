//! CPU egui regressions for complete traversal, navigation and inert controls.
use super::*;
use crate::private_ipc::protocol::{Identity, Kind};
use serde_json::{Value, json};

fn document() -> Value {
    json!({"format_version":2,"binding_version":1,"document_id":"adapter-test",
        "windows":[{"id":"w","title":"Window","tabs":[{"id":"t","title":"Tab",
            "panels":[{"id":"p","title":"Panel","components":["group"]}]}]}],
        "components":[{"id":"group","kind":"group","label":"Group","layout":"column",
            "children":["leaf","leaf","leaf","leaf","leaf","leaf"]},
            {"id":"leaf","kind":"indicator","label":"Value","data_id":"data","precision":2}],
        "data_bindings":[{"id":"data","selector":"signal_sample","target":{"kind":"signal","instrument":"1","parameter":"1"}}],
        "editor_bindings":[],"action_bindings":[],"plots":[]})
}
fn header(revision: u64) -> Header {
    let mut h = Header::control(
        Kind::Document,
        Identity {
            workbench: [1; 16],
            incarnation: 1,
        },
        1,
    );
    h.epoch.revision = revision;
    h
}
fn draw(
    ctx: &egui::Context,
    document: &v2::AdmittedV2Document,
    h: Header,
    state: &mut Navigation,
) -> Report {
    let mut report = Report::default();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 720.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                report = paint(
                    ui,
                    document,
                    h,
                    None,
                    state,
                    &mut crate::controls::Controls::default(),
                )
            });
        },
    );
    // CPU-only traversal does not upload font images. Explicitly discard the
    // standard egui output rather than invoking any renderer.
    output.textures_delta.clear();
    report
}
fn admit(value: &Value) -> v2::AdmittedV2Document {
    v2::admit(&serde_json::to_vec(value).unwrap()).unwrap()
}
#[test]
fn all_sixty_four_roots_and_nested_occurrences_are_displayed() {
    let mut value = document();
    value["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(vec!["group"; 64]);
    let doc = admit(&value);
    let report = draw(
        &egui::Context::default(),
        &doc,
        header(1),
        &mut Navigation::default(),
    );
    assert_eq!(report.scalars.len(), 64 * 6);
    assert!(
        report
            .scalars
            .iter()
            .all(|s| s.freshness.starts_with("Unavailable"))
    );
}
#[test]
fn every_panel_tab_and_window_is_addressable_and_revision_resets_navigation() {
    let mut value = document();
    value["windows"][0]["tabs"][0]["panels"] = json!(
        (0..64)
            .map(
                |i| json!({"id":format!("p{i}"),"title":format!("Panel {i}"),"components":["leaf"]})
            )
            .collect::<Vec<_>>()
    );
    let doc = admit(&value);
    let ctx = egui::Context::default();
    let mut nav = Navigation::default();
    assert_eq!(draw(&ctx, &doc, header(1), &mut nav).scalars.len(), 64);
    nav.panel = Some(63);
    assert_eq!(draw(&ctx, &doc, header(1), &mut nav).panels, ["Panel 63"]);
    assert_eq!(draw(&ctx, &doc, header(2), &mut nav).panels.len(), 64);
    let mut value = document();
    value["windows"] = json!((0..64).map(|i|json!({"id":format!("w{i}"),"title":format!("Window {i}"),"tabs":[{"id":format!("t{i}"),"title":"T","panels":[]}]})).collect::<Vec<_>>());
    let doc = admit(&value);
    nav.window = 63;
    assert_eq!(draw(&ctx, &doc, header(2), &mut nav).heading, "Window 63");
    let mut value = document();
    value["windows"][0]["tabs"] = json!((0..64).map(|i|json!({"id":format!("t{i}"),"title":"T","panels":[{"id":format!("p{i}"),"title":format!("Tab {i}"),"components":["leaf"]}]})).collect::<Vec<_>>());
    let doc = admit(&value);
    nav.window = 0;
    nav.tab = 63;
    assert_eq!(draw(&ctx, &doc, header(2), &mut nav).panels, ["Tab 63"]);
}
#[test]
fn neutral_contract_examples_draw_all_components_and_disable_every_control() {
    let contract = include_str!("../../../../docs/generic-controls-v2-contract.md");
    let section = contract
        .split("## 11. Neutral complete document examples")
        .nth(1)
        .unwrap()
        .split("## 12.")
        .next()
        .unwrap();
    let mut disabled = 0;
    for block in section.split("```json").skip(1) {
        let doc = admit(&serde_json::from_str(block.split("```").next().unwrap()).unwrap());
        let ctx = egui::Context::default();
        let mut nav = Navigation {
            epoch: Some((1, 1)),
            ..Default::default()
        };
        for (wi, window) in doc.dto().windows.0.iter().enumerate() {
            for ti in 0..window.tabs.0.len() {
                nav.window = wi;
                nav.tab = ti;
                let report = draw(&ctx, &doc, header(1), &mut nav);
                for widget in report.widgets.iter().filter(|w| {
                    !w.label.starts_with("Previous ")
                        && !w.label.starts_with("Next ")
                        && w.label != "All panels"
                }) {
                    assert!(!widget.enabled, "{}", widget.label);
                    disabled += 1;
                }
            }
        }
    }
    assert_eq!(disabled, 4);
}
#[test]
fn shared_plot_occurrences_have_independent_stock_memory() {
    let mut value = document();
    value["components"][0]["children"] = json!(["plot", "plot"]);
    value["components"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"plot","kind":"plot","label":"Plot","plot_id":"plot-def"}));
    value["plots"] = json!([{"id":"plot-def","title":"P","time_window_seconds":60,"axes":{"y_min":null,"y_max":null},"traces":[]}]);
    let doc = admit(&value);
    let ctx = egui::Context::default();
    let mut nav = Navigation::default();
    let first = draw(&ctx, &doc, header(1), &mut nav);
    let second = draw(&ctx, &doc, header(1), &mut nav);
    assert_eq!(first.views.len(), 2);
    assert_eq!(second.views.len(), 2);
    assert_ne!(second.views[0].rect, second.views[1].rect);
    assert_ne!(second.views[0].occurrence, second.views[1].occurrence);
    nav.panel = Some(0);
    let focused = draw(&ctx, &doc, header(1), &mut nav);
    assert_eq!(second.views[0].occurrence, focused.views[0].occurrence);
}

#[test]
fn all_editor_widget_kinds_are_inert_without_local_candidate_storage() {
    let mut value = document();
    value["components"][0]["children"] =
        json!(["toggle", "integer", "number", "text", "choice", "button"]);
    for (id, kind, candidate) in [
        ("toggle", "toggle", "boolean"),
        ("integer", "integer_input", "integer"),
        ("number", "number_input", "number"),
        ("text", "text_input", "text"),
        ("choice", "select", "enum"),
    ] {
        value["components"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"kind":kind,"label":id,"editor_id":format!("editor-{id}")}));
        value["editor_bindings"].as_array_mut().unwrap().push(json!({"id":format!("editor-{id}"),"data_id":"data","purpose":"local_only","candidate_kind":candidate,
            "constraints":if candidate=="enum" {json!({"choices":[{"token":"on","label":"On"}]})} else {json!({})}}));
    }
    value["components"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"button","kind":"button","label":"button","action_id":null,"disabled_reason":"Read-only"}));
    let doc = admit(&value);
    let report = draw(
        &egui::Context::default(),
        &doc,
        header(1),
        &mut Navigation::default(),
    );
    let controls: Vec<_> = report
        .widgets
        .iter()
        .filter(|w| {
            ["toggle", "integer", "number", "text", "choice", "button"].contains(&w.label.as_str())
        })
        .collect();
    assert_eq!(controls.len(), 6);
    assert!(controls.iter().all(|w| !w.enabled));
}

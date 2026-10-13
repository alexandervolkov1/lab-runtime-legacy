use super::{
    plot_widget_tests::{EPOCH, click, frame, memory},
    *,
};
use egui_plot::PlotPoint;
use serde_json::json;

fn read_full(preview: &mut Preview<'_>, obs: &RuntimeObservations, expected: &str) {
    let mut bytes = Vec::new();
    let count = expected
        .chars()
        .count()
        .div_ceil(plot_text::SCALARS_PER_PAGE)
        .max(1);
    for page in 0..count {
        if page > 0 {
            click(preview, obs, "Next fragment");
        }
        let (report, output) = frame(preview, obs, vec![]);
        assert!(!report.guarded && report.plot_points == 0);
        let hex = output
            .iter()
            .find_map(|(s, _)| s.strip_prefix("UTF-8: "))
            .unwrap();
        let fragment: Vec<u8> = hex
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        let decoded = std::str::from_utf8(&fragment).unwrap();
        assert!(decoded.chars().count() <= 16 && fragment.len() <= 64);
        // Inspect real escaped text, not only the independent byte annotation.
        let rendered: String = decoded.chars().flat_map(char::escape_debug).collect();
        assert!(rendered.is_empty() || output.iter().any(|(text, _)| text == &rendered));
        assert!(!rendered.contains(['\n', '\r']));
        assert!(output.len() <= 24);
        bytes.extend_from_slice(&fragment);
    }
    assert_eq!(bytes, expected.as_bytes());
    if count > 1 {
        click(preview, obs, "Previous fragment");
        let (_, out) = frame(preview, obs, vec![]);
        assert!(
            out.iter()
                .any(|(text, _)| text.starts_with(&format!("Fragment {} /", count - 1)))
        );
    }
}

#[test]
fn all_full_unicode_labels_title_units_and_32nd_trace_are_accessible_in_actual_output() {
    let (doc, obs) = plot_fixtures::unicode_scene(0);
    let before = obs.clone();
    let canonical = doc.canonical_bytes().to_vec();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    let (report, out) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.plot_points, 16384);
    assert!(out.iter().all(|(text, _)| text.is_ascii()));
    assert!(out.iter().any(|(text, _)| text == "T32"));
    click(&mut preview, &obs, "Title details");
    read_full(&mut preview, &obs, doc.dto().plots.0[0].title.as_str());
    click(&mut preview, &obs, "Back to Plot");
    for i in 0..32 {
        if i > 0 && i % 4 == 0 {
            click(&mut preview, &obs, "Next traces");
        }
        click(&mut preview, &obs, &format!("Details T{:02}", i + 1));
        let trace = &doc.dto().plots.0[0].traces.0[i];
        assert_eq!(trace.display_label.as_str().len(), 512);
        read_full(&mut preview, &obs, trace.display_label.as_str());
        click(&mut preview, &obs, "Cosmetic display unit");
        read_full(
            &mut preview,
            &obs,
            trace.display_unit.as_ref().unwrap().as_str(),
        );
        for (caption, key) in [("Runtime unit ID", "id"), ("Runtime unit symbol", "symbol")] {
            click(&mut preview, &obs, caption);
            read_full(
                &mut preview,
                &obs,
                obs.entities[&plot_fixtures::target(i)].value["unit"][key]
                    .as_str()
                    .unwrap(),
            );
        }
        click(&mut preview, &obs, "Back to Plot");
    }
    click(&mut preview, &obs, "T32");
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 1 << 31);
    assert_eq!(before, obs);
    assert_eq!(canonical, doc.canonical_bytes());
}

#[test]
fn controls_combining_multibyte_and_missing_glyphs_have_lossless_bounded_pages() {
    for repertoire in [1, 2] {
        let (doc, obs) = plot_fixtures::unicode_scene(repertoire);
        let mut preview = Preview::new(&doc, EPOCH);
        preview.open_plot(0);
        click(&mut preview, &obs, "Title details");
        read_full(&mut preview, &obs, doc.dto().plots.0[0].title.as_str());
        click(&mut preview, &obs, "Back to Plot");
        click(&mut preview, &obs, "Details T01");
        read_full(
            &mut preview,
            &obs,
            doc.dto().plots.0[0].traces.0[0].display_label.as_str(),
        );
    }
    // Deliberately straddle the 16-scalar page boundary with 2/3/4-byte code points.
    let mut syntax = plot_fixtures::syntax(1, false);
    syntax["plots"][0]["title"] = json!(format!("{}é汉🦀e\u{301}\n\r\0\\", "x".repeat(15)));
    let doc = plot_fixtures::admit(&syntax);
    let obs = plot_fixtures::observations(1, 2);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    click(&mut preview, &obs, "Title details");
    read_full(&mut preview, &obs, doc.dto().plots.0[0].title.as_str());
}

#[test]
fn text_retirement_preserves_manual_bounds_visibility_and_reset_without_foreign_memory() {
    let (doc, mut obs) = plot_fixtures::unicode_scene(1);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for _ in 0..3 {
        frame(&mut preview, &obs, vec![]);
    }
    click(&mut preview, &obs, "T02");
    let pos = memory(&preview).transform().frame().center();
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(pos), egui::Event::Zoom(2.0)],
    );
    let manual = *memory(&preview).bounds();
    let sentinel = egui::Id::new("unrelated-context-state");
    preview
        .context
        .as_ref()
        .unwrap()
        .data_mut(|data| data.insert_temp(sentinel, 42_u64));
    click(&mut preview, &obs, "Details T01");
    assert!(
        preview
            .context
            .as_ref()
            .unwrap()
            .data(|data| data.get_temp::<u64>(sentinel))
            .is_none()
    );
    let text = doc.dto().plots.0[0].traces.0[0].display_label.as_str();
    read_full(&mut preview, &obs, text);
    assert_eq!(preview.plot.as_ref().unwrap().bounds, Some(manual));
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 2);
    // Dynamic unit text must be reread; a changed fragment retires the Context.
    click(&mut preview, &obs, "Runtime unit symbol");
    let unit = json!({"id":"new-unit","symbol":"new"});
    obs.observe_read_descriptor(
        plot_fixtures::target(0),
        &json!({"value_type":"float","generation":"1","unit":unit}),
    );
    obs.entities
        .get_mut(&plot_fixtures::target(0))
        .unwrap()
        .value["unit"] = unit;
    preview
        .context
        .as_ref()
        .unwrap()
        .data_mut(|data| data.insert_temp(sentinel, 42_u64));
    let (_, out) = frame(&mut preview, &obs, vec![]);
    assert!(out.iter().any(|(text, _)| text == "new"));
    assert!(
        preview
            .context
            .as_ref()
            .unwrap()
            .data(|data| data.get_temp::<u64>(sentinel))
            .is_none()
    );
    click(&mut preview, &obs, "Back to Plot");
    assert_eq!(memory(&preview).bounds(), &manual);
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 2);
    obs.live
        .get_mut(&plot_fixtures::target(0))
        .unwrap()
        .push(crate::model::LivePoint {
            time_seconds: 5000.0,
            value: 3000.0,
        })
        .unwrap();
    frame(&mut preview, &obs, vec![]);
    assert_eq!(memory(&preview).bounds(), &manual);
    click(&mut preview, &obs, "Reset / auto bounds");
    assert!(memory(&preview).auto_bounds.all());
    click(&mut preview, &obs, "Title details");
    preview.open_plot(1);
    assert!(preview.plot.as_ref().unwrap().details.is_none());
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 0);
    click(&mut preview, &obs, "Title details");
    preview.replace(
        &doc,
        PresentationEpoch {
            revision: 2,
            ..EPOCH
        },
    );
    assert!(preview.plot.is_none());
}

#[test]
fn aliases_cannot_be_spoofed_by_labels_and_do_not_join_signal_property_identities() {
    let mut syntax = plot_fixtures::syntax(3, false);
    for trace in syntax["plots"][0]["traces"].as_array_mut().unwrap() {
        trace["display_label"] = json!("T02");
    }
    syntax["data_bindings"][2]["target"] = json!({"kind":"configuration_property","owner":{"kind":"instrument","instrument":"instrument"},"property":"s0"});
    let doc = plot_fixtures::admit(&syntax);
    let mut obs = plot_fixtures::observations(2, 0);
    for i in 0..2 {
        for (time, value) in [
            (0.0, 20.0 * (i + 1) as f64),
            (10.0, 20.0 * (i + 1) as f64 + 10.0),
        ] {
            obs.live
                .entry(plot_fixtures::target(i))
                .or_default()
                .push(crate::model::LivePoint {
                    time_seconds: time,
                    value,
                })
                .unwrap();
        }
    }
    let unit = json!({"id":"kelvin","symbol":"K"});
    obs.observe_read_descriptor(
        plot_fixtures::target(1),
        &json!({"value_type":"float","generation":"1","unit":unit}),
    );
    obs.entities
        .get_mut(&plot_fixtures::target(1))
        .unwrap()
        .value["unit"] = unit;
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for _ in 0..3 {
        frame(&mut preview, &obs, vec![]);
    }
    for (alias, value, unit) in [("T01", 25.0, "V"), ("T02", 45.0, "K")] {
        let pos = memory(&preview)
            .transform()
            .position_from_point(&PlotPoint::new(5.0, value));
        frame(&mut preview, &obs, vec![egui::Event::PointerMoved(pos)]);
        let (_, out) = frame(&mut preview, &obs, vec![]);
        let hover = out
            .iter()
            .find(|(text, _)| text.starts_with(&format!("{alias}\nCurve position:")))
            .unwrap();
        assert!(hover.0.contains(&format!(" {unit}\n")));
    }
    let view = plot_projection::project(
        &doc,
        &obs,
        EPOCH,
        Location::default(),
        0,
        plot_projection::PROJECTION_RESERVATION,
    )
    .unwrap();
    assert!(plot_text::trace(&view, "T2").is_none());
    assert!(plot_text::trace(&view, "Same label").is_none());
    assert!(view.points(2).is_empty());
    assert_ne!(view.traces[0].binding.target, view.traces[2].binding.target);
    assert_ne!(
        plot_text::trace(&view, "T01").unwrap().identity,
        plot_text::trace(&view, "T02").unwrap().identity
    );
}

#[test]
fn full_document_labels_remain_available_when_metadata_is_missing_or_incompatible() {
    let (doc, mut obs) = plot_fixtures::unicode_scene(2);
    obs.entities.clear();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    let (report, _) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.plot_points, 0);
    click(&mut preview, &obs, "Details T01");
    read_full(
        &mut preview,
        &obs,
        doc.dto().plots.0[0].traces.0[0].display_label.as_str(),
    );
    click(&mut preview, &obs, "Cosmetic display unit");
    read_full(
        &mut preview,
        &obs,
        doc.dto().plots.0[0].traces.0[0]
            .display_unit
            .as_ref()
            .unwrap()
            .as_str(),
    );
    click(&mut preview, &obs, "Runtime unit symbol");
    // Missing samples do not erase still-valid descriptor units.
    read_full(&mut preview, &obs, &plot_fixtures::unicode_text(32, 2, 16));
    obs.clear_read_descriptors();
    let (_, out) = frame(&mut preview, &obs, vec![]);
    assert!(
        out.iter()
            .any(|(text, _)| text.starts_with("Not available"))
    );
    plot_fixtures::observe(&mut obs, 0, "float", json!(1.0));
    obs.observe_read_descriptor(
        plot_fixtures::target(0),
        &json!({"value_type":"float","generation":"2","unit":{"id":"z".repeat(33),"symbol":"bad"}}),
    );
    click(&mut preview, &obs, "Back to Plot");
    let (report, _) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.plot_points, 0);
    click(&mut preview, &obs, "Details T01");
    read_full(
        &mut preview,
        &obs,
        doc.dto().plots.0[0].traces.0[0].display_label.as_str(),
    );
}

#[test]
fn unicode_hover_is_ascii_bounded_and_preserves_exact_runtime_unit_without_conversion() {
    let (doc, obs) = plot_fixtures::unicode_scene(1);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for _ in 0..3 {
        frame(&mut preview, &obs, vec![]);
    }
    let pos = memory(&preview)
        .transform()
        .position_from_point(&PlotPoint::new(500.0, 3.0));
    frame(&mut preview, &obs, vec![egui::Event::PointerMoved(pos)]);
    let (_, out) = frame(&mut preview, &obs, vec![]);
    let hover = out
        .iter()
        .find(|(s, _)| s.contains("\nCurve position:"))
        .unwrap();
    let view = plot_projection::project(
        &doc,
        &obs,
        EPOCH,
        Location::default(),
        0,
        plot_projection::PROJECTION_RESERVATION,
    )
    .unwrap();
    let trace = plot_text::trace(&view, hover.0.lines().next().unwrap()).unwrap();
    let unit: String = trace
        .runtime_unit_symbol
        .as_str()
        .chars()
        .flat_map(char::escape_default)
        .collect();
    assert!(hover.0.contains(&format!(" {unit}\n")));
    assert!(hover.0.contains("[full text in Details] (no conversion)"));
    assert!(hover.0.is_ascii() && hover.0.len() <= 2048);
    assert!(out.iter().all(|(s, _)| s.is_ascii()));
}

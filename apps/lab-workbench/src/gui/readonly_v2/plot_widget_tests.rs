use super::*;
use crate::model::{Freshness, LivePoint};
use egui_plot::{PlotMemory, PlotPoint};
use serde_json::json;

pub(super) const EPOCH: PresentationEpoch = PresentationEpoch {
    owner: [7; 16],
    revision: 1,
};

pub(super) fn frame(
    preview: &mut Preview<'_>,
    obs: &RuntimeObservations,
    events: Vec<egui::Event>,
) -> (RenderReport, Vec<(String, egui::Rect)>) {
    let input = egui::RawInput {
        events,
        ..Default::default()
    };
    let (mut output, report) = preview.run(obs, input).unwrap();
    let text = navigation_tests::painted_text(&output);
    assert_eq!(output.platform_output.num_completed_passes, 1);
    let meshes = preview.tessellate(&mut output);
    assert!(!meshes.is_empty());
    drop(meshes);
    output.drop_without_applying_deltas();
    (report, text)
}
pub(super) fn memory(preview: &Preview<'_>) -> PlotMemory {
    PlotMemory::load(
        preview.context.as_ref().unwrap(),
        preview.plot.as_ref().unwrap().id(),
    )
    .unwrap()
}
fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}
pub(super) fn click(preview: &mut Preview<'_>, obs: &RuntimeObservations, caption: &str) {
    let (_, text) = frame(preview, obs, vec![]);
    let pos = text
        .iter()
        .find(|(s, _)| s == caption)
        .unwrap_or_else(|| panic!("missing {caption}: {text:?}"))
        .1
        .center();
    for pressed in [true, false] {
        frame(
            preview,
            obs,
            vec![egui::Event::PointerMoved(pos), button(pos, pressed)],
        );
    }
}
fn has(text: &[(String, egui::Rect)], needle: &str) -> bool {
    text.iter().any(|(text, _)| text.contains(needle))
}

#[test]
fn navigation_output_keeps_its_generating_context_until_consumed() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(2, false));
    let obs = plot_fixtures::observations(2, 512);
    let mut preview = Preview::new(&doc, EPOCH);
    // Both UI-driven Page -> Plot and Plot -> Page used to retire the Context
    // before returning the transition frame, silently losing its CPU meshes.
    for caption in ["Open Plot", "Back", "Open Plot", "Back"] {
        let (_, text) = frame(&mut preview, &obs, vec![]);
        let pos = text.iter().find(|(s, _)| s == caption).unwrap().1.center();
        let (mut output, _) = preview
            .run(
                &obs,
                egui::RawInput {
                    events: vec![egui::Event::PointerMoved(pos), button(pos, true)],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(preview.plot.is_some(), caption == "Open Plot");
        assert!(preview.context.is_some());
        let meshes = preview.tessellate(&mut output);
        assert!(!meshes.is_empty(), "transition output must be consumable");
        assert!(output.shapes.is_empty());
        drop(meshes);
        output.drop_without_applying_deltas();
        let (mut next, _) = preview
            .run(
                &obs,
                egui::RawInput {
                    events: vec![button(pos, false)],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(preview.context.as_ref().unwrap().cumulative_frame_nr(), 1);
        assert!(!preview.tessellate(&mut next).is_empty());
        next.drop_without_applying_deltas();
    }
}

#[test]
fn actual_plot_output_and_tessellation() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(32, false));
    let obs = plot_fixtures::observations(32, 4096);
    let mut preview = Preview::new(
        &doc,
        PresentationEpoch {
            owner: [7; 16],
            revision: 1,
        },
    );
    assert!(preview.open_plot(0));
    for _ in 0..3 {
        let (mut output, report) = preview.run(&obs, egui::RawInput::default()).unwrap();
        assert_eq!(report.plot_points, 16384);
        let text = navigation_tests::painted_text(&output);
        assert!(text.iter().any(|(text, _)| text == "Title details"));
        assert!(text.iter().any(|(text, _)| text == "T32"));
        let meshes = preview.tessellate(&mut output);
        assert!(!meshes.is_empty());
        output.drop_without_applying_deltas();
    }
    click(&mut preview, &obs, "T32");
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 1 << 31);
}

#[test]
fn real_card_open_back_and_forget_reentered_view() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(1, false));
    let obs = plot_fixtures::observations(1, 20);
    let mut preview = Preview::new(&doc, EPOCH);
    let (report, text) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.plot_points, 0);
    assert!(has(&text, "Open Plot"));
    click(&mut preview, &obs, "Open Plot");
    assert!(preview.plot.is_some());
    let (_, text) = frame(&mut preview, &obs, vec![]);
    assert!(has(&text, "T01"));
    click(&mut preview, &obs, "Back");
    assert!(preview.plot.is_none());
    assert_eq!(preview.location(), Location::default());
    click(&mut preview, &obs, "Open Plot");
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 0);
    assert_eq!(preview.plot.as_ref().unwrap().auto, true.into());
}

#[test]
fn legend_pan_wheel_zoom_reset_and_context_recreation_preserve_only_current_slot() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(2, false));
    let mut obs = plot_fixtures::observations(2, 200);
    let mut preview = Preview::new(&doc, EPOCH);
    assert!(preview.open_plot(0));
    for _ in 0..3 {
        frame(&mut preview, &obs, vec![]);
    }
    click(&mut preview, &obs, "T02");
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 2);
    let initial = *memory(&preview).bounds();
    let pos = memory(&preview).transform().frame().center();
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(pos), egui::Event::Zoom(2.0)],
    );
    assert!(memory(&preview).bounds().width() < initial.width());
    let zoomed = *memory(&preview).bounds();
    frame(&mut preview, &obs, vec![button(pos, true)]);
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(pos + egui::vec2(50.0, 25.0))],
    );
    frame(
        &mut preview,
        &obs,
        vec![button(pos + egui::vec2(50.0, 25.0), false)],
    );
    assert_ne!(memory(&preview).bounds(), &zoomed);
    let panned = *memory(&preview).bounds();
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 50.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_ne!(memory(&preview).bounds(), &panned);
    // Let egui's finite wheel smoothing settle before testing exact persistence.
    for _ in 0..30 {
        frame(&mut preview, &obs, vec![]);
    }
    let manual = *memory(&preview).bounds();
    obs.live
        .get_mut(&plot_fixtures::target(0))
        .unwrap()
        .push(LivePoint {
            time_seconds: 400.0,
            value: 4000.0,
        })
        .unwrap();
    frame(&mut preview, &obs, vec![]);
    assert_eq!(memory(&preview).bounds(), &manual);
    for _ in 0..4 {
        preview.retire_context();
        frame(&mut preview, &obs, vec![]);
        assert_eq!(memory(&preview).bounds(), &manual);
        assert_eq!(preview.plot.as_ref().unwrap().hidden, 2);
        assert_eq!(memory(&preview).hidden_items.len(), 1);
    }
    click(&mut preview, &obs, "Reset / auto bounds");
    assert!(memory(&preview).auto_bounds.all());
    assert!(memory(&preview).bounds().max()[1] > 4000.0);
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 2);
    let old_id = preview.plot.as_ref().unwrap().id();
    assert!(preview.open_plot(1));
    assert_ne!(preview.plot.as_ref().unwrap().id(), old_id);
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 0);
    assert!(preview.plot.as_ref().unwrap().bounds.is_none());
    frame(&mut preview, &obs, vec![]);
    for epoch in [
        PresentationEpoch {
            revision: 2,
            ..EPOCH
        },
        PresentationEpoch {
            owner: [8; 16],
            ..EPOCH
        },
    ] {
        preview.replace(&doc, epoch);
        assert!(preview.plot.is_none() && preview.context.is_none());
        preview.open_plot(0);
        assert_ne!(preview.plot.as_ref().unwrap().id(), old_id);
        frame(&mut preview, &obs, vec![]);
    }
    let replacement = plot_fixtures::admit(&plot_fixtures::syntax(2, false));
    preview.replace(&replacement, preview.epoch);
    assert!(preview.plot.is_none());
}

#[test]
fn hover_routes_duplicate_labels_to_exact_trace_with_units_staleness_and_curve_provenance() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(2, false));
    let mut obs = plot_fixtures::observations(2, 0);
    for i in 0..2 {
        for (time, value) in [
            (0.0, 20.0 * (i + 1) as f64),
            (10.0, 20.0 * (i + 1) as f64 + 10.0),
        ] {
            obs.live
                .entry(plot_fixtures::target(i))
                .or_default()
                .push(LivePoint {
                    time_seconds: time,
                    value,
                })
                .unwrap();
        }
    }
    obs.observe_read_descriptor(
        plot_fixtures::target(1),
        &json!({"value_type":"float","generation":"1","unit":{"id":"kelvin","symbol":"K"}}),
    );
    obs.entities
        .get_mut(&plot_fixtures::target(1))
        .unwrap()
        .value["unit"] = json!({"id":"kelvin","symbol":"K"});
    obs.mark_stale();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for _ in 0..3 {
        frame(&mut preview, &obs, vec![]);
    }
    for (i, value) in [(1, 25.0), (2, 45.0)] {
        let pos = memory(&preview)
            .transform()
            .position_from_point(&PlotPoint::new(5.0, value));
        frame(&mut preview, &obs, vec![egui::Event::PointerMoved(pos)]);
        let (_, text) = frame(&mut preview, &obs, vec![]);
        assert!(has(&text, &format!("T{i:02}\nCurve position:")), "{text:?}");
        assert!(has(&text, "T+00:00:05.000"));
        assert!(has(
            &text,
            &format!(
                " {}\nDisplay label: cosmetic (no conversion)",
                if i == 1 { "V" } else { "K" }
            )
        ));
        assert!(has(&text, "Stale"));
        assert!(!has(&text, "Measured point"));
    }
}

#[test]
fn generation_metadata_expectation_hidden_and_rebuild_changes_render_no_old_curve() {
    for case in 0..6 {
        let mut syntax = plot_fixtures::syntax(1, false);
        if case == 3 {
            syntax["data_bindings"][0]["expect_kind"] = json!("integer");
        }
        if case == 4 {
            syntax["plots"][0]["traces"][0]["visible"] = json!(false);
        }
        let doc = plot_fixtures::admit(&syntax);
        let mut obs = plot_fixtures::observations(1, 3);
        if case == 0 {
            let mut sample = obs.entities[&plot_fixtures::target(0)].value.clone();
            sample["generation"] = json!("2");
            obs.observe(plot_fixtures::target(0), sample, None);
        }
        if case == 1 {
            let sample = obs.entities[&plot_fixtures::target(0)].value.clone();
            let live = obs.live.clone();
            obs = RuntimeObservations::default();
            obs.observe(plot_fixtures::target(0), sample, None);
            obs.live = live;
            obs.complete_rebuild();
        }
        if case == 2 {
            *obs.live.get_mut(&plot_fixtures::target(0)).unwrap() = Default::default();
        }
        let mut preview = Preview::new(&doc, EPOCH);
        preview.open_plot(0);
        if case == 5 {
            frame(&mut preview, &obs, vec![]);
            obs.begin_rebuild();
        }
        let (report, text) = frame(&mut preview, &obs, vec![]);
        assert_eq!(report.plot_points, 0);
        assert!(
            has(
                &text,
                [
                    "IncompatibleMetadata",
                    "MissingMetadata",
                    "EmptyWindow",
                    "ExpectKindMismatch",
                    "definition hidden",
                    "Rebuilding"
                ][case]
            ),
            "{text:?}"
        );
    }
}

#[test]
fn projected_spikes_are_real_colored_meshes_and_invalid_hover_has_no_tooltip() {
    let mut syntax = plot_fixtures::syntax(32, true);
    for trace in syntax["plots"][0]["traces"].as_array_mut().unwrap() {
        trace["style"] = json!({"color":"#ff0000","width":2.5});
    }
    let doc = plot_fixtures::admit(&syntax);
    let obs = plot_fixtures::observations(1, 4096);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    frame(&mut preview, &obs, vec![]);
    let (mut output, report) = preview.run(&obs, egui::RawInput::default()).unwrap();
    assert_eq!(report.plot_points, 16384);
    let transform = memory(&preview).transform();
    let peak = transform.position_from_point(&PlotPoint::new(123.0, 1000.0));
    let trough = transform.position_from_point(&PlotPoint::new(234.0, -1000.0));
    let curves: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Mesh(m) = &s.shape {
                (m.vertices.len() == 511 * 4
                    && m.vertices.iter().all(|v| v.color == egui::Color32::RED))
                .then_some(m)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(curves.len(), 32);
    for mesh in curves {
        // Square caps stay within sqrt(2) * half-width of each original spike.
        for spike in [peak, trough] {
            assert!(mesh.vertices.iter().any(|v| v.pos.distance(spike) < 1.8));
        }
    }
    drop(preview.tessellate(&mut output));
    output.drop_without_applying_deltas();
    let (_, text) = frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(egui::pos2(
            f32::NAN,
            f32::INFINITY,
        ))],
    );
    assert!(!has(&text, "Curve position"));
}

#[test]
fn long_text_opens_real_curves_and_budget_refusal_still_precedes_widgets() {
    let mut syntax = plot_fixtures::syntax(32, false);
    for trace in syntax["plots"][0]["traces"].as_array_mut().unwrap() {
        trace["display_label"] = json!("Ж".repeat(256));
    }
    let doc = plot_fixtures::admit(&syntax);
    let obs = plot_fixtures::observations(32, 4096);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    let (report, text) = frame(&mut preview, &obs, vec![]);
    assert!(!report.guarded && report.plot_points == 16384);
    assert!(has(&text, "T32"));
    assert!(!has(&text, "Ж"));
    click(&mut preview, &obs, "Back");
    assert!(preview.plot.is_none());
    preview.open_plot(0);
    preview.set_allowance(PREVIEW_RESERVATION - 1);
    assert!(frame(&mut preview, &obs, vec![]).0.guarded);
    preview.set_allowance(NOTICE_RESERVATION - 1);
    assert!(preview.run(&obs, egui::RawInput::default()).is_err());
}

#[test]
fn empty_missing_unconfirmed_and_incompatible_curves_are_diagnostics_without_fake_paths() {
    for count in [0, 1] {
        let doc = plot_fixtures::admit(&plot_fixtures::syntax(count, false));
        let obs = RuntimeObservations::default();
        let mut preview = Preview::new(&doc, EPOCH);
        preview.open_plot(0);
        let (report, text) = frame(&mut preview, &obs, vec![]);
        assert_eq!(report.plot_points, 0);
        assert!(has(
            &text,
            if count == 0 {
                "Empty Plot"
            } else {
                "MissingObservation"
            }
        ));
    }
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(1, false));
    for (kind, value) in [
        ("boolean", json!(false)),
        ("integer", json!(i64::MAX.to_string())),
        ("text", json!("")),
        ("enum", json!("x")),
    ] {
        let mut obs = plot_fixtures::observations(1, 2);
        plot_fixtures::observe(&mut obs, 0, kind, value);
        let mut preview = Preview::new(&doc, EPOCH);
        preview.open_plot(0);
        let (report, text) = frame(&mut preview, &obs, vec![]);
        assert_eq!(report.plot_points, 0);
        assert!(has(&text, "UnsupportedKind"), "{text:?}");
    }
    for freshness in [
        Freshness::Unknown,
        Freshness::Rebuilding,
        Freshness::Stale,
        Freshness::Fresh,
    ] {
        let mut obs = plot_fixtures::observations(1, 2);
        obs.freshness = freshness;
        let mut preview = Preview::new(&doc, EPOCH);
        preview.open_plot(0);
        let (report, text) = frame(&mut preview, &obs, vec![]);
        assert_eq!(
            report.plot_points,
            if matches!(freshness, Freshness::Fresh | Freshness::Stale) {
                2
            } else {
                0
            }
        );
        assert!(has(&text, &format!("{freshness:?}")));
    }
    let mut syntax = plot_fixtures::syntax(1, false);
    syntax["data_bindings"][0]["target"] = json!({"kind":"instrument","instrument":"instrument"});
    let doc = plot_fixtures::admit(&syntax);
    let obs = plot_fixtures::observations(1, 2);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    let (report, text) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.plot_points, 0);
    assert!(has(&text, "IncompatibleTarget"));
}

#[test]
fn skewed_shared_sources_and_all_occurrences_keep_independent_identities_and_sources_immutable() {
    let mut syntax = plot_fixtures::syntax(32, false);
    syntax["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["group", "group"]);
    syntax["components"].as_array_mut().unwrap().push(json!({"id":"group","kind":"group","label":"Group","layout":"row","children":vec!["chart";32]}));
    let doc = plot_fixtures::admit(&syntax);
    let bytes = doc.canonical_bytes().to_vec();
    let mut lengths = [1; 32];
    lengths[0] = 4096;
    let obs = plot_fixtures::observations_with_lengths(&lengths);
    let before = obs.clone();
    let mut preview = Preview::new(&doc, EPOCH);
    let mut ids = std::collections::HashSet::new();
    for group in 0..2 {
        for index in 0..32 {
            let mut location = Location::default().enter(group);
            location.page = index / PAGE_SIZE;
            assert!(preview.navigate(location));
            assert!(preview.open_plot(index));
            assert!(ids.insert(preview.plot.as_ref().unwrap().id()));
            let (report, text) = frame(&mut preview, &obs, vec![]);
            assert_eq!(report.plot_points, 4127);
            assert!(has(&text, "decimated 0"));
            preview.close_plot();
        }
    }
    assert_eq!(ids.len(), 64);
    assert_eq!(obs, before);
    assert_eq!(doc.canonical_bytes(), bytes);
    let shared = plot_fixtures::admit(&plot_fixtures::syntax(2, true));
    let view = plot_projection::project(
        &shared,
        &obs,
        EPOCH,
        Location::default(),
        0,
        plot_projection::PROJECTION_RESERVATION,
    )
    .unwrap();
    assert_ne!(
        plot_widget::PlotState::trace_id(&view.traces[0]),
        plot_widget::PlotState::trace_id(&view.traces[1])
    );
    assert_eq!(view.traces[0].binding.target, view.traces[1].binding.target);
    let mut preview = Preview::new(&shared, EPOCH);
    preview.open_plot(0);
    let (report, text) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.plot_points, 8192);
    assert!(has(&text, "T01") && has(&text, "T02"));
}

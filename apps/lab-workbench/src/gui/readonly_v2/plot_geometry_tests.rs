//! Positive current-frame geometry qualification, including former failures.
use super::{
    plot_geometry_fixtures::{self as fixtures, Geometry, GeometryStats},
    plot_widget_tests::{EPOCH, click, frame, memory},
    *,
};

fn geometry_frame(
    preview: &mut Preview<'_>,
    obs: &RuntimeObservations,
    events: Vec<egui::Event>,
) -> GeometryStats {
    let (mut output, report) = preview
        .run(
            obs,
            egui::RawInput {
                events,
                ..Default::default()
            },
        )
        .unwrap();
    let stats = GeometryStats::inspect(preview, &mut output, report);
    output.drop_without_applying_deltas();
    stats
}

#[test]
fn legal_geometry_matrix_has_finite_bounded_output_or_explicit_numeric_diagnostics() {
    for case in fixtures::matrix() {
        let (doc, obs) = fixtures::scene(&case, 0);
        let before = obs.clone();
        let bytes = doc.canonical_bytes().to_vec();
        let mut preview = Preview::new(&doc, EPOCH);
        preview.open_plot(0);
        let mut stats = GeometryStats::default();
        for _ in 0..4 {
            stats.max_assign(geometry_frame(&mut preview, &obs, vec![]));
        }
        assert!(stats.points <= 16384 && stats.traces <= 32);
        // Curve structural limits plus a conservative fixture-only UI allowance;
        // these are NOT allocator or full-schema byte guarantees.
        assert!(stats.vertices <= 8 * stats.points + 32768);
        assert!(stats.indices <= 36 * stats.points + 131072);
        assert!(stats.finite(), "{}: {stats:?}", case.name);
        assert!(
            case.accepts_output(stats),
            "{}: incorrect curve/refusal semantics: {stats:?}",
            case.name
        );
        assert_eq!(before, obs);
        assert_eq!(bytes, doc.canonical_bytes());
    }
    assert!(!std::hint::black_box(ACTIVATION_QUALIFIED));
}

#[test]
fn invalid_pointer_wheel_zoom_events_preserve_manual_bounds_and_visibility() {
    let (doc, obs) = fixtures::scene(&fixtures::Case::combined(), 0);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for _ in 0..3 {
        frame(&mut preview, &obs, vec![]);
    }
    click(&mut preview, &obs, "T02");
    let center = memory(&preview).transform().frame().center();
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(center), egui::Event::Zoom(2.0)],
    );
    let bounds = *memory(&preview).bounds();
    let stats = geometry_frame(
        &mut preview,
        &obs,
        vec![
            egui::Event::PointerMoved(egui::pos2(f32::NAN, f32::INFINITY)),
            egui::Event::PointerButton {
                pos: egui::pos2(f32::NEG_INFINITY, 0.0),
                pressed: true,
                button: egui::PointerButton::Primary,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Zoom(f32::NAN),
            egui::Event::Zoom(0.0),
            egui::Event::Zoom(-1.0),
            egui::Event::Zoom(11.0),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(f32::INFINITY, f32::NAN),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(stats.finite());
    assert_eq!(memory(&preview).bounds(), &bounds);
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 2);
    preview.retire_context();
    geometry_frame(&mut preview, &obs, vec![]);
    assert_eq!(memory(&preview).bounds(), &bounds);
    assert_eq!(preview.plot.as_ref().unwrap().hidden, 2);
}

#[test]
fn nonfinite_span_is_not_captured_or_restored_as_a_valid_viewport() {
    let case = fixtures::Case {
        traces: 1,
        source_points: 2,
        geometry: Geometry::HugeY,
        ..fixtures::Case::combined()
    };
    let (doc, obs) = fixtures::scene(&case, 0);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    let stats = geometry_frame(&mut preview, &obs, vec![]);
    assert!(stats.finite());
    assert_eq!(stats.segments, 0);
    assert_eq!(stats.unavailable, 1);
    assert!(plot_bounds::valid(
        preview.plot.as_ref().unwrap().bounds.unwrap()
    ));
    assert!(preview.plot.as_ref().unwrap().auto.all());
    let sentinel = egui::Id::new("old-geometry-context");
    preview
        .context
        .as_ref()
        .unwrap()
        .data_mut(|d| d.insert_temp(sentinel, 42_u64));
    preview.retire_context();
    geometry_frame(&mut preview, &obs, vec![]);
    assert!(
        preview
            .context
            .as_ref()
            .unwrap()
            .data(|d| d.get_temp::<u64>(sentinel))
            .is_none()
    );
    assert!(plot_bounds::valid(
        preview.plot.as_ref().unwrap().bounds.unwrap()
    ));
    assert!(!std::hint::black_box(ACTIVATION_QUALIFIED));
}

#[test]
fn current_frame_coalescing_and_340_zoom_events_remain_finite() {
    let case = fixtures::Case {
        traces: 1,
        source_points: 512,
        ..fixtures::Case::combined()
    };
    let (doc, obs) = fixtures::scene(&case, 0);
    let before = obs.clone();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for _ in 0..3 {
        geometry_frame(&mut preview, &obs, vec![]);
    }
    let center = memory(&preview).transform().frame().center();
    let mut stats = GeometryStats::default();
    for _ in 0..340 {
        stats.max_assign(geometry_frame(
            &mut preview,
            &obs,
            vec![egui::Event::PointerMoved(center), egui::Event::Zoom(0.1)],
        ));
    }
    assert!(stats.finite(), "{stats:?}");
    for _ in 0..340 {
        assert!(
            geometry_frame(
                &mut preview,
                &obs,
                vec![egui::Event::PointerMoved(center), egui::Event::Zoom(10.0)]
            )
            .finite()
        );
    }
    click(&mut preview, &obs, "Reset / auto bounds");
    let reset = geometry_frame(&mut preview, &obs, vec![]);
    assert!(reset.finite() && reset.segments > 0);
    let bounds = preview.plot.as_ref().unwrap().bounds.unwrap();
    assert!(bounds.width().is_finite() && bounds.height().is_finite());
    assert_eq!(before, obs);
    assert!(!std::hint::black_box(ACTIVATION_QUALIFIED));
}

#[test]
fn extreme_trace_is_diagnosed_while_ordinary_line_and_exact_hover_remain_visible() {
    use crate::model::LivePoint;
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(2, false));
    let bytes = doc.canonical_bytes().to_vec();
    let mut obs = plot_fixtures::observations(2, 0);
    for (i, values) in [[-f64::MAX, f64::MAX], [20.0, 30.0]]
        .into_iter()
        .enumerate()
    {
        for (t, y) in [0.0, 10.0].into_iter().zip(values) {
            obs.live
                .entry(plot_fixtures::target(i))
                .or_default()
                .push(LivePoint {
                    time_seconds: t,
                    value: y,
                })
                .unwrap();
        }
    }
    let before = obs.clone();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for _ in 0..3 {
        let stats = geometry_frame(&mut preview, &obs, vec![]);
        assert!(stats.finite());
        assert_eq!(stats.segments, 1);
        assert_eq!(stats.unavailable, 1);
    }
    let (_, text) = frame(&mut preview, &obs, vec![]);
    assert!(
        text.iter()
            .any(|(t, _)| t == "GeometryUnavailable T01: Bounds")
    );
    let pos = memory(&preview)
        .transform()
        .position_from_point(&egui_plot::PlotPoint::new(5.0, 25.0));
    frame(&mut preview, &obs, vec![egui::Event::PointerMoved(pos)]);
    let (_, text) = frame(&mut preview, &obs, vec![]);
    assert!(
        text.iter()
            .any(|(t, _)| t.contains("T02\nCurve position:") && t.contains(" V\n"))
    );
    assert_eq!(obs, before);
    assert_eq!(doc.canonical_bytes(), bytes);
}

#[test]
fn unsafe_restoration_and_extreme_pan_never_reach_axis_geometry() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(1, false));
    let obs = plot_fixtures::observations(1, 20);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    for bounds in [
        egui_plot::PlotBounds::from_min_max([-f64::MAX; 2], [f64::MAX; 2]),
        egui_plot::PlotBounds::from_min_max([0.0; 2], [f64::from_bits(1); 2]),
        egui_plot::PlotBounds::from_min_max([f64::NAN; 2], [1.0; 2]),
    ] {
        let state = preview.plot.as_mut().unwrap();
        state.bounds = Some(bounds);
        state.auto = false.into();
        preview.retire_context();
        let stats = geometry_frame(&mut preview, &obs, vec![]);
        assert!(stats.finite() && stats.segments > 0 && stats.unavailable > 0);
        assert!(plot_bounds::valid(*memory(&preview).bounds()));
    }
    let center = memory(&preview).transform().frame().center();
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(center), egui::Event::Zoom(2.0)],
    );
    let before = *memory(&preview).bounds();
    for pos in [
        egui::pos2(f32::MAX, -f32::MAX),
        egui::pos2(-f32::MAX, f32::MAX),
    ] {
        let stats = geometry_frame(
            &mut preview,
            &obs,
            vec![
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerMoved(pos),
            ],
        );
        assert!(stats.finite());
        assert_eq!(memory(&preview).bounds(), &before);
    }
}

#[test]
fn one_current_frame_zoom_collapses_three_points_without_a_degenerate_join() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(1, false));
    let mut obs = plot_fixtures::observations(1, 0);
    for time_seconds in [0.5, 0.5 + 1e-7, 0.5 + 2e-7] {
        obs.live
            .entry(plot_fixtures::target(0))
            .or_default()
            .push(crate::model::LivePoint {
                time_seconds,
                value: 0.5,
            })
            .unwrap();
    }
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    let state = preview.plot.as_mut().unwrap();
    state.bounds = Some(egui_plot::PlotBounds::from_min_max([0.0; 2], [1.0; 2]));
    state.auto = false.into();
    for _ in 0..3 {
        assert!(geometry_frame(&mut preview, &obs, vec![]).finite());
    }
    let center = memory(&preview).transform().frame().center();
    let before = *memory(&preview).bounds();
    let stats = geometry_frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(center), egui::Event::Zoom(0.1)],
    );
    assert_ne!(memory(&preview).bounds(), &before);
    assert!(stats.finite());
    assert_eq!(stats.segments, 1, "one visible marker, no duplicate joins");
    assert_eq!(stats.points, 3, "B1 is not screen-deduplicated");
}

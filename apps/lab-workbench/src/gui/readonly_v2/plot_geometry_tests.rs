//! Qualification regressions include explicitly named unresolved counterexamples.
//! Passing these tests does not turn BLOCKED research results into qualification.
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
fn legal_geometry_matrix_is_bounded_but_known_nan_cases_block_full_qualification() {
    let mut failures = Vec::new();
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
        if !stats.finite() {
            failures.push(case.name.clone());
        }
        assert_eq!(before, obs);
        assert_eq!(bytes, doc.canonical_bytes());
    }
    assert_eq!(
        failures,
        [
            "Coincident-9e-1",
            "Coincident-1e0",
            "Coincident-3.2e1",
            "numeric-SignedZero",
            "numeric-HugeY",
            "numeric-HugeX"
        ]
    );
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
    assert!(
        !stats.finite(),
        "unresolved screen-coordinate counterexample"
    );
    assert!(preview.plot.as_ref().unwrap().bounds.is_none());
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
    assert!(preview.plot.as_ref().unwrap().bounds.is_none());
    assert!(!std::hint::black_box(ACTIVATION_QUALIFIED));
}

#[test]
fn admissible_zoom_out_can_coalesce_distinct_points_into_nonfinite_mesh_geometry() {
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
    for _ in 0..40 {
        stats.max_assign(geometry_frame(
            &mut preview,
            &obs,
            vec![egui::Event::PointerMoved(center), egui::Event::Zoom(0.1)],
        ));
    }
    assert!(
        stats.bad_mesh_positions > 0,
        "known f32 coalescing counterexample"
    );
    let bounds = preview.plot.as_ref().unwrap().bounds.unwrap();
    assert!(bounds.width().is_finite() && bounds.height().is_finite());
    assert_eq!(before, obs);
    assert!(!std::hint::black_box(ACTIVATION_QUALIFIED));
}

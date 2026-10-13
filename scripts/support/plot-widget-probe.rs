//! Real production lifecycle plus an explicitly UNQUALIFIED text counterexample.
use super::{ZERO, epoch, meter, readonly_v2};
use crate::{model::RuntimeObservations, plot_fixtures as fixtures};
use eframe::egui;

fn charge(r: meter::Reading) -> usize {
    r.overlap_peak
        .checked_add(127_usize.checked_mul(r.peak_blocks).unwrap())
        .unwrap()
        .checked_add(65536)
        .unwrap()
}
fn bound(r: meter::Reading) {
    assert!(charge(r) <= readonly_v2::PREVIEW_RESERVATION, "{r:?}");
}
fn frame(
    preview: &mut readonly_v2::Preview<'_>,
    obs: &RuntimeObservations,
    events: Vec<egui::Event>,
) -> (readonly_v2::RenderReport, usize, usize, Option<egui::Pos2>) {
    let (mut output, report) = preview
        .run(
            obs,
            egui::RawInput {
                events,
                ..Default::default()
            },
        )
        .unwrap();
    let legend = output.shapes.iter().find_map(|shape| {
        if let egui::Shape::Text(text) = &shape.shape {
            (text.galley.text() == "2: Same label").then(|| text.visual_bounding_rect().center())
        } else {
            None
        }
    });
    let shapes = output.shapes.len();
    let meshes = preview.tessellate(&mut output);
    let vertices = meshes
        .iter()
        .map(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
            _ => 0,
        })
        .sum::<usize>();
    drop(meshes);
    output.drop_without_applying_deltas();
    (report, shapes, vertices, legend)
}
fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

pub(super) fn run() {
    let doc = fixtures::admit(&fixtures::syntax(32, false));
    let small = fixtures::admit(&fixtures::syntax(1, false));
    let obs = fixtures::observations(32, 4096);
    let updated = fixtures::observations(32, 3072);
    let before = obs.clone();
    let mut lengths = [1; 32];
    lengths[0] = 4096;
    let skewed = fixtures::observations_with_lengths(&lengths);
    let mut text_syntax = fixtures::syntax(32, false);
    let mut text: String = (0..84)
        .map(|i| char::from_u32(0x100 + i).unwrap())
        .collect();
    text.push('x');
    text_syntax["plots"][0]["title"] = serde_json::json!(text);
    let text_doc = fixtures::admit(&text_syntax);
    let mut width_syntax = fixtures::syntax(32, false);
    for trace in width_syntax["plots"][0]["traces"].as_array_mut().unwrap() {
        trace["style"]["width"] = serde_json::json!(32.0);
    }
    let wide_doc = fixtures::admit(&width_syntax);
    let empty = crate::fixtures::empty();
    egui::Context::default()
        .run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();
    let mut phases = [ZERO; 10];
    let mut epochs = [ZERO; 21];
    let mut drops = [ZERO; 3];
    meter::start();
    let mut preview = readonly_v2::Preview::new(&small, epoch(0));
    preview.open_plot(0);
    for _ in 0..4 {
        frame(&mut preview, &obs, vec![]);
    }
    phases[0] = meter::phase();
    preview.replace(&doc, epoch(1));
    preview.open_plot(0);
    let mut max_shapes = 0;
    let mut max_vertices = 0;
    let mut legend = None;
    for _ in 0..8 {
        let (report, shapes, vertices, pos) = frame(&mut preview, &obs, vec![]);
        assert_eq!(report.plot_points, 16384);
        max_shapes = max_shapes.max(shapes);
        max_vertices = max_vertices.max(vertices);
        legend = pos;
    }
    phases[1] = meter::phase();
    for i in 0..24 {
        frame(
            &mut preview,
            if i % 2 == 0 { &updated } else { &obs },
            vec![],
        );
    }
    phases[2] = meter::phase();
    let pos = egui::pos2(600.0, 300.0);
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(pos), egui::Event::Zoom(2.0)],
    );
    frame(&mut preview, &obs, vec![pointer(pos, true)]);
    frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(pos + egui::vec2(40.0, 15.0))],
    );
    frame(
        &mut preview,
        &obs,
        vec![pointer(pos + egui::vec2(40.0, 15.0), false)],
    );
    for pressed in [true, false] {
        let pos = legend.unwrap();
        frame(
            &mut preview,
            &obs,
            vec![egui::Event::PointerMoved(pos), pointer(pos, pressed)],
        );
    }
    for _ in 0..4 {
        frame(&mut preview, &obs, vec![]);
    }
    phases[3] = meter::phase();
    for _ in 0..21 {
        preview.retire_context();
        frame(&mut preview, &obs, vec![]);
    }
    phases[4] = meter::phase();
    preview.close_plot();
    frame(&mut preview, &obs, vec![]);
    for index in [0, 1, 0, 1] {
        preview.open_plot(index);
        frame(&mut preview, &obs, vec![]);
        preview.close_plot();
        frame(&mut preview, &obs, vec![]);
    }
    phases[5] = meter::phase();
    preview.open_plot(0);
    let (report, _, _, _) = frame(&mut preview, &skewed, vec![]);
    assert_eq!(report.plot_points, 4127);
    phases[6] = meter::phase();
    preview.replace(&text_doc, epoch(2));
    preview.open_plot(0);
    for _ in 0..4 {
        let (report, _, _, _) = frame(&mut preview, &obs, vec![]);
        assert!(!report.guarded);
    }
    phases[7] = meter::phase();
    preview.replace(&wide_doc, epoch(3));
    preview.open_plot(0);
    for _ in 0..4 {
        frame(&mut preview, &obs, vec![]);
    }
    phases[9] = meter::phase();
    for (i, reading) in epochs.iter_mut().enumerate() {
        preview.replace(&doc, epoch(10 + i as u64));
        preview.open_plot(i % 2);
        for _ in 0..3 {
            frame(&mut preview, &obs, vec![]);
        }
        *reading = meter::phase();
    }
    preview.replace(&empty, epoch(40));
    frame(&mut preview, &obs, vec![]);
    phases[8] = meter::phase();
    drop(preview);
    drops[0] = meter::phase();
    for reading in &mut drops[1..] {
        let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
        preview.open_plot(0);
        for _ in 0..3 {
            frame(&mut preview, &obs, vec![]);
        }
        drop(preview);
        *reading = meter::phase();
    }
    meter::stop();
    for (name, r) in [
        "one",
        "32/max",
        "updates",
        "interactions",
        "context_recreation",
        "plot_page_plot",
        "skewed",
        "text_boundary",
        "max_to_empty",
        "width_boundary",
    ]
    .iter()
    .zip(phases)
    {
        println!("{name}: {r:?}; accounted={}", charge(r));
        bound(r);
    }
    for (i, r) in epochs.iter().enumerate() {
        println!("epoch {i}: {r:?}; accounted={}", charge(*r));
        bound(*r);
    }
    for r in drops {
        println!("drop: {r:?}; accounted={}", charge(r));
        assert_eq!(r.live, 0);
        bound(r);
    }
    println!("max shapes={max_shapes}, vertices={max_vertices}");
    assert_eq!(before, obs);
    counterexample();
}

fn counterexample() {
    let mut syntax = fixtures::syntax(32, false);
    for (i, trace) in syntax["plots"][0]["traces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        let text: String = (0..256)
            .map(|n| char::from_u32(0x100 + (i * 256 + n) as u32).unwrap())
            .collect();
        trace["display_label"] = serde_json::json!(text[..text.floor_char_boundary(512)]);
    }
    let doc = fixtures::admit(&syntax);
    let obs = fixtures::observations(32, 4096);
    meter::start();
    let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
    preview.open_plot(0);
    for _ in 0..4 {
        let (report, _, _, _) = frame(&mut preview, &obs, vec![]);
        assert!(report.guarded && report.plot_points == 0);
    }
    let guarded = meter::phase();
    drop(preview);
    let dropped = meter::phase();
    meter::stop();
    println!("text_guard: {guarded:?}; accounted={}", charge(guarded));
    bound(guarded);
    assert_eq!(dropped.live, 0);

    // Diagnostic ONLY: identical low-level widget, explicitly bypassing Preview's
    // preflight. This is an over-budget reproduction, NOT qualification evidence.
    meter::start();
    let context = egui::Context::default();
    context.options_mut(|o| {
        o.max_passes = std::num::NonZeroUsize::new(1).unwrap();
        o.zoom_with_keyboard = false;
    });
    let initial = readonly_v2::plot_projection::project(
        &doc,
        &obs,
        epoch(0),
        readonly_v2::Location::default(),
        0,
        512 * 1024,
    )
    .unwrap();
    let mut state = readonly_v2::plot_widget::PlotState::from_definition(
        initial.identity.clone(),
        initial.definition,
        0,
    );
    drop(initial);
    for i in 0..8 {
        let view = readonly_v2::plot_projection::project(
            &doc,
            &obs,
            epoch(0),
            readonly_v2::Location::default(),
            0,
            512 * 1024,
        )
        .unwrap();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 960.0),
                )),
                ..Default::default()
            },
            |ui| {
                state.paint(ui, &view, i == 0);
            },
        );
        drop(view);
        drop(context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point));
        output.drop_without_applying_deltas();
    }
    let unqualified = meter::phase();
    drop(context);
    let dropped = meter::phase();
    meter::stop();
    println!(
        "UNQUALIFIED full-schema counterexample: {unqualified:?}; accounted={}",
        charge(unqualified)
    );
    assert!(charge(unqualified) > readonly_v2::PREVIEW_RESERVATION);
    assert_eq!(dropped.live, 0);
}

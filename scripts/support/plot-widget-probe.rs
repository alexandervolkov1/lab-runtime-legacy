//! Production graph/details lifetimes, including the former B2 counterexample.
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
    frame_at(preview, obs, events, "T02")
}
fn frame_at(
    preview: &mut readonly_v2::Preview<'_>,
    obs: &RuntimeObservations,
    events: Vec<egui::Event>,
    caption: &str,
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
            (text.galley.text() == caption).then(|| text.visual_bounding_rect().center())
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

fn click(preview: &mut readonly_v2::Preview<'_>, obs: &RuntimeObservations, caption: &str) {
    let pos = frame_at(preview, obs, vec![], caption)
        .3
        .expect("actual visible navigation button");
    for pressed in [true, false] {
        frame(
            preview,
            obs,
            vec![egui::Event::PointerMoved(pos), pointer(pos, pressed)],
        );
    }
}

fn read_pages(preview: &mut readonly_v2::Preview<'_>, obs: &RuntimeObservations, bytes: &str) {
    let mut offset = 0;
    for page in 0..bytes.chars().count().div_ceil(16).max(1) {
        if page > 0 {
            click(preview, obs, "Next fragment");
        }
        let (mut output, report) = preview.run(obs, egui::RawInput::default()).unwrap();
        assert!(!report.guarded && report.plot_points == 0);
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) => t.galley.text().strip_prefix("UTF-8: "),
                _ => None,
            })
            .expect("actual full-text page output");
        for token in text.split_whitespace() {
            assert_eq!(
                u8::from_str_radix(token, 16).unwrap(),
                bytes.as_bytes()[offset]
            );
            offset += 1;
        }
        drop(preview.tessellate(&mut output));
        output.drop_without_applying_deltas();
    }
    assert_eq!(offset, bytes.len());
}

fn hover_curve(preview: &mut readonly_v2::Preview<'_>, obs: &RuntimeObservations) {
    let (mut output, _) = preview.run(obs, egui::RawInput::default()).unwrap();
    let interior = egui::Rect::from_min_max(egui::pos2(300.0, 150.0), egui::pos2(1000.0, 500.0));
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.vertices.len() >= 256 => mesh
                .vertices
                .chunks_exact(4)
                .map(|quad| (quad[0].pos + quad[2].pos.to_vec2()) / 2.0)
                .find(|p| interior.contains(*p) && shape.clip_rect.contains(*p)),
            _ => None,
        })
        .expect("actual visible curve geometry");
    drop(preview.tessellate(&mut output));
    output.drop_without_applying_deltas();
    frame(preview, obs, vec![egui::Event::PointerMoved(pos)]);
    let (mut output, _) = preview.run(obs, egui::RawInput::default()).unwrap();
    assert!(
        output.shapes.iter().any(|shape| match &shape.shape {
            egui::Shape::Text(t) => {
                let text = t.galley.text();
                text.is_ascii()
                    && text.contains("\nCurve position:")
                    && text.contains("no conversion")
            }
            _ => false,
        }),
        "real bounded Unicode-unit hover output"
    );
    drop(preview.tessellate(&mut output));
    output.drop_without_applying_deltas();
}

fn unicode_case(repertoire: usize) {
    let (doc, obs) = fixtures::unicode_scene(repertoire);
    let mut updated = obs.clone();
    for buffer in updated.live.values_mut() {
        buffer
            .push(crate::model::LivePoint {
                time_seconds: 4096.0,
                value: -500.0,
            })
            .unwrap();
    }
    let before = obs.clone();
    let canonical = doc.canonical_bytes().to_vec();
    let mut epochs = [ZERO; 21];
    meter::start();
    let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
    preview.open_plot(0);
    for _ in 0..4 {
        let report = frame(&mut preview, &obs, vec![]).0;
        assert!(!report.guarded && report.plot_points == 16384);
    }
    let graph = meter::phase();
    click(&mut preview, &obs, "Title details");
    read_pages(&mut preview, &obs, doc.dto().plots.0[0].title.as_str());
    click(&mut preview, &obs, "Back to Plot");
    let title = meter::phase();
    for i in 0..32 {
        if i > 0 && i % 4 == 0 {
            click(&mut preview, &obs, "Next traces");
        }
        click(&mut preview, &obs, &format!("Details T{:02}", i + 1));
        read_pages(
            &mut preview,
            &obs,
            doc.dto().plots.0[0].traces.0[i].display_label.as_str(),
        );
        click(&mut preview, &obs, "Cosmetic display unit");
        read_pages(
            &mut preview,
            &obs,
            doc.dto().plots.0[0].traces.0[i]
                .display_unit
                .as_ref()
                .unwrap()
                .as_str(),
        );
        for (caption, field) in [("Runtime unit ID", "id"), ("Runtime unit symbol", "symbol")] {
            click(&mut preview, &obs, caption);
            let unit = obs.entities[&fixtures::target(i)].value["unit"][field]
                .as_str()
                .unwrap();
            read_pages(&mut preview, &obs, unit);
        }
        click(&mut preview, &obs, "Back to Plot");
    }
    let details = meter::phase();
    hover_curve(&mut preview, &obs);
    for i in 0..24 {
        frame(
            &mut preview,
            if i % 2 == 0 { &updated } else { &obs },
            vec![],
        );
    }
    let updates = meter::phase();
    for (i, reading) in epochs.iter_mut().enumerate() {
        preview.replace(&doc, epoch(100 + i as u64));
        preview.open_plot(i % 2);
        click(&mut preview, &obs, "Title details");
        click(&mut preview, &obs, "Next fragment");
        click(&mut preview, &obs, "Back to Plot");
        hover_curve(&mut preview, &obs);
        frame(&mut preview, &obs, vec![egui::Event::PointerGone]);
        for _ in 0..3 {
            frame(&mut preview, &obs, vec![]);
        }
        *reading = meter::phase();
    }
    drop(preview);
    let dropped = meter::phase();
    meter::stop();
    for (label, r) in [
        ("graph", graph),
        ("title", title),
        ("all-details", details),
        ("hover/updates", updates),
        ("drop", dropped),
    ] {
        println!(
            "Unicode repertoire {repertoire} {label}: {r:?}; accounted={}",
            charge(r)
        );
        bound(r);
    }
    for (i, r) in epochs.iter().enumerate() {
        println!(
            "Unicode repertoire {repertoire} epoch {i}: {r:?}; accounted={}",
            charge(*r)
        );
        assert_eq!(r.live, epochs[0].live);
        bound(*r);
    }
    assert_eq!(dropped.live, 0);
    assert_eq!(before, obs);
    assert_eq!(doc.canonical_bytes(), canonical);
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
    let mut turns = fixtures::observations(32, 0);
    for source in 0..32 {
        for i in 0..4096 {
            turns
                .live
                .entry(fixtures::target(source))
                .or_default()
                .push(crate::model::LivePoint {
                    time_seconds: i as f64,
                    value: (if i % 2 == 0 { 100.0 } else { -100.0 }) + source as f64 / 32.0,
                })
                .unwrap();
        }
    }
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
    let mut phases = [ZERO; 11];
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
    preview.replace(&doc, epoch(4));
    preview.open_plot(0);
    for _ in 0..4 {
        let report = frame(&mut preview, &turns, vec![]).0;
        assert_eq!(report.plot_points, 16384);
    }
    phases[10] = meter::phase();
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
        "independent_turns",
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
    for repertoire in 0..3 {
        unicode_case(repertoire);
    }
}

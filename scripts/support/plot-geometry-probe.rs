//! Research probe: BLOCKED is an explicit result, never numerical qualification.
use super::{epoch, meter, readonly_v2};
use crate::{
    model::RuntimeObservations,
    plot_geometry_fixtures::{self as fixtures, GeometryStats},
};
use eframe::egui;

fn charge(r: meter::Reading) -> usize {
    r.overlap_peak
        .checked_add(127_usize.checked_mul(r.peak_blocks).unwrap())
        .unwrap()
        .checked_add(65536)
        .unwrap()
}
fn frame(
    preview: &mut readonly_v2::Preview<'_>,
    obs: &RuntimeObservations,
    events: Vec<egui::Event>,
) -> GeometryStats {
    painted(preview, obs, events, None).0
}
fn painted(
    preview: &mut readonly_v2::Preview<'_>,
    obs: &RuntimeObservations,
    events: Vec<egui::Event>,
    caption: Option<&str>,
) -> (GeometryStats, Option<egui::Pos2>) {
    // Research harness only: record a library panic as BLOCKED and continue the
    // finite matrix. No production checks or panic handling are changed.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (mut output, report) = preview
            .run(
                obs,
                egui::RawInput {
                    events,
                    ..Default::default()
                },
            )
            .unwrap();
        let target = output
            .shapes
            .iter()
            .find_map(|shape| match (&shape.shape, caption) {
                (egui::Shape::Text(t), Some(caption)) if t.galley.text() == caption => {
                    Some(t.visual_bounding_rect().center())
                }
                (egui::Shape::Path(p), None) if p.points.len() > 2 => p
                    .points
                    .iter()
                    .find(|pos| {
                        pos.x > 300.0
                            && pos.x < 900.0
                            && pos.y > 150.0
                            && pos.y < 500.0
                            && shape.clip_rect.contains(**pos)
                    })
                    .copied(),
                _ => None,
            });
        let stats = GeometryStats::inspect(preview, &mut output, report);
        output.drop_without_applying_deltas();
        (stats, target)
    }));
    match result {
        Ok(frame) => frame,
        Err(payload) => {
            drop(payload);
            preview.retire_context();
            (
                GeometryStats {
                    panics: 1,
                    ..Default::default()
                },
                None,
            )
        }
    }
}
fn click(
    preview: &mut readonly_v2::Preview<'_>,
    obs: &RuntimeObservations,
    caption: &str,
    stats: &mut GeometryStats,
) {
    let (s, pos) = painted(preview, obs, vec![], Some(caption));
    stats.max_assign(s);
    let pos = pos.expect("actual navigation/legend caption");
    for pressed in [true, false] {
        stats.max_assign(frame(
            preview,
            obs,
            vec![egui::Event::PointerMoved(pos), button(pos, pressed)],
        ));
    }
}
fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}
fn record(
    name: &str,
    width: f64,
    s: GeometryStats,
    r: meter::Reading,
    dropped: meter::Reading,
) -> bool {
    let pass = charge(r) <= readonly_v2::PREVIEW_RESERVATION && s.finite() && dropped.live == 0;
    println!(
        "{name},{},{},{width:e},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        s.points,
        s.traces,
        s.shapes,
        s.vertices,
        s.indices,
        r.live,
        r.peak,
        r.overlap_peak,
        r.peak_blocks,
        charge(r),
        s.bad_shape_positions,
        s.bad_mesh_positions,
        s.panics,
        dropped.live,
        if pass { "PASS" } else { "BLOCKED" }
    );
    !pass
}

fn read_pages(
    preview: &mut readonly_v2::Preview<'_>,
    obs: &RuntimeObservations,
    text: &str,
    stats: &mut GeometryStats,
) {
    let mut offset = 0;
    for i in 0..text.chars().count().div_ceil(16).max(1) {
        if i > 0 {
            click(preview, obs, "Next fragment", stats);
        }
        let (mut out, report) = preview.run(obs, egui::RawInput::default()).unwrap();
        let hex = out
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) => t.galley.text().strip_prefix("UTF-8: "),
                _ => None,
            })
            .unwrap();
        for byte in hex.split_whitespace() {
            assert_eq!(
                u8::from_str_radix(byte, 16).unwrap(),
                text.as_bytes()[offset]
            );
            offset += 1;
        }
        stats.max_assign(GeometryStats::inspect(preview, &mut out, report));
        out.drop_without_applying_deltas();
    }
    assert_eq!(offset, text.len());
}

fn interactions() -> usize {
    let case = fixtures::Case::combined();
    let (doc, obs) = fixtures::scene(&case, 0);
    let (_, updated) = fixtures::scene(&case, 1);
    let before = obs.clone();
    let before_updated = updated.clone();
    let bytes = doc.canonical_bytes().to_vec();
    let mut rows = [("", GeometryStats::default(), super::ZERO); 16];
    let mut epochs = [(GeometryStats::default(), super::ZERO); 21];
    let mut lifetimes = [(GeometryStats::default(), super::ZERO, super::ZERO); 3];
    meter::start();
    let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
    preview.open_plot(0);
    let mut stats = GeometryStats::default();
    for _ in 0..4 {
        stats.max_assign(frame(&mut preview, &obs, vec![]));
    }
    rows[0] = ("combined-initial", stats, meter::phase());
    stats = GeometryStats::default();
    // Both actual legend hover and curve hover go through pointer input.
    for i in [0, 15, 31] {
        let (s, pos) = painted(&mut preview, &obs, vec![], Some(&format!("T{:02}", i + 1)));
        stats.max_assign(s);
        stats.max_assign(frame(
            &mut preview,
            &obs,
            vec![egui::Event::PointerMoved(pos.unwrap())],
        ));
    }
    let (s, pos) = painted(&mut preview, &obs, vec![egui::Event::PointerGone], None);
    stats.max_assign(s);
    if let Some(pos) = pos {
        stats.max_assign(frame(
            &mut preview,
            &obs,
            vec![egui::Event::PointerMoved(pos)],
        ));
    }
    rows[1] = ("combined-hover", stats, meter::phase());
    for (row, name) in [(2, "hide-all"), (3, "show-all")] {
        stats = GeometryStats::default();
        for i in 0..32 {
            click(&mut preview, &obs, &format!("T{:02}", i + 1), &mut stats);
        }
        rows[row] = (name, stats, meter::phase());
    }
    let center = egui::pos2(600.0, 300.0);
    // Cross the f64 exponent range, not just the earlier f32-coalescing point.
    // PlotTransform may refuse a bound update; those frames still exercise the
    // actual saturated viewport and all its geometry/axis allocations.
    for (row, name, factor) in [(4, "zoom-out-340", 0.1), (5, "zoom-in-340", 10.0)] {
        stats = GeometryStats::default();
        click(&mut preview, &obs, "Reset / auto bounds", &mut stats);
        for _ in 0..340 {
            stats.max_assign(frame(
                &mut preview,
                &obs,
                vec![egui::Event::PointerMoved(center), egui::Event::Zoom(factor)],
            ));
        }
        rows[row] = (name, stats, meter::phase());
    }
    stats = GeometryStats::default();
    click(&mut preview, &obs, "Reset / auto bounds", &mut stats);
    for sign in [-1.0, 1.0] {
        stats.max_assign(frame(
            &mut preview,
            &obs,
            vec![egui::Event::PointerMoved(center), button(center, true)],
        ));
        for i in 1..=16 {
            stats.max_assign(frame(
                &mut preview,
                &obs,
                vec![egui::Event::PointerMoved(
                    center + egui::vec2(sign * i as f32 * 100000.0, sign * 50000.0),
                )],
            ));
        }
        stats.max_assign(frame(&mut preview, &obs, vec![button(center, false)]));
    }
    rows[6] = ("far-pan-both-directions", stats, meter::phase());
    stats = GeometryStats::default();
    for i in 0..32 {
        click(&mut preview, &obs, "Reset / auto bounds", &mut stats);
        stats.max_assign(frame(
            &mut preview,
            &obs,
            vec![
                egui::Event::PointerMoved(center),
                egui::Event::Zoom(if i % 2 == 0 { 10.0 } else { 0.1 }),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(800.0, -600.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        ));
    }
    rows[7] = ("zoom-pan-reset-alternation", stats, meter::phase());
    stats = GeometryStats::default();
    click(&mut preview, &obs, "Reset / auto bounds", &mut stats);
    stats.max_assign(frame(
        &mut preview,
        &obs,
        vec![egui::Event::PointerMoved(center), egui::Event::Zoom(2.0)],
    ));
    for i in 0..24 {
        stats.max_assign(frame(
            &mut preview,
            if i % 2 == 0 { &updated } else { &obs },
            vec![],
        ));
    }
    rows[8] = ("manual-updates-and-unit-changes", stats, meter::phase());
    stats = GeometryStats::default();
    click(&mut preview, &obs, "Reset / auto bounds", &mut stats);
    click(&mut preview, &obs, "Title details", &mut stats);
    read_pages(
        &mut preview,
        &obs,
        doc.dto().plots.0[0].title.as_str(),
        &mut stats,
    );
    click(&mut preview, &obs, "Back to Plot", &mut stats);
    for i in 0..32 {
        if i > 0 && i % 4 == 0 {
            click(&mut preview, &obs, "Next traces", &mut stats);
        }
        click(
            &mut preview,
            &obs,
            &format!("Details T{:02}", i + 1),
            &mut stats,
        );
        let trace = &doc.dto().plots.0[0].traces.0[i];
        read_pages(&mut preview, &obs, trace.display_label.as_str(), &mut stats);
        click(&mut preview, &obs, "Cosmetic display unit", &mut stats);
        read_pages(
            &mut preview,
            &obs,
            trace.display_unit.as_ref().unwrap().as_str(),
            &mut stats,
        );
        for (caption, key) in [("Runtime unit ID", "id"), ("Runtime unit symbol", "symbol")] {
            click(&mut preview, &obs, caption, &mut stats);
            read_pages(
                &mut preview,
                &obs,
                obs.entities[&crate::plot_fixtures::target(i)].value["unit"][key]
                    .as_str()
                    .unwrap(),
                &mut stats,
            );
        }
        click(&mut preview, &obs, "Back to Plot", &mut stats);
    }
    rows[9] = ("combined-all-Unicode-fields", stats, meter::phase());
    stats = GeometryStats::default();
    for _ in 0..21 {
        preview.retire_context();
        stats.max_assign(frame(&mut preview, &obs, vec![]));
    }
    rows[10] = ("21-Context-recreations", stats, meter::phase());
    stats = GeometryStats::default();
    for i in [1, 0, 1, 0] {
        preview.open_plot(i);
        stats.max_assign(frame(&mut preview, &obs, vec![]));
    }
    rows[11] = ("occurrence-changes", stats, meter::phase());
    stats = GeometryStats::default();
    for _ in 0..3 {
        preview.close_plot();
        stats.max_assign(frame(&mut preview, &obs, vec![]));
        preview.open_plot(0);
        stats.max_assign(frame(&mut preview, &obs, vec![]));
    }
    rows[12] = ("plot-page-plot", stats, meter::phase());
    stats = GeometryStats::default();
    stats.max_assign(frame(
        &mut preview,
        &obs,
        vec![
            egui::Event::PointerMoved(egui::pos2(f32::NAN, f32::INFINITY)),
            egui::Event::Zoom(f32::NAN),
            egui::Event::Zoom(0.0),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(f32::INFINITY, f32::NAN),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    ));
    rows[13] = ("invalid-events", stats, meter::phase());
    for (i, (s, r)) in epochs.iter_mut().enumerate() {
        let mut epoch = epoch(100 + i as u64);
        epoch.owner = [i as u8; 16];
        preview.replace(&doc, epoch);
        preview.open_plot(i % 2);
        click(&mut preview, &obs, "Title details", s);
        click(&mut preview, &obs, "Next fragment", s);
        click(&mut preview, &obs, "Back to Plot", s);
        for _ in 0..3 {
            s.max_assign(frame(&mut preview, &obs, vec![]));
        }
        *r = meter::phase();
    }
    drop(preview);
    let dropped = meter::phase();
    for (s, r, d) in &mut lifetimes {
        let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
        preview.open_plot(0);
        for _ in 0..4 {
            s.max_assign(frame(&mut preview, &obs, vec![]));
        }
        *r = meter::phase();
        drop(preview);
        *d = meter::phase();
    }
    meter::stop();
    let mut blocked = 0;
    for (name, s, r) in rows.into_iter().filter(|(name, _, _)| !name.is_empty()) {
        blocked += usize::from(record(name, 32.0, s, r, dropped));
    }
    for (i, (s, r)) in epochs.iter().enumerate() {
        blocked += usize::from(record(
            &format!("combined-epoch-{i}"),
            32.0,
            *s,
            *r,
            dropped,
        ));
        assert_eq!(r.live, epochs[0].1.live);
    }
    for (i, (s, r, d)) in lifetimes.into_iter().enumerate() {
        blocked += usize::from(record(&format!("lifetime-{i}"), 32.0, s, r, d));
        assert_eq!(d.live, 0);
    }
    assert_eq!(dropped.live, 0);
    assert_eq!(before, obs);
    assert_eq!(before_updated, updated);
    assert_eq!(bytes, doc.canonical_bytes());
    blocked
}
pub(super) fn run() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    egui::Context::default()
        .run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();
    println!(
        "scenario,points,traces,width,shapes,vertices,indices,retained,peak,realloc_overlap,peak_blocks,accounted,bad_shape,bad_mesh,panics,drop_bytes,status"
    );
    let mut blocked = 0;
    for case in fixtures::matrix() {
        let (doc, obs) = fixtures::scene(&case, 0);
        let before = obs.clone();
        let bytes = doc.canonical_bytes().to_vec();
        meter::start();
        let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
        preview.open_plot(0);
        let mut stats = GeometryStats::default();
        for _ in 0..4 {
            stats.max_assign(frame(&mut preview, &obs, vec![]));
        }
        let r = meter::phase();
        drop(preview);
        let dropped = meter::phase();
        meter::stop();
        blocked += usize::from(record(&case.name, case.width, stats, r, dropped));
        assert_eq!((dropped.live, dropped.blocks), (0, 0));
        assert_eq!(before, obs);
        assert_eq!(bytes, doc.canonical_bytes());
    }
    blocked += interactions();
    std::panic::set_hook(hook);
    println!("geometry qualification: {blocked} blocked scenarios; activation=false");
    if blocked > 0 {
        std::process::exit(2);
    }
}

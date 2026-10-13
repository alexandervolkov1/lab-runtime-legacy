//! C3 sequence and fallible-reservation qualification using production modules.
//! Inputs and result slots predate the meter; no production allocator hooks.
use super::{epoch, meter, plot_geometry as harness, readonly_v2};
use crate::{
    model::RuntimeObservations,
    plot_fixtures as base,
    plot_geometry_fixtures::{self as fixtures, Case, Geometry, GeometryStats},
    readonly_v2::plot_projection,
};
use eframe::egui;

#[derive(Clone, Copy, Default)]
struct Row {
    phase: &'static str,
    ordinal: usize,
    stats: GeometryStats,
    reading: Option<meter::Reading>,
    expected_faults: usize,
}
fn accounted(r: meter::Reading) -> usize {
    r.overlap_peak
        .checked_add(127_usize.checked_mul(r.peak_blocks).unwrap())
        .unwrap()
        .checked_add(65_536)
        .unwrap()
}
fn record(width: f64, row: Row, dropped: meter::Reading) {
    let r = row.reading.unwrap();
    let s = row.stats;
    assert_eq!(s.allocation_faults, row.expected_faults);
    assert_eq!(s.policy_failures, row.expected_faults);
    assert!(
        GeometryStats {
            policy_failures: 0,
            ..s
        }
        .finite()
    );
    assert_eq!((dropped.live, dropped.blocks), (0, 0));
    assert!(accounted(dropped) <= readonly_v2::PREVIEW_RESERVATION);
    assert!(
        accounted(r) <= readonly_v2::PREVIEW_RESERVATION,
        "{}: {r:?}",
        row.phase
    );
    println!(
        "{},{},{width},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},PASS",
        row.phase,
        row.ordinal,
        s.points,
        s.traces,
        r.live,
        r.peak,
        r.overlap_peak,
        r.peak_blocks,
        accounted(r),
        s.shapes,
        s.vertices,
        s.indices,
        s.unavailable,
        s.allocation_faults,
        dropped.live,
        s.curve_vertices,
        s.curve_indices,
        s.curve_vertex_capacity,
        s.curve_index_capacity
    );
}
fn save(rows: &mut Vec<Row>, phase: &'static str, ordinal: usize, stats: GeometryStats) {
    // Capacity was allocated before measurement; never add evidence history to
    // the renderer's measured heap, or subtract renderer allocations afterward.
    assert!(rows.len() < rows.capacity());
    rows.push(Row {
        phase,
        ordinal,
        stats,
        reading: Some(meter::phase()),
        expected_faults: 0,
    });
}
fn zoom(
    preview: &mut readonly_v2::Preview<'_>,
    obs: &RuntimeObservations,
    factor: f32,
    s: &mut GeometryStats,
) {
    s.max_assign(harness::frame(
        preview,
        obs,
        vec![
            egui::Event::PointerMoved(egui::pos2(600.0, 300.0)),
            egui::Event::Zoom(factor),
        ],
    ));
}
fn hover(preview: &mut readonly_v2::Preview<'_>, obs: &RuntimeObservations, s: &mut GeometryStats) {
    let (stats, pos) = harness::painted(preview, obs, vec![egui::Event::PointerGone], None);
    s.max_assign(stats);
    if let Some(pos) = pos {
        s.max_assign(harness::frame(
            preview,
            obs,
            vec![egui::Event::PointerMoved(pos)],
        ));
        s.max_assign(harness::frame(preview, obs, vec![]));
    }
}
fn sequences(width: f64) {
    let case = Case {
        width,
        ..Case::combined()
    };
    let (doc, ordinary) = fixtures::scene(&case, 0);
    let (_, updated) = fixtures::scene(&case, 1);
    let (_, coincident) = fixtures::scene(
        &Case {
            geometry: Geometry::Coincident,
            ..case.clone()
        },
        0,
    );
    let (_, near) = fixtures::scene(
        &Case {
            geometry: Geometry::NearlyCoincident,
            ..case.clone()
        },
        1,
    );
    let (_, huge) = fixtures::scene(
        &Case {
            geometry: Geometry::HugeY,
            ..case.clone()
        },
        1,
    );
    let (_, huge_x) = fixtures::scene(
        &Case {
            geometry: Geometry::HugeX,
            ..case.clone()
        },
        0,
    );
    let mut mixed = ordinary.clone();
    for i in [0, 7, 15, 31] {
        *mixed.live.get_mut(&base::target(i)).unwrap() = huge.live[&base::target(i)].clone();
    }
    let mut lengths = [1; 32];
    lengths[..3].fill(4096);
    lengths[3] = 4068;
    let (_, skewed) = fixtures::scene(
        &Case {
            lengths: Some(lengths),
            ..case.clone()
        },
        1,
    );
    lengths.reverse();
    let (_, reversed) = fixtures::scene(
        &Case {
            lengths: Some(lengths),
            ..case.clone()
        },
        0,
    );
    let inputs = [
        &ordinary,
        &updated,
        &coincident,
        &near,
        &mixed,
        &huge_x,
        &skewed,
        &reversed,
    ];
    let before = inputs.map(Clone::clone);
    let canonical = doc.canonical_bytes().to_vec();
    let mut rows = Vec::with_capacity(40);
    meter::start();
    let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
    preview.open_plot(0);
    let mut s = GeometryStats::default();
    for _ in 0..4 {
        s.max_assign(harness::frame(&mut preview, &ordinary, vec![]));
    }
    assert_eq!((s.points, s.traces), (16_384, 32));
    assert!(s.segments > 16_000 && s.unavailable == 0);
    save(&mut rows, "initial", 0, s);
    // Data changes while the same graph Context lives; then return to ordinary
    // data and require visible curves. Numeric refusal cannot pass as success.
    for (i, obs) in inputs.iter().enumerate() {
        s = GeometryStats::default();
        for _ in 0..3 {
            s.max_assign(harness::frame(&mut preview, obs, vec![]));
        }
        assert_eq!((s.points, s.traces), (16_384, 32));
        if i == 4 {
            assert!(s.unavailable >= 4 && s.segments > 0);
        } else if i == 5 {
            assert!(s.unavailable > 0 && s.segments == 0);
        } else {
            assert!(s.unavailable == 0 && s.segments > 0);
        }
        save(&mut rows, "data-sequence", i, s);
    }
    s = GeometryStats::default();
    for factor in [0.1, 10.0] {
        harness::click(&mut preview, &ordinary, "Reset / auto bounds", &mut s);
        for i in 0..340 {
            zoom(&mut preview, inputs[i % inputs.len()], factor, &mut s);
        }
    }
    // Exactly 64 accepted events, not an unbounded queue; input construction is
    // included in the measured window along with egui's event copies.
    let mut events = vec![egui::Event::PointerMoved(egui::pos2(600.0, 300.0))];
    events.extend((0..63).map(|i| egui::Event::Zoom(if i % 2 == 0 { 0.1 } else { 10.0 })));
    s.max_assign(harness::frame(&mut preview, &ordinary, events));
    save(&mut rows, "zoom-data-events", 0, s);
    s = GeometryStats::default();
    harness::click(&mut preview, &ordinary, "Reset / auto bounds", &mut s);
    // Deliberate caller spare capacity is included in this stress window. It
    // must not become retained Context capacity after filtering one event.
    let mut events = Vec::with_capacity(4096);
    events.push(egui::Event::PointerGone);
    s.max_assign(harness::frame(&mut preview, &ordinary, events));
    save(&mut rows, "input-spare-capacity", 0, s);
    // Every byte of every long field is checked against its borrowed source.
    s = GeometryStats::default();
    harness::click(&mut preview, &ordinary, "Reset / auto bounds", &mut s);
    harness::click(&mut preview, &ordinary, "Title details", &mut s);
    harness::read_pages(
        &mut preview,
        &ordinary,
        doc.dto().plots.0[0].title.as_str(),
        &mut s,
    );
    harness::click(&mut preview, &ordinary, "Back to Plot", &mut s);
    for i in 0..32 {
        if i > 0 && i % 4 == 0 {
            harness::click(&mut preview, &ordinary, "Next traces", &mut s);
        }
        harness::click(
            &mut preview,
            &ordinary,
            &format!("Details T{:02}", i + 1),
            &mut s,
        );
        let trace = &doc.dto().plots.0[0].traces.0[i];
        harness::read_pages(
            &mut preview,
            &ordinary,
            trace.display_label.as_str(),
            &mut s,
        );
        harness::click(&mut preview, &ordinary, "Cosmetic display unit", &mut s);
        harness::read_pages(
            &mut preview,
            &ordinary,
            trace.display_unit.as_ref().unwrap().as_str(),
            &mut s,
        );
        for (caption, field) in [("Runtime unit ID", "id"), ("Runtime unit symbol", "symbol")] {
            harness::click(&mut preview, &ordinary, caption, &mut s);
            harness::read_pages(
                &mut preview,
                &ordinary,
                ordinary.entities[&base::target(i)].value["unit"][field]
                    .as_str()
                    .unwrap(),
                &mut s,
            );
            // Changed Runtime metadata while a Details field is open must retire
            // the old glyph key. A later frame restores the current source.
            s.max_assign(harness::frame(&mut preview, &updated, vec![]));
            s.max_assign(harness::frame(&mut preview, &ordinary, vec![]));
        }
        harness::click(&mut preview, &ordinary, "Back to Plot", &mut s);
        hover(&mut preview, &updated, &mut s);
    }
    save(&mut rows, "all-details-updates-hover", 0, s);
    s = GeometryStats::default();
    for after_tessellation in [false, true] {
        let (mut output, report) = preview.run(&ordinary, egui::RawInput::default()).unwrap();
        assert_eq!(report.allocation_faults, 0);
        if after_tessellation {
            s.max_assign(GeometryStats::inspect(&preview, &mut output, report));
        }
        // Model an owner abandoning processing at either CPU boundary. It must
        // release deltas/output before Context retirement, with no output queue.
        output.drop_without_applying_deltas();
        preview.retire_context();
        s.max_assign(harness::frame(&mut preview, &ordinary, vec![]));
    }
    save(&mut rows, "output-discard", 0, s);
    for revision in 0..22 {
        s = GeometryStats::default();
        let mut next = epoch(revision as u64 + 1);
        next.owner = [revision as u8; 16];
        preview.replace(&doc, next);
        preview.open_plot(revision % 2);
        for i in 0..4 {
            let obs = inputs[(revision + i) % inputs.len()];
            s.max_assign(harness::frame(&mut preview, obs, vec![]));
            let caption = format!("T{:02}", (revision + i * 7) % 32 + 1);
            harness::click(&mut preview, obs, &caption, &mut s);
            zoom(
                &mut preview,
                obs,
                if i % 2 == 0 { 0.1 } else { 10.0 },
                &mut s,
            );
            harness::click(&mut preview, obs, &caption, &mut s);
            harness::click(&mut preview, obs, "Reset / auto bounds", &mut s);
        }
        hover(&mut preview, &ordinary, &mut s);
        preview.retire_context();
        s.max_assign(harness::frame(&mut preview, &updated, vec![]));
        // Actual buttons exercise output ownership on the transition frame.
        harness::click(&mut preview, &ordinary, "Back", &mut s);
        harness::click(&mut preview, &ordinary, "Open Plot", &mut s);
        let restored = harness::frame(&mut preview, &ordinary, vec![]);
        assert_eq!(restored.unavailable, 0);
        assert!(restored.segments > 16_000);
        s.max_assign(restored);
        save(&mut rows, "epoch-actions", revision, s);
    }
    drop(preview);
    let dropped = meter::phase();
    meter::stop();
    for row in rows {
        record(width, row, dropped);
    }
    for (a, b) in inputs.into_iter().zip(before) {
        assert_eq!(*a, b);
    }
    assert_eq!(doc.canonical_bytes(), canonical);
    for lifetime in 0..3 {
        meter::start();
        let mut preview = readonly_v2::Preview::new(&doc, epoch(lifetime));
        preview.open_plot(1);
        let mut stats = GeometryStats::default();
        for obs in inputs {
            stats.max_assign(harness::frame(&mut preview, obs, vec![]));
        }
        let reading = Some(meter::phase());
        drop(preview);
        let dropped = meter::phase();
        meter::stop();
        record(
            width,
            Row {
                phase: "repeated-lifetime",
                ordinal: lifetime as usize,
                stats,
                reading,
                expected_faults: 0,
            },
            dropped,
        );
    }
}

fn faults() {
    let mut syntax = base::syntax(32, false);
    for (i, trace) in syntax["plots"][0]["traces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        trace["style"]["color"] = serde_json::json!(format!("#{:02x}1020", i + 1));
    }
    let doc = base::admit(&syntax);
    let mut lengths = [512; 32];
    lengths[0] = 513;
    lengths[31] = 511;
    let obs = base::observations_with_lengths(&lengths);
    let before = obs.clone();
    let bytes = doc.canonical_bytes().to_vec();
    let identity_before = {
        let view = plot_projection::project(
            &doc,
            &obs,
            epoch(0),
            readonly_v2::Location::default(),
            0,
            plot_projection::PROJECTION_RESERVATION,
        )
        .unwrap();
        std::array::from_fn::<_, 32, _>(|i| view.traces[i].identity.clone())
    };
    // Fail first and last trace, with vertex failure and with index failure
    // after the vertex buffer is already owned. All other curves must survive.
    for (ordinal, (index, element_bytes)) in [(0, 80), (0, 24), (31, 80), (31, 24)]
        .into_iter()
        .enumerate()
    {
        meter::start();
        let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
        preview.open_plot(0);
        harness::frame(&mut preview, &obs, vec![]);
        meter::fail((lengths[index] - 1) * element_bytes);
        let (mut output, report) = preview.run(&obs, egui::RawInput::default()).unwrap();
        meter::assert_fault_consumed();
        assert_eq!(report.allocation_faults, 1);
        assert_eq!(report.geometry_failures, 1);
        assert_eq!(report.geometry_segments, 16_384 - 32 - (lengths[index] - 1));
        let mut present = [false; 32];
        for shape in &output.shapes {
            if let egui::Shape::Mesh(m) = &shape.shape {
                let i = usize::from(m.vertices[0].color.r()) - 1;
                assert!(!present[i]);
                present[i] = true;
                assert_eq!(m.vertices.len(), 4 * (lengths[i] - 1));
                assert_eq!(m.indices.len(), 6 * (lengths[i] - 1));
            }
        }
        assert!(!present[index]);
        assert_eq!(present.into_iter().filter(|v| *v).count(), 31);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(t) if t.galley.text() == "GeometryUnavailable: Allocation (1 traces)")));
        let stats = GeometryStats::inspect(&preview, &mut output, report);
        output.drop_without_applying_deltas();
        let reading = Some(meter::phase());
        // No in-frame retry; one later caller-requested frame succeeds normally.
        let recovered = harness::frame(&mut preview, &obs, vec![]);
        assert!(recovered.finite());
        assert_eq!(recovered.allocation_faults, 0);
        assert_eq!(recovered.segments, 16_384 - 32);
        let recovery_reading = Some(meter::phase());
        drop(preview);
        let dropped = meter::phase();
        meter::stop();
        record(
            1.0,
            Row {
                phase: "injected-mesh",
                ordinal,
                stats,
                reading,
                expected_faults: 1,
            },
            dropped,
        );
        record(
            1.0,
            Row {
                phase: "explicit-frame-after-fault",
                ordinal,
                stats: recovered,
                reading: recovery_reading,
                expected_faults: 0,
            },
            dropped,
        );
    }
    // Shared B1 arena/metadata refusal is frame-wide and returns no FullOutput
    // or RenderReport. A partial PlotView can never reach the widget.
    for (ordinal, size) in [
        32 * std::mem::size_of::<plot_projection::TraceView<'_>>(),
        16_384 * 16,
    ]
    .into_iter()
    .enumerate()
    {
        meter::start();
        let mut preview = readonly_v2::Preview::new(&doc, epoch(0));
        preview.open_plot(0);
        meter::fail(size);
        assert!(preview.run(&obs, egui::RawInput::default()).is_err());
        meter::assert_fault_consumed();
        let reading = Some(meter::phase());
        drop(preview);
        let dropped = meter::phase();
        meter::stop();
        record(
            1.0,
            Row {
                phase: "injected-projection",
                ordinal,
                reading,
                stats: GeometryStats {
                    allocation_faults: 1,
                    policy_failures: 1,
                    ..Default::default()
                },
                expected_faults: 1,
            },
            dropped,
        );
    }
    // Independently verify semantic identity after all failures.
    let view = plot_projection::project(
        &doc,
        &obs,
        epoch(0),
        readonly_v2::Location::default(),
        0,
        plot_projection::PROJECTION_RESERVATION,
    )
    .unwrap();
    for (i, trace) in view.traces.iter().enumerate() {
        assert_eq!(trace.identity, identity_before[i]);
        assert_eq!(trace.identity.index, i);
        assert_eq!(trace.identity.occurrence, view.identity);
        assert_eq!(
            trace.identity.trace_id,
            &doc.dto().plots.0[0].traces.0[i].id
        );
        assert_eq!(trace.binding.target, doc.dto().data_bindings.0[i].target);
    }
    assert_eq!(obs, before);
    assert_eq!(doc.canonical_bytes(), bytes);
}

fn projection_capacities() {
    let doc = base::admit(&base::syntax(32, false));
    let obs = base::observations(32, 4096);
    meter::start();
    let view = plot_projection::project(
        &doc,
        &obs,
        epoch(0),
        readonly_v2::Location::default(),
        0,
        plot_projection::PROJECTION_RESERVATION,
    )
    .unwrap();
    assert_eq!(view.traces.capacity(), 32);
    assert_eq!(view.prepared_points(), 16_384);
    let reading = meter::phase();
    // These are the only owned heap blocks in PlotView: TraceView fields are
    // borrowed or inline. Verify the native point capacity without exposing its
    // private arena through the production API just for instrumentation.
    assert_eq!(reading.blocks, 2);
    assert_eq!(
        reading.live,
        16_384 * 16 + 32 * std::mem::size_of::<plot_projection::TraceView<'_>>()
    );
    drop(view);
    let dropped = meter::phase();
    meter::stop();
    record(
        1.0,
        Row {
            phase: "projection-capacities",
            reading: Some(reading),
            stats: GeometryStats {
                points: 16_384,
                traces: 32,
                ..Default::default()
            },
            ..Default::default()
        },
        dropped,
    );
}
pub(super) fn run() {
    egui::Context::default()
        .run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();
    println!(
        "scenario,ordinal,width,points,traces,retained,phase_peak,realloc_overlap_peak,peak_blocks,accounted,shapes,vertices,indices,geometry_faults,allocation_faults,post_drop_bytes,curve_vertices,curve_indices,curve_vertex_capacity,curve_index_capacity,status"
    );
    for width in [0.9, 1.0, 32.0] {
        sequences(width);
    }
    faults();
    projection_capacities();
}

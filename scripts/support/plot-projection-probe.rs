//! Actual production frame-local projection; no Context, workers or Runtime.
use super::meter;
use crate::{
    plot_fixtures as fixtures,
    readonly_v2::{Location, PresentationEpoch, plot_projection::*},
};
use egui_plot::PlotPoint;

pub(super) fn run() {
    // Admission and the existing shared observation buffers precede measurement.
    // No output is printed or source cloned within an allocation window.
    let doc = fixtures::admit(&fixtures::syntax(32, false));
    let obs = fixtures::observations(32, 4096);
    let before = obs.clone();
    let mut readings = [(super::ZERO, super::ZERO); 21];
    meter::start();
    for (revision, (retained, dropped)) in readings.iter_mut().enumerate() {
        let view = project(
            &doc,
            &obs,
            PresentationEpoch {
                owner: [3; 16],
                revision: revision as u64,
            },
            Location::default(),
            0,
            PROJECTION_RESERVATION,
        )
        .unwrap();
        assert_eq!(view.prepared_points(), MAX_POINTS);
        assert_eq!(view.point_visits, MAX_POINT_VISITS);
        for trace in &view.traces {
            let text = hover(trace, PlotPoint::new(123.0, 1000.0)).unwrap();
            std::hint::black_box(text);
        }
        *retained = meter::phase();
        drop(view);
        *dropped = meter::phase();
        assert_eq!((dropped.live, dropped.blocks), (0, 0));
        // Same allocator-block slack policy as the CPU preview; 64 KiB also
        // charges inline result, formatting, iterator and typed-read scratch.
        assert!(
            retained.overlap_peak + retained.peak_blocks * 127 + 64 * 1024
                <= PROJECTION_RESERVATION
        );
    }
    meter::stop();
    assert!(readings.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(obs, before);
    for (revision, (retained, dropped)) in readings.iter().enumerate() {
        println!("projection epoch {revision}: {retained:?}; drop: {dropped:?}");
    }

    let epoch = PresentationEpoch {
        owner: [3; 16],
        revision: 0,
    };
    meter::start();
    assert!(matches!(
        project(
            &doc,
            &obs,
            epoch,
            Location::default(),
            0,
            PROJECTION_RESERVATION - 1
        ),
        Err(ProjectionError::Budget)
    ));
    let guard = meter::phase();
    assert_eq!(guard, super::ZERO);
    // Fail both substantial allocations independently, proving no partial view.
    meter::fail(MAX_TRACES * std::mem::size_of::<TraceView<'_>>());
    assert!(matches!(
        project(
            &doc,
            &obs,
            epoch,
            Location::default(),
            0,
            PROJECTION_RESERVATION
        ),
        Err(ProjectionError::Allocation)
    ));
    let failed_traces = meter::phase();
    meter::fail(MAX_POINTS * std::mem::size_of::<PlotPoint>());
    assert!(matches!(
        project(
            &doc,
            &obs,
            epoch,
            Location::default(),
            0,
            PROJECTION_RESERVATION
        ),
        Err(ProjectionError::Allocation)
    ));
    let failed_points = meter::phase();
    assert_eq!((failed_points.live, failed_points.blocks), (0, 0));
    meter::stop();
    println!(
        "projection guard: {guard:?}; failed traces: {failed_traces:?}; failed points: {failed_points:?}"
    );
    assert_eq!(obs, before);
    println!(
        "projection reservation={PROJECTION_RESERVATION}; point bytes={}; trace bytes={}; no Context/cache/retained epochs",
        MAX_POINTS * std::mem::size_of::<PlotPoint>(),
        MAX_TRACES * std::mem::size_of::<TraceView<'_>>()
    );
    measure_quota_fixtures();
}

fn measure_quota_fixtures() {
    let doc = fixtures::admit(&fixtures::syntax(32, false));
    let bytes = doc.canonical_bytes().to_vec();
    let mut skewed = [1; 32];
    skewed[0] = 4096;
    let mut exact = [1; 32];
    exact[..3].fill(4096);
    exact[3] = 4068;
    let mut over = exact;
    over[3] += 1;
    let mut mixed = [1; 32];
    mixed[..5].fill(4096);
    for (label, lengths, expected) in [
        ("skewed-4127", skewed, 4127),
        ("exact-budget", exact, MAX_POINTS),
        ("budget-plus-one", over, MAX_POINTS - 1),
        ("redistributed", mixed, MAX_POINTS - 3),
    ] {
        let obs = fixtures::observations_with_lengths(&lengths);
        let before = obs.clone();
        meter::start();
        let view = project(
            &doc,
            &obs,
            PresentationEpoch {
                owner: [3; 16],
                revision: 0,
            },
            Location::default(),
            0,
            PROJECTION_RESERVATION,
        )
        .unwrap();
        assert_eq!(view.prepared_points(), expected);
        let retained = meter::phase();
        drop(view);
        let dropped = meter::phase();
        assert_eq!((dropped.live, dropped.blocks), (0, 0));
        let charged = retained.overlap_peak + retained.peak_blocks * 127 + 64 * 1024;
        assert!(charged <= PROJECTION_RESERVATION);
        meter::stop();
        assert_eq!(obs, before);
        assert_eq!(doc.canonical_bytes(), bytes);
        println!(
            "quota {label}: points={expected}; {retained:?}; drop: {dropped:?}; charged={charged}"
        );
    }
}

//! Isolated native egui measurements, including Context-owned allocations.

#[path = "allocation-meter.rs"]
mod meter;

use crate::{fixtures, model::RuntimeObservations, presentation::v2, readonly_v2};
use eframe::egui;

fn frame(
    context: &egui::Context,
    document: &v2::AdmittedV2Document,
    observations: &RuntimeObservations,
    revision: u64,
) {
    context
        .run_ui(egui::RawInput::default(), |ui| {
            let report = readonly_v2::show(
                ui,
                document,
                observations,
                readonly_v2::PresentationEpoch {
                    owner: [1; 16],
                    revision,
                },
            );
            assert_eq!(report.occurrences, document.summary().expanded_components);
        })
        .drop_without_applying_deltas();
    assert_eq!(context.data(|data| data.len()), 0);
}

fn measure(
    document: &v2::AdmittedV2Document,
    empty: &v2::AdmittedV2Document,
) -> [meter::Reading; 6] {
    let observations = RuntimeObservations::default();
    meter::start();
    let context = egui::Context::default();
    frame(&context, empty, &observations, 0);
    let baseline = meter::phase();
    frame(&context, document, &observations, 1);
    let first = meter::phase();
    for _ in 0..3 {
        frame(&context, document, &observations, 1);
    }
    let stable = meter::phase();
    frame(&context, document, &observations, 2);
    let epochs = meter::phase();
    for _ in 0..3 {
        frame(&context, empty, &observations, 3);
    }
    let retired = meter::phase();
    drop(context);
    let released = meter::phase();
    meter::stop();
    [baseline, first, stable, epochs, retired, released]
}

pub(crate) fn run() {
    let empty = v2::convert_v1(
        v2::ValidatedV1Document::check(&crate::presentation::PresentationDocument::empty("empty"))
            .unwrap(),
    )
    .unwrap();
    let document = fixtures::maximum_graph();
    let before = document.canonical_bytes().to_vec();
    // Warm one-time library/TLS initialization outside the accounting window.
    // Each mode runs in a separate process: egui 0.36.2 debug Id/IdSalt SOURCE_MAP
    // tables are global, have no eviction API and outlive Context. They must not
    // be misreported as renderer state or quietly excluded from qualification.
    let warm = egui::Context::default();
    frame(&warm, &empty, &RuntimeObservations::default(), 0);
    drop(warm);
    let mode = std::env::args()
        .nth(1)
        .expect("context or debug-epochs mode");
    let readings = match mode.as_str() {
        "debug-epochs" => {
            let readings = measure(&document, &empty);
            assert!(readings[3].blocks > readings[2].blocks);
            assert!(readings[5].live > 0, "debug ID tables outlive Context");
            readings
        }
        "context" => {
            // Warm every ID used by the measured frames, so the following
            // windows isolate Context-owned state from the debug global tables.
            // The other process measures their previously cold allocations.
            let _ = measure(&document, &empty);
            let first = measure(&document, &empty);
            let second = measure(&document, &empty);
            assert_eq!(first, second, "fixture must reproduce capacity readings");
            assert_eq!(
                first[5].live, 0,
                "Context must release measured allocations"
            );
            assert_eq!(first[5].blocks, 0);
            first
        }
        _ => panic!("expected context or debug-epochs mode"),
    };
    assert_eq!(before, document.canonical_bytes());
    for (phase, reading) in [
        "baseline",
        "first",
        "stable",
        "new epoch",
        "retired",
        "drop",
    ]
    .into_iter()
    .zip(readings)
    {
        println!("{mode} {phase}: {reading:?}");
    }
    println!(
        "visual reservation: {}; owned persistent renderer heap: 0; activation qualified: {}",
        document.visual_reservation().accounted_bytes(),
        readonly_v2::ACTIVATION_QUALIFIED
    );
    println!(
        "20480 occurrences / 3 definitions; IdTypeMap remained empty; no activation qualification"
    );
}

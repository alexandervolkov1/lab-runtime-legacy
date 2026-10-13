//! Native lifecycle measurements. Each scenario executes in a separate process.

#[path = "allocation-meter.rs"]
mod meter;

use crate::{
    fixtures,
    gui::V1Scene,
    model::{RuntimeObservations, WorkbenchModel},
    presentation::{PresentationDocument, v2},
    readonly_v2,
};
use eframe::egui;
use std::{
    fmt,
    hash::Hash,
    sync::atomic::{AtomicUsize, Ordering::SeqCst},
};

const EPOCHS: usize = 16;
const RECREATIONS: usize = 4;
const ZERO: meter::Reading = meter::Reading {
    live: 0,
    blocks: 0,
    peak: 0,
    overlap_peak: 0,
};

#[derive(Debug, PartialEq, Eq)]
struct Report {
    created: meter::Reading,
    first: meter::Reading,
    repeated: meter::Reading,
    same_epoch: meter::Reading,
    epochs: [meter::Reading; EPOCHS],
    dropped: meter::Reading,
    recreated: [meter::Reading; RECREATIONS],
    redropped: [meter::Reading; RECREATIONS],
}

enum Scene<'a> {
    Empty,
    V2(&'a v2::AdmittedV2Document),
    V1(&'a WorkbenchModel),
}
impl Scene<'_> {
    fn frame(&self, context: &egui::Context, v1: &mut V1Scene, revision: u64) {
        match self {
            Self::V1(model) => v1.frame(context, model, revision),
            Self::Empty | Self::V2(_) => {
                context
                    .run_ui(egui::RawInput::default(), |ui| {
                        if let Self::V2(document) = self {
                            let report = readonly_v2::show(
                                ui,
                                document,
                                &RuntimeObservations::default(),
                                readonly_v2::PresentationEpoch {
                                    owner: [1; 16],
                                    revision,
                                },
                            );
                            assert_eq!(report.occurrences, document.summary().expanded_components);
                        }
                    })
                    .drop_without_applying_deltas();
                assert_eq!(context.data(|data| data.len()), 0);
            }
        }
    }
}

fn measure(scene: &Scene<'_>, epochs: usize, recreations: usize) -> Report {
    meter::start();
    let context = egui::Context::default();
    let mut v1 = V1Scene::default();
    let created = meter::phase();
    scene.frame(&context, &mut v1, 0);
    let first = meter::phase();
    for _ in 0..3 {
        scene.frame(&context, &mut v1, 0);
    }
    let repeated = meter::phase();
    for _ in 0..3 {
        scene.frame(&context, &mut v1, 0);
    }
    let same_epoch = meter::phase();
    assert_eq!(
        (repeated.live, repeated.blocks),
        (same_epoch.live, same_epoch.blocks),
        "settled same-epoch frames must not retain more allocations"
    );
    let mut later = [ZERO; EPOCHS];
    for (i, reading) in later.iter_mut().enumerate().take(epochs) {
        scene.frame(&context, &mut v1, i as u64 + 1);
        *reading = meter::phase();
    }
    drop(v1);
    drop(context);
    let dropped = meter::phase();
    let mut recreated = [ZERO; RECREATIONS];
    let mut redropped = [ZERO; RECREATIONS];
    for i in 0..recreations {
        let context = egui::Context::default();
        let mut v1 = V1Scene::default();
        for _ in 0..4 {
            scene.frame(&context, &mut v1, (epochs + i + 1) as u64);
        }
        recreated[i] = meter::phase();
        drop(v1);
        drop(context);
        redropped[i] = meter::phase();
    }
    meter::stop();
    Report {
        created,
        first,
        repeated,
        same_epoch,
        epochs: later,
        dropped,
        recreated,
        redropped,
    }
}

static FORMATS: AtomicUsize = AtomicUsize::new(0);
#[derive(Hash)]
struct DiagnosticSource(u64);
impl fmt::Debug for DiagnosticSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        FORMATS.fetch_add(1, SeqCst);
        // Deliberately verbose but finite. Hash semantics equal the numeric case.
        write!(
            f,
            "DiagnosticSource({}: {})",
            self.0,
            "diagnostic ".repeat(16)
        )
    }
}

fn ids(numeric: bool, provenance: bool) {
    let mut readings = [ZERO; EPOCHS];
    let mut digest = 0u64;
    meter::start();
    for (epoch, reading) in readings.iter_mut().enumerate() {
        for occurrence in 0..64 {
            let value = (epoch * 64 + occurrence) as u64;
            let (root, salt, child) = if numeric {
                let root = egui::Id::new(value);
                (root, egui::IdSalt::new(value), root.with(value))
            } else {
                let root = egui::Id::new(DiagnosticSource(value));
                (
                    root,
                    egui::IdSalt::new(DiagnosticSource(value)),
                    root.with(DiagnosticSource(value)),
                )
            };
            digest = digest.rotate_left(1) ^ root.value() ^ salt.value() ^ child.value();
        }
        *reading = meter::phase();
    }
    meter::stop();
    if provenance {
        assert!(
            readings
                .windows(2)
                .all(|pair| pair[1].blocks > pair[0].blocks)
        );
    } else {
        assert_eq!(FORMATS.load(SeqCst), 0, "egui must never format ID sources");
        assert!(
            readings
                .iter()
                .all(|r| r.live == 0 && r.peak == 0 && r.blocks == 0)
        );
    }
    println!("ID digest {digest:016x}; numeric={numeric}; provenance={provenance}");
    for (i, r) in readings.iter().enumerate() {
        println!("epoch {}: {r:?}", i + 1);
    }
}

fn v1_fixture() -> WorkbenchModel {
    let document: PresentationDocument = serde_json::from_value(serde_json::json!({
        "format_version":1,"document_id":"existing-v1","windows":[{"id":"w","title":"Window","tabs":[{"id":"t","title":"Tab","panels":[
            {"id":"p","title":"Status","kind":{"kind":"status","source":{"kind":"reference","reference":"r"}}},
            {"id":"c","title":"Controls","kind":{"kind":"controls","control_ids":["reconnect"]}}
        ]}]}],"plots":[],"controls":[{"id":"reconnect","label":"Reconnect","kind":"reconnect_resource","target":{"kind":"resource","resource":"r"},"confirm":false}]
    })).unwrap();
    document.validate().unwrap();
    WorkbenchModel::new(document)
}

fn print_report(label: &str, report: &Report, epochs: usize, recreations: usize) {
    println!(
        "{label} created: {:?}; first: {:?}; repeated: {:?}",
        report.created, report.first, report.repeated
    );
    println!(
        "{label} same epoch, three further frames: {:?}",
        report.same_epoch
    );
    let mut previous = report.same_epoch.live;
    for (i, r) in report.epochs.iter().enumerate().take(epochs) {
        println!(
            "{label} epoch {}: {r:?}; retained delta {}",
            i + 1,
            r.live as i128 - previous as i128
        );
        previous = r.live;
    }
    println!("{label} Context drop: {:?}", report.dropped);
    for i in 0..recreations {
        println!(
            "{label} new Context {i}: {:?}; drop: {:?}",
            report.recreated[i], report.redropped[i]
        );
    }
}

pub(crate) fn run() {
    let args: Vec<_> = std::env::args().collect();
    let scenario = args.get(1).expect("scenario").as_str();
    let provenance = args.get(2).expect("provenance on/off") == "on";
    if scenario == "hash-baseline" {
        meter::start();
        std::hint::black_box(ahash::RandomState::new());
        let first = meter::phase();
        for _ in 0..32 {
            std::hint::black_box(ahash::RandomState::new());
        }
        let repeated = meter::phase();
        meter::stop();
        // ahash 0.8.12 RAND_SOURCE owns an 8-byte counter and a 16-byte boxed
        // trait object on this qualified target, once for the process lifetime.
        assert_eq!((first.live, first.blocks), (24, 2));
        assert_eq!((repeated.live, repeated.blocks), (24, 2));
        println!("ahash singleton first: {first:?}; repeated: {repeated:?}");
        return;
    }
    // A generic instantiated in this harness must still use egui's own cfg.
    let _ = egui::Id::new(DiagnosticSource(u64::MAX));
    assert_eq!(FORMATS.swap(0, SeqCst) != 0, provenance);
    if scenario.starts_with("ids-") {
        ids(scenario == "ids-numeric", provenance);
        return;
    }
    let small = fixtures::repeated_small_graph();
    let large = (scenario == "maximum").then(fixtures::maximum_graph);
    let v1 = v1_fixture();
    let before_v1 = v1.clone();
    let scene = match scenario {
        "empty" => Scene::Empty,
        "small" => Scene::V2(&small),
        "maximum" => Scene::V2(large.as_ref().unwrap()),
        "v1" => Scene::V1(&v1),
        _ => panic!("unknown scenario"),
    };
    let before = match scene {
        Scene::V2(doc) => Some(doc.canonical_bytes().to_vec()),
        _ => None,
    };
    // Only the library's one-time empty-context/TLS initialization is warmed.
    // No renderer ID/epoch or widgets in the scene have been submitted yet.
    // The empty scenario deliberately also measures the first-ever Context.
    if scenario != "empty" {
        let warm = egui::Context::default();
        Scene::Empty.frame(&warm, &mut V1Scene::default(), 0);
        drop(warm);
    }
    let (epochs, recreations) = if scenario == "maximum" {
        (1, 1)
    } else {
        (EPOCHS, RECREATIONS)
    };
    let cold = measure(&scene, epochs, recreations);
    // All IDs from the exact sequence are now present in diagnostic builds.
    // Two further runs separate Context-owned memory and check repeatability.
    let warm = measure(&scene, epochs, recreations);
    let repeat = measure(&scene, epochs, recreations);
    assert_eq!(warm, repeat, "Context capacity readings must reproduce");
    assert_eq!(warm.dropped.live, 0);
    assert_eq!(warm.dropped.blocks, 0);
    assert!(
        warm.redropped[..recreations]
            .iter()
            .all(|r| r.live == 0 && r.blocks == 0)
    );
    if scenario == "empty" {
        // Separate a finite one-time library baseline from per-epoch retention.
        assert!(
            cold.redropped[..recreations]
                .iter()
                .all(|r| (r.live, r.blocks) == (cold.dropped.live, cold.dropped.blocks))
        );
    } else if provenance {
        assert!(
            cold.epochs[..epochs]
                .windows(2)
                .all(|p| p[1].blocks > p[0].blocks)
        );
        assert!(cold.dropped.live > 0);
        assert!(cold.redropped[0].live > cold.dropped.live);
    } else {
        assert_eq!(cold.dropped.live, 0);
        assert_eq!(cold.dropped.blocks, 0);
        assert!(
            cold.redropped[..recreations]
                .iter()
                .all(|r| r.live == 0 && r.blocks == 0)
        );
    }
    assert_eq!(v1, before_v1);
    if let Scene::V2(doc) = scene {
        assert_eq!(before.as_deref().unwrap(), doc.canonical_bytes());
        // Printed only after measurement: fingerprinting must not warm cold IDs.
        let mut fingerprint = 0u64;
        for revision in 0..=20 {
            for root in 0..2 {
                for child in 0..4 {
                    let id = readonly_v2::OccurrenceIdentity {
                        epoch: readonly_v2::PresentationEpoch {
                            owner: [1; 16],
                            revision,
                        },
                        occurrence: doc
                            .source()
                            .occurrence(0, 0, 0, root, &[child])
                            .unwrap()
                            .identity,
                    }
                    .widget_id();
                    fingerprint = fingerprint.rotate_left(1) ^ id.value();
                }
            }
        }
        println!("occurrence fingerprint {fingerprint:016x}");
        println!(
            "occurrences {}; visual reservation {}",
            doc.summary().expanded_components,
            doc.visual_reservation().accounted_bytes()
        );
    }
    print_report("cold", &cold, epochs, recreations);
    print_report("Context-only", &warm, epochs, recreations);
    assert!(!std::hint::black_box(readonly_v2::ACTIVATION_QUALIFIED));
    println!("scenario={scenario}; provenance={provenance}; activation qualified=false");
}

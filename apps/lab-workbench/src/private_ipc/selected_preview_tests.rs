//! CPU admission gate before any native-child GL integration. The fixture acts
//! as the parent encoder; only validated selected bytes reach the Preview.
use super::{protocol::*, reads, replica::Replica};
use crate::{
    gui::readonly_v2::{
        self as renderer, PresentationEpoch, Preview,
        host_session::HostSession,
        plot_fixtures,
        plot_projection::{self, CurveState, MAX_POINT_VISITS, MAX_POINTS, PROJECTION_RESERVATION},
        texture_owner::CpuOnly,
    },
    model::{Freshness, RuntimeObservations},
    presentation::v2,
};
use eframe::egui;
use serde_json::json;
use std::time::{Duration, Instant};

const EPOCH: PresentationEpoch = PresentationEpoch {
    owner: [8; 16],
    revision: 4,
};
fn header(kind: Kind, snapshot: u64, total: usize) -> Header {
    Header {
        kind,
        bytes: HEADER,
        total,
        identity: Identity {
            workbench: [7; 16],
            incarnation: 3,
        },
        epoch: Epoch {
            owner: EPOCH.owner,
            revision: EPOCH.revision,
        },
        sequence: 1,
        snapshot,
        offset: 0,
        source: 40,
    }
}
fn feed(replica: &mut Replica, header: Header, bytes: &[u8], now: Instant) {
    for (index, part) in bytes.chunks(CHUNK - HEADER).enumerate() {
        replica
            .chunk(
                Header {
                    bytes: HEADER + part.len(),
                    offset: index * (CHUNK - HEADER),
                    ..header
                },
                part,
                now,
            )
            .unwrap();
    }
}
fn encoded(observations: &RuntimeObservations, count: usize) -> Vec<u8> {
    let targets: Vec<_> = (0..count).map(plot_fixtures::target).collect();
    let selected: Vec<_> = targets
        .iter()
        .map(|target| reads::Selection {
            target,
            origin: Some(reads::Origin {
                boot: "original-boot",
                projection: Some("parent-approved"),
                revision: Some(40),
                trace_from: Some(0.),
            }),
        })
        .collect();
    let mut bytes = vec![0; SNAPSHOT];
    let len = reads::encode(&mut bytes, observations, &selected).unwrap();
    bytes.truncate(len);
    bytes
}
fn document(traces: usize, shared: bool) -> Vec<u8> {
    let mut syntax = plot_fixtures::syntax(traces, shared);
    syntax["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["scalar", "chart", "raw"]);
    syntax["components"].as_array_mut().unwrap().extend([
        json!({"id":"scalar","kind":"indicator","label":"Parent selected scalar","data_id":"d0"}),
        json!({"id":"raw","kind":"status","label":"Unsupported raw entity","source":{"kind":"entity","target":{"kind":"instrument","instrument":"instrument"}}}),
    ]);
    v2::admit(&serde_json::to_vec(&syntax).unwrap())
        .unwrap()
        .canonical_bytes()
        .to_vec()
}
fn painted(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn visit(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for shape in shapes {
        visit(&shape.shape, &mut out);
    }
    out
}

#[test]
fn selected_preview_cpu_gate_canonical_scalar_trace_origin_and_geometry() {
    let bytes = document(1, false);
    let mut cache = plot_fixtures::observations(1, 3);
    plot_fixtures::observe(&mut cache, 0, "float", json!(-0.0));
    let selected = encoded(&cache, 1);
    let now = Instant::now();
    let mut session = HostSession::new(CpuOnly);
    let (mut replica, mut owner) = Replica::renderer(&mut session);
    feed(
        &mut replica,
        header(Kind::Document, 1, bytes.len()),
        &bytes,
        now,
    );
    assert!(owner.document().is_none());
    assert!(replica.admit_renderer_document(&mut owner).unwrap().applied);
    replica.fence(EPOCH.revision, 40, now).unwrap();
    feed(
        &mut replica,
        header(Kind::Observations, 2, selected.len()),
        &selected,
        now,
    );
    let provider = replica.selected_provider(now).unwrap().unwrap();
    let read = provider.selected(&plot_fixtures::target(0)).unwrap();
    assert_eq!(read.fields.origin.boot, Some("original-boot"));
    assert_eq!(read.fields.origin.projection, Some("parent-approved"));
    assert_eq!(read.fields.origin.revision, Some(40));
    assert_eq!(read.fields.origin.generation, Some(1));
    let (document, _, session) = owner.parts().unwrap();
    assert_eq!(document.canonical_bytes(), bytes);
    let view = plot_projection::project(
        document,
        &provider,
        EPOCH,
        renderer::Location::default(),
        1,
        PROJECTION_RESERVATION,
    )
    .unwrap();
    assert_eq!(
        view.points(0),
        &[
            egui_plot::PlotPoint::new(0., 0.),
            egui_plot::PlotPoint::new(1., 1.),
            egui_plot::PlotPoint::new(2., 2.)
        ]
    );
    assert_eq!(view.traces[0].buffer_dropped, None);
    assert_eq!(view.traces[0].state, CurveState::Ready);
    assert!(view.point_visits <= MAX_POINT_VISITS && view.prepared_points() <= MAX_POINTS);
    let mut preview = Preview::with_session(document, EPOCH, session);
    let frame = preview.run(&provider, Default::default()).unwrap();
    let labels = painted(frame.shapes());
    assert!(labels.iter().any(|s| s == "Value (cached): -0.0"));
    assert!(labels.iter().any(|s| s == "Runtime unit: V"));
    assert!(labels.iter().any(|s| s == "Fresh"));
    assert!(
        labels
            .iter()
            .any(|s| s.contains("raw entity presentation is not supplied"))
    );
    frame.finish_cpu().unwrap();
    assert!(preview.open_plot(1));
    let mut frame = preview.run(&provider, Default::default()).unwrap();
    assert_eq!(frame.report().plot_points, 3);
    assert_eq!(frame.report().geometry_failures, 0);
    let meshes = frame.tessellate().unwrap();
    assert!(meshes.iter().all(|p| p.clip_rect.is_finite()));
    frame.finish_cpu().unwrap();
    println!(
        "SELECTED_PREVIEW_CPU canonical=true scalar=-0.0 unit=V exact_points=3 source_generation=1 raw_entity=unavailable limits_preserved=true runtime_submissions=0"
    );
}

#[test]
fn selected_preview_missing_stale_rebuild_and_epoch_fences_do_not_promote_reads() {
    let bytes = document(1, false);
    let now = Instant::now();
    for state in [
        Freshness::Fresh,
        Freshness::Stale,
        Freshness::Unknown,
        Freshness::Rebuilding,
    ] {
        for missing in [false, true] {
            let mut observations = if missing {
                RuntimeObservations::default()
            } else {
                plot_fixtures::observations(1, 3)
            };
            observations.freshness = state;
            let selected = encoded(&observations, 1);
            let mut replica = Replica::new();
            feed(
                &mut replica,
                header(Kind::Document, 1, bytes.len()),
                &bytes,
                now,
            );
            assert!(replica.admit_encoded_document().unwrap().applied);
            replica.fence(EPOCH.revision, 40, now).unwrap();
            feed(
                &mut replica,
                header(Kind::Observations, 2, selected.len()),
                &selected,
                now,
            );
            let document = v2::admit(&bytes).unwrap();
            for stale in [false, true] {
                let provider = replica
                    .selected_provider(
                        now + if stale {
                            Duration::from_secs(3)
                        } else {
                            Duration::ZERO
                        },
                    )
                    .unwrap()
                    .unwrap();
                let mut preview = Preview::new(&document, EPOCH);
                let frame = preview.run(&provider, Default::default()).unwrap();
                let labels = painted(frame.shapes());
                if missing || stale || state != Freshness::Fresh {
                    assert!(
                        !labels.iter().any(|s| s == "Fresh"),
                        "{state:?}, missing={missing}, stale={stale}: {labels:?}"
                    );
                }
                if missing {
                    assert!(
                        labels
                            .iter()
                            .any(|s| s == "Value (cached): no Runtime observation")
                    );
                }
                frame.finish_cpu().unwrap();
                let plot = plot_projection::project(
                    &document,
                    &provider,
                    EPOCH,
                    renderer::Location::default(),
                    1,
                    PROJECTION_RESERVATION,
                )
                .unwrap();
                if missing || matches!(state, Freshness::Unknown | Freshness::Rebuilding) {
                    assert_eq!(plot.prepared_points(), 0);
                }
                let mut wrong = Preview::new(
                    &document,
                    PresentationEpoch {
                        revision: 5,
                        ..EPOCH
                    },
                );
                assert!(wrong.run(&provider, Default::default()).is_err());
            }
        }
    }
}

#[test]
fn selected_preview_repeated_large_trace_respects_unchanged_point_visit_budget() {
    // One LRD1 source can contain all 16384 points, but repeating it in 32
    // traces cannot silently authorize 32 scans of that size.
    let bytes = document(32, true);
    let now = Instant::now();
    let observations = plot_fixtures::observations(1, 4096);
    let selected = encoded(&observations, 1);
    let document = v2::admit(&bytes).unwrap();
    let mut replica = Replica::new();
    feed(
        &mut replica,
        header(Kind::Document, 1, bytes.len()),
        &bytes,
        now,
    );
    assert!(replica.admit_encoded_document().unwrap().applied);
    replica.fence(EPOCH.revision, 40, now).unwrap();
    feed(
        &mut replica,
        header(Kind::Observations, 2, selected.len()),
        &selected,
        now,
    );
    let provider = replica.selected_provider(now).unwrap().unwrap();
    let view = plot_projection::project(
        &document,
        &provider,
        EPOCH,
        renderer::Location::default(),
        1,
        PROJECTION_RESERVATION,
    )
    .unwrap();
    assert_eq!(view.point_visits, MAX_POINT_VISITS);
    assert_eq!(view.prepared_points(), MAX_POINTS);
    assert!(
        view.traces
            .iter()
            .all(|trace| trace.buffer_dropped.is_none())
    );
    drop(view);
    drop(provider);
    // A codec-valid adversarial 16384-point single selection. This test-only
    // wire expansion does not raise the parent cache's 4096-point ring limit.
    let mut large = selected.clone();
    let count_at = large.len() - 4096 * 16 - 4;
    large[count_at..count_at + 4].copy_from_slice(&(16384u32).to_le_bytes());
    for _ in 0..3 {
        large.extend_from_slice(&selected[count_at + 4..]);
    }
    reads::validate(&large).unwrap();
    feed(
        &mut replica,
        header(Kind::Observations, 3, large.len()),
        &large,
        now,
    );
    let provider = replica.selected_provider(now).unwrap().unwrap();
    assert_eq!(
        provider
            .selected(&plot_fixtures::target(0))
            .unwrap()
            .fields
            .trace
            .len(),
        16384
    );
    assert!(matches!(
        plot_projection::project(
            &document,
            &provider,
            EPOCH,
            renderer::Location::default(),
            1,
            PROJECTION_RESERVATION
        ),
        Err(plot_projection::ProjectionError::Budget)
    ));
}

#[test]
fn selected_preview_global_stale_and_parent_source_fence_override_original_fresh() {
    let bytes = document(1, false);
    let now = Instant::now();
    let cache = plot_fixtures::observations(1, 3);
    for global_stale in [false, true] {
        let mut selected = encoded(&cache, 1);
        if global_stale {
            selected[5] = 3;
            // Existing codec rejects a falsely Fresh entry before the consumer.
            assert_eq!(
                reads::validate(&selected),
                Err("false Fresh selected observation")
            );
            continue;
        }
        let mut replica = Replica::new();
        feed(
            &mut replica,
            header(Kind::Document, 1, bytes.len()),
            &bytes,
            now,
        );
        assert!(replica.admit_encoded_document().unwrap().applied);
        replica.fence(EPOCH.revision, 40, now).unwrap();
        feed(
            &mut replica,
            header(Kind::Observations, 2, selected.len()),
            &selected,
            now,
        );
        if !global_stale {
            replica.fence(EPOCH.revision, 41, now).unwrap();
        }
        let provider = replica.selected_provider(now).unwrap().unwrap();
        assert_eq!(
            provider
                .selected(&plot_fixtures::target(0))
                .unwrap()
                .fields
                .freshness,
            Freshness::Fresh
        );
        let document = v2::admit(&bytes).unwrap();
        let mut preview = Preview::new(&document, EPOCH);
        let frame = preview.run(&provider, Default::default()).unwrap();
        let labels = painted(frame.shapes());
        assert!(!labels.iter().any(|text| text == "Fresh"));
        assert!(
            labels
                .iter()
                .any(|text| text == "Stale — cached observation")
        );
        frame.finish_cpu().unwrap();
        let plot = plot_projection::project(
            &document,
            &provider,
            EPOCH,
            renderer::Location::default(),
            1,
            PROJECTION_RESERVATION,
        )
        .unwrap();
        assert_eq!(plot.traces[0].freshness, Freshness::Stale);
    }
}

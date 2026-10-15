//! Selected semantic views borrow real encoded fixture snapshots, without
//! rebuilding Runtime events, discovery, or a child RuntimeObservations model.
use super::{protocol::*, reads, selected_provider::SelectedSnapshot, tests};

#[test]
fn provider_borrows_replica_credit_and_heartbeat_cannot_clear_non_current_latch() {
    use super::replica::Replica;
    use std::time::{Duration, Instant};
    let (observations, references) = tests::maximum_reads();
    let mut encoded = vec![0; SNAPSHOT];
    let size = reads::encode(&mut encoded, &observations, &tests::selected(&references)).unwrap();
    let document=crate::presentation::v2::admit(br#"{"format_version":2,"binding_version":1,"document_id":"provider","windows":[],"components":[],"data_bindings":[],"editor_bindings":[],"action_bindings":[],"plots":[]}"#).unwrap();
    let mut replica = Replica::new();
    let now = Instant::now();
    let bytes = document.canonical_bytes();
    replica
        .chunk(
            Header {
                kind: Kind::Document,
                total: bytes.len(),
                bytes: HEADER + bytes.len(),
                snapshot: 1,
                ..header()
            },
            bytes,
            now,
        )
        .unwrap();
    assert!(replica.admit_encoded_document().unwrap().applied);
    replica.fence(4, 42, now).unwrap();
    for (index, part) in encoded[..size].chunks(CHUNK - HEADER).enumerate() {
        replica
            .chunk(
                Header {
                    total: size,
                    bytes: HEADER + part.len(),
                    offset: index * (CHUNK - HEADER),
                    ..header()
                },
                part,
                now,
            )
            .unwrap();
    }
    {
        let provider = replica.selected_provider(now).unwrap().unwrap();
        assert!(provider.current());
        consume(&provider, &references[0]);
    }
    let later = now + Duration::from_secs(3);
    assert!(!replica.selected_provider(later).unwrap().unwrap().current());
    replica.parent_progress(later);
    let provider = replica.selected_provider(later).unwrap().unwrap();
    let view = provider.selected(&references[0]).unwrap();
    assert_eq!(view.fields.freshness, Freshness::Fresh);
    assert_eq!(view.display_freshness(), Freshness::Stale);
}
use crate::{
    gui::readonly_v2::observation_source::{ReadOnlyProvider, Scalar},
    model::{Freshness, ReadAbsence, ReadQuality, ScalarKind},
    presentation::RuntimeRef,
};

fn header() -> Header {
    Header {
        kind: Kind::Observations,
        bytes: HEADER,
        total: 0,
        identity: Identity {
            workbench: [5; 16],
            incarnation: 9,
        },
        sequence: 1,
        epoch: Epoch {
            owner: [6; 16],
            revision: 4,
        },
        snapshot: 2,
        offset: 0,
        source: 42,
    }
}
fn consume(provider: &impl ReadOnlyProvider, target: &RuntimeRef) {
    assert_eq!(provider.epoch().unwrap().revision, 4);
    assert_eq!(provider.incarnation(), Some(9));
    assert_eq!(provider.original_freshness(), Freshness::Fresh);
    provider.with_selected(
        target,
        crate::readonly_values::ReadBinding::for_target(target).unwrap(),
        &mut |selected| {
            let selected = selected.unwrap();
            assert_eq!(selected.target, target);
            assert_eq!(selected.fields.scalar, Scalar::Number(-0.0));
            let Scalar::Number(value) = selected.fields.scalar else {
                panic!("typed Number missing")
            };
            assert_eq!(value.to_bits(), (-0.0f64).to_bits());
            assert_eq!(selected.fields.kind, Some(ScalarKind::Number));
            let origin = selected.fields.origin;
            assert_eq!(origin.boot, Some("original-runtime-boot"));
            assert_eq!(origin.projection, Some("frozen-projection"));
            assert_eq!(origin.revision, Some(20));
            assert_eq!(origin.entity_revision, Some(9));
            assert_eq!(origin.cursor, Some(20));
            assert_eq!(origin.generation, None);
            assert_eq!(selected.fields.unit.unwrap().symbol, "°C");
            assert_eq!(selected.fields.trace.len(), POINTS / TRACES);
            assert_eq!(selected.fields.trace.source_floor, Some(0.0));
            assert_eq!(
                selected
                    .fields
                    .trace
                    .points()
                    .next()
                    .unwrap()
                    .value
                    .to_bits(),
                (-0.0f64).to_bits()
            );
            assert_eq!(
                selected.fields.trace.points().last().unwrap().time_seconds,
                511.0
            );
        },
    );
}
#[test]
fn selected_provider_preserves_exact_source_scalars_units_and_borrowed_maximum_trace() {
    let (observations, references) = tests::maximum_reads();
    let mut encoded = vec![0; SNAPSHOT];
    let size = reads::encode(&mut encoded, &observations, &tests::selected(&references)).unwrap();
    let source = encoded.as_ptr();
    let provider = SelectedSnapshot::decode(header(), &encoded[..size], true).unwrap();
    assert!(
        provider.accounted_bytes()
            <= std::mem::size_of::<SelectedSnapshot<'_>>() + TRACES * reads::REFERENCE
    );
    println!(
        "SELECTED_PROVIDER accounted_view_bytes={} fixed_bytes={} target_string_ceiling={} encoded_copy_bytes=0",
        provider.accounted_bytes(),
        std::mem::size_of::<SelectedSnapshot<'_>>(),
        TRACES * reads::REFERENCE
    );
    consume(&provider, &references[0]);
    assert!(provider.current());
    assert_eq!(
        provider
            .selected(&references[0])
            .unwrap()
            .display_freshness(),
        Freshness::Fresh
    );
    let address = provider
        .selected(&references[0])
        .unwrap()
        .fields
        .trace
        .encoded_bytes()
        .unwrap()
        .as_ptr() as usize;
    assert!((source as usize..source as usize + size).contains(&address));
    drop(provider);
    let non_current = SelectedSnapshot::decode(header(), &encoded[..size], false).unwrap();
    consume(&non_current, &references[0]);
    let read = non_current.selected(&references[0]).unwrap();
    assert_eq!(read.fields.freshness, Freshness::Fresh);
    assert_eq!(read.display_freshness(), Freshness::Stale);
    println!(
        "SELECTED_PROVIDER borrowed_points={POINTS} references={TRACES} original_freshness_preserved=true independent_latch=true"
    );
}

#[test]
fn selected_provider_exposes_original_absence_quality_generation_and_unknown_reference() {
    let target = RuntimeRef::Signal {
        instrument: "1".into(),
        parameter: "2".into(),
    };
    let mut observations = crate::model::RuntimeObservations::default();
    observations.observe_read_descriptor(target.clone(),&serde_json::json!({"value_type":"float","generation":"3","unit":{"id":"volt","symbol":"V"}}));
    observations.observe(target.clone(),serde_json::json!({"signal":{"instrument":"1","parameter":"2"},"generation":"3","quality":"unavailable","status":"not_observed","value":null,"observed_at_ns":"19","source_at_ns":"17"}),Some(23));
    observations.freshness = Freshness::Stale;
    let selections = [reads::Selection {
        target: &target,
        origin: Some(reads::Origin {
            boot: "boot",
            projection: Some("projection"),
            revision: Some(24),
            trace_from: None,
        }),
    }];
    let mut encoded = vec![0; SNAPSHOT];
    let size = reads::encode(&mut encoded, &observations, &selections).unwrap();
    let provider = SelectedSnapshot::decode(header(), &encoded[..size], false).unwrap();
    let read = provider.selected(&target).unwrap();
    assert_eq!(read.fields.scalar, Scalar::Absent(ReadAbsence::NotObserved));
    assert_eq!(read.fields.quality, Some(ReadQuality::NotObserved));
    assert_eq!(read.fields.origin.generation, Some(3));
    assert_eq!(read.fields.origin.source_at, Some(17));
    assert_eq!(read.fields.origin.observed_at, Some(19));
    assert_eq!(read.fields.origin.cursor, Some(23));
    assert_eq!(read.fields.origin.revision, Some(24));
    assert_eq!(read.fields.trace.len(), 0);
    assert_eq!(read.display_freshness(), Freshness::Stale);
    assert!(provider.selected(&RuntimeRef::Recorder).is_none());
    assert!(SelectedSnapshot::decode(header(), &encoded[..size - 1], true).is_err());
}

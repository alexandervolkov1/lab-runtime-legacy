use super::protocol::*;
use super::{reads, replica::Replica};
use crate::{
    model::{Freshness, LivePoint, RuntimeObservations},
    presentation::{RuntimeRef, v2},
};
use std::time::{Duration, Instant};

use crate::presentation::v2::admission_fixtures as documents;

pub(super) fn maximum_document() -> v2::AdmittedV2Document {
    v2::admit(&serde_json::to_vec(&documents::large()).unwrap()).unwrap()
}
pub(super) fn maximum_reads() -> (RuntimeObservations, Vec<RuntimeRef>) {
    let mut observations = RuntimeObservations::default();
    let mut references = Vec::with_capacity(TRACES);
    for index in 0..TRACES {
        let target = RuntimeRef::Reference {
            reference: index.to_string(),
        };
        observations.observe(target.clone(), serde_json::json!({"reference":index.to_string(),"value":-0.0,"revision":"9","unit":{"id":"temperature","symbol":"°C"}}), Some(20));
        let trace = observations.live.entry(target.clone()).or_default();
        for point in 0..POINTS / TRACES {
            trace
                .push(LivePoint {
                    time_seconds: point as f64,
                    value: if point == 0 { -0.0 } else { point as f64 },
                })
                .unwrap();
        }
        references.push(target);
    }
    observations.freshness = Freshness::Fresh;
    (observations, references)
}
pub(super) fn selected(references: &[RuntimeRef]) -> Vec<reads::Selection<'_>> {
    references
        .iter()
        .map(|target| reads::Selection {
            target,
            origin: Some(reads::Origin {
                boot: "original-runtime-boot",
                projection: Some("frozen-projection"),
                revision: Some(20),
                trace_from: Some(0.),
            }),
        })
        .collect()
}

const ID: Identity = Identity {
    workbench: [1; 16],
    incarnation: 1,
};
const EPOCH: Epoch = Epoch {
    owner: [2; 16],
    revision: 1,
};
fn document() -> Header {
    Header {
        kind: Kind::Document,
        bytes: CHUNK,
        total: SNAPSHOT,
        identity: ID,
        sequence: 1,
        epoch: EPOCH,
        snapshot: 1,
        offset: 0,
        source: 0,
    }
}
#[test]
fn header_rejects_bounds_identity_version_variant_and_order_before_any_credit() {
    let mut bytes = [0; HEADER];
    let header = document();
    header.encode(&mut bytes).unwrap();
    assert_eq!(Header::decode(&bytes, ID, true).unwrap(), header);
    for position in [0, 4, 6, 16, 32, 84] {
        let mut bad = bytes;
        bad[position] = 255;
        assert!(Header::decode(&bad, ID, true).is_err(), "offset {position}");
    }
    for invalid in [
        Header {
            bytes: CHUNK + 1,
            ..header
        },
        Header {
            total: SNAPSHOT + 1,
            ..header
        },
        Header {
            offset: SNAPSHOT,
            ..header
        },
        Header {
            bytes: HEADER,
            ..header
        },
        Header {
            sequence: 0,
            ..header
        },
        Header {
            epoch: Epoch::default(),
            ..header
        },
    ] {
        invalid.encode(&mut bytes).unwrap();
        assert!(Header::decode(&bytes, ID, true).is_err());
    }
    let mut control = Header::control(Kind::Hello, ID, 1);
    control.bytes = CONTROL;
    control.encode(&mut bytes).unwrap();
    assert_eq!(Header::decode(&bytes, ID, false).unwrap(), control);
    control.bytes += 1;
    control.encode(&mut bytes).unwrap();
    assert!(Header::decode(&bytes, ID, false).is_err());
    let mut sequence = Sequence::default();
    sequence.accept(1).unwrap();
    assert!(sequence.accept(1).is_err());
    assert!(sequence.accept(3).is_err());
    sequence.accept(2).unwrap();
    assert_eq!(ENCODED_STORAGE, 4399104);
}
#[test]
fn latest_only_credits_retain_inflight_bytes_and_restore_only_retired_snapshots() {
    let mut pool = Pool::new();
    let publish = |pool: &mut Pool, stream, snapshot| {
        pool.publish(stream, EPOCH, snapshot, 1, SNAPSHOT, |bytes| {
            bytes.fill(snapshot as u8);
        })
    };
    publish(&mut pool, 0, 1).unwrap();
    publish(&mut pool, 1, 1).unwrap();
    let doc = pool.start_write(0).unwrap();
    let reads = pool.start_write(1).unwrap();
    assert_eq!(pool.slots[doc].credit, Credit::Writing(0));
    publish(&mut pool, 0, 2).unwrap();
    publish(&mut pool, 1, 2).unwrap();
    for revision in 3..100 {
        publish(&mut pool, 0, revision).unwrap();
    }
    assert_eq!(
        pool.slots
            .iter()
            .filter(|s| s.credit != Credit::Free)
            .count(),
        4
    );
    assert!(pool.slots[doc].bytes.iter().all(|b| *b == 1));
    assert!(pool.restore().is_err());
    pool.complete_write(doc, 0).unwrap();
    pool.complete_write(reads, 1).unwrap();
    pool.restore().unwrap();
    assert_eq!(
        pool.slots
            .iter()
            .filter(|s| matches!(s.credit, Credit::Latest(_)))
            .count(),
        2
    );
    assert_eq!(
        pool.slots
            .iter()
            .filter(|s| s.credit == Credit::Free)
            .count(),
        2
    );
    let latest = pool.start_write(0).unwrap();
    assert_eq!(pool.slots[latest].snapshot, 99);
    assert!(pool.slots[latest].bytes.iter().all(|b| *b == 99));
    assert!(publish(&mut pool, 0, 99).is_err());
}

fn deliver(
    replica: &mut Replica,
    bytes: &[u8],
    kind: Kind,
    epoch: Epoch,
    snapshot: u64,
    now: Instant,
    chunk: usize,
) -> Result<()> {
    for offset in (0..bytes.len()).step_by(chunk) {
        let payload = &bytes[offset..(offset + chunk).min(bytes.len())];
        let header = Header {
            kind,
            bytes: HEADER + payload.len(),
            total: bytes.len(),
            identity: ID,
            sequence: 1,
            epoch,
            snapshot,
            offset,
            source: 1,
        };
        let complete = replica.chunk(header, payload, now)?;
        assert_eq!(complete, offset + payload.len() == bytes.len());
        assert_eq!(
            replica
                .pool()
                .slots
                .iter()
                .filter(|s| matches!(s.credit, Credit::Assembly(_)))
                .count(),
            usize::from(!complete)
        );
    }
    Ok(())
}
#[test]
fn maximum_canonical_document_fragmentation_atomic_validation_and_epoch_fences() {
    let doc = maximum_document();
    assert_eq!(doc.canonical_bytes().len(), SNAPSHOT);
    let mut replica = Replica::new();
    let now = Instant::now();
    deliver(
        &mut replica,
        doc.canonical_bytes(),
        Kind::Document,
        EPOCH,
        1,
        now,
        CHUNK - HEADER,
    )
    .unwrap();
    assert!(replica.admission_peak() <= 4 * SNAPSHOT);
    println!(
        "DOCUMENT_LEDGER canonical_bytes={SNAPSHOT} parsed_accounted_peak={} encoded_slots={} io_diagnostic_ceiling={ENCODED_STORAGE}",
        replica.admission_peak(),
        SLOTS * SNAPSHOT
    );
    assert_eq!(replica.accepted(0).unwrap().snapshot, 1);
    let accepted = replica
        .pool()
        .slots
        .iter()
        .position(|s| s.credit == Credit::Current(0))
        .unwrap();
    assert_eq!(
        &replica.pool().slots[accepted].bytes[..SNAPSHOT],
        doc.canonical_bytes()
    );
    let epoch = Epoch {
        revision: 2,
        ..EPOCH
    };
    assert!(deliver(&mut replica, b"{}", Kind::Document, epoch, 2, now, 1).is_err());
    assert_eq!(replica.accepted(0).unwrap().epoch, EPOCH);
    assert_eq!(
        &replica.pool().slots[accepted].bytes[..SNAPSHOT],
        doc.canonical_bytes()
    );
    assert!(replica.expire(now + Duration::from_secs(2)).is_err());
    let mut fresh = Replica::new();
    deliver(
        &mut fresh,
        doc.canonical_bytes(),
        Kind::Document,
        EPOCH,
        1,
        now,
        CHUNK - HEADER,
    )
    .unwrap();
    assert!(
        deliver(
            &mut fresh,
            doc.canonical_bytes(),
            Kind::Document,
            EPOCH,
            2,
            now,
            CHUNK - HEADER
        )
        .is_err()
    );
    assert!(
        deliver(
            &mut fresh,
            b"LRD1\0\0",
            Kind::Observations,
            epoch,
            2,
            now,
            16
        )
        .is_err()
    );
}
#[test]
fn selected_codec_maximum_profile_finite_values_missing_and_stale_are_preserved() {
    let (mut observations, references) = maximum_reads();
    let mut bytes = vec![0; SNAPSHOT];
    let length = reads::encode(&mut bytes, &observations, &selected(&references)).unwrap();
    reads::validate(&bytes[..length]).unwrap();
    assert!(length < SNAPSHOT);
    let fresh = bytes[..length].to_vec();
    observations.mark_stale();
    let stale_length = reads::encode(&mut bytes, &observations, &selected(&references)).unwrap();
    reads::validate(&bytes[..stale_length]).unwrap();
    assert_eq!(bytes[5], 3);
    assert_eq!(fresh[5], 2);
    assert_ne!(&bytes[..stale_length], &fresh);
    let missing = RuntimeRef::Reference {
        reference: "missing".into(),
    };
    let length = reads::encode(
        &mut bytes,
        &observations,
        &[reads::Selection {
            target: &missing,
            origin: None,
        }],
    )
    .unwrap();
    reads::validate(&bytes[..length]).unwrap();
    observations
        .live
        .get_mut(&references[0])
        .unwrap()
        .push(LivePoint {
            time_seconds: 1000.,
            value: 1.,
        })
        .unwrap();
    assert!(reads::encode(&mut bytes, &observations, &selected(&references)).is_err());
    let too_many: Vec<_> = (0..TRACES + 1)
        .map(|_| reads::Selection {
            target: &missing,
            origin: None,
        })
        .collect();
    assert!(reads::encode(&mut bytes, &observations, &too_many).is_err());
    assert!(reads::validate(&vec![0; SNAPSHOT + 1]).is_err());
    assert!(
        reads::encode(
            &mut bytes,
            &observations,
            &[reads::Selection {
                target: &references[0],
                origin: None
            }]
        )
        .is_err()
    );
    for end in [0, 1, 5, fresh.len() - 1] {
        assert!(reads::validate(&fresh[..end]).is_err());
    }
    let mut malformed = fresh;
    malformed[5] = 3;
    assert!(
        reads::validate(&malformed).is_err(),
        "Fresh entry under stale global fence"
    );
}

#[test]
fn parent_fence_timeout_and_heartbeat_never_promote_a_cached_read() {
    let doc = maximum_document();
    let now = Instant::now();
    let mut replica = Replica::new();
    replica.fence(1, 1, now).unwrap();
    deliver(
        &mut replica,
        doc.canonical_bytes(),
        Kind::Document,
        EPOCH,
        1,
        now,
        CHUNK - HEADER,
    )
    .unwrap();
    let (mut observations, references) = maximum_reads();
    let mut bytes = vec![0; SNAPSHOT];
    let len = reads::encode(&mut bytes, &observations, &selected(&references)).unwrap();
    deliver(
        &mut replica,
        &bytes[..len],
        Kind::Observations,
        EPOCH,
        2,
        now,
        CHUNK - HEADER,
    )
    .unwrap();
    assert!(replica.read_is_current(now));
    let late = now + Duration::from_secs(2);
    assert!(!replica.read_is_current(late));
    replica.parent_progress(late);
    replica.fence(1, 1, late).unwrap();
    assert!(!replica.read_is_current(late));
    deliver(
        &mut replica,
        &bytes[..len],
        Kind::Observations,
        EPOCH,
        3,
        late,
        CHUNK - HEADER,
    )
    .unwrap();
    assert!(replica.read_is_current(late));
    replica.fence(1, 2, late).unwrap();
    assert!(!replica.read_is_current(late));
    deliver(
        &mut replica,
        &bytes[..len],
        Kind::Observations,
        EPOCH,
        4,
        late,
        CHUNK - HEADER,
    )
    .unwrap();
    assert_eq!(
        replica.accepted(1).unwrap().snapshot,
        3,
        "complete obsolete snapshot was coalesced"
    );
    observations.mark_stale();
    let len = reads::encode(&mut bytes, &observations, &selected(&references)).unwrap();
    let header = Header {
        kind: Kind::Observations,
        bytes: HEADER + len.min(CHUNK - HEADER),
        total: len,
        identity: ID,
        sequence: 1,
        epoch: EPOCH,
        snapshot: 5,
        offset: 0,
        source: 2,
    };
    for offset in (0..len).step_by(CHUNK - HEADER) {
        let count = (len - offset).min(CHUNK - HEADER);
        replica
            .chunk(
                Header {
                    bytes: HEADER + count,
                    offset,
                    ..header
                },
                &bytes[offset..offset + count],
                late,
            )
            .unwrap();
    }
    assert!(replica.read_is_current(late));
    assert!(
        reads::inspect(&bytes[..len])
            .unwrap()
            .iter()
            .flatten()
            .all(|m| m.freshness != 2)
    );
}

#[test]
fn private_boundary_contains_no_child_application_or_gpu_calls_and_claims_one_worker() {
    for source in [
        include_str!("fixture.rs"),
        include_str!("replica.rs"),
        include_str!("reads.rs"),
        include_str!("transport.rs"),
    ] {
        for forbidden in [
            "ClientHandle::",
            ".operation_status(",
            ".exact_retry(",
            "OutputAuthority::",
            "Recorder::",
            "eframe::run_native",
            "HostSession::",
            "Preview::",
            "TcpListener::",
        ] {
            assert!(!source.contains(forbidden), "child boundary: {forbidden}");
        }
    }
    assert!(!include_str!("backend.rs").contains("thread::spawn"));
    let bridge = super::backend::Bridge::new([31; 16]);
    let factory = bridge.worker_factory().unwrap();
    assert!(bridge.worker_factory().is_err());
    drop(factory);
    let factory = bridge.worker_factory().unwrap();
    let backend = factory();
    assert!(bridge.worker_factory().is_err());
    drop(backend);
    assert!(bridge.worker_factory().is_err());
}
#[test]
fn selected_scalar_kinds_units_exact_integer_and_nonfinite_wire_rejection() {
    use serde_json::json;
    let target = RuntimeRef::Signal {
        instrument: "1".into(),
        parameter: "1".into(),
    };
    for (kind, value) in [
        ("boolean", json!(false)),
        ("integer", json!(i64::MIN.to_string())),
        ("float", json!(-0.0)),
        ("text", json!("ж".repeat(256))),
        ("enum", json!("standby")),
    ] {
        let mut observations = RuntimeObservations::default();
        observations.observe_read_descriptor(
            target.clone(),
            &json!({"value_type":kind,"generation":"1","unit":{"id":"u","symbol":"U"}}),
        );
        observations.observe(target.clone(), json!({"signal":{"instrument":"1","parameter":"1"},"generation":"1","value":value,"quality":"good","status":"available","observed_at_ns":"2","source_at_ns":"1","unit":{"id":"u","symbol":"U"}}),Some(2));
        observations.freshness = Freshness::Fresh;
        let selections = selected(std::slice::from_ref(&target));
        let plan = reads::plan(&observations, &selections).unwrap();
        let mut bytes = vec![0; plan.length()];
        plan.fill(&mut bytes);
        reads::validate(&bytes).unwrap();
        let meta = reads::inspect(&bytes).unwrap()[0].unwrap();
        assert_eq!(
            (meta.generation, meta.source_at, meta.revision),
            (Some(1), Some(1), Some(20))
        );
        if kind == "integer" {
            assert!(bytes.windows(8).any(|b| b == i64::MIN.to_le_bytes()));
        }
        if kind == "float" {
            let position = bytes
                .windows(8)
                .position(|b| b == (-0.0f64).to_le_bytes())
                .unwrap();
            bytes[position..position + 8].copy_from_slice(&f64::NAN.to_le_bytes());
            assert!(reads::validate(&bytes).is_err());
        }
    }
}

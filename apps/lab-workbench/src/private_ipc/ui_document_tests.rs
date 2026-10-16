//! Stock UI admission preserves the original credits, parser and epoch fences.
use super::{protocol::*, replica::Replica};
use crate::presentation::v2;
use std::time::Instant;
fn bytes(name: &str) -> Vec<u8> {
    let source = serde_json::json!({"format_version":2,"binding_version":1,"document_id":name,"windows":[],"components":[],"data_bindings":[],"editor_bindings":[],"action_bindings":[],"plots":[]});
    v2::admit(&serde_json::to_vec(&source).unwrap())
        .unwrap()
        .canonical_bytes()
        .to_vec()
}
fn deliver(replica: &mut Replica, bytes: &[u8], revision: u64) {
    replica
        .chunk(
            Header {
                kind: Kind::Document,
                bytes: HEADER + bytes.len(),
                total: bytes.len(),
                identity: Identity {
                    workbench: [1; 16],
                    incarnation: 1,
                },
                sequence: revision,
                epoch: Epoch {
                    owner: [1; 16],
                    revision,
                },
                snapshot: revision,
                offset: 0,
                source: 0,
            },
            bytes,
            Instant::now(),
        )
        .unwrap();
}
#[test]
fn stock_ui_replacement_uses_one_parsed_document_and_no_early_applied() {
    let old = bytes("old");
    let new = bytes("new");
    v2::document_lifetimes(true);
    let (mut replica, mut ui) = Replica::stock_ui();
    deliver(&mut replica, &old, 1);
    assert!(ui.document().is_none());
    assert!(replica.accepted(0).is_none());
    assert!(replica.admit_ui_document(&mut ui).unwrap().applied);
    deliver(&mut replica, &new, 2);
    assert_eq!(ui.document().unwrap().dto().document_id.as_str(), "old");
    assert_eq!(replica.accepted(0).unwrap().epoch.revision, 1);
    assert!(replica.admit_ui_document(&mut ui).unwrap().applied);
    assert_eq!(ui.document().unwrap().dto().document_id.as_str(), "new");
    let count = v2::document_lifetimes(false);
    assert_eq!(
        (count.live, count.peak, count.admission_calls, count.dropped),
        (1, 1, 2, 1)
    );
    assert_eq!(
        replica
            .pool()
            .slots
            .iter()
            .filter(|s| s.credit == Credit::Current(0))
            .count(),
        1
    );
    assert!(replica.admit_ui_document(&mut ui).is_err());
    drop(ui);
    assert_eq!(v2::document_lifetimes(false).live, 0);
}
#[test]
fn stock_ui_foreign_owner_and_superseded_candidate_cannot_publish() {
    let canonical = bytes("candidate");
    let (mut replica, mut ui) = Replica::stock_ui();
    let (_, mut foreign) = Replica::stock_ui();
    deliver(&mut replica, &canonical, 1);
    assert!(replica.admit_ui_document(&mut foreign).is_err());
    assert!(replica.document_candidate().is_some());
    replica.fence(2, 0, Instant::now()).unwrap();
    assert!(!replica.admit_ui_document(&mut ui).unwrap().applied);
    assert!(ui.document().is_none());
    assert!(replica.accepted(0).is_none());
    assert!(
        replica
            .pool()
            .slots
            .iter()
            .all(|s| s.credit == Credit::Free)
    );
}
#[test]
fn stock_ui_malformed_or_stopped_candidate_has_no_admission_receipt() {
    let (mut replica, mut ui) = Replica::stock_ui();
    deliver(&mut replica, b"invalid", 1);
    assert!(replica.admit_ui_document(&mut ui).is_err());
    assert!(ui.document().is_none());
    assert!(replica.accepted(0).is_none());
    assert!(
        replica
            .pool()
            .slots
            .iter()
            .all(|s| s.credit == Credit::Free)
    );
    let canonical = bytes("stopped");
    deliver(&mut replica, &canonical, 2);
    replica.stop();
    assert!(replica.admit_ui_document(&mut ui).is_err());
    assert!(replica.accepted(0).is_none());
}
#[test]
fn window_close_is_a_bounded_private_control_message() {
    let id = Identity {
        workbench: [3; 16],
        incarnation: 7,
    };
    let header = Header::control(Kind::WindowClose, id, 1);
    let mut encoded = [0; HEADER];
    header.encode(&mut encoded).unwrap();
    assert_eq!(Header::decode(&encoded, id, false).unwrap(), header);
    assert!(Header::decode(&encoded, id, true).is_err());
    assert!(
        Header::decode(
            &encoded,
            Identity {
                incarnation: 8,
                ..id
            },
            false
        )
        .is_err()
    );
}

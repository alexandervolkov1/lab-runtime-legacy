//! Deterministic no-GPU consumer harness. Actual Preview/session ownership and
//! actual admitted-value constructors/destructors are observed at each boundary.
use super::{protocol::*, replica::Replica};
use crate::{
    gui::readonly_v2::{
        PresentationEpoch, Preview,
        completion::*,
        deletion::DeletionResult,
        frame::{ContextGeneration, FrameError},
        host_session::{CleanupOutcome, HostSession},
        texture_owner::{FrameBackend, TextureKey},
    },
    presentation::v2,
};
use eframe::egui;
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::gui::readonly_v2::plot_fixtures as fixtures;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct State {
    pending: bool,
    proof: bool,
    submissions: usize,
    uploads: usize,
    deletions: usize,
}
struct Mock(Rc<Cell<State>>);
impl FrameBackend for Mock {
    const CPU_ONLY: bool = false;
    fn set(
        &mut self,
        _: TextureKey,
        _: &egui::epaint::ImageDelta,
    ) -> std::result::Result<(), FrameError> {
        let mut state = self.0.get();
        state.uploads += 1;
        state.pending = true;
        self.0.set(state);
        Ok(())
    }
    fn platform(
        &mut self,
        _: &egui::PlatformOutput,
        _: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> std::result::Result<(), FrameError> {
        Ok(())
    }
    fn submit(
        &mut self,
        _: ContextGeneration,
        _: &[egui::ClippedPrimitive],
        _: f32,
    ) -> std::result::Result<(), FrameError> {
        let mut state = self.0.get();
        state.submissions += 1;
        state.pending = true;
        self.0.set(state);
        Ok(())
    }
    fn wait(&mut self, _: WaitRequest) -> Completion {
        let mut state = self.0.get();
        if state.proof {
            state.pending = false;
            self.0.set(state);
            Completion::Quiesced
        } else {
            Completion::Timeout
        }
    }
    fn release_frame(&mut self, _: ContextGeneration) -> DeletionResult {
        assert!(!self.0.get().pending);
        Ok(())
    }
    fn free(&mut self, _: TextureKey) -> DeletionResult {
        let mut state = self.0.get();
        assert!(!state.pending);
        state.deletions += 1;
        self.0.set(state);
        Ok(())
    }
    fn release_generation(&mut self, _: ContextGeneration) -> DeletionResult {
        assert!(!self.0.get().pending);
        Ok(())
    }
}
fn epoch(revision: u64) -> Epoch {
    Epoch {
        owner: [7; 16],
        revision,
    }
}
fn display(epoch: Epoch) -> PresentationEpoch {
    PresentationEpoch {
        owner: epoch.owner,
        revision: epoch.revision,
    }
}
fn bytes(label: &str) -> Vec<u8> {
    let mut syntax = fixtures::syntax(1, false);
    syntax["windows"][0]["title"] = serde_json::json!(label);
    fixtures::admit(&syntax).canonical_bytes().to_vec()
}
fn deliver(
    replica: &mut Replica,
    bytes: &[u8],
    revision: u64,
    snapshot: u64,
    now: Instant,
) -> Result<()> {
    for (index, payload) in bytes.chunks(CHUNK - HEADER).enumerate() {
        let header = Header {
            kind: Kind::Document,
            bytes: HEADER + payload.len(),
            total: bytes.len(),
            identity: Identity {
                workbench: [8; 16],
                incarnation: 1,
            },
            sequence: 1,
            epoch: epoch(revision),
            snapshot,
            offset: index * (CHUNK - HEADER),
            source: 1,
        };
        let complete = replica.chunk(header, payload, now)?;
        assert_eq!(complete, header.offset + payload.len() == bytes.len());
    }
    Ok(())
}
fn credits(replica: &Replica, current: usize, assembly: usize) {
    let pool = replica.pool();
    assert_eq!(pool.slots.len(), SLOTS);
    assert_eq!(
        pool.slots.iter().map(|s| s.bytes.len()).sum::<usize>(),
        SLOTS * SNAPSHOT
    );
    assert_eq!(
        pool.slots
            .iter()
            .filter(|s| matches!(s.credit, Credit::Current(_)))
            .count(),
        current
    );
    assert_eq!(
        pool.slots
            .iter()
            .filter(|s| matches!(s.credit, Credit::Assembly(_)))
            .count(),
        assembly
    );
}
fn counts(live: usize, calls: usize, constructed: usize, dropped: usize) {
    let counts = v2::document_lifetimes(false);
    assert_eq!(
        (
            counts.live,
            counts.admission_calls,
            counts.constructed,
            counts.dropped
        ),
        (live, calls, constructed, dropped)
    );
    assert!(
        counts.peak <= 1,
        "two actual admitted documents overlap: {counts:?}"
    );
}
fn one_wait() -> WaitPolicy {
    WaitPolicy::new(1, Duration::ZERO, Duration::from_millis(100))
}

#[test]
fn replacement_waits_for_exact_session_and_old_document_drop_before_admission() {
    let old = bytes("old");
    let new = bytes("new");
    v2::document_lifetimes(true);
    let state = Rc::new(Cell::new(State::default()));
    let mut session = HostSession::new(Mock(state.clone()));
    let (mut replica, mut owner) = Replica::renderer(&mut session);
    let now = Instant::now();
    deliver(&mut replica, &old, 1, 1, now).unwrap();
    counts(0, 0, 0, 0);
    credits(&replica, 0, 1);
    assert!(replica.admit_renderer_document(&mut owner).unwrap().applied);
    counts(1, 1, 1, 0);
    let (document, identity, host) = owner.parts().unwrap();
    let mut preview = Preview::with_session(document, display(identity.epoch), host);
    let mut frame = preview
        .run(
            &crate::model::RuntimeObservations::default(),
            Default::default(),
        )
        .unwrap();
    frame.tessellate().unwrap();
    let original_generation = frame.generation();
    // Full Assembly arrives while the old Preview and exclusive frame are live.
    deliver(&mut replica, &new, 2, 2, now).unwrap();
    assert_eq!(replica.document_candidate().unwrap().epoch, epoch(2));
    assert_eq!(replica.accepted(0).unwrap().epoch, epoch(1));
    counts(1, 1, 1, 0);
    credits(&replica, 1, 1);
    assert_eq!(frame.finish(), Err(FrameError::Completion));
    let original = preview.retire_context();
    assert!(
        matches!(original,CleanupOutcome::Pending(status) if status.generation==original_generation)
    );
    let pointer = replica.candidate_bytes().unwrap().as_ptr();
    for tick in 1..=8 {
        let time = now + Duration::from_secs(tick);
        replica.parent_progress(time);
        replica.fence(2, 1, time).unwrap();
        assert!(!replica.read_is_current(time));
        replica.expire(time).unwrap(); // Complete Assembly is not an incomplete transfer.
        assert!(matches!(
            preview.poll_retirement(one_wait()),
            CleanupOutcome::Pending(_)
        ));
        assert!(matches!(
            preview.run(
                &crate::model::RuntimeObservations::default(),
                Default::default()
            ),
            Err(FrameError::RetirementPending)
        ));
        assert_eq!(replica.candidate_bytes().unwrap().as_ptr(), pointer);
        assert_eq!(state.get().deletions, 0);
        counts(1, 1, 1, 0);
        credits(&replica, 1, 1);
    }
    let before = state.get();
    state.set(State {
        proof: true,
        ..before
    });
    assert!(matches!(
        preview.poll_retirement(one_wait()),
        CleanupOutcome::Retired(_)
    ));
    drop(preview);
    assert!(!owner.session().retained_context() && !owner.session().retained_cpu_frame());
    assert_eq!(owner.session().owned_keys(), 0);
    counts(1, 1, 1, 0);
    assert!(replica.admit_renderer_document(&mut owner).is_err());
    counts(1, 1, 1, 0);
    owner.release_retired().unwrap();
    counts(0, 1, 1, 1);
    let decision = replica.admit_renderer_document(&mut owner).unwrap();
    assert!(decision.applied);
    assert_eq!(decision.header.epoch, epoch(2));
    counts(1, 2, 2, 1);
    credits(&replica, 1, 0);
    assert!(replica.admit_renderer_document(&mut owner).is_err());
    let (document, identity, host) = owner.parts().unwrap();
    let mut next = Preview::with_session(document, display(identity.epoch), host);
    let mut frame = next
        .run(
            &crate::model::RuntimeObservations::default(),
            Default::default(),
        )
        .unwrap();
    assert_ne!(frame.generation(), original_generation);
    frame.tessellate().unwrap();
    frame.finish().unwrap();
    assert!(matches!(next.retire_context(), CleanupOutcome::Retired(_)));
    drop(next);
    owner.release_retired().unwrap();
    counts(0, 2, 2, 2);
    println!(
        "HANDOFF actual_admitted_peak=1 calls=2 constructed=2 dropped=2 slots=4 pending_control_turns=8 no_terminal_shutdown=true"
    );
}

#[test]
fn malformed_candidate_preserves_accepted_bytes_and_can_only_restore_sequentially() {
    let old = bytes("accepted");
    v2::document_lifetimes(true);
    let mut session = HostSession::new(Mock(Rc::new(Cell::new(State::default()))));
    let (mut replica, mut owner) = Replica::renderer(&mut session);
    let now = Instant::now();
    deliver(&mut replica, &old, 1, 1, now).unwrap();
    assert!(replica.admit_renderer_document(&mut owner).unwrap().applied);
    deliver(&mut replica, b"{}", 2, 2, now).unwrap();
    counts(1, 1, 1, 0);
    assert!(replica.admit_renderer_document(&mut owner).is_err());
    counts(1, 1, 1, 0);
    owner.release_retired().unwrap();
    counts(0, 1, 1, 1);
    assert_eq!(
        replica.admit_renderer_document(&mut owner).unwrap_err(),
        "canonical document admission"
    );
    assert_eq!(replica.snapshot(0).unwrap().1, &old);
    assert_eq!(replica.accepted(0).unwrap().epoch, epoch(1));
    assert!(replica.document_candidate().is_none());
    credits(&replica, 1, 0);
    counts(0, 2, 1, 1);
    assert!(deliver(&mut replica, b"{}", 2, 2, now).is_err());
    assert!(deliver(&mut replica, b"{}", 2, 3, now).is_err());
    counts(0, 2, 1, 1);
    replica.restore_renderer_document(&mut owner).unwrap();
    counts(1, 3, 2, 1);
    assert!(!replica.read_is_current(now));
    owner.release_retired().unwrap();
    replica.fence(2, 1, now).unwrap();
    assert!(replica.restore_renderer_document(&mut owner).is_err());
    counts(0, 3, 2, 2);
}

#[test]
fn pending_drop_stop_and_cancellation_keep_original_payload_document_and_assembly() {
    let old = bytes("old");
    let new = bytes("new");
    v2::document_lifetimes(true);
    let state = Rc::new(Cell::new(State::default()));
    let mut session = HostSession::new(Mock(state.clone()));
    let (mut replica, mut owner) = Replica::renderer(&mut session);
    let now = Instant::now();
    deliver(&mut replica, &old, 1, 1, now).unwrap();
    assert!(replica.admit_renderer_document(&mut owner).unwrap().applied);
    let (document, identity, host) = owner.parts().unwrap();
    let mut preview = Preview::with_session(document, display(identity.epoch), host);
    let mut frame = preview
        .run(
            &crate::model::RuntimeObservations::default(),
            Default::default(),
        )
        .unwrap();
    frame.tessellate().unwrap();
    assert_eq!(frame.finish(), Err(FrameError::Completion));
    drop(preview);
    deliver(&mut replica, &new, 2, 2, now).unwrap();
    let pointer = replica.candidate_bytes().unwrap().as_ptr();
    let original = owner.session().status();
    let before = state.get();
    for _ in 0..3 {
        replica.stop();
        assert!(owner.release_retired().is_err());
        assert!(replica.admit_renderer_document(&mut owner).is_err());
        assert_eq!(owner.session().status(), original);
        assert!(owner.session().retained_context() && owner.session().retained_cpu_frame());
        assert_eq!(state.get(), before);
        assert_eq!(replica.candidate_bytes().unwrap().as_ptr(), pointer);
        assert_eq!(owner.document().unwrap().canonical_bytes(), old);
        counts(1, 1, 1, 0);
        credits(&replica, 1, 1);
    }
    // Harness supplies a later mock completion proof, never inferred from stop,
    // crash notification, cancellation request, or a timeout.
    state.set(State {
        proof: true,
        ..before
    });
    owner.session_mut().poll_retirement(one_wait());
    owner.release_retired().unwrap();
    counts(0, 1, 1, 1);
    assert!(replica.admit_renderer_document(&mut owner).is_err());
    assert_eq!(replica.candidate_bytes().unwrap().as_ptr(), pointer);
    credits(&replica, 1, 1);
}

#[test]
fn no_gpu_consumer_keeps_zero_persistent_documents_and_rejects_duplicate_or_interleaved_candidates()
{
    let old = bytes("old");
    let next = bytes("next");
    v2::document_lifetimes(true);
    let now = Instant::now();
    let mut replica = Replica::new();
    deliver(&mut replica, &old, 1, 1, now).unwrap();
    counts(0, 0, 0, 0);
    assert!(deliver(&mut replica, &next, 2, 2, now).is_err());
    counts(0, 0, 0, 0);
    credits(&replica, 0, 1);
    assert!(replica.admit_encoded_document().unwrap().applied);
    counts(0, 1, 1, 1);
    credits(&replica, 1, 0);
    assert!(deliver(&mut replica, &old, 1, 2, now).is_err());
    counts(0, 1, 1, 1);
    deliver(&mut replica, &next, 2, 3, now).unwrap();
    replica.fence(3, 1, now).unwrap();
    assert!(!replica.admit_encoded_document().unwrap().applied);
    assert_eq!(replica.accepted(0).unwrap().epoch, epoch(1));
    counts(0, 2, 2, 2);
    credits(&replica, 1, 0);
    println!("NO_GPU persistent_parsed_documents=0 actual_admitted_peak=1 encoded_slots=4");
}

#[test]
fn noncanonical_candidate_drops_its_only_admission_before_old_encoded_restoration() {
    let old = bytes("old");
    let invalid = [b" ".as_slice(), old.as_slice()].concat();
    v2::document_lifetimes(true);
    let mut session = HostSession::new(Mock(Rc::new(Cell::new(State::default()))));
    let (mut replica, mut owner) = Replica::renderer(&mut session);
    let now = Instant::now();
    deliver(&mut replica, &old, 1, 1, now).unwrap();
    assert!(replica.admit_renderer_document(&mut owner).unwrap().applied);
    deliver(&mut replica, &invalid, 2, 2, now).unwrap();
    owner.release_retired().unwrap();
    counts(0, 1, 1, 1);
    assert_eq!(
        replica.admit_renderer_document(&mut owner).unwrap_err(),
        "noncanonical document bytes"
    );
    counts(0, 2, 2, 2);
    assert_eq!(replica.snapshot(0).unwrap().1, old);
    replica.restore_renderer_document(&mut owner).unwrap();
    counts(1, 3, 3, 2);
    owner.release_retired().unwrap();
    counts(0, 3, 3, 3);
}

#[test]
fn unpaired_owner_and_encoded_only_bypass_cannot_admit_renderer_candidate() {
    let old = bytes("old");
    v2::document_lifetimes(true);
    let mut first = HostSession::new(Mock(Rc::new(Cell::new(State::default()))));
    let mut second = HostSession::new(Mock(Rc::new(Cell::new(State::default()))));
    let (mut replica, mut owner) = Replica::renderer(&mut first);
    let (_other, mut wrong) = Replica::renderer(&mut second);
    deliver(&mut replica, &old, 1, 1, Instant::now()).unwrap();
    assert!(replica.admit_renderer_document(&mut wrong).is_err());
    assert!(replica.admit_encoded_document().is_err());
    counts(0, 0, 0, 0);
    credits(&replica, 0, 1);
    assert!(replica.admit_renderer_document(&mut owner).unwrap().applied);
    counts(1, 1, 1, 0);
    owner.release_retired().unwrap();
    counts(0, 1, 1, 1);
}

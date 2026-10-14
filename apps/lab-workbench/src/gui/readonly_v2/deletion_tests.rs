//! Deterministic deletion faults, independently tracked native-like obligations.
//! These tests simulate failures; they do not qualify hardware device loss.
use super::{completion::*, deletion::*, frame::*, host_session::*, texture_owner::*, *};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Default)]
struct Ledger {
    objects: [Option<ObjectIdentity>; 15],
    calls: [u8; 15],
    failure: Option<(usize, DeleteDisposition)>,
    latched: Option<DeletionFailure>,
    execution: bool,
    wait: Option<Completion>,
    submissions: usize,
    waits: usize,
}
struct Backend(Rc<RefCell<Ledger>>);
impl Backend {
    fn delete(&mut self, slot: usize) -> DeletionResult {
        let mut ledger = self.0.borrow_mut();
        if let Some(error) = ledger.latched {
            return Err(error);
        }
        let Some(object) = ledger.objects[slot] else {
            return Ok(());
        };
        let disposition = ledger.failure.filter(|f| f.0 == slot).map(|f| f.1);
        if disposition != Some(DeleteDisposition::NotStarted) {
            ledger.calls[slot] += 1;
        }
        if let Some(disposition) = disposition {
            let error = DeletionFailure {
                object,
                disposition,
            };
            ledger.latched = Some(error);
            return Err(error);
        }
        ledger.objects[slot] = None;
        Ok(())
    }
}
impl FrameBackend for Backend {
    const CPU_ONLY: bool = false;
    fn set(&mut self, key: TextureKey, _: &egui::epaint::ImageDelta) -> Result<(), FrameError> {
        let mut ledger = self.0.borrow_mut();
        let slot = ledger.objects[..8]
            .iter()
            .position(|o| o.is_some_and(|o| o.texture == Some(key)))
            .or_else(|| ledger.objects[..8].iter().position(Option::is_none))
            .unwrap();
        ledger.objects[slot] = Some(ObjectIdentity {
            generation: key.generation,
            kind: ObjectKind::Texture,
            name: slot as u64 + 1,
            texture: Some(key),
        });
        Ok(())
    }
    fn platform(
        &mut self,
        _: &egui::PlatformOutput,
        _: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> Result<(), FrameError> {
        Ok(())
    }
    fn submit(
        &mut self,
        generation: ContextGeneration,
        _: &[egui::ClippedPrimitive],
        _: f32,
    ) -> Result<(), FrameError> {
        let mut ledger = self.0.borrow_mut();
        ledger.submissions += 1;
        for (slot, kind) in [
            (8, ObjectKind::VertexArray),
            (9, ObjectKind::Buffer),
            (10, ObjectKind::Buffer),
            (11, ObjectKind::Program),
            (12, ObjectKind::Shader),
            (13, ObjectKind::Shader),
            (14, ObjectKind::Fence),
        ] {
            ledger.objects[slot] = Some(ObjectIdentity {
                generation,
                kind,
                name: slot as u64 + 1,
                texture: None,
            });
        }
        if ledger.execution {
            Err(FrameError::Submission)
        } else {
            Ok(())
        }
    }
    fn wait(&mut self, _: WaitRequest) -> Completion {
        let mut ledger = self.0.borrow_mut();
        ledger.waits += 1;
        ledger.wait.unwrap_or(Completion::Quiesced)
    }
    fn release_frame(&mut self, _: ContextGeneration) -> DeletionResult {
        self.delete(14)
    }
    fn free(&mut self, key: TextureKey) -> DeletionResult {
        let slot = self.0.borrow().objects[..8]
            .iter()
            .position(|o| o.is_some_and(|o| o.texture == Some(key)))
            .expect("unknown/double-free key");
        self.delete(slot)
    }
    fn release_generation(&mut self, _: ContextGeneration) -> DeletionResult {
        for slot in 8..14 {
            self.delete(slot)?;
        }
        Ok(())
    }
}
fn policy() -> WaitPolicy {
    WaitPolicy::new(1, Duration::ZERO, Duration::from_millis(100))
}
fn pending(host: &HostSession<Backend>) -> RetirementStatus {
    let CleanupOutcome::Pending(status) = host.status() else {
        panic!("false retirement receipt");
    };
    status
}
fn populated() -> (
    HostSession<Backend>,
    Rc<RefCell<Ledger>>,
    ContextGeneration,
    egui::FullOutput,
) {
    let ledger = Rc::new(RefCell::new(Ledger::default()));
    let mut host = HostSession::new(Backend(ledger.clone()));
    let generation = ContextGeneration::allocate().unwrap();
    host.begin(generation).unwrap();
    let mut output = egui::FullOutput::default();
    for id in [0, 1] {
        output.textures_delta.push(
            egui::TextureId::Managed(id),
            egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
                Default::default(),
            ),
        );
    }
    host.textures.process(generation, &mut output, &[]).unwrap();
    (host, ledger, generation, output)
}

#[test]
fn every_object_deletion_failure_latches_exact_identity_and_never_retries() {
    // Two textures, VAO, both buffers, program, both shaders: distinct identities.
    for slot in [0, 1, 8, 9, 10, 11, 12, 13] {
        for disposition in [DeleteDisposition::NotStarted, DeleteDisposition::Ambiguous] {
            let (mut host, ledger, generation, output) = populated();
            let original = ledger.borrow().objects[slot].unwrap();
            ledger.borrow_mut().failure = Some((slot, disposition));
            host.retire(
                RetirementReason::FrameFailed,
                Some(FrameOutcome::Failed(FrameError::Submission)),
                RetirementPayload {
                    context: Some(egui::Context::default()),
                    frame: Some(FrameData::new(output, RenderReport::default(), generation)),
                },
            );
            let status = pending(&host);
            assert_eq!(
                status.deletion,
                Some(DeletionFailure {
                    object: original,
                    disposition
                })
            );
            assert_eq!(status.wait.completion, Completion::Quiesced);
            assert!(host.retained_context() && host.retained_cpu_frame());
            assert_eq!(ledger.borrow().objects[slot], Some(original));
            let calls = ledger.borrow().calls;
            let remaining = ledger.borrow().objects;
            for _ in 0..25 {
                assert_eq!(host.shutdown(policy()), ShutdownOutcome::Unresolved(status));
                assert_eq!(pending(&host), status);
                assert_eq!(
                    host.begin(ContextGeneration::allocate().unwrap()),
                    Err(FrameError::RetirementPending)
                );
                assert_eq!(ledger.borrow().objects, remaining);
                assert_eq!(ledger.borrow().calls, calls);
            }
            assert_eq!(
                calls[slot],
                u8::from(disposition == DeleteDisposition::Ambiguous)
            );
            assert!(calls.iter().all(|c| *c <= 1));
            drop(host); // Mock only: demonstrate Drop issues no second delete.
            assert_eq!(ledger.borrow().calls, calls);
        }
    }
}

#[test]
fn partial_texture_retirement_preserves_second_key_and_untouched_pipeline() {
    let (mut host, ledger, generation, _) = populated();
    let second = ledger.borrow().objects[1].unwrap();
    ledger.borrow_mut().failure = Some((1, DeleteDisposition::Ambiguous));
    assert!(matches!(
        host.shutdown(policy()),
        ShutdownOutcome::Unresolved(_)
    ));
    assert!(ledger.borrow().objects[0].is_none());
    assert_eq!(ledger.borrow().objects[1], Some(second));
    assert_eq!(host.owned_keys(), 1);
    assert!(ledger.borrow().objects[8..14].iter().all(Option::is_some));
    assert_eq!(ledger.borrow().calls[8..14], [0; 6]);
    assert_eq!(pending(&host).generation, generation);
}

#[test]
fn fence_deletion_failure_is_not_a_completion_failure_or_render_success() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(1, false));
    for disposition in [DeleteDisposition::NotStarted, DeleteDisposition::Ambiguous] {
        let ledger = Rc::new(RefCell::new(Ledger {
            failure: Some((14, disposition)),
            ..Default::default()
        }));
        let mut host = HostSession::new(Backend(ledger.clone()));
        let mut preview = Preview::with_session(&doc, plot_widget_tests::EPOCH, &mut host);
        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        frame.tessellate().unwrap();
        assert_eq!(frame.finish(), Err(FrameError::Deletion));
        drop(preview);
        let status = pending(&host);
        assert_eq!(
            status.frame,
            Some(FrameOutcome::Failed(FrameError::Deletion))
        );
        assert_eq!(status.wait.completion, Completion::Quiesced);
        assert_eq!(status.deletion.unwrap().object.kind, ObjectKind::Fence);
        assert_eq!(host.owned_keys(), 1);
        assert!(host.retained_cpu_frame() && host.retained_context());
        let before = ledger.borrow().calls;
        assert!(matches!(
            host.shutdown(policy()),
            ShutdownOutcome::Unresolved(_)
        ));
        assert_eq!(ledger.borrow().calls, before);
    }
}

#[test]
fn execution_error_survives_completion_and_subsequent_deletion_failure() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(1, false));
    let ledger = Rc::new(RefCell::new(Ledger {
        execution: true,
        failure: Some((12, DeleteDisposition::Ambiguous)),
        ..Default::default()
    }));
    let mut host = HostSession::new(Backend(ledger.clone()));
    let mut preview = Preview::with_session(&doc, plot_widget_tests::EPOCH, &mut host);
    let mut frame = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    frame.tessellate().unwrap();
    assert_eq!(frame.finish(), Err(FrameError::Submission));
    drop(preview);
    let status = pending(&host);
    assert_eq!(
        status.frame,
        Some(FrameOutcome::Failed(FrameError::Submission))
    );
    assert_eq!(status.wait.completion, Completion::Quiesced);
    assert_eq!(status.deletion.unwrap().object.kind, ObjectKind::Shader);
    assert_eq!(host.owned_keys(), 0); // Pipeline still owns the generation.
    assert!(host.retained_cpu_frame() && host.retained_context());
    assert!(matches!(
        host.shutdown(policy()),
        ShutdownOutcome::Unresolved(_)
    ));
}

#[test]
fn per_frame_free_failure_retains_original_texture_key() {
    let (mut host, ledger, generation, mut output) = populated();
    ledger.borrow_mut().failure = Some((0, DeleteDisposition::Ambiguous));
    output.textures_delta.free(egui::TextureId::Managed(0));
    assert_eq!(
        host.textures.process(generation, &mut output, &[]),
        Err(FrameError::Deletion)
    );
    assert_eq!(host.owned_keys(), 2);
    host.retire(
        RetirementReason::FrameFailed,
        Some(FrameOutcome::Failed(FrameError::Deletion)),
        RetirementPayload {
            context: None,
            frame: Some(FrameData::new(output, RenderReport::default(), generation)),
        },
    );
    assert_eq!(
        pending(&host).deletion.unwrap().object.texture,
        Some(TextureKey {
            generation,
            id: egui::TextureId::Managed(0)
        })
    );
    let calls = ledger.borrow().calls;
    assert!(matches!(
        host.shutdown(policy()),
        ShutdownOutcome::Unresolved(_)
    ));
    assert_eq!(ledger.borrow().calls, calls);
}

#[test]
fn timeout_requires_later_proof_and_all_object_receipts_before_released() {
    let doc = plot_fixtures::admit(&plot_fixtures::syntax(1, false));
    for completion in [
        Completion::Timeout,
        Completion::Failed(WaitFailure::WaitError),
        Completion::Failed(WaitFailure::MissingFence),
        Completion::Failed(WaitFailure::InvalidFence),
        Completion::DeviceLost,
    ] {
        let ledger = Rc::new(RefCell::new(Ledger {
            wait: Some(completion),
            ..Default::default()
        }));
        let mut host = HostSession::new(Backend(ledger.clone()));
        let mut preview = Preview::with_session(&doc, plot_widget_tests::EPOCH, &mut host);
        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        frame.tessellate().unwrap();
        assert_eq!(frame.finish(), Err(FrameError::Completion));
        drop(preview);
        assert!(matches!(
            host.shutdown(policy()),
            ShutdownOutcome::Unresolved(_)
        ));
        assert_eq!(ledger.borrow().calls, [0; 15]);
        assert!(host.retained_cpu_frame() && host.retained_context());
        if completion != Completion::Timeout {
            continue;
        } // No invented loss proof.
        ledger.borrow_mut().wait = Some(Completion::Quiesced);
        assert_eq!(host.shutdown(policy()), ShutdownOutcome::Released);
        assert!(ledger.borrow().objects.iter().all(Option::is_none));
        assert!(!host.retained_cpu_frame() && !host.retained_context());
        let before = (
            ledger.borrow().calls,
            ledger.borrow().submissions,
            ledger.borrow().waits,
        );
        assert_eq!(host.shutdown(policy()), ShutdownOutcome::Released);
        assert_eq!(
            (
                ledger.borrow().calls,
                ledger.borrow().submissions,
                ledger.borrow().waits
            ),
            before
        );
        assert!(before.0.iter().all(|c| *c <= 1));
    }
}

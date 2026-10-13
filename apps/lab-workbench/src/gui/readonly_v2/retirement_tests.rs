//! Deterministic ambiguous-work, retention and finite-wait regressions. No GPU.
use super::{
    completion::*,
    frame::*,
    frame_tests::{Event, Log, Mock},
    host_session::*,
    plot_fixtures as fixtures,
    plot_widget_tests::EPOCH,
    texture_owner::*,
    *,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

fn no_free(log: &Rc<RefCell<Log>>) {
    assert!(log.borrow().busy);
    assert!(
        !log.borrow()
            .events
            .iter()
            .any(|e| matches!(e, Event::Free(_)))
    );
}
fn pending(outcome: CleanupOutcome) -> RetirementStatus {
    let CleanupOutcome::Pending(status) = outcome else {
        panic!("expected pending retirement")
    };
    status
}
fn one_wait() -> WaitPolicy {
    WaitPolicy::new(1, Duration::from_millis(5), Duration::from_millis(100))
}

#[test]
fn every_unresolved_completion_retains_payload_after_preview_drop() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    for completion in [
        Completion::Timeout,
        Completion::Failed(WaitFailure::WaitError),
        Completion::Failed(WaitFailure::MissingFence),
        Completion::Failed(WaitFailure::InvalidFence),
        Completion::DeviceLost,
    ] {
        let log = Rc::new(RefCell::new(Log {
            frame_wait: Some(completion),
            retirement_wait: Some(completion),
            ..Default::default()
        }));
        let mut host = HostSession::new(Mock(log.clone()));
        let mut preview = Preview::with_session(&doc, EPOCH, &mut host);
        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        let generation = frame.generation();
        frame.tessellate().unwrap();
        assert_eq!(frame.finish(), Err(FrameError::Completion));
        let status = pending(preview.cleanup_outcome());
        assert_eq!(status.generation, generation);
        assert_eq!(
            status.frame,
            Some(FrameOutcome::Failed(FrameError::Completion))
        );
        assert_eq!(status.wait.completion, completion);
        assert_eq!(status.initial_wait.completion, completion);
        assert!(status.wait.attempts <= 20);
        no_free(&log);
        let before = log.borrow().events.len();
        drop(preview);
        assert_eq!(log.borrow().events.len(), before); // Drop never waits.
        assert_eq!(host.status(), CleanupOutcome::Pending(status));
        assert!(host.retained_cpu_frame() && host.retained_context());
        assert_eq!(host.owned_keys(), 1);
        assert!(matches!(
            host.shutdown(one_wait()),
            ShutdownOutcome::Unresolved(_)
        ));
        assert!(host.retained_cpu_frame() && host.retained_context());
        no_free(&log);
        // A separately supplied mock proof, not inferred from timeout or loss.
        log.borrow_mut().retirement_wait = Some(Completion::Quiesced);
        assert_eq!(host.shutdown(one_wait()), ShutdownOutcome::Released);
        let CleanupOutcome::Retired(retired) = host.status() else {
            panic!("missing retirement receipt")
        };
        assert_eq!(retired.initial_wait.completion, completion);
        assert_eq!(retired.wait.completion, Completion::Quiesced);
        assert_eq!(retired.frame, status.frame);
        assert_eq!(host.owned_keys(), 0);
        assert!(!host.retained_cpu_frame() && !host.retained_context());
        let before = log.borrow().events.len();
        assert_eq!(host.shutdown(one_wait()), ShutdownOutcome::Released);
        drop(host);
        assert_eq!(log.borrow().events.len(), before); // No double free/wait.
    }
}

#[test]
fn pending_blocks_frames_document_owner_revision_and_new_generation() {
    let doc = fixtures::admit(&fixtures::syntax(2, false));
    let other = fixtures::admit(&fixtures::syntax(2, false));
    let log = Rc::new(RefCell::new(Log {
        frame_wait: Some(Completion::Timeout),
        retirement_wait: Some(Completion::Timeout),
        ..Default::default()
    }));
    let mut host = HostSession::new(Mock(log.clone()));
    let mut preview = Preview::with_session(&doc, EPOCH, &mut host);
    let mut frame = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    let generation = frame.generation();
    frame.tessellate().unwrap();
    assert_eq!(frame.finish(), Err(FrameError::Completion));
    let original = pending(preview.cleanup_outcome());
    let before = log.borrow().events.len();
    for revision in 1..=25 {
        assert!(!preview.replace(
            &other,
            PresentationEpoch {
                owner: [9; 16],
                revision
            }
        ));
        assert!(!preview.open_plot(0));
        assert!(!preview.close_plot());
        assert!(!preview.navigate(preview.location()));
        assert!(matches!(
            preview.run(&Default::default(), Default::default()),
            Err(FrameError::RetirementPending)
        ));
        assert_eq!(preview.retire_context(), CleanupOutcome::Pending(original));
        assert!(std::ptr::eq(preview.document, &doc));
        assert_eq!(preview.epoch, EPOCH);
    }
    assert_eq!(log.borrow().events.len(), before);
    no_free(&log);
    for _ in 0..25 {
        let status = pending(preview.poll_retirement(one_wait()));
        assert_eq!(
            (status.generation, status.reason, status.frame),
            (original.generation, original.reason, original.frame)
        );
        assert_eq!(status.wait.completion, Completion::Timeout);
        assert_eq!(status.wait.attempts, 1);
    }
    no_free(&log);
    log.borrow_mut().retirement_wait = Some(Completion::Quiesced);
    assert!(matches!(
        preview.poll_retirement(one_wait()),
        CleanupOutcome::Retired(_)
    ));
    log.borrow_mut().frame_wait = Some(Completion::Quiesced);
    assert!(preview.replace(
        &other,
        PresentationEpoch {
            owner: [9; 16],
            revision: 26
        }
    ));
    let mut next = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    assert_ne!(next.generation(), generation);
    next.tessellate().unwrap();
    assert!(matches!(
        next.finish(),
        Ok(FrameOutcome::HostCompleted { .. })
    ));
    drop(preview);
    assert_eq!(host.shutdown(one_wait()), ShutdownOutcome::Released);
    let keys: Vec<_> = log
        .borrow()
        .events
        .iter()
        .filter_map(|event| {
            if let Event::Set(key, _) = event {
                Some(*key)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0].id, keys[1].id);
    assert_ne!(keys[0].generation, keys[1].generation);
}

#[test]
fn partial_upload_keeps_original_error_and_every_attempted_key() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    let log = Rc::new(RefCell::new(Log {
        fault: Some(FrameError::TextureUpdate),
        fail_set_on: 2,
        retirement_wait: Some(Completion::Failed(WaitFailure::WaitError)),
        ..Default::default()
    }));
    let mut host = HostSession::new(Mock(log.clone()));
    let mut preview = Preview::with_session(&doc, EPOCH, &mut host);
    let mut frame = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    frame.tessellate().unwrap();
    // Full font upload succeeds, the following partial upload starts but fails;
    // a third, distinct texture is never attempted and must not be registered.
    let delta = &mut frame
        .preview
        .pending
        .as_mut()
        .unwrap()
        .output
        .textures_delta;
    delta.push(
        egui::TextureId::Managed(0),
        egui::epaint::ImageDelta::partial(
            [0, 0],
            egui::ColorImage::filled([1, 1], egui::Color32::WHITE),
            Default::default(),
        ),
    );
    delta.push(
        egui::TextureId::Managed(1),
        egui::epaint::ImageDelta::full(
            egui::ColorImage::filled([1, 1], egui::Color32::WHITE),
            Default::default(),
        ),
    );
    assert_eq!(frame.finish(), Err(FrameError::TextureUpdate));
    let original = pending(preview.cleanup_outcome());
    assert_eq!(
        original.frame,
        Some(FrameOutcome::Failed(FrameError::TextureUpdate))
    );
    assert_eq!(
        original.wait.completion,
        Completion::Failed(WaitFailure::WaitError)
    );
    drop(preview);
    assert_eq!(host.owned_keys(), 1);
    assert_eq!(
        log.borrow()
            .events
            .iter()
            .filter(|e| matches!(e, Event::Set(_, _)))
            .count(),
        2
    );
    for result in [Completion::Timeout, Completion::DeviceLost] {
        log.borrow_mut().retirement_wait = Some(result);
        let ShutdownOutcome::Unresolved(status) = host.shutdown(one_wait()) else {
            panic!("lost obligation")
        };
        assert_eq!(status.frame, original.frame);
        assert_eq!(status.reason, original.reason);
        assert_eq!(status.initial_wait, original.initial_wait);
        no_free(&log);
    }
    log.borrow_mut().retirement_wait = Some(Completion::Quiesced);
    assert_eq!(host.shutdown(one_wait()), ShutdownOutcome::Released);
    assert_eq!(
        log.borrow()
            .events
            .iter()
            .filter(|e| matches!(e, Event::Set(_, _)))
            .count(),
        2
    );
    assert_eq!(
        log.borrow()
            .events
            .iter()
            .filter(|e| matches!(e, Event::Free(_)))
            .count(),
        1
    );
}

#[test]
fn platform_and_submission_failure_preserve_ambiguous_work_without_new_uploads() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    for error in [FrameError::PlatformOutput, FrameError::Submission] {
        let log = Rc::new(RefCell::new(Log::default()));
        let mut host = HostSession::new(Mock(log.clone()));
        let mut preview = Preview::with_session(&doc, EPOCH, &mut host);
        let mut warm = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        warm.tessellate().unwrap();
        warm.finish().unwrap();
        log.borrow_mut().events.clear();
        log.borrow_mut().fault = Some(error);
        log.borrow_mut().retirement_wait = Some(Completion::Timeout);
        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        frame.tessellate().unwrap();
        assert_eq!(frame.finish(), Err(error));
        assert!(
            !log.borrow()
                .events
                .iter()
                .any(|e| matches!(e, Event::Set(_, _)))
        );
        assert_eq!(
            pending(preview.cleanup_outcome()).frame,
            Some(FrameOutcome::Failed(error))
        );
        no_free(&log);
        drop(preview);
        assert!(host.retained_context() && host.retained_cpu_frame());
        log.borrow_mut().retirement_wait = Some(Completion::Quiesced);
        assert_eq!(host.shutdown(one_wait()), ShutdownOutcome::Released);
        assert_eq!(host.owned_keys(), 0);
    }
}

#[test]
fn new_preview_cannot_replace_pending_payload_and_shutdown_closes_admission() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    let log = Rc::new(RefCell::new(Log {
        frame_wait: Some(Completion::Timeout),
        retirement_wait: Some(Completion::Timeout),
        ..Default::default()
    }));
    let mut host = HostSession::new(Mock(log.clone()));
    {
        let mut preview = Preview::with_session(&doc, EPOCH, &mut host);
        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        frame.tessellate().unwrap();
        assert_eq!(frame.finish(), Err(FrameError::Completion));
    }
    let original = pending(host.status());
    {
        let mut preview = Preview::with_session(&doc, EPOCH, &mut host);
        assert!(matches!(
            preview.run(&Default::default(), Default::default()),
            Err(FrameError::RetirementPending)
        ));
    }
    assert_eq!(host.status(), CleanupOutcome::Pending(original));
    assert!(host.retained_context() && host.retained_cpu_frame());
    assert!(matches!(
        host.shutdown(one_wait()),
        ShutdownOutcome::Unresolved(_)
    ));
    log.borrow_mut().retirement_wait = Some(Completion::Quiesced);
    assert_eq!(host.shutdown(one_wait()), ShutdownOutcome::Released);
    let mut preview = Preview::with_session(&doc, EPOCH, &mut host);
    assert!(matches!(
        preview.run(&Default::default(), Default::default()),
        Err(FrameError::HostClosed)
    ));
}

struct ClockBackend<'a> {
    clock: &'a Cell<Duration>,
    calls: usize,
    delay: Duration,
    result: Completion,
}
impl FrameBackend for ClockBackend<'_> {
    const CPU_ONLY: bool = false;
    fn set(&mut self, _: TextureKey, _: &egui::epaint::ImageDelta) -> Result<(), FrameError> {
        unreachable!()
    }
    fn platform(
        &mut self,
        _: &egui::PlatformOutput,
        _: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> Result<(), FrameError> {
        unreachable!()
    }
    fn submit(
        &mut self,
        _: ContextGeneration,
        _: &[egui::ClippedPrimitive],
        _: f32,
    ) -> Result<(), FrameError> {
        unreachable!()
    }
    fn wait(&mut self, request: WaitRequest) -> Completion {
        assert!(request.timeout <= Duration::from_millis(5));
        self.calls += 1;
        self.clock.set(self.clock.get() + self.delay);
        self.result
    }
    fn free(&mut self, _: TextureKey) {
        unreachable!()
    }
}

#[test]
fn wait_call_cap_deadline_and_driver_duration_are_independent() {
    for (delay, attempts, elapsed, reached) in [
        (Duration::ZERO, 20, Duration::ZERO, false),
        (
            Duration::from_millis(5),
            20,
            Duration::from_millis(100),
            true,
        ),
        (
            Duration::from_millis(250),
            1,
            Duration::from_millis(250),
            true,
        ),
    ] {
        let clock = Cell::new(Duration::ZERO);
        let base = Instant::now();
        let mut backend = ClockBackend {
            clock: &clock,
            calls: 0,
            delay,
            result: Completion::Timeout,
        };
        let observed = observe_with_clock(
            &mut backend,
            ContextGeneration::allocate().unwrap(),
            WaitPurpose::Retirement,
            WaitPolicy::new(u8::MAX, Duration::from_secs(1), Duration::from_secs(10)),
            || base + clock.get(),
        );
        assert_eq!(observed.completion, Completion::Timeout);
        assert_eq!(observed.attempts, attempts);
        assert_eq!(backend.calls, usize::from(attempts));
        assert_eq!(observed.elapsed, elapsed);
        assert_eq!(
            observed.requested_wait,
            Duration::from_millis(u64::from(attempts) * 5)
        );
        assert_eq!(observed.deadline_reached, reached);
        assert_eq!(observed.attempt_limit_reached, attempts == 20);
    }
}

#[test]
fn zero_wait_budget_and_non_timeout_status_never_loop() {
    let clock = Cell::new(Duration::ZERO);
    let base = Instant::now();
    let generation = ContextGeneration::allocate().unwrap();
    for policy in [
        WaitPolicy::new(0, Duration::ZERO, Duration::from_millis(100)),
        WaitPolicy::new(20, Duration::ZERO, Duration::ZERO),
    ] {
        let mut backend = ClockBackend {
            clock: &clock,
            calls: 0,
            delay: Duration::ZERO,
            result: Completion::Quiesced,
        };
        let observed = observe_with_clock(
            &mut backend,
            generation,
            WaitPurpose::Retirement,
            policy,
            || base,
        );
        assert_eq!(observed.completion, Completion::Timeout);
        assert_eq!(backend.calls, 0);
    }
    for result in [
        Completion::Quiesced,
        Completion::Failed(WaitFailure::WaitError),
        Completion::Failed(WaitFailure::MissingFence),
        Completion::Failed(WaitFailure::InvalidFence),
        Completion::DeviceLost,
    ] {
        let mut backend = ClockBackend {
            clock: &clock,
            calls: 0,
            delay: Duration::ZERO,
            result,
        };
        let observed = observe_with_clock(
            &mut backend,
            generation,
            WaitPurpose::Retirement,
            WaitPolicy::default(),
            || base,
        );
        assert_eq!(observed.completion, result);
        assert_eq!(observed.attempts, 1);
    }
}

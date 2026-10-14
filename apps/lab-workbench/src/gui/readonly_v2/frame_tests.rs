//! Ownership and mock host regressions.
use super::{
    completion::{Completion, WaitFailure, WaitPurpose, WaitRequest},
    frame::*,
    plot_fixtures as fixtures,
    plot_widget_tests::{EPOCH, click},
    texture_owner::*,
    *,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Set(TextureKey, bool),
    Platform,
    Submit(ContextGeneration),
    Complete,
    Drain,
    Free(TextureKey),
}
#[derive(Default)]
pub(super) struct Log {
    pub(super) events: Vec<Event>,
    pub(super) fault: Option<FrameError>,
    pub(super) fail_set_on: usize,
    pub(super) busy: bool,
    pub(super) frame_wait: Option<Completion>,
    pub(super) retirement_wait: Option<Completion>,
}
pub(super) struct Mock(pub(super) Rc<RefCell<Log>>);
impl FrameBackend for Mock {
    const CPU_ONLY: bool = false;
    fn set(&mut self, key: TextureKey, delta: &egui::epaint::ImageDelta) -> Result<(), FrameError> {
        let mut log = self.0.borrow_mut();
        log.events.push(Event::Set(key, delta.pos.is_some()));
        log.busy = true; // Even a failed set may have started an upload.
        let sets = log
            .events
            .iter()
            .filter(|event| matches!(event, Event::Set(_, _)))
            .count();
        if log.fault == Some(FrameError::TextureUpdate)
            && (log.fail_set_on == 0 || log.fail_set_on == sets)
        {
            Err(FrameError::TextureUpdate)
        } else {
            Ok(())
        }
    }
    fn platform(
        &mut self,
        _: &egui::PlatformOutput,
        _: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> Result<(), FrameError> {
        let mut log = self.0.borrow_mut();
        log.events.push(Event::Platform);
        // Platform processing can start dependent work without a texture set.
        log.busy = true;
        if log.fault == Some(FrameError::PlatformOutput) {
            Err(FrameError::PlatformOutput)
        } else {
            Ok(())
        }
    }
    fn submit(
        &mut self,
        generation: ContextGeneration,
        _: &[egui::ClippedPrimitive],
        _: f32,
    ) -> Result<(), FrameError> {
        let mut log = self.0.borrow_mut();
        log.events.push(Event::Submit(generation));
        log.busy = true;
        if log.fault == Some(FrameError::Submission) {
            Err(FrameError::Submission)
        } else {
            Ok(())
        }
    }
    fn wait(&mut self, request: WaitRequest) -> Completion {
        let mut log = self.0.borrow_mut();
        let result = if request.purpose == WaitPurpose::Frame {
            log.events.push(Event::Complete);
            log.frame_wait
                .unwrap_or(if log.fault == Some(FrameError::Completion) {
                    Completion::Failed(WaitFailure::WaitError)
                } else {
                    Completion::Quiesced
                })
        } else {
            log.events.push(Event::Drain);
            log.retirement_wait.unwrap_or(Completion::Quiesced)
        };
        if result == Completion::Quiesced {
            log.busy = false;
        }
        result
    }
    fn release_frame(&mut self, _: ContextGeneration) -> super::deletion::DeletionResult {
        Ok(())
    }
    fn release_generation(&mut self, _: ContextGeneration) -> super::deletion::DeletionResult {
        Ok(())
    }
    fn free(&mut self, key: TextureKey) -> super::deletion::DeletionResult {
        let mut log = self.0.borrow_mut();
        assert!(!log.busy, "free before final dependent work");
        log.events.push(Event::Free(key));
        Ok(())
    }
}

fn generation(preview: &mut Preview<'_>, obs: &RuntimeObservations) -> ContextGeneration {
    let frame = preview.run(obs, Default::default()).unwrap();
    let generation = frame.generation();
    assert!(matches!(
        frame.finish_cpu(),
        Ok(FrameOutcome::CpuOnly { .. })
    ));
    generation
}

#[test]
fn page_plot_occurrence_document_owner_revision_and_recreation_generations() {
    let doc = fixtures::admit(&fixtures::syntax(2, false));
    let replacement = fixtures::admit(&fixtures::syntax(2, false));
    let obs = fixtures::observations(2, 20);
    let mut preview = Preview::new(&doc, EPOCH);
    let mut generations = vec![generation(&mut preview, &obs)];
    assert_eq!(generation(&mut preview, &obs), generations[0]);
    for occurrence in [0, 1, 0] {
        preview.open_plot(occurrence);
        generations.push(generation(&mut preview, &obs));
    }
    preview.close_plot();
    generations.push(generation(&mut preview, &obs));
    preview.replace(&replacement, EPOCH);
    generations.push(generation(&mut preview, &obs));
    preview.replace(
        &replacement,
        PresentationEpoch {
            owner: [9; 16],
            ..EPOCH
        },
    );
    generations.push(generation(&mut preview, &obs));
    preview.replace(
        &replacement,
        PresentationEpoch {
            revision: 99,
            ..EPOCH
        },
    );
    generations.push(generation(&mut preview, &obs));
    for _ in 0..24 {
        preview.retire_context();
        generations.push(generation(&mut preview, &obs));
    }
    drop(preview);
    let mut recreated = Preview::new(&doc, EPOCH);
    generations.push(generation(&mut recreated, &obs));
    for (i, generation) in generations.iter().enumerate() {
        assert!(!generations[..i].contains(generation));
    }
}

#[test]
fn unicode_fragment_and_runtime_unit_change_generation() {
    let (doc, mut obs) = fixtures::unicode_scene(0);
    let mut preview = Preview::new(&doc, EPOCH);
    preview.open_plot(0);
    let graph = generation(&mut preview, &obs);
    click(&mut preview, &obs, "Title details");
    let title = generation(&mut preview, &obs);
    assert_ne!(graph, title);
    click(&mut preview, &obs, "Next fragment");
    let fragment = generation(&mut preview, &obs);
    assert_ne!(fragment, title);
    click(&mut preview, &obs, "Back to Plot");
    click(&mut preview, &obs, "Details T01");
    click(&mut preview, &obs, "Runtime unit symbol");
    let unit = generation(&mut preview, &obs);
    let changed = serde_json::json!({"id":"new","symbol":"Ж🦀"});
    obs.observe_read_descriptor(
        fixtures::target(0),
        &serde_json::json!({"value_type":"float","generation":"1","unit":changed}),
    );
    obs.entities.get_mut(&fixtures::target(0)).unwrap().value["unit"] = changed;
    assert_ne!(generation(&mut preview, &obs), unit);
}

#[test]
fn transition_frame_has_generating_generation_not_selected_next_view() {
    let doc = fixtures::admit(&fixtures::syntax(2, false));
    let obs = fixtures::observations(2, 20);
    let mut preview = Preview::new(&doc, EPOCH);
    for caption in ["Open Plot", "Back", "Open Plot", "Back"] {
        let before = generation(&mut preview, &obs);
        let f = preview.run(&obs, Default::default()).unwrap();
        let pos = navigation_tests::painted_text(f.shapes())
            .into_iter()
            .find(|(text, _)| text == caption)
            .unwrap()
            .1
            .center();
        f.finish_cpu().unwrap();
        let f = preview
            .run(
                &obs,
                egui::RawInput {
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(f.generation(), before);
        f.finish_cpu().unwrap();
        assert_ne!(generation(&mut preview, &obs), before);
        preview
            .run(
                &obs,
                egui::RawInput {
                    events: vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
            )
            .unwrap()
            .finish_cpu()
            .unwrap();
    }
}

#[test]
fn once_only_tessellation_and_no_completion_without_tessellation() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    let mut preview = Preview::new(&doc, EPOCH);
    let mut frame = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    assert!(!frame.tessellate().unwrap().is_empty());
    assert!(matches!(
        frame.tessellate(),
        Err(FrameError::AlreadyTessellated)
    ));
    assert_eq!(frame.finish(), Err(FrameError::AlreadyTessellated));
    assert_eq!(
        preview
            .run(&Default::default(), Default::default())
            .unwrap()
            .finish(),
        Err(FrameError::NotTessellated)
    );
    assert!(preview.context.is_none());
}

#[test]
fn explicit_discard_and_drop_before_and_after_tessellation_retire() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    let mut preview = Preview::new(&doc, EPOCH);
    for tessellate in [false, true] {
        for explicit in [false, true] {
            let before = generation(&mut preview, &Default::default());
            let mut frame = preview
                .run(&Default::default(), Default::default())
                .unwrap();
            if tessellate {
                frame.tessellate().unwrap();
            }
            if explicit {
                frame.discard();
            } else {
                drop(frame);
            }
            assert_eq!(
                preview.last_outcome(),
                Some(if explicit {
                    FrameOutcome::Discarded
                } else {
                    FrameOutcome::Abandoned
                })
            );
            assert!(preview.context.is_none() && preview.pending.is_none());
            assert_ne!(generation(&mut preview, &Default::default()), before);
        }
    }
}

#[test]
fn forgotten_guard_blocks_next_frame_until_explicit_retirement_or_shutdown() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    let mut preview = Preview::new(&doc, EPOCH);
    let frame = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    std::mem::forget(frame);
    assert!(matches!(
        preview.run(&Default::default(), Default::default()),
        Err(FrameError::OutstandingFrame)
    ));
    assert!(preview.pending.is_some());
    preview.retire_context();
    assert!(preview.pending.is_none());
    assert_eq!(preview.last_outcome(), Some(FrameOutcome::Abandoned));
    std::mem::forget(
        preview
            .run(&Default::default(), Default::default())
            .unwrap(),
    );
    drop(preview); // Payload is owned by Preview, not by the forgotten guard.
}

#[test]
fn preparation_failure_returns_no_ack_and_retires_previous_resources() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    let mut preview = Preview::new(&doc, EPOCH);
    generation(&mut preview, &Default::default());
    preview.set_allowance(0);
    assert!(matches!(
        preview.run(&Default::default(), Default::default()),
        Err(FrameError::Preparation(_))
    ));
    assert!(preview.pending.is_none() && preview.context.is_none());
    assert!(matches!(
        preview.last_outcome(),
        Some(FrameOutcome::Failed(FrameError::Preparation(_)))
    ));
}

#[test]
fn host_faults_drain_partial_work_then_free_without_success_or_retry() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    for error in [
        FrameError::TextureUpdate,
        FrameError::PlatformOutput,
        FrameError::Submission,
        FrameError::Completion,
    ] {
        let log = Rc::new(RefCell::new(Log {
            fault: Some(error),
            ..Default::default()
        }));
        let mut session = HostSession::new(Mock(log.clone()));
        let mut preview = Preview::with_session(&doc, EPOCH, &mut session);
        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        let generation = frame.generation();
        frame.tessellate().unwrap();
        assert_eq!(frame.finish(), Err(error));
        if error == FrameError::Completion {
            assert!(matches!(
                preview.cleanup_outcome(),
                CleanupOutcome::Pending(_)
            ));
            preview.poll_retirement(WaitPolicy::default());
        }
        assert!(preview.context.is_none() && preview.pending.is_none());
        assert_eq!(preview.last_outcome(), Some(FrameOutcome::Failed(error)));
        let events = &log.borrow().events;
        assert!(matches!(events.last(), Some(Event::Free(key)) if key.generation == generation));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Set(_, _)))
                .count(),
            1
        );
        assert!(matches!(events[events.len() - 2], Event::Drain));
    }
}

#[test]
fn warmed_frame_faults_without_uploads_never_acknowledge_or_retry() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    for error in [
        FrameError::PlatformOutput,
        FrameError::Submission,
        FrameError::Completion,
    ] {
        let log = Rc::new(RefCell::new(Log::default()));
        let mut session = HostSession::new(Mock(log.clone()));
        let mut preview = Preview::with_session(&doc, EPOCH, &mut session);
        let mut warm = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        let generation = warm.generation();
        warm.tessellate().unwrap();
        warm.finish().unwrap();
        log.borrow_mut().events.clear();
        log.borrow_mut().fault = Some(error);

        let mut frame = preview
            .run(&Default::default(), Default::default())
            .unwrap();
        assert_eq!(frame.generation(), generation);
        frame.tessellate().unwrap();
        assert_eq!(frame.finish(), Err(error));
        if error == FrameError::Completion {
            assert!(matches!(
                preview.cleanup_outcome(),
                CleanupOutcome::Pending(_)
            ));
            preview.poll_retirement(WaitPolicy::default());
        }
        assert_eq!(preview.last_outcome(), Some(FrameOutcome::Failed(error)));
        assert!(preview.context.is_none() && preview.pending.is_none());
        let events = log.borrow().events.clone();
        assert!(!events.iter().any(|event| matches!(event, Event::Set(_, _))));
        assert_eq!(
            &events[events.len() - 2..],
            &[
                Event::Drain,
                Event::Free(TextureKey {
                    generation,
                    id: egui::TextureId::Managed(0)
                })
            ]
        );
        drop(preview);
        assert_eq!(log.borrow().events, events);
        assert!(!log.borrow().busy);
    }
}

#[test]
fn host_shutdown_and_cancel_retire_even_without_final_free_delta() {
    let doc = fixtures::admit(&fixtures::syntax(1, false));
    let log = Rc::new(RefCell::new(Log::default()));
    let mut session = HostSession::new(Mock(log.clone()));
    let mut preview = Preview::with_session(&doc, EPOCH, &mut session);
    let mut frame = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    let first = frame.generation();
    frame.tessellate().unwrap();
    assert_eq!(
        frame.finish(),
        Ok(FrameOutcome::HostCompleted { degraded: false })
    );
    assert!(
        !log.borrow()
            .events
            .iter()
            .any(|e| matches!(e, Event::Free(_)))
    );
    preview.retire_context();
    assert!(
        matches!(log.borrow().events.last(), Some(Event::Free(key)) if key.generation == first)
    );
    let mut frame = preview
        .run(&Default::default(), Default::default())
        .unwrap();
    let second = frame.generation();
    assert_ne!(first, second);
    frame.tessellate().unwrap();
    frame.finish().unwrap();
    // No final delta is needed; shutdown owns the namespace, including when a
    // subsequent frame's guard has been forgotten.
    std::mem::forget(
        preview
            .run(&Default::default(), Default::default())
            .unwrap(),
    );
    drop(preview);
    let events = &log.borrow().events;
    assert!(matches!(events.last(), Some(Event::Free(key)) if key.generation == second));
    let keys: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Set(key, false) => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(keys[0].id, keys[1].id);
    assert_ne!(keys[0], keys[1]);
}

#[test]
fn fixed_owner_and_page_storage_fit_the_existing_inline_charge() {
    let preview = std::mem::size_of::<Preview<'_>>();
    let owner = std::mem::size_of::<TextureOwner<CpuOnly>>();
    let frame = std::mem::size_of::<FrameData>();
    let page = std::mem::size_of::<Page>();
    let session = std::mem::size_of::<HostSession<Mock>>();
    // Preview includes its retained Page and FrameData. Charge a new Page plan
    // and bounded text scratch as well; library heap stays in the native meter.
    // Also charge an external session, conservatively including its one moved
    // frame header even though no second heap payload can coexist with it.
    let inline = preview
        + session
        + page
        + std::mem::size_of::<TextBuffer>()
        + std::mem::size_of::<plot_bounds::Preflight>()
        + std::mem::size_of::<plot_projection::PlotView<'_>>();
    assert!(inline < 65_536);
    println!(
        "Preview={preview}; HostSession<Mock>={session}; TextureOwner<CpuOnly>={owner}; FrameData={frame}; Page={page}; inline subtotal={inline}"
    );
}

//! Native CPU metering of pending retirement. All host resources are mock keys.
use super::{epoch, meter, readonly_v2};
use crate::{plot_fixtures as fixtures, plot_geometry_fixtures::GeometryStats};
use eframe::egui;
use readonly_v2::{
    completion::{Completion, WaitFailure, WaitPolicy, WaitPurpose, WaitRequest},
    frame::{ContextGeneration, FrameError, FrameOutcome},
    host_session::{CleanupOutcome, HostSession, ShutdownOutcome},
    texture_owner::{FrameBackend, MAX_TEXTURES, TextureKey},
};
use std::{cell::Cell, time::Duration};

struct Control {
    keys: Cell<[Option<TextureKey>; MAX_TEXTURES]>,
    busy: Cell<bool>,
    error: Cell<Option<FrameError>>,
    completion: Cell<Completion>,
    sets: Cell<usize>,
    submits: Cell<usize>,
    completed: Cell<usize>,
    waits: Cell<usize>,
    frees: Cell<usize>,
}
impl Control {
    fn new() -> Self {
        Self {
            keys: Cell::new([None; MAX_TEXTURES]),
            busy: Cell::new(false),
            error: Cell::new(None),
            completion: Cell::new(Completion::Quiesced),
            sets: Cell::new(0),
            submits: Cell::new(0),
            completed: Cell::new(0),
            waits: Cell::new(0),
            frees: Cell::new(0),
        }
    }
}
struct Host<'a>(&'a Control);
impl FrameBackend for Host<'_> {
    const CPU_ONLY: bool = false;
    fn set(&mut self, key: TextureKey, _: &egui::epaint::ImageDelta) -> Result<(), FrameError> {
        let c = self.0;
        let mut keys = c.keys.get();
        assert!(
            keys.iter()
                .flatten()
                .all(|k| k.generation == key.generation)
        );
        if !keys.contains(&Some(key)) {
            *keys.iter_mut().find(|k| k.is_none()).unwrap() = Some(key);
        }
        c.keys.set(keys);
        c.busy.set(true);
        c.sets.set(c.sets.get() + 1);
        if c.error.get() == Some(FrameError::TextureUpdate) {
            Err(FrameError::TextureUpdate)
        } else {
            Ok(())
        }
    }
    fn platform(
        &mut self,
        _: &egui::PlatformOutput,
        viewports: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> Result<(), FrameError> {
        assert_eq!(viewports.len(), 1);
        self.0.busy.set(true);
        if self.0.error.get() == Some(FrameError::PlatformOutput) {
            Err(FrameError::PlatformOutput)
        } else {
            Ok(())
        }
    }
    fn submit(
        &mut self,
        generation: ContextGeneration,
        meshes: &[egui::ClippedPrimitive],
        _: f32,
    ) -> Result<(), FrameError> {
        assert!(!meshes.is_empty());
        assert!(
            self.0
                .keys
                .get()
                .iter()
                .flatten()
                .all(|k| k.generation == generation)
        );
        self.0.busy.set(true);
        self.0.submits.set(self.0.submits.get() + 1);
        if self.0.error.get() == Some(FrameError::Submission) {
            Err(FrameError::Submission)
        } else {
            Ok(())
        }
    }
    fn wait(&mut self, request: WaitRequest) -> Completion {
        assert!(request.timeout <= Duration::from_millis(5));
        assert!(
            self.0
                .keys
                .get()
                .iter()
                .flatten()
                .all(|k| k.generation == request.generation)
        );
        self.0.waits.set(self.0.waits.get() + 1);
        let result = self.0.completion.get();
        if result == Completion::Quiesced {
            self.0.busy.set(false);
            if request.purpose == WaitPurpose::Frame {
                self.0.completed.set(self.0.completed.get() + 1);
            }
        }
        result
    }
    fn free(&mut self, key: TextureKey) {
        assert!(!self.0.busy.get(), "premature free");
        let mut keys = self.0.keys.get();
        *keys
            .iter_mut()
            .find(|k| **k == Some(key))
            .expect("double free") = None;
        self.0.keys.set(keys);
        self.0.frees.set(self.0.frees.get() + 1);
    }
}
fn include(total: &mut meter::Reading, reading: meter::Reading) {
    total.peak = total.peak.max(reading.peak);
    total.overlap_peak = total.overlap_peak.max(reading.overlap_peak);
    total.peak_blocks = total.peak_blocks.max(reading.peak_blocks);
}

pub(super) fn run() {
    let _ = ahash::RandomState::new();
    egui::Context::default()
        .run_ui(Default::default(), |_| {})
        .drop_without_applying_deltas();
    println!(
        "scenario,pending_retained,pending_blocks,phase_peak,realloc_overlap_peak,peak_blocks,accounted,shapes,vertices,indices,geometry_faults,allocation_faults,frame_faults,pending_keys,frees_before_proof,post_drop,post_drop_blocks,sets,submit_calls,completed,wait_calls,frees,outstanding,host_fixed_bytes,result"
    );
    for (case, completion) in [
        Completion::Quiesced,
        Completion::Timeout,
        Completion::Failed(WaitFailure::WaitError),
        Completion::Failed(WaitFailure::MissingFence),
        Completion::Failed(WaitFailure::InvalidFence),
        Completion::DeviceLost,
        Completion::Timeout,
        Completion::Timeout,
        Completion::Timeout,
    ]
    .into_iter()
    .enumerate()
    {
        let name = [
            "normal",
            "timeout",
            "wait-error",
            "missing-fence",
            "invalid-fence",
            "device-loss",
            "upload-error",
            "no-upload-platform-error",
            "no-upload-submit-error",
        ][case];
        let doc = fixtures::admit(&fixtures::syntax(32, false));
        let obs = fixtures::observations(32, 512);
        let control = Control::new();
        let mut stats = GeometryStats::default();
        let policy = WaitPolicy::new(1, Duration::from_millis(5), Duration::from_millis(100));
        meter::start();
        let mut session = HostSession::new(Host(&control));
        let mut preview = readonly_v2::Preview::with_session(&doc, epoch(0), &mut session);
        preview.open_plot(0);
        if case >= 7 {
            let mut warm = preview.run(&obs, Default::default()).unwrap();
            stats.max_assign(GeometryStats::inspect(&mut warm));
            warm.finish().unwrap();
        }
        let error = match case {
            0 => None,
            6 => Some(FrameError::TextureUpdate),
            7 => Some(FrameError::PlatformOutput),
            8 => Some(FrameError::Submission),
            _ => Some(FrameError::Completion),
        };
        control.error.set(error);
        control.completion.set(completion);
        let mut frame = preview.run(&obs, Default::default()).unwrap();
        assert_eq!(
            (frame.report().plot_traces, frame.report().plot_points),
            (32, 16_384)
        );
        let generation = frame.generation();
        stats.max_assign(GeometryStats::inspect(&mut frame));
        let result = frame.finish();
        if let Some(error) = error {
            assert_eq!(result, Err(error));
            assert_eq!(preview.last_outcome(), Some(FrameOutcome::Failed(error)));
            let CleanupOutcome::Pending(status) = preview.cleanup_outcome() else {
                panic!("obligation lost")
            };
            assert_eq!(status.generation, generation);
            assert_eq!(status.frame, Some(FrameOutcome::Failed(error)));
            for revision in 1..=25 {
                assert!(!preview.replace(&doc, epoch(revision)));
                assert!(matches!(
                    preview.run(&obs, Default::default()),
                    Err(FrameError::RetirementPending)
                ));
            }
        } else {
            assert_eq!(result, Ok(FrameOutcome::HostCompleted { degraded: false }));
        }
        let mut peaks = meter::phase();
        let calls = (
            control.sets.get(),
            control.submits.get(),
            control.waits.get(),
        );
        drop(preview);
        assert_eq!(
            calls,
            (
                control.sets.get(),
                control.submits.get(),
                control.waits.get()
            )
        );
        let retained = meter::phase();
        include(&mut peaks, retained);
        let pending_keys = session.owned_keys();
        let frees_before = control.frees.get();
        if let Some(error) = error {
            assert_eq!(pending_keys, 1);
            assert_eq!(frees_before, 0);
            assert!(retained.live > 0 && retained.blocks > 0);
            assert!(session.retained_context() && session.retained_cpu_frame());
            for _ in 0..25 {
                let CleanupOutcome::Pending(status) = session.poll_retirement(policy) else {
                    panic!("unproved retirement")
                };
                assert_eq!(status.generation, generation);
                assert_eq!(status.frame, Some(FrameOutcome::Failed(error)));
                assert_eq!(status.wait.completion, completion);
                assert_eq!(status.wait.attempts, 1);
                let repeated = meter::phase();
                assert_eq!(
                    (repeated.live, repeated.blocks),
                    (retained.live, retained.blocks)
                );
                include(&mut peaks, repeated);
            }
            assert!(matches!(
                session.shutdown(policy),
                ShutdownOutcome::Unresolved(_)
            ));
            assert_eq!(session.owned_keys(), pending_keys);
            assert_eq!(control.frees.get(), 0);
            assert_eq!(
                (control.sets.get(), control.submits.get()),
                (calls.0, calls.1)
            );
            assert_eq!(control.sets.get(), 1); // Includes both no-upload error cases.
            // Explicit test proof of eventual completion, after measuring and
            // asserting the unresolved state. Never manufacture zero on failure.
            control.completion.set(Completion::Quiesced);
        }
        assert_eq!(session.shutdown(policy), ShutdownOutcome::Released);
        assert_eq!(session.owned_keys(), 0);
        assert!(!session.retained_context() && !session.retained_cpu_frame());
        let calls = control.waits.get();
        assert_eq!(session.shutdown(policy), ShutdownOutcome::Released);
        assert_eq!(control.waits.get(), calls);
        drop(session);
        let dropped = meter::phase();
        include(&mut peaks, dropped);
        meter::stop();
        assert_eq!((dropped.live, dropped.blocks), (0, 0));
        let outstanding = control.keys.get().iter().flatten().count();
        assert_eq!(outstanding, 0);
        assert_eq!(control.frees.get(), 1);
        assert!(stats.finite());
        let accounted = peaks.overlap_peak + 127 * peaks.peak_blocks + 65_536;
        assert!(accounted <= readonly_v2::PREVIEW_RESERVATION);
        let host_fixed =
            std::mem::size_of::<HostSession<Host<'_>>>() + std::mem::size_of::<Control>();
        println!(
            "{name},{},{},{},{},{},{accounted},{},{},{},{},{},{},{pending_keys},{frees_before},{},{},{},{},{},{},{},{outstanding},{host_fixed},PASS",
            retained.live,
            retained.blocks,
            peaks.peak,
            peaks.overlap_peak,
            peaks.peak_blocks,
            stats.shapes,
            stats.vertices,
            stats.indices,
            stats.policy_failures,
            stats.allocation_faults,
            usize::from(error.is_some()),
            dropped.live,
            dropped.blocks,
            control.sets.get(),
            control.submits.get(),
            control.completed.get(),
            control.waits.get(),
            control.frees.get()
        );
    }
}

//! Native CPU allocation/lifecycle evidence with a fixed, non-GPU host backend.
use super::{epoch, meter, readonly_v2};
use crate::{plot_fixtures as fixtures, plot_geometry_fixtures::GeometryStats};
use eframe::egui;
use readonly_v2::{
    frame::{ContextGeneration, FrameError, FrameOutcome},
    texture_owner::{FrameBackend, MAX_TEXTURES, TextureKey},
};

#[derive(Default)]
struct Host {
    keys: [Option<TextureKey>; MAX_TEXTURES],
    busy: bool,
    fault: Option<FrameError>,
    sets: usize,
    draws: usize,
    drains: usize,
    frees: usize,
}
impl FrameBackend for &mut Host {
    const CPU_ONLY: bool = false;
    fn set(&mut self, key: TextureKey, _: &egui::epaint::ImageDelta) -> Result<(), FrameError> {
        self.busy = true;
        if !self.keys.contains(&Some(key)) {
            *self.keys.iter_mut().find(|k| k.is_none()).unwrap() = Some(key);
        }
        self.sets += 1;
        if self.fault == Some(FrameError::TextureUpdate) {
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
        if self.fault == Some(FrameError::PlatformOutput) {
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
            self.keys
                .iter()
                .flatten()
                .all(|k| k.generation == generation)
        );
        self.busy = true;
        self.draws += 1;
        if self.fault == Some(FrameError::Submission) {
            Err(FrameError::Submission)
        } else {
            Ok(())
        }
    }
    fn complete(&mut self) -> Result<(), FrameError> {
        if self.fault == Some(FrameError::Completion) {
            Err(FrameError::Completion)
        } else {
            self.busy = false;
            Ok(())
        }
    }
    fn drain(&mut self) {
        self.busy = false;
        self.drains += 1;
    }
    fn free(&mut self, key: TextureKey) {
        assert!(!self.busy);
        *self.keys.iter_mut().find(|k| **k == Some(key)).unwrap() = None;
        self.frees += 1;
    }
}

pub(super) fn run() {
    // Same process singleton attribution as the other native probes.
    let _ = ahash::RandomState::new();
    egui::Context::default()
        .run_ui(Default::default(), |_| {})
        .drop_without_applying_deltas();
    println!(
        "scenario,retained,phase_peak,realloc_overlap_peak,peak_blocks,accounted,shapes,vertices,indices,geometry_faults,allocation_faults,host_faults,post_drop,post_drop_blocks,sets,draws,drains,frees,result"
    );
    for case in 0..12 {
        let doc = fixtures::admit(&fixtures::syntax(32, false));
        let obs = fixtures::observations(32, 512);
        let mut host = Host {
            fault: match case {
                8 => Some(FrameError::TextureUpdate),
                9 => Some(FrameError::PlatformOutput),
                10 => Some(FrameError::Submission),
                11 => Some(FrameError::Completion),
                _ => None,
            },
            ..Default::default()
        };
        let expected_fault = host.fault;
        let mut stats = GeometryStats::default();
        meter::start();
        let mut preview = readonly_v2::Preview::with_backend(&doc, epoch(0), &mut host);
        if case != 0 {
            preview.open_plot(0);
        }
        let repetitions = if case == 2 { 25 } else { 1 };
        for revision in 0..repetitions {
            if case == 2 {
                preview.replace(&doc, epoch(revision));
                preview.open_plot((revision % 2) as usize);
            }
            let mut frame = preview.run(&obs, Default::default()).unwrap();
            if ![3, 5, 7].contains(&case) {
                stats.max_assign(GeometryStats::inspect(&mut frame));
            }
            match case {
                3 | 4 => {
                    frame.discard();
                    assert_eq!(preview.last_outcome(), Some(FrameOutcome::Discarded));
                }
                5 | 6 => {
                    drop(frame);
                    assert_eq!(preview.last_outcome(), Some(FrameOutcome::Abandoned));
                }
                7 => {
                    std::mem::forget(frame);
                    assert!(matches!(
                        preview.run(&obs, Default::default()),
                        Err(FrameError::OutstandingFrame)
                    ));
                }
                _ => {
                    let result = frame.finish();
                    if let Some(error) = expected_fault {
                        assert_eq!(result, Err(error));
                    } else {
                        assert!(matches!(
                            result,
                            Ok(FrameOutcome::HostCompleted { degraded: false })
                        ));
                    }
                }
            }
        }
        let reading = meter::phase();
        drop(preview);
        let dropped = meter::phase();
        meter::stop();
        assert_eq!((dropped.live, dropped.blocks), (0, 0));
        assert!(!host.busy && host.keys.iter().all(Option::is_none));
        assert!(stats.finite());
        // Include shutdown in phase maxima even if future library cleanup itself
        // allocates; zero retained bytes alone does not bound a drop-time peak.
        let peak = reading.peak.max(dropped.peak);
        let overlap = reading.overlap_peak.max(dropped.overlap_peak);
        let blocks = reading.peak_blocks.max(dropped.peak_blocks);
        let accounted = overlap + 127 * blocks + 65_536;
        assert!(accounted <= readonly_v2::PREVIEW_RESERVATION);
        println!(
            "{case},{},{},{},{},{accounted},{},{},{},{},{},{},{},{},{},{},{},{},PASS",
            reading.live,
            peak,
            overlap,
            blocks,
            stats.shapes,
            stats.vertices,
            stats.indices,
            stats.policy_failures,
            stats.allocation_faults,
            usize::from(expected_fault.is_some()),
            dropped.live,
            dropped.blocks,
            host.sets,
            host.draws,
            host.drains,
            host.frees
        );
    }
}

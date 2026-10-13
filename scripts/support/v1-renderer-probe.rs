//! Test-only bridge to the actual v1 renderer, compiled in its GUI parent.

use super::{controls::OperatorControlUi, presentation::PresentationRenderer};
use crate::model::{OperatorWorkflow, WorkbenchModel};
use eframe::egui;

#[derive(Default)]
pub(crate) struct V1Scene {
    renderer: PresentationRenderer,
    label: String,
}

impl V1Scene {
    pub(crate) fn frame(&mut self, context: &egui::Context, model: &WorkbenchModel, revision: u64) {
        let workflow = OperatorWorkflow::default();
        context
            .run_ui(egui::RawInput::default(), |ui| {
                let response = self.renderer.show(
                    ui,
                    &model.presentation,
                    &model.observations,
                    revision,
                    &mut OperatorControlUi {
                        model,
                        operator: &workflow,
                        observation_only: true,
                        label: &mut self.label,
                    },
                );
                assert!(response.declarative);
                assert!(response.intent.is_none());
            })
            .drop_without_applying_deltas();
    }
}

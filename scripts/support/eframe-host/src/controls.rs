//! Bounded local target/rate drafts and take-once typed user intentions.
//! This module has no Runtime client, mutation names or operation-status path.
use crate::private_ipc::{
    controls::{Action, Feedback, Request, Stage},
    protocol::Header,
};
use eframe::egui;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Default)]
struct Draft {
    target: String,
    rate: String,
}
#[derive(Default)]
pub(super) struct Controls {
    epoch: Option<(u64, u64)>,
    drafts: BTreeMap<String, Draft>,
    feedback: Option<Feedback>,
    nonce: u64,
    waiting: Option<u64>,
    outgoing: Option<Request>,
}
#[derive(Serialize)]
pub(super) struct Report {
    id: String,
    target: String,
    rate: String,
    valid: bool,
}
impl Controls {
    pub(super) fn receive(&mut self, feedback: Feedback) {
        if self.waiting.is_some_and(|nonce| feedback.receipt >= nonce) {
            self.waiting = None;
        }
        self.feedback = Some(feedback);
    }
    pub(super) fn take_request(&mut self) -> Option<Request> {
        self.outgoing.take()
    }
    pub(super) fn has_request(&self) -> bool {
        self.outgoing.is_some()
    }
    pub(super) fn stop(&mut self) {
        self.outgoing = None;
    }
    pub(super) fn begin_frame(&mut self, header: Header) {
        let epoch = (header.identity.incarnation, header.epoch.revision);
        if self.epoch != Some(epoch) {
            self.drafts.clear();
            self.outgoing = None;
            self.waiting = None;
            self.epoch = Some(epoch);
        }
    }
    fn queue(&mut self, action: Action) {
        if self.waiting.is_some() {
            return;
        }
        let Some((_, revision)) = self.epoch else {
            return;
        };
        let Some(nonce) = self.nonce.checked_add(1) else {
            return;
        };
        let request = Request {
            nonce,
            revision,
            action,
        };
        if request.validate().is_err() {
            return;
        }
        self.nonce = nonce;
        self.waiting = Some(nonce);
        self.outgoing = Some(request);
    }
    pub(super) fn status(&mut self, ui: &mut egui::Ui, report: &mut crate::ui::Report) {
        let Some(state) = self.feedback.clone() else {
            ui.label("Waiting for Parent control policy");
            return;
        };
        report.control_state = Some(state.clone());
        ui.group(|ui| {
            ui.heading("Operator action");
            let caption = match state.stage {
                Stage::Idle => "Draft only · no action sent",
                Stage::Confirmation => "Confirmation required · no action sent",
                Stage::Submitted => "Submitted · admission not yet known",
                Stage::Accepted => "Accepted / in progress · not completion",
                Stage::Completed => "Operation completed",
                Stage::Failed => "Operation failed",
                Stage::Ambiguous => "Unknown outcome · reconciliation required",
                Stage::Reconciled => "Authoritative recovery result",
            };
            ui.label(caption);
            ui.label(&state.summary);
            ui.label(&state.detail);
            if let Some(result) = &state.result {
                ui.label(format!("Operation result: Reference {}, revision {}; committed value {:?}, target {:?}, rate {:?}",
                    result.reference, result.revision, result.value, result.target, result.rate));
                ui.small("This operation result is separate from the live observed value below");
            }
            if state.accepted_observed {
                ui.small("Runtime acceptance observed");
            }
            if let Some(problem) = &state.problem {
                ui.colored_label(egui::Color32::LIGHT_RED, problem);
            }
            if !state.enabled {
                ui.label("Controls disabled: observation mode or explicit control opt-in absent");
            }
            let current = self
                .epoch
                .is_some_and(|(_, revision)| revision == state.revision);
            let idle = self.waiting.is_none() && current;
            if state.stage == Stage::Confirmation {
                let confirm = ui.add_enabled(
                    idle && state.can_confirm,
                    egui::Button::new("Confirm retune"),
                );
                crate::ui::widget(report, "Confirm retune", &confirm);
                if confirm.clicked() {
                    self.queue(Action::Confirm {
                        ticket: state.ticket,
                    });
                }
                let cancel =
                    ui.add_enabled(idle && state.enabled, egui::Button::new("Cancel action"));
                crate::ui::widget(report, "Cancel action", &cancel);
                if cancel.clicked() {
                    self.queue(Action::Cancel {
                        ticket: state.ticket,
                    });
                }
            } else if matches!(
                state.stage,
                Stage::Completed | Stage::Failed | Stage::Reconciled
            ) {
                let ack = ui.add_enabled(
                    idle && state.enabled,
                    egui::Button::new("Acknowledge result"),
                );
                crate::ui::widget(report, "Acknowledge result", &ack);
                if ack.clicked() {
                    self.queue(Action::Acknowledge {
                        ticket: state.ticket,
                    });
                }
            }
            if self.waiting.is_some() {
                ui.label("Waiting for Parent response; no automatic resend");
            }
        });
    }
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        reference: &str,
        fresh: bool,
        report: &mut crate::ui::Report,
    ) {
        if !self.drafts.contains_key(id) && self.drafts.len() >= 64 {
            ui.label("Control draft capacity reached");
            return;
        }
        let draft = self.drafts.entry(id.into()).or_default();
        ui.label(format!(
            "Retune ramp Reference {reference} · values in Runtime units"
        ));
        ui.small("Local draft. Editing sends nothing. Apply prepares a separate confirmation.");
        for (name, text) in [("Target", &mut draft.target), ("Rate", &mut draft.rate)] {
            ui.horizontal(|ui| {
                ui.label(name);
                let response = ui.add(
                    egui::TextEdit::singleline(text)
                        .id_salt((id, name))
                        .char_limit(65)
                        .desired_width(160.),
                );
                crate::ui::widget(report, &format!("{id}: {name}"), &response);
            });
        }
        let candidate = candidate(&draft.target, &draft.rate);
        report.controls.push(Report {
            id: id.into(),
            target: draft.target.clone(),
            rate: draft.rate.clone(),
            valid: candidate.is_some(),
        });
        if candidate.is_none() {
            ui.label("Enter a finite target and a finite, positive rate (maximum 64 bytes each)");
        }
        let available = self.feedback.as_ref().is_some_and(|s| {
            s.enabled && s.stage == Stage::Idle && self.epoch.is_some_and(|(_, r)| r == s.revision)
        }) && self.waiting.is_none()
            && fresh;
        let apply = ui.add_enabled(
            available && candidate.is_some(),
            egui::Button::new("Apply retune"),
        );
        crate::ui::widget(report, &format!("{id}: Apply retune"), &apply);
        if apply.clicked()
            && let Some((target, rate)) = candidate
        {
            self.queue(Action::ProposeRetune {
                control: id.into(),
                target,
                rate,
            });
        }
    }
}
fn candidate(target: &str, rate: &str) -> Option<(f64, f64)> {
    if target.len() > 64 || rate.len() > 64 {
        return None;
    }
    let target: f64 = target.trim().parse().ok()?;
    let rate: f64 = rate.trim().parse().ok()?;
    (target.is_finite() && rate.is_finite() && rate > 0.).then_some((target, rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::private_ipc::protocol::{Identity, Kind};
    fn header(revision: u64) -> Header {
        let mut h = Header::control(
            Kind::Document,
            Identity {
                workbench: [1; 16],
                incarnation: 1,
            },
            1,
        );
        h.epoch.revision = revision;
        h
    }
    #[test]
    fn edits_are_local_and_outgoing_intention_is_taken_once() {
        let mut controls = Controls::default();
        controls.begin_frame(header(1));
        controls.receive(Feedback {
            revision: 1,
            enabled: true,
            ..Default::default()
        });
        controls.drafts.insert(
            "tune".into(),
            Draft {
                target: "42".into(),
                rate: "2".into(),
            },
        );
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                controls.show(ui, "tune", "1", true, &mut crate::ui::Report::default())
            });
        });
        // CPU-only egui test deliberately performs no graphics upload.
        output.textures_delta.clear();
        assert!(!controls.has_request());
        controls.queue(Action::ProposeRetune {
            control: "tune".into(),
            target: 42.,
            rate: 2.,
        });
        assert!(controls.take_request().is_some());
        assert!(controls.take_request().is_none());
        controls.queue(Action::ProposeRetune {
            control: "tune".into(),
            target: 42.,
            rate: 2.,
        });
        assert!(
            !controls.has_request(),
            "pending response consumes the sole user-intention credit"
        );
        controls.begin_frame(header(2));
        assert!(controls.drafts.is_empty());
        assert!(!controls.has_request());
    }
    #[test]
    fn candidate_validation_keeps_incomplete_nonfinite_and_oversized_drafts_inert() {
        for (target, rate) in [
            ("", "1"),
            ("NaN", "1"),
            ("inf", "1"),
            ("42", "0"),
            ("42", "-1"),
            ("42", "NaN"),
        ] {
            assert!(candidate(target, rate).is_none());
        }
        assert!(candidate(&"1".repeat(65), "1").is_none());
        assert_eq!(candidate("42.5", "2"), Some((42.5, 2.)));
    }
}

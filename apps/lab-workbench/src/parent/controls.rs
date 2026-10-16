//! Parent-only CONTROL-1 gate and projection over the existing operator workflow.
use super::*;
use crate::{
    client::types::CommandSendError,
    model::{MutationSubmitter, OperatorIntent, OperatorWorkflowState as State},
    presentation::{ControlKind, PanelKind},
    private_ipc::{
        Bridge, Identity,
        controls::{Action, Envelope, Feedback, RetuneResult, Stage},
    },
};
use serde_json::Value;

fn retune_result(envelope: &Value) -> Option<RetuneResult> {
    let result = envelope.get("result")?;
    let value = RetuneResult {
        reference: result.get("reference")?.as_str()?.into(),
        revision: result.get("revision")?.as_str()?.into(),
        value: result.get("value")?.as_f64()?,
        target: result.get("target")?.as_f64()?,
        rate: result.get("rate")?.as_f64()?,
    };
    (value.reference.len() <= 128
        && value.revision.len() <= 20
        && value.value.is_finite()
        && value.target.is_finite()
        && value.rate.is_finite())
    .then_some(value)
}

#[derive(Default)]
pub(crate) struct StockControl {
    /// Explicit host opt-in, additionally disabled by --observe at composition.
    pub(crate) enabled: bool,
    incarnation: u64,
    nonce: u64,
    next_ticket: u64,
    binding: Option<Binding>,
    problem: Option<String>,
}
struct Binding {
    identity: Identity,
    revision: u64,
    control: String,
    ticket: u64,
}
struct Submitter<'a, C>(&'a C);
impl<C: LabClient> MutationSubmitter for Submitter<'_, C> {
    fn submit_mutation(&self, op: &str, args: Value) -> Result<u64, CommandSendError> {
        self.0.mutation(op, args)
    }
}
fn bounded(value: &str, maximum: usize) -> String {
    let mut end = value.len().min(maximum);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}
impl StockControl {
    pub(crate) fn turn<C: ParentClient>(
        &mut self,
        bridge: &Bridge,
        dispatcher: &mut WorkbenchDispatcher<C>,
        workflow: &mut OperatorWorkflow,
    ) {
        let Ok(status) = bridge.status() else {
            return;
        };
        if self.incarnation != status.incarnation {
            self.incarnation = status.incarnation;
            self.nonce = 0;
            // Only unsent confirmation is GUI-incarnation-local. Submitted,
            // accepted and uncertain work remains owned by Parent and recovery.
            if matches!(workflow.state, State::AwaitingConfirmation(_)) {
                workflow.cancel_or_acknowledge();
                self.binding = None;
            }
            self.problem = None;
        }
        if let Ok(Some(envelope)) = bridge.take_intent() {
            let result = if envelope.identity.workbench != bridge.identity()
                || envelope.identity.incarnation != status.incarnation
                || !status.handshaken
                || status.retiring
                || status.retired
                || status.window_close_requested
            {
                Err("GUI incarnation is no longer active".into())
            } else {
                self.handle(envelope, dispatcher, workflow)
            };
            self.problem = result.err().map(|s: String| bounded(&s, 256));
        }
        // Latest state only. Metadata contention never re-executes an intention.
        let _ = bridge.control_feedback(&self.feedback(dispatcher, workflow));
    }
    fn handle<C: ParentClient>(
        &mut self,
        envelope: Envelope,
        model: &mut WorkbenchDispatcher<C>,
        workflow: &mut OperatorWorkflow,
    ) -> Result<(), String> {
        let request = envelope.request;
        if envelope.identity.incarnation != self.incarnation {
            return Err("Stale GUI incarnation".into());
        }
        if request.nonce <= self.nonce {
            return Err("Duplicate operator intention".into());
        }
        self.nonce = request.nonce;
        request.validate().map_err(str::to_owned)?;
        if !self.enabled {
            return Err("Control is disabled (observation mode or missing explicit opt-in)".into());
        }
        if request.revision != model.presentation_revision() {
            return Err("Presentation revision is stale; review the current document".into());
        }
        match request.action {
            Action::ProposeRetune {
                control,
                target,
                rate,
            } => {
                let intent = resolve(model, &control, target, rate)?;
                let ticket = self
                    .next_ticket
                    .checked_add(1)
                    .ok_or("Operator ticket exhausted")?;
                workflow.begin(model, intent).map_err(|e| e.to_string())?;
                self.next_ticket = ticket;
                self.binding = Some(Binding {
                    identity: envelope.identity,
                    revision: request.revision,
                    control,
                    ticket,
                });
            }
            Action::Confirm { ticket } => {
                let binding = self.binding.as_ref().ok_or("No prepared action")?;
                if binding.identity != envelope.identity
                    || binding.revision != request.revision
                    || binding.ticket != ticket
                {
                    return Err("Confirmation binding/incarnation/revision is stale".into());
                }
                // Re-resolve the approved binding and Runtime guards before the
                // existing workflow's final confirmation/submission check.
                if !self.can_confirm(model, workflow) {
                    return Err("Prepared action is stale or no longer eligible".into());
                }
                let client = model.client().ok_or("Application client unavailable")?;
                let command = workflow
                    .confirm(model, &Submitter(client))
                    .map_err(|e| e.to_string())?;
                model
                    .track_operator_intent(command)
                    .map_err(str::to_owned)?;
            }
            Action::Cancel { ticket } => {
                let binding = self.binding.as_ref().ok_or("No prepared action")?;
                if binding.identity != envelope.identity
                    || binding.ticket != ticket
                    || !matches!(workflow.state, State::AwaitingConfirmation(_))
                {
                    return Err("Only an unsent confirmation can be canceled".into());
                }
                workflow.cancel_or_acknowledge();
                self.binding = None;
            }
            Action::Acknowledge { ticket } => {
                if self.binding.as_ref().is_none_or(|b| b.ticket != ticket)
                    || !matches!(
                        workflow.state,
                        State::Completed { .. }
                            | State::Failed { .. }
                            | State::ReconciledTerminal { .. }
                    )
                {
                    return Err("Only a terminal operation can be acknowledged".into());
                }
                workflow.cancel_or_acknowledge();
                self.binding = None;
            }
        }
        Ok(())
    }
    fn can_confirm<C>(&self, model: &WorkbenchDispatcher<C>, workflow: &OperatorWorkflow) -> bool {
        let Some(binding) = &self.binding else {
            return false;
        };
        let State::AwaitingConfirmation(prepared) = &workflow.state else {
            return false;
        };
        if !self.enabled
            || binding.revision != model.presentation_revision()
            || binding.identity.incarnation != self.incarnation
            || prepared.confirmation.len() > 1024
            || !prepared.is_current(model)
        {
            return false;
        }
        let Ok(intent) = resolve(model, &binding.control, 0., 1.) else {
            return false;
        };
        matches!(&intent, OperatorIntent::RetuneReference { reference, .. } if prepared.target == (RuntimeRef::Reference { reference: reference.clone() }))
            && crate::model::prepare_operator_intent(model, intent).is_ok()
    }
    pub(super) fn feedback<C>(
        &self,
        model: &WorkbenchDispatcher<C>,
        workflow: &OperatorWorkflow,
    ) -> Feedback {
        let (stage, prepared, detail) = match &workflow.state {
            State::Idle => (Stage::Idle, None, "No operator action is active".into()),
            State::AwaitingConfirmation(p) => (
                Stage::Confirmation,
                Some(p),
                "Confirm expresses intent; it is not completion".into(),
            ),
            State::Submitted { prepared, .. } => (
                Stage::Submitted,
                Some(prepared),
                "Submitted; admission not yet known".into(),
            ),
            State::Accepted { prepared, .. } => (
                Stage::Accepted,
                Some(prepared),
                "Accepted / in progress; Runtime observation remains authoritative".into(),
            ),
            State::Completed { prepared, .. } => (
                Stage::Completed,
                Some(prepared),
                "Operation completed; observed Reference is shown separately".into(),
            ),
            State::Failed {
                prepared, envelope, ..
            } => (
                Stage::Failed,
                Some(prepared),
                bounded(
                    envelope
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Operation failed; inspect the Parent recovery history"),
                    1024,
                ),
            ),
            State::Ambiguous { prepared, .. } => (
                Stage::Ambiguous,
                Some(prepared),
                "Unknown outcome / reconciliation required. No automatic retry or status request"
                    .into(),
            ),
            State::ReconciledTerminal {
                prepared,
                admission,
                ..
            } => (
                Stage::Reconciled,
                Some(prepared),
                format!("Authoritative recovery outcome: {admission:?}"),
            ),
        };
        Feedback {
            revision: model.presentation_revision(),
            receipt: self.nonce,
            enabled: self.enabled,
            stage,
            ticket: self.binding.as_ref().map_or(0, |b| b.ticket),
            result: match &workflow.state {
                State::Completed { envelope, .. } => retune_result(envelope),
                _ => None,
            },
            summary: prepared.map_or(String::new(), |p| bounded(&p.confirmation, 1024)),
            detail,
            problem: self.problem.clone(),
            accepted_observed: workflow.accepted_observed,
            can_confirm: self.can_confirm(model, workflow),
        }
    }
}
fn resolve<C>(
    model: &WorkbenchDispatcher<C>,
    id: &str,
    target: f64,
    rate: f64,
) -> Result<OperatorIntent, String> {
    let control = model
        .presentation
        .controls
        .iter()
        .find(|c| c.id == id)
        .ok_or("Control binding is absent")?;
    let referenced = model.presentation.windows.iter().flat_map(|w| &w.tabs).flat_map(|t| &t.panels)
        .any(|p| matches!(&p.kind, PanelKind::Controls { control_ids } if control_ids.contains(&control.id)));
    if !referenced || control.kind != ControlKind::SetReference {
        return Err("Control binding does not permit set_reference".into());
    }
    let RuntimeRef::Reference { reference } = &control.target else {
        return Err("Control target is not a Reference".into());
    };
    Ok(OperatorIntent::RetuneReference {
        reference: reference.clone(),
        target,
        rate,
    })
}

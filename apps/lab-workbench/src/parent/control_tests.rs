//! CONTROL-1 owner regressions, with a finite mutation capture and no OS/GPU.
use super::{controls::StockControl, *};
use crate::{
    client::types::{CommandSendError, ConnectionState, EventCursor, HelloState, ReplyKind},
    model::{OperatorWorkflowState as State, UiCommand},
    private_ipc::{
        Bridge, Identity,
        controls::{Action, Envelope, Request, Stage},
    },
};
use serde_json::{Value, json};
use std::cell::RefCell;

#[derive(Default)]
struct Client(RefCell<Vec<Value>>);
impl LabClient for Client {
    fn query(&self, _: &str, _: Value) -> Result<u64, CommandSendError> {
        panic!("control caused a query")
    }
    fn operation_status(
        &self,
        _: crate::client::types::MutationIdentity,
    ) -> Result<u64, CommandSendError> {
        panic!("control caused a status request")
    }
    fn mutation(&self, op: &str, args: Value) -> Result<u64, CommandSendError> {
        let mut sent = self.0.borrow_mut();
        assert!(sent.len() < 4);
        sent.push(json!({"op":op,"args":args}));
        Ok(sent.len() as u64)
    }
}
impl RebuildClient for Client {
    fn query(&self, _: &str, _: Value) -> Result<u64, CommandSendError> {
        panic!("control caused rebuild")
    }
    fn subscribe(&self, _: EventCursor, _: Value) -> Result<u64, CommandSendError> {
        panic!("control caused subscribe")
    }
}
impl ParentClient for Client {
    fn try_update(&self) -> Result<ClientUpdate, TryRecvError> {
        Err(TryRecvError::Empty)
    }
    fn stop(self) -> Result<(), &'static str> {
        Ok(())
    }
}
fn document() -> PresentationDocument {
    serde_json::from_value(json!({"format_version":1,"document_id":"control-test","plots":[],
        "windows":[{"id":"w","title":"Window","tabs":[{"id":"t","title":"Tab","panels":[{"id":"p","title":"Reference","kind":{"kind":"controls","control_ids":["tune"]}}]}]}],
        "controls":[{"id":"tune","label":"Set reference","kind":"set_reference","target":{"kind":"reference","reference":"1"},"confirm":false}]})).unwrap()
}
fn fixture(
    enabled: bool,
) -> (
    StockControl,
    Bridge,
    WorkbenchDispatcher<Client>,
    OperatorWorkflow,
) {
    let mut model = WorkbenchModel::new(document());
    model.connection = ConnectionState::Ready;
    model.hello = Some(HelloState {
        time_anchor: None,
        boot_id: "boot".into(),
        scope: "scope".into(),
        next_seq: 1,
        operations: vec!["reference_retune".into()],
        capabilities: json!([]),
        limits: json!({}),
        event_oldest: EventCursor {
            boot_id: "boot".into(),
            seq: 0,
        },
        event_latest: EventCursor {
            boot_id: "boot".into(),
            seq: 0,
        },
    });
    model.observations.observe(
        RuntimeRef::Reference {
            reference: "1".into(),
        },
        json!({"reference":"1","revision":"7","kind":"ramp","value":3.}),
        None,
    );
    model.observations.complete_rebuild();
    let dispatcher = WorkbenchDispatcher::new(model, Client::default());
    let bridge = Bridge::new([1; 16]);
    bridge.control_test_incarnation(1);
    let mut gate = StockControl::default();
    gate.enabled = enabled;
    (gate, bridge, dispatcher, OperatorWorkflow::default())
}
#[expect(
    clippy::too_many_arguments,
    reason = "explicit identity/fence matrix in the finite control test"
)]
fn request(
    bridge: &Bridge,
    control: &mut StockControl,
    model: &mut WorkbenchDispatcher<Client>,
    flow: &mut OperatorWorkflow,
    nonce: u64,
    revision: u64,
    incarnation: u64,
    action: Action,
) {
    bridge.control_test_request(Envelope {
        identity: Identity {
            workbench: [1; 16],
            incarnation,
        },
        request: Request {
            nonce,
            revision,
            action,
        },
    });
    control.turn(bridge, model, flow);
}
fn propose() -> Action {
    Action::ProposeRetune {
        control: "tune".into(),
        target: 42.,
        rate: 2.,
    }
}
fn sent(model: &WorkbenchDispatcher<Client>) -> Vec<Value> {
    model.client().unwrap().0.borrow().clone()
}

#[test]
fn proposal_is_inert_and_one_confirmation_submits_once_without_status_or_retry() {
    let (mut gate, bridge, mut model, mut flow) = fixture(true);
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        1,
        1,
        1,
        propose(),
    );
    assert!(sent(&model).is_empty());
    assert!(matches!(flow.state, State::AwaitingConfirmation(_)));
    let ticket = gate.feedback(&model, &flow).ticket;
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        2,
        1,
        1,
        Action::Confirm { ticket },
    );
    assert_eq!(
        sent(&model),
        vec![
            json!({"op":"reference_retune","args":{"reference":"1","expected_revision":"7","target":42.,"rate":2.}})
        ]
    );
    assert_eq!(gate.feedback(&model, &flow).stage, Stage::Submitted);
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        2,
        1,
        1,
        Action::Confirm { ticket },
    );
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        3,
        1,
        1,
        Action::Confirm { ticket },
    );
    assert_eq!(sent(&model).len(), 1);
    for _ in 0..20 {
        gate.turn(&bridge, &mut model, &mut flow);
    }
    assert_eq!(sent(&model).len(), 1);
    // Acceptance is not completion and does not overwrite the observed value.
    let update = |kind, envelope| ClientUpdate::Reply {
        command_id: 1,
        msg_id: "1".into(),
        op: "reference_retune".into(),
        kind,
        envelope,
        recovery: None,
    };
    flow.after_update(&update(
        ReplyKind::MutationAccepted,
        json!({"state":"accepted"}),
    ));
    let accepted = gate.feedback(&model, &flow);
    assert_eq!(accepted.stage, Stage::Accepted);
    assert!(accepted.result.is_none());
    flow.after_update(&update(ReplyKind::MutationCompleted,json!({"state":"completed","result":{"reference":"1","revision":"8","value":3.,"target":42.,"rate":2.}})));
    let completed = gate.feedback(&model, &flow);
    assert_eq!(completed.stage, Stage::Completed);
    assert_eq!(completed.result.unwrap().revision, "8");
    assert_eq!(
        model.observations.entities[&RuntimeRef::Reference {
            reference: "1".into()
        }]
            .value["value"],
        3.
    );
    assert!(flow.accepted_observed);
}

#[test]
fn disabled_invalid_stale_and_wrong_binding_requests_never_submit() {
    for action in [
        propose(),
        Action::ProposeRetune {
            control: "tune".into(),
            target: f64::NAN,
            rate: 1.,
        },
        Action::ProposeRetune {
            control: "tune".into(),
            target: 1.,
            rate: 0.,
        },
        Action::ProposeRetune {
            control: "missing".into(),
            target: 1.,
            rate: 1.,
        },
    ] {
        let (mut gate, bridge, mut model, mut flow) =
            fixture(!matches!(&action,Action::ProposeRetune {target,..} if *target==42.));
        request(&bridge, &mut gate, &mut model, &mut flow, 1, 1, 1, action);
        assert!(sent(&model).is_empty());
        assert!(!flow.state.is_active());
    }
    for (revision, incarnation) in [(2, 1), (1, 2)] {
        let (mut gate, bridge, mut model, mut flow) = fixture(true);
        request(
            &bridge,
            &mut gate,
            &mut model,
            &mut flow,
            1,
            revision,
            incarnation,
            propose(),
        );
        assert!(sent(&model).is_empty());
        assert!(!flow.state.is_active());
    }
    let (mut gate, bridge, mut model, mut flow) = fixture(true);
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        1,
        1,
        1,
        propose(),
    );
    let ticket = gate.feedback(&model, &flow).ticket;
    let replacement = model.presentation.clone();
    model
        .apply_gui_ui(UiCommand::ReplaceDocument {
            document: replacement,
        })
        .unwrap();
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        2,
        1,
        1,
        Action::Confirm { ticket },
    );
    assert!(sent(&model).is_empty());
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        3,
        2,
        1,
        Action::Confirm { ticket },
    );
    assert!(sent(&model).is_empty());
}

#[test]
fn changed_runtime_guard_and_gui_restart_cannot_confirm_or_replay() {
    let (mut gate, bridge, mut model, mut flow) = fixture(true);
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        1,
        1,
        1,
        propose(),
    );
    let ticket = gate.feedback(&model, &flow).ticket;
    model.apply_client_update(ClientUpdate::State(ConnectionState::Stale));
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        2,
        1,
        1,
        Action::Confirm { ticket },
    );
    assert!(sent(&model).is_empty());
    assert!(!gate.feedback(&model, &flow).can_confirm);
    bridge.control_test_incarnation(2);
    gate.turn(&bridge, &mut model, &mut flow);
    assert!(matches!(flow.state, State::Idle));
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        1,
        1,
        1,
        Action::Confirm { ticket },
    );
    assert!(sent(&model).is_empty());
    let (mut gate, bridge, mut model, mut flow) = fixture(true);
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        1,
        1,
        1,
        propose(),
    );
    let ticket = gate.feedback(&model, &flow).ticket;
    request(
        &bridge,
        &mut gate,
        &mut model,
        &mut flow,
        2,
        1,
        1,
        Action::Confirm { ticket },
    );
    bridge.control_test_incarnation(2);
    for _ in 0..20 {
        gate.turn(&bridge, &mut model, &mut flow);
    }
    assert_eq!(sent(&model).len(), 1);
    assert!(matches!(flow.state, State::Submitted { .. }));
    flow.after_update(&ClientUpdate::TransportFailure {
        reason: "lost".into(),
    });
    for _ in 0..20 {
        gate.turn(&bridge, &mut model, &mut flow);
    }
    assert_eq!(sent(&model).len(), 1);
    assert!(matches!(flow.state, State::Ambiguous { .. }));
}

//! Renderer-free owner/runner regressions. All fake mailboxes have finite credit.
use super::*;
use crate::{
    client::types::{
        CommandSendError, EventCursor, HelloState, KnownAdmission, MutationIdentity,
        RecoveryRecord, ReplyKind,
    },
    model::{ExactRetryState, ExactRetrySubmitter, RecoveryStatusState, RecoveryStatusSubmitter},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    sync::{
        Barrier,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};

#[derive(Debug, PartialEq)]
enum Sent {
    Query(u64, String),
    Mutation(u64, String),
    Status(u64, MutationIdentity),
    Subscribe(u64),
    Retry(u64, MutationIdentity),
}
struct FakeClient {
    updates: Receiver<ClientUpdate>,
    sent: SyncSender<Sent>,
    next: AtomicU64,
    stops: Arc<AtomicUsize>,
    late: RefCell<Option<(SyncSender<ClientUpdate>, ClientUpdate)>>,
    pause: RefCell<Option<ReceivePause>>,
    fail_stop: Cell<bool>,
}
/// Test-only receive seam: pause inside the real owner turn after dequeue, then
/// report Empty once so the published update requires another scheduler turn.
struct ReceivePause {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    polls: usize,
}
impl FakeClient {
    fn send(&self, make: impl FnOnce(u64) -> Sent) -> Result<u64, CommandSendError> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        self.sent
            .try_send(make(id))
            .map_err(|_| CommandSendError::Busy)?;
        Ok(id)
    }
}
impl LabClient for FakeClient {
    fn query(&self, op: &str, _: Value) -> Result<u64, CommandSendError> {
        self.send(|id| Sent::Query(id, op.into()))
    }
    fn mutation(&self, op: &str, _: Value) -> Result<u64, CommandSendError> {
        self.send(|id| Sent::Mutation(id, op.into()))
    }
    fn operation_status(&self, identity: MutationIdentity) -> Result<u64, CommandSendError> {
        self.send(|id| Sent::Status(id, identity))
    }
}
impl RebuildClient for FakeClient {
    fn query(&self, op: &str, args: Value) -> Result<u64, CommandSendError> {
        LabClient::query(self, op, args)
    }
    fn subscribe(&self, _: EventCursor, _: Value) -> Result<u64, CommandSendError> {
        self.send(Sent::Subscribe)
    }
}
impl ExactRetrySubmitter for FakeClient {
    fn retry_exact(&self, identity: MutationIdentity) -> Result<u64, CommandSendError> {
        self.send(|id| Sent::Retry(id, identity))
    }
}
impl RecoveryStatusSubmitter for FakeClient {
    fn operation_status(&self, identity: MutationIdentity) -> Result<u64, CommandSendError> {
        LabClient::operation_status(self, identity)
    }
}
impl ParentClient for FakeClient {
    fn try_update(&self) -> Result<ClientUpdate, TryRecvError> {
        let mut pause = self.pause.borrow_mut();
        if let Some(pause) = pause.as_mut() {
            pause.polls += 1;
            if pause.polls == 2 {
                return Err(TryRecvError::Empty);
            }
        }
        let update = self.updates.try_recv()?;
        if let Some(pause) = pause.as_ref()
            && pause.polls == 1
        {
            pause.entered.wait();
            pause.release.wait();
        }
        if let Some((sender, late)) = self.late.borrow_mut().take() {
            sender.try_send(late).unwrap();
        }
        Ok(update)
    }
    fn stop(self) -> Result<(), &'static str> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        if self.fail_stop.get() {
            Err("injected client stop failure")
        } else {
            Ok(())
        }
    }
}
fn fixture() -> (
    ParentOwner<FakeClient>,
    SyncSender<ClientUpdate>,
    Receiver<Sent>,
    Arc<AtomicUsize>,
) {
    let (tx, updates) = sync_channel(crate::client::types::UPDATE_QUEUE);
    let (sent, rx) = sync_channel(crate::client::types::COMMAND_QUEUE);
    let stops = Arc::new(AtomicUsize::new(0));
    let client = FakeClient {
        updates,
        sent,
        next: AtomicU64::new(1),
        stops: stops.clone(),
        late: RefCell::new(None),
        pause: RefCell::new(None),
        fail_stop: Cell::new(false),
    };
    (
        ParentOwner::new(PresentationDocument::empty("parent"), client, None),
        tx,
        rx,
        stops,
    )
}
pub(crate) fn assert_bounded_fifo_turn() {
    let (mut owner, tx, sent, _) = fixture();
    for index in 0..UPDATES_PER_TURN {
        tx.try_send(ClientUpdate::RecoveryJournalProblem {
            reason: format!("{index}"),
        })
        .unwrap();
    }
    assert!(matches!(
        tx.try_send(ClientUpdate::WorkerStopped),
        Err(std::sync::mpsc::TrySendError::Full(_))
    ));
    owner.dispatcher.client().unwrap().late.replace(Some((
        tx.clone(),
        ClientUpdate::RecoveryJournalProblem {
            reason: "64".into(),
        },
    )));
    let begin = Instant::now();
    assert_eq!(
        owner.turn(),
        TurnReport {
            updates: 64,
            external_calls: 0,
            rebuild_steps: 64
        }
    );
    println!(
        "parent bounded turn: updates=64 rebuild_steps=64 external_calls=0 elapsed_us={}",
        begin.elapsed().as_micros()
    );
    assert_eq!(owner.dispatcher.client_error.as_deref(), Some("63"));
    assert_eq!(owner.turn().updates, 1);
    assert_eq!(owner.dispatcher.client_error.as_deref(), Some("64"));
    assert!(sent.try_recv().is_err());
}

#[test]
fn recovery_workflow_observers_survive_reconnect_without_implicit_status_or_retry() {
    let (mut owner, tx, rx, _) = fixture();
    let mut h = hello();
    if let ClientUpdate::Hello(h) = &mut h {
        h.operations = vec!["operation_status".into()];
    }
    tx.try_send(h.clone()).unwrap();
    tx.try_send(ClientUpdate::RecoveryProjection {
        active: vec![record(KnownAdmission::Ambiguous)],
        quarantined: vec![],
    })
    .unwrap();
    owner.turn();
    let identity = record(KnownAdmission::Ambiguous).identity;
    assert!(
        owner
            .exact_retry
            .can_begin(&owner.dispatcher, &owner.recovery_status, &identity)
    );
    owner
        .exact_retry
        .begin(&owner.dispatcher, &owner.recovery_status, identity.clone())
        .unwrap();
    tx.try_send(ClientUpdate::State(
        crate::client::types::ConnectionState::Disconnected,
    ))
    .unwrap();
    owner.turn();
    assert!(matches!(
        owner.exact_retry.state,
        ExactRetryState::DraftStale { .. }
    ));
    assert_eq!(
        owner.dispatcher.recovery.mutations,
        vec![record(KnownAdmission::Ambiguous)]
    );
    tx.try_send(h).unwrap();
    owner.turn();
    owner.exact_retry.cancel_or_acknowledge();
    assert!(
        owner
            .exact_retry
            .can_begin(&owner.dispatcher, &owner.recovery_status, &identity)
    );
    assert!(rx.try_recv().is_err());
    let status = owner
        .recovery_status
        .check_status(
            &owner.dispatcher,
            owner.dispatcher.client().unwrap(),
            identity.clone(),
        )
        .unwrap();
    assert_eq!(sent(&rx), Sent::Status(status, identity.clone()));
    tx.try_send(reply(
        status,
        "operation_status",
        json!({"state":"outcome_unknown"}),
    ))
    .unwrap();
    owner.turn();
    assert!(
        owner
            .recovery_status
            .presentations(&owner.dispatcher)
            .iter()
            .any(|p| matches!(p.status, RecoveryStatusState::OutcomeUnknown))
    );
    owner
        .exact_retry
        .begin(&owner.dispatcher, &owner.recovery_status, identity.clone())
        .unwrap();
    let retry = owner
        .exact_retry
        .confirm(
            &owner.dispatcher,
            &owner.recovery_status,
            owner.dispatcher.client().unwrap(),
        )
        .unwrap();
    assert_eq!(sent(&rx), Sent::Retry(retry, identity));
    tx.try_send(ClientUpdate::RecoveryProjection {
        active: vec![record(KnownAdmission::Accepted)],
        quarantined: vec![],
    })
    .unwrap();
    owner.turn();
    assert!(matches!(
        owner.exact_retry.state,
        ExactRetryState::Accepted { .. }
    ));
    tx.try_send(ClientUpdate::State(
        crate::client::types::ConnectionState::Stale,
    ))
    .unwrap();
    owner.turn();
    assert!(matches!(
        owner.exact_retry.state,
        ExactRetryState::Interrupted { .. }
    ));
    for _ in 0..4 {
        assert_eq!(owner.turn().updates, 0);
    }
    assert!(rx.try_recv().is_err());
}

#[test]
fn owner_panic_remains_failure_on_repeated_root_shutdown() {
    let (owner, _, _, stops) = fixture();
    let mut pump = ParentPump::spawn_with(owner, PumpSignal::default(), |owner, _| {
        Ok(thread::spawn(move || {
            let _owner = owner;
            panic!("injected trusted owner panic")
        }))
    })
    .unwrap();
    assert_eq!(pump.shutdown(), Err("parent owner panicked"));
    assert_eq!(pump.shutdown(), Err("parent owner panicked"));
    assert_eq!(stops.load(Ordering::SeqCst), 1);
}

#[test]
fn unconfirmed_stop_retains_same_join_and_owner_until_later_observation() {
    let (owner, _, _, stops) = fixture();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let mut pump = ParentPump::spawn_with(owner, PumpSignal::default(), {
        let entered = entered.clone();
        let release = release.clone();
        move |mut owner, _| {
            thread::Builder::new().spawn(move || {
                entered.wait();
                release.wait();
                owner.shutdown().unwrap();
                owner
            })
        }
    })
    .unwrap();
    entered.wait();
    let join_id = pump.join.as_ref().unwrap().thread().id();
    assert_eq!(
        pump.shutdown_until(Instant::now()),
        Err("parent owner stop remains unconfirmed")
    );
    assert!(pump.join.is_some() && pump.stopped_owner.is_none());
    assert_eq!(
        pump.shutdown_until(Instant::now()),
        Err("parent owner stop remains unconfirmed")
    );
    assert_eq!(pump.join.as_ref().unwrap().thread().id(), join_id);
    assert!(pump.stopped_owner.is_none());
    assert_eq!(stops.load(Ordering::SeqCst), 0);
    release.wait();
    pump.shutdown().unwrap();
    pump.shutdown().unwrap();
    assert!(pump.join.is_none());
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    drop(pump); // Confirmed owner destruction cannot stop the client again.
    assert_eq!(stops.load(Ordering::SeqCst), 1);
}
fn hello() -> ClientUpdate {
    ClientUpdate::Hello(HelloState {
        boot_id: "boot".into(),
        scope: "scope".into(),
        next_seq: 9,
        operations: [
            "discover",
            "discovery_page",
            "measurements_current",
            "measurements_page",
            "reference",
            "subscribe",
            "operation_status",
            "reference_retune",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        capabilities: json!([]),
        limits: json!({}),
        event_oldest: EventCursor {
            boot_id: "boot".into(),
            seq: 0,
        },
        event_latest: EventCursor {
            boot_id: "boot".into(),
            seq: 1,
        },
    })
}
fn reply(id: u64, op: &str, result: Value) -> ClientUpdate {
    ClientUpdate::Reply {
        command_id: id,
        msg_id: id.to_string(),
        op: op.into(),
        kind: ReplyKind::Result,
        envelope: json!({"v":1,"type":"result","op":op,"result":result}),
        recovery: None,
    }
}
fn send_update(tx: &SyncSender<ClientUpdate>, signal: &PumpSignal, update: ClientUpdate) {
    tx.try_send(update).unwrap();
    signal.wake();
}
fn sent(rx: &Receiver<Sent>) -> Sent {
    rx.recv_timeout(Duration::from_secs(2)).unwrap()
}
fn query(rx: &Receiver<Sent>, op: &str) -> u64 {
    let Sent::Query(id, actual) = sent(rx) else {
        panic!("expected query")
    };
    assert_eq!(actual, op);
    id
}
fn bootstrap(tx: &SyncSender<ClientUpdate>, rx: &Receiver<Sent>, signal: &PumpSignal) {
    send_update(tx, signal, hello());
    let id = query(rx, "discover");
    send_update(
        tx,
        signal,
        reply(
            id,
            "discover",
            json!({"projection":"d","revision":{"boot_id":"boot","event_seq":"2"},"records":[],"next_index":null,"complete":true}),
        ),
    );
    let id = query(rx, "measurements_current");
    send_update(
        tx,
        signal,
        reply(
            id,
            "measurements_current",
            json!({"projection":"m","revision":{"boot_id":"boot","event_seq":"3"},"records":[],"next_index":null,"complete":true}),
        ),
    );
    let id = query(rx, "discover");
    send_update(
        tx,
        signal,
        reply(
            id,
            "discover",
            json!({"projection":"f","revision":{"boot_id":"boot","event_seq":"5"},"records":[],"next_index":null,"complete":true}),
        ),
    );
    let Sent::Subscribe(id) = sent(rx) else {
        panic!("expected subscribe")
    };
    send_update(
        tx,
        signal,
        reply(
            id,
            "subscribe",
            json!({"subscription":"s","accepted_cursor":{"boot_id":"boot","seq":"2"}}),
        ),
    );
    send_update(
        tx,
        signal,
        ClientUpdate::SubscriptionProgress(json!({"cursor":{"boot_id":"boot","seq":"5"}})),
    );
    signal.wait_updates(6);
}
fn open_api(owner: &mut ParentOwner<FakeClient>, signal: &PumpSignal) -> BufReader<TcpStream> {
    let prepared = PreparedEndpoint::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = prepared.address();
    owner.endpoint = Some(WorkbenchEndpoint::start(prepared, signal.callback()).unwrap());
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    BufReader::new(stream)
}
fn request(stream: &mut BufReader<TcpStream>, id: &str, op: &str, args: Value) {
    writeln!(
        stream.get_mut(),
        "{}",
        json!({"v":1,"type":"request","call_id":id,"op":op,"args":args})
    )
    .unwrap();
}
fn response(stream: &mut BufReader<TcpStream>, id: &str) -> Value {
    for _ in 0..32 {
        let mut line = String::new();
        assert_ne!(stream.read_line(&mut line).unwrap(), 0);
        let value: Value = serde_json::from_str(&line).unwrap();
        if value.get("call_id").and_then(Value::as_str) == Some(id) {
            return value;
        }
    }
    panic!("bounded response frame count exceeded")
}
fn record(admission: KnownAdmission) -> RecoveryRecord {
    RecoveryRecord {
        boot_id: "boot".into(),
        identity: MutationIdentity {
            scope: "scope".into(),
            seq: 7,
        },
        op: "reference_retune".into(),
        args: json!({"reference":"1","target":2.0,"rate":1.0}),
        admission,
    }
}

#[test]
fn stopped_renderer_serves_api_updates_recovery_without_replays() {
    let (mut owner, tx, rx, stops) = fixture();
    let signal = PumpSignal::default();
    let mut stream = open_api(&mut owner, &signal);
    // A deterministic fake renderer/event loop remains blocked for the entire
    // experiment. It never receives a model callback or owns the parent handle.
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let renderer = {
        let entered = entered.clone();
        let release = release.clone();
        thread::spawn(move || {
            entered.wait();
            release.wait();
        })
    };
    entered.wait();
    let mut pump = ParentPump::spawn(owner, signal.clone()).unwrap();
    request(&mut stream, "h", "hello", json!({}));
    let h = response(&mut stream, "h");
    assert_eq!(h["result"]["protocol"]["version"], 1);
    bootstrap(&tx, &rx, &signal);
    send_update(
        &tx,
        &signal,
        ClientUpdate::RecoveryProjection {
            active: vec![record(KnownAdmission::Ambiguous)],
            quarantined: vec![],
        },
    );
    signal.wait_updates(7);
    request(&mut stream, "s", "client_status", json!({}));
    let status = response(&mut stream, "s");
    assert_eq!(status["result"]["runtime_client"]["freshness"], "fresh");
    assert_eq!(status["result"]["recovery"]["active_count"], "1");
    request(&mut stream, "g", "presentation_get", json!({}));
    let g = response(&mut stream, "g");
    let id = g["result"]["workbench_id"].clone();
    assert_eq!(g["result"]["presentation_revision"], "1");
    let doc = PresentationDocument::empty("replaced");
    request(
        &mut stream,
        "r",
        "ui_replace_document",
        json!({"expected":{"workbench_id":id,"revision":"1"},"document":doc}),
    );
    assert_eq!(
        response(&mut stream, "r")["result"]["presentation_revision"],
        "2"
    );
    request(
        &mut stream,
        "conflict",
        "ui_replace_document",
        json!({"expected":{"workbench_id":id,"revision":"1"},"document":PresentationDocument::empty("wrong")}),
    );
    assert_eq!(
        response(&mut stream, "conflict")["error"]["code"],
        "revision_conflict"
    );
    request(
        &mut stream,
        "rec",
        "recovery_get",
        json!({"kind":"active","index":"0","expected":null}),
    );
    assert_eq!(
        response(&mut stream, "rec")["result"]["record"]["admission"],
        "ambiguous"
    );
    assert!(rx.try_recv().is_err());
    release.wait();
    renderer.join().unwrap(); // Display lifetime ends, parent stays.
    request(&mut stream, "after-display", "client_status", json!({}));
    assert_eq!(
        response(&mut stream, "after-display")["result"]["presentation"]["presentation_revision"],
        "2"
    );
    assert_eq!(stops.load(Ordering::SeqCst), 0);
    pump.shutdown().unwrap();
    pump.shutdown().unwrap();
    let owner = pump.stopped_owner.as_ref().unwrap();
    assert_eq!(
        owner.dispatcher.recovery.mutations[0],
        record(KnownAdmission::Ambiguous)
    );
    assert!(matches!(owner.exact_retry.state, ExactRetryState::Idle));
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert!(rx.try_recv().is_err());
}

#[test]
fn independent_wake_coalesces_and_publish_during_turn_is_not_lost() {
    assert_publish_during_real_turn(false);
}

#[cfg(windows)]
#[test]
fn stalled_supervisor_keeps_parent_updates_api_and_recovery_independent() {
    use crate::supervisor::{Phase, Policy, Stop, tests::StalledBackend};
    let (mut owner, tx, rx, stops) = fixture();
    let signal = PumpSignal::default();
    let mut stream = open_api(&mut owner, &signal);
    let mut pump = ParentPump::spawn(owner, signal.clone()).unwrap();
    request(&mut stream, "hello", "hello", json!({}));
    response(&mut stream, "hello");
    bootstrap(&tx, &rx, &signal);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    pump.install_supervisor(
        {
            let entered = entered.clone();
            let release = release.clone();
            move || StalledBackend::new(entered, release)
        },
        Policy::default(),
    )
    .unwrap();
    pump.supervisor()
        .unwrap()
        .restart(crate::supervisor::tests::launch())
        .unwrap();
    entered.wait(); // Real supervisor worker is blocked inside prepare.
    assert_eq!(
        pump.supervisor().unwrap().status().unwrap().phase,
        Phase::Starting
    );
    assert_eq!(
        pump.install_supervisor(
            || StalledBackend::new(Arc::new(Barrier::new(1)), Arc::new(Barrier::new(1))),
            Policy::default()
        )
        .err()
        .unwrap()
        .kind(),
        io::ErrorKind::AlreadyExists
    );
    send_update(
        &tx,
        &signal,
        ClientUpdate::RecoveryProjection {
            active: vec![record(KnownAdmission::Accepted)],
            quarantined: vec![],
        },
    );
    signal.wait_updates(7);
    request(&mut stream, "status-stalled", "client_status", json!({}));
    let status = response(&mut stream, "status-stalled");
    assert_eq!(status["result"]["runtime_client"]["freshness"], "fresh");
    request(&mut stream, "get-stalled", "presentation_get", json!({}));
    let document = response(&mut stream, "get-stalled");
    request(
        &mut stream,
        "replace-stalled",
        "ui_replace_document",
        json!({
            "expected": {"workbench_id":document["result"]["workbench_id"],"revision":"1"},
            "document":PresentationDocument::empty("while-os-stalled")
        }),
    );
    assert_eq!(
        response(&mut stream, "replace-stalled")["result"]["presentation_revision"],
        "2"
    );
    assert!(rx.try_recv().is_err()); // No mutation/status/retry from child lifetime.
    assert_eq!(stops.load(Ordering::SeqCst), 0);
    pump.supervisor().unwrap().stop(Stop::Graceful).unwrap();
    release.wait(); // Stop during prepare wins before resume.
    pump.supervisor()
        .unwrap()
        .wait_status(|s| s.phase == Phase::ExitedContained);
    request(&mut stream, "after-child", "client_status", json!({}));
    assert_eq!(
        response(&mut stream, "after-child")["result"]["presentation"]["presentation_revision"],
        "2"
    );
    assert_eq!(stops.load(Ordering::SeqCst), 0);
    pump.supervisor().unwrap().shutdown();
    pump.supervisor()
        .unwrap()
        .wait_status(|s| s.worker_finished);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !pump.supervisor().unwrap().worker_finished() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    pump.shutdown().unwrap();
    pump.shutdown().unwrap();
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert!(rx.try_recv().is_err());
    assert_eq!(
        pump.stopped_owner
            .as_ref()
            .unwrap()
            .dispatcher
            .recovery
            .mutations,
        vec![record(KnownAdmission::Accepted)]
    );
}

#[test]
fn stop_precedes_pending_wake_after_real_owner_turn() {
    assert_publish_during_real_turn(true);
}

#[cfg(windows)]
#[test]
fn parent_shutdown_does_not_acknowledge_stalled_supervisor_or_repeat_client_stop() {
    use crate::supervisor::{Policy, tests::StalledBackend};
    let (owner, _, rx, stops) = fixture();
    let mut pump = ParentPump::spawn(owner, PumpSignal::default()).unwrap();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    pump.install_supervisor(
        {
            let entered = entered.clone();
            let release = release.clone();
            move || StalledBackend::new(entered, release)
        },
        Policy::default(),
    )
    .unwrap();
    pump.supervisor()
        .unwrap()
        .restart(crate::supervisor::tests::launch())
        .unwrap();
    entered.wait();
    assert_eq!(
        pump.shutdown(),
        Err("supervisor child exit remains unconfirmed")
    );
    assert_eq!(
        pump.shutdown(),
        Err("supervisor child exit remains unconfirmed")
    );
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert!(!pump.supervisor().unwrap().worker_finished());
    assert!(pump.stopped_owner.is_some() && pump.join.is_none());
    assert_eq!(
        pump.supervisor()
            .unwrap()
            .restart(crate::supervisor::tests::launch()),
        Err(crate::supervisor::Rejected::Closed)
    );
    release.wait();
    pump.supervisor()
        .unwrap()
        .wait_status(|s| s.worker_finished);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !pump.supervisor().unwrap().worker_finished() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    pump.shutdown().unwrap();
    pump.shutdown().unwrap();
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert!(rx.try_recv().is_err());
}

fn assert_publish_during_real_turn(stop: bool) {
    let (owner, tx, submissions, stops) = fixture();
    let signal = PumpSignal::default();
    for _ in 0..10_000 {
        signal.wake();
    }
    assert!(signal.0.0.lock().unwrap().pending);
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    owner
        .dispatcher
        .client()
        .unwrap()
        .pause
        .replace(Some(ReceivePause {
            entered: entered.clone(),
            release: release.clone(),
            polls: 0,
        }));
    tx.try_send(ClientUpdate::RecoveryJournalProblem {
        reason: "first".into(),
    })
    .unwrap();
    let mut pump = ParentPump::spawn(owner, signal.clone()).unwrap();
    let producer = {
        let signal = signal.clone();
        thread::spawn(move || {
            entered.wait(); // First update dequeued inside ParentOwner::turn().
            {
                let state = signal.0.0.lock().unwrap();
                assert!(!state.pending); // Initial coalesced wake cleared before turn.
                assert_eq!(state.applied_updates, 0);
            }
            tx.try_send(ClientUpdate::RecoveryJournalProblem {
                reason: "second".into(),
            })
            .unwrap();
            signal.wake();
            assert!(signal.0.0.lock().unwrap().pending);
            if stop {
                signal.stop();
            }
            release.wait();
        })
    };
    producer.join().unwrap();
    signal.wait_updates(if stop { 1 } else { 2 });
    pump.shutdown().unwrap();
    pump.shutdown().unwrap();
    let state = signal.0.0.lock().unwrap();
    let first = state.first_turns[0].as_ref().unwrap();
    assert_eq!(
        first.report,
        TurnReport {
            updates: 1,
            rebuild_steps: 1,
            external_calls: 0
        }
    );
    assert_eq!(first.client_error.as_deref(), Some("first"));
    assert_eq!(first.idle_timeouts, 0);
    if stop {
        assert_eq!(state.completed_turns, 1);
        assert_eq!(state.applied_updates, 1);
        assert!(state.pending); // Stop won; next() did not consume pending wake.
        assert!(state.first_turns[1].is_none());
    } else {
        let second = state.first_turns[1].as_ref().unwrap();
        assert_eq!(second.report, first.report);
        assert_eq!(second.client_error.as_deref(), Some("second"));
        assert_eq!(second.idle_timeouts, 0); // A lost wake cannot pass via idle fallback.
        assert_eq!(state.applied_updates, 2);
    }
    assert_eq!(
        pump.stopped_owner
            .as_ref()
            .unwrap()
            .dispatcher
            .client_error
            .as_deref(),
        Some(if stop { "first" } else { "second" })
    );
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert!(submissions.try_recv().is_err());
}

#[test]
fn owner_shutdown_is_idempotent_and_start_failure_stops_single_client() {
    let (owner, _, _, stops) = fixture();
    let result = ParentPump::spawn_with(owner, PumpSignal::default(), |_, _| {
        Err(io::Error::other("injected thread start failure"))
    });
    assert!(result.is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    let (mut owner, tx, _, stops) = fixture();
    owner.shutdown().unwrap();
    owner.shutdown().unwrap();
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert_eq!(owner.turn(), TurnReport::default());
    assert!(tx.try_send(ClientUpdate::WorkerStopped).is_err());
}

#[test]
fn native_constructor_rejects_invalid_startup_before_worker_or_endpoint() {
    let result = ParentPump::connect(
        RuntimeEndpoint::Tcp {
            address: "192.0.2.1:7420".parse().unwrap(),
            allow_remote: false,
        },
        true,
        None,
        std::env::temp_dir().join("unused-parent-invalid-endpoint.json"),
        PresentationDocument::empty("invalid"),
        None,
    );
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn idle_turn_waits_without_gpu_wake_or_busy_spin() {
    let signal = PumpSignal::default();
    let begin = Instant::now();
    assert!(signal.next()); // No wake producer: Condvar must reach idle timeout.
    assert!(begin.elapsed() >= Duration::from_millis(80));
    assert!(!signal.0.0.lock().unwrap().pending);
    signal.stop();
    assert!(!signal.next());
}

#[test]
fn root_shutdown_error_is_preserved_without_second_worker_stop() {
    let (owner, _, _, stops) = fixture();
    owner.dispatcher.client().unwrap().fail_stop.set(true);
    let mut pump = ParentPump::spawn(owner, PumpSignal::default()).unwrap();
    assert_eq!(pump.shutdown(), Err("injected client stop failure"));
    assert_eq!(pump.shutdown(), Err("injected client stop failure"));
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        pump.stopped_owner
            .as_ref()
            .unwrap()
            .dispatcher
            .client_error
            .as_deref(),
        Some("injected client stop failure")
    );
}

#[test]
fn partial_input_and_detach_keep_accepted_work_and_unknown_status_exact() {
    let (mut owner, tx, rx, stops) = fixture();
    let signal = PumpSignal::default();
    let mut stream = open_api(&mut owner, &signal);
    let address = stream.get_ref().peer_addr().unwrap();
    let mut pump = ParentPump::spawn(owner, signal.clone()).unwrap();
    // Splitting physical reads cannot invent another owner request.
    stream
        .get_mut()
        .write_all(b"{\"v\":1,\"type\":\"request\",\"call_id\":\"h\",")
        .unwrap();
    stream
        .get_mut()
        .write_all(b"\"op\":\"hello\",\"args\":{}}\n")
        .unwrap();
    response(&mut stream, "h");
    bootstrap(&tx, &rx, &signal);
    request(
        &mut stream,
        "mut",
        "lab_mutation",
        json!({"op":"reference_retune","args":{"reference":"1","target":2.0,"rate":1.0}}),
    );
    let Sent::Mutation(id, op) = sent(&rx) else {
        panic!("expected one explicit mutation")
    };
    assert_eq!(op, "reference_retune");
    assert_eq!(response(&mut stream, "mut")["result"]["state"], "submitted");
    let mut accepted = reply(id, &op, json!({"state":"accepted"}));
    if let ClientUpdate::Reply { kind, recovery, .. } = &mut accepted {
        *kind = ReplyKind::MutationAccepted;
        *recovery = Some(record(KnownAdmission::Accepted));
    }
    send_update(&tx, &signal, accepted);
    send_update(
        &tx,
        &signal,
        ClientUpdate::RecoveryProjection {
            active: vec![record(KnownAdmission::Accepted)],
            quarantined: vec![],
        },
    );
    signal.wait_updates(8);
    drop(stream); // Correlation detach cannot cancel or locally reject acceptance.
    send_update(
        &tx,
        &signal,
        ClientUpdate::RecoveryProjection {
            active: vec![record(KnownAdmission::Ambiguous)],
            quarantined: vec![],
        },
    );
    signal.wait_updates(9);
    // Connect a new external caller to the same endpoint (no new Runtime client).
    let raw = TcpStream::connect(address).unwrap();
    raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut caller = BufReader::new(raw);
    request(&mut caller, "h2", "hello", json!({}));
    response(&mut caller, "h2");
    request(&mut caller, "s", "client_status", json!({}));
    let status = response(&mut caller, "s");
    let target = json!({"workbench_id":status["result"]["workbench_id"],"recovery_generation":status["result"]["recovery"]["recovery_generation"],"boot_id":"boot","request_id":{"scope":"scope","seq":"7"}});
    request(
        &mut caller,
        "status",
        "lab_operation_status",
        json!({"target":target}),
    );
    let Sent::Status(status_id, identity) = sent(&rx) else {
        panic!("expected one explicit status")
    };
    assert_eq!(identity, record(KnownAdmission::Ambiguous).identity);
    assert_eq!(
        response(&mut caller, "status")["result"]["state"],
        "submitted"
    );
    send_update(
        &tx,
        &signal,
        reply(
            status_id,
            "operation_status",
            json!({"state":"outcome_unknown"}),
        ),
    );
    signal.wait_updates(10);
    pump.shutdown().unwrap();
    let owner = pump.stopped_owner.as_ref().unwrap();
    assert_eq!(
        owner.dispatcher.recovery.mutations,
        vec![record(KnownAdmission::Ambiguous)]
    );
    assert!(rx.try_recv().is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 1);
}

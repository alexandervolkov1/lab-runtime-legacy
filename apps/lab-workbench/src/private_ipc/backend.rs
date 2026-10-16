//! One supervisor-owned Job and two pipe endpoints. No additional I/O thread.
use super::{
    controls::{Envelope, Feedback, Request},
    protocol::*,
    reads,
    transport::{Framed, failure},
};
use crate::{
    presentation::v2,
    supervisor::{Backend, Launch, Progress},
};
use lab_windows_job::{Error, Job, Objects, Reading, pipe::Pipe};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Status {
    pub(crate) incarnation: u64,
    pub(crate) window_close_requested: bool,
    pub(crate) connected: bool,
    pub(crate) handshaken: bool,
    pub(crate) applied: [u64; 2],
    pub(crate) pending_io: usize,
    pub(crate) handles: usize,
    pub(crate) io_bytes: usize,
    pub(crate) coalesced: u64,
    pub(crate) diagnostic_records: u64,
    pub(crate) turns: u64,
    pub(crate) received_frames: u64,
    pub(crate) child_read_current: bool,
    pub(crate) renderer: Option<super::renderer_progress::RendererProgress>,
    #[cfg(test)]
    pub(crate) sampled_commit: Option<Reading>,
    #[cfg(test)]
    pub(crate) io_held: bool,
    pub(crate) retiring: bool,
    pub(crate) retired: bool,
    pub(crate) credits: [usize; 5],
    pub(crate) error: Option<&'static str>,
}
struct Shared {
    pool: Pool,
    status: Status,
    intent: Option<Envelope>,
    feedback: Option<Vec<u8>>,
}
/// Parent root holds this publication handle, not a Runtime socket/worker clone.
#[derive(Clone)]
pub(crate) struct Bridge {
    identity: [u8; 16],
    shared: Arc<Mutex<Shared>>,
    worker_claimed: Arc<AtomicBool>,
    wanted: Arc<[AtomicU64; 2]>,
}
impl Bridge {
    #[cfg(test)]
    pub(crate) fn control_test_incarnation(&self, incarnation: u64) {
        let mut shared = self.shared.lock().unwrap();
        shared.status.incarnation = incarnation;
        shared.status.handshaken = true;
        shared.status.connected = true;
        shared.status.retiring = false;
        shared.status.retired = false;
        shared.intent = None;
        shared.feedback = None;
    }
    #[cfg(test)]
    pub(crate) fn control_test_request(&self, envelope: Envelope) {
        let mut shared = self.shared.lock().unwrap();
        assert!(shared.intent.is_none());
        shared.intent = Some(envelope);
    }
    /// Capacity-one, take-once operator mailbox. No automatic replay on restart.
    pub(crate) fn take_intent(&self) -> Result<Option<Envelope>> {
        self.shared
            .try_lock()
            .map(|mut s| s.intent.take())
            .map_err(|_| "IPC metadata busy")
    }
    pub(crate) fn control_feedback(&self, feedback: &Feedback) -> Result<()> {
        let bytes = feedback.encode()?;
        self.shared
            .try_lock()
            .map_err(|_| "IPC metadata busy")?
            .feedback = Some(bytes);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn trace_provenance(&self) -> Vec<(Option<u64>, Option<f64>, usize)> {
        let shared = self.shared.lock().unwrap();
        let slot = shared
            .pool
            .slots
            .iter()
            .filter(|s| {
                matches!(
                    s.credit,
                    Credit::Latest(1) | Credit::Writing(1) | Credit::Current(1)
                )
            })
            .max_by_key(|s| s.snapshot)
            .unwrap();
        reads::inspect(&slot.bytes[..slot.len])
            .unwrap()
            .into_iter()
            .flatten()
            .map(|m| (m.generation, m.trace_from, m.points))
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn read_provenance(&self) -> Vec<(Option<String>, Option<u64>, u8)> {
        let shared = self.shared.lock().unwrap();
        let Some(slot) = shared
            .pool
            .slots
            .iter()
            .filter(|s| {
                matches!(
                    s.credit,
                    Credit::Latest(1) | Credit::Writing(1) | Credit::Current(1)
                )
            })
            .max_by_key(|s| s.snapshot)
        else {
            return vec![];
        };
        reads::inspect(&slot.bytes[..slot.len])
            .unwrap()
            .into_iter()
            .flatten()
            .map(|m| (m.boot.map(str::to_owned), m.revision, m.freshness))
            .collect()
    }
    pub(crate) fn identity(&self) -> [u8; 16] {
        self.identity
    }
    /// Scalar invalidation does not acquire a pool lock held by an OS call.
    pub(crate) fn announce(&self, revision: u64, source: u64) {
        self.wanted[0].fetch_max(revision, Ordering::Release);
        self.wanted[1].fetch_max(source, Ordering::Release);
    }
    fn wanted(&self) -> [u64; 2] {
        [
            self.wanted[0].load(Ordering::Acquire),
            self.wanted[1].load(Ordering::Acquire),
        ]
    }
    pub(crate) fn new(identity: [u8; 16]) -> Self {
        Self {
            identity,
            worker_claimed: Arc::new(AtomicBool::new(false)),
            wanted: Arc::new([AtomicU64::new(0), AtomicU64::new(0)]),
            shared: Arc::new(Mutex::new(Shared {
                pool: Pool::new(),
                status: Status::default(),
                intent: None,
                feedback: None,
            })),
        }
    }
    pub(crate) fn status(&self) -> Result<Status> {
        self.shared
            .try_lock()
            .map(|s| Status {
                credits: credit_counts(&s.pool),
                coalesced: s.pool.coalesced,
                ..s.status
            })
            .map_err(|_| "IPC metadata busy")
    }
    pub(crate) fn document(
        &self,
        document: &v2::AdmittedV2Document,
        epoch: Epoch,
        snapshot: u64,
    ) -> Result<()> {
        let bytes = document.canonical_bytes();
        if bytes.len() > SNAPSHOT {
            return Err("document bytes cap");
        }
        self.announce(epoch.revision, self.wanted()[1]);
        self.shared
            .try_lock()
            .map_err(|_| "IPC metadata busy")?
            .pool
            .publish(0, epoch, snapshot, 0, bytes.len(), |out| {
                out.copy_from_slice(bytes)
            })
    }
    pub(crate) fn observations(
        &self,
        observations: &crate::model::RuntimeObservations,
        selected: &[reads::Selection<'_>],
        epoch: Epoch,
        snapshot: u64,
        source: u64,
    ) -> Result<()> {
        // Validate the complete immutable projection before overwriting a latest
        // slot. The count-only encoder needs no fifth assembly allocation.
        let plan = reads::plan(observations, selected)?;
        let mut shared = self.shared.try_lock().map_err(|_| "IPC metadata busy")?;
        for slot in &shared.pool.slots {
            if matches!(
                slot.credit,
                Credit::Latest(1) | Credit::Writing(1) | Credit::Current(1)
            ) {
                plan.successor_of(&slot.bytes[..slot.len], source > slot.source)?;
            }
        }
        shared
            .pool
            .publish(1, epoch, snapshot, source, plan.length(), |out| {
                plan.fill(out)
            })?;
        self.announce(epoch.revision, self.wanted()[1]);
        Ok(())
    }
    /// Reserve one worker before spawning. An unstarted factory releases this
    /// reservation; a started worker permanently owns the sole episode root.
    pub(crate) fn worker_factory(&self) -> Result<impl FnOnce() -> IpcBackend + Send + 'static> {
        self.worker_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "private worker already owned")?;
        let mut claim = WorkerClaim {
            bridge: self.clone(),
            started: false,
        };
        Ok(move || {
            claim.started = true;
            IpcBackend::new(claim.bridge.clone())
        })
    }
}
fn credit_counts(pool: &Pool) -> [usize; 5] {
    let mut counts = [0; 5];
    for slot in &pool.slots {
        counts[match slot.credit {
            Credit::Free => 0,
            Credit::Latest(_) => 1,
            Credit::Current(_) => 2,
            Credit::Writing(_) => 3,
            Credit::Assembly(_) => 4,
        }] += 1;
    }
    counts
}
struct WorkerClaim {
    bridge: Bridge,
    started: bool,
}
impl Drop for WorkerClaim {
    fn drop(&mut self) {
        if !self.started {
            self.bridge.worker_claimed.store(false, Ordering::Release);
        }
    }
}
struct Transfer {
    index: usize,
    stream: usize,
    offset: usize,
    chunk: usize,
    started: Instant,
}
pub(crate) struct IpcBackend {
    bridge: Bridge,
    job: Option<Job>,
    control: Option<Framed<CONTROL>>,
    snapshot: Option<Framed<CHUNK>>,
    identity: Identity,
    connected: [bool; 2],
    hello_sent: bool,
    hello_ack: bool,
    sequences: [Sequence; 2],
    challenge: Option<u64>,
    close: Option<u64>,
    close_sent: bool,
    close_received: bool,
    window_close_requested: bool,
    control_failed: bool,
    bulk_failed: bool,
    in_bulk: bool,
    sent_fence: Option<[u64; 2]>,
    progress: Option<Progress>,
    writing: Option<Transfer>,
    next_stream: usize,
    applied: [u64; 2],
    waiting: [Option<Header>; 2],
    applied_epoch: Option<Epoch>,
    pending_retirement: bool,
    retiring: bool,
    // Fixed bounded best-effort diagnostics, including metadata in each record.
    diagnostics: Box<[u8]>,
    diagnostic_cursor: usize,
    diagnostic_count: u64,
    turns: u64,
    received_frames: u64,
    child_read_current: bool,
    renderer: Option<super::renderer_progress::RendererProgress>,
    diagnostic_burst: usize,
    sent_feedback: Option<Vec<u8>>,
    #[cfg(test)]
    retirement_gate: Option<Arc<AtomicBool>>,
    #[cfg(test)]
    io_gate: Option<Arc<AtomicBool>>,
    #[cfg(test)]
    snapshot_fault: Option<Arc<std::sync::atomic::AtomicU8>>,
}
impl IpcBackend {
    fn new(bridge: Bridge) -> Self {
        Self {
            identity: Identity {
                workbench: bridge.identity,
                incarnation: 0,
            },
            bridge,
            job: None,
            control: None,
            snapshot: None,
            connected: [false; 2],
            hello_sent: false,
            hello_ack: false,
            sequences: Default::default(),
            challenge: None,
            close: None,
            close_sent: false,
            close_received: false,
            window_close_requested: false,
            control_failed: false,
            bulk_failed: false,
            in_bulk: false,
            sent_fence: None,
            progress: None,
            writing: None,
            next_stream: 0,
            applied: [0; 2],
            waiting: [None; 2],
            applied_epoch: None,
            pending_retirement: false,
            retiring: false,
            diagnostics: vec![0; DIAGNOSTICS * DIAGNOSTIC].into_boxed_slice(),
            diagnostic_cursor: 0,
            diagnostic_count: 0,
            turns: 0,
            received_frames: 0,
            child_read_current: false,
            renderer: None,
            diagnostic_burst: 0,
            sent_feedback: None,
            #[cfg(test)]
            retirement_gate: None,
            #[cfg(test)]
            io_gate: None,
            #[cfg(test)]
            snapshot_fault: None,
        }
    }
    fn publish(&self, error: Option<&'static str>) {
        #[cfg(test)]
        let sampled_commit = self.job.as_ref().and_then(|job| job.verify_limits().ok());
        if let Ok(mut shared) = self.bridge.shared.lock() {
            let credits = credit_counts(&shared.pool);
            let previous_error = shared.status.error;
            shared.status = Status {
                incarnation: self.identity.incarnation,
                window_close_requested: self.window_close_requested,
                connected: self.connected == [true; 2],
                handshaken: self.hello_ack,
                applied: self.applied,
                pending_io: self.control.as_ref().map_or(0, |s| s.pipe.pending())
                    + self.snapshot.as_ref().map_or(0, |s| s.pipe.pending()),
                handles: self.control.as_ref().map_or(0, |s| s.pipe.handles())
                    + self.snapshot.as_ref().map_or(0, |s| s.pipe.handles()),
                io_bytes: self.control.as_ref().map_or(0, |s| s.pipe.buffer_bytes())
                    + self.snapshot.as_ref().map_or(0, |s| s.pipe.buffer_bytes()),
                coalesced: shared.pool.coalesced,
                diagnostic_records: self.diagnostic_count,
                turns: self.turns,
                received_frames: self.received_frames,
                child_read_current: self.child_read_current,
                renderer: self.renderer,
                #[cfg(test)]
                sampled_commit,
                #[cfg(test)]
                io_held: self
                    .io_gate
                    .as_ref()
                    .is_some_and(|gate| !gate.load(Ordering::Acquire)),
                retiring: self.retiring,
                retired: self.control.is_none() && self.snapshot.is_none() && self.job.is_none(),
                credits,
                error: previous_error.or(error),
            };
        }
    }
    fn pump(&mut self) -> std::result::Result<(), Error> {
        if self.retiring {
            return Ok(());
        }
        if self.close_received {
            return Ok(());
        }
        #[cfg(test)]
        if self
            .io_gate
            .as_ref()
            .is_some_and(|gate| !gate.load(Ordering::Acquire))
        {
            return Ok(());
        }
        self.turns = self.turns.saturating_add(1);
        self.in_bulk = false;
        let pid = self
            .job
            .as_ref()
            .and_then(Job::pid)
            .ok_or(failure("missing original child"))?;
        let control = self
            .control
            .as_mut()
            .ok_or(failure("missing control endpoint"))?;
        let snapshot = self
            .snapshot
            .as_mut()
            .ok_or(failure("missing snapshot endpoint"))?;
        if !self.connected[0] {
            self.connected[0] = control.pipe.connected(pid)?;
        }
        if !self.connected[1] {
            self.connected[1] = snapshot.pipe.connected(pid)?;
        }
        if self.connected != [true; 2] {
            return Ok(());
        }
        control.poll_send()?;
        // At most four bounded receive steps; never more than four control frames
        // or 16 KiB, independently of one snapshot TX chunk. Draining a checked
        // close receipt can finish before the separate process-exit observation.
        for _ in 0..OWNER_MESSAGES {
            if self.control_failed {
                break;
            }
            if let Some((header, payload)) = control.receive(self.identity, Instant::now())? {
                self.received_frames = self.received_frames.saturating_add(1);
                match header.kind {
                    Kind::HelloAck if self.hello_sent && !self.hello_ack && payload.is_empty() => {
                        self.hello_ack = true
                    }
                    Kind::WindowClose if self.hello_ack && payload.is_empty() => {
                        self.window_close_requested = true;
                    }
                    Kind::ControlIntent
                        if self.hello_ack && !self.close_sent && !self.window_close_requested =>
                    {
                        let request = Request::decode(payload).map_err(failure)?;
                        let mut shared = self.bridge.shared.lock().unwrap();
                        if shared.intent.is_some() {
                            return Err(failure("operator intent credit exhausted"));
                        }
                        shared.intent = Some(Envelope {
                            identity: self.identity,
                            request,
                        });
                    }
                    Kind::Status | Kind::Closed if self.hello_ack && payload.len() == 32 => {
                        if payload[24] > 1 || payload[25..] != [0; 7] {
                            return Err(failure("private status flags"));
                        }
                        let read = u64::from_le_bytes(payload[16..24].try_into().unwrap());
                        self.child_read_current =
                            payload[24] != 0 && read != 0 && read == self.applied[1];
                        if header.kind == Kind::Closed && !self.close_sent {
                            return Err(failure("unsolicited close receipt"));
                        }
                        self.diagnostic_burst = 0;
                        let challenge = u64::from_le_bytes(payload[..8].try_into().unwrap());
                        let owner_counter = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                        // Handshake alone is not Ready. At least one parent document
                        // must have passed child admission before owner progress counts.
                        if self.applied[0] != 0 {
                            self.progress = Some(Progress {
                                incarnation: self.identity.incarnation,
                                challenge,
                                owner_counter,
                                healthy_close: header.kind == Kind::Closed,
                            });
                        }
                        if header.kind == Kind::Closed {
                            self.close_received = true;
                            break;
                        }
                    }
                    Kind::Applied | Kind::Skipped
                        if self.hello_ack && payload.len() == 1 && payload[0] <= 1 =>
                    {
                        let stream = payload[0] as usize;
                        if self.waiting[stream].is_none_or(|sent| {
                            sent.snapshot != header.snapshot
                                || sent.epoch != header.epoch
                                || sent.source != header.source
                        }) || header.snapshot <= self.applied[stream]
                        {
                            return Err(failure("unfenced applied receipt"));
                        }
                        if header.kind == Kind::Applied {
                            self.applied[stream] = header.snapshot;
                        }
                        self.waiting[stream] = None;
                        if stream == 0 && header.kind == Kind::Applied {
                            self.applied_epoch = Some(header.epoch);
                        }
                    }
                    Kind::Diagnostic if self.hello_ack && payload.len() + 16 <= DIAGNOSTIC => {
                        if let Some(progress) =
                            super::renderer_progress::RendererProgress::decode(payload)
                        {
                            self.renderer = Some(progress);
                        }
                        // Allow four records between owner status messages. Kernel
                        // pipe storage is not an application queue: never let a
                        // diagnostic flood continuously precede close/heartbeat.
                        self.diagnostic_burst += 1;
                        if self.diagnostic_burst > OWNER_MESSAGES {
                            return Err(failure("private control flood"));
                        }
                        let start = self.diagnostic_cursor * DIAGNOSTIC;
                        self.diagnostics[start..start + DIAGNOSTIC].fill(0);
                        self.diagnostics[start..start + 8]
                            .copy_from_slice(&self.identity.incarnation.to_le_bytes());
                        self.diagnostics[start + 8..start + 16]
                            .copy_from_slice(&header.sequence.to_le_bytes());
                        self.diagnostics[start + 16..start + 16 + payload.len()]
                            .copy_from_slice(payload);
                        self.diagnostic_cursor = (self.diagnostic_cursor + 1) % DIAGNOSTICS;
                        self.diagnostic_count = self.diagnostic_count.saturating_add(1);
                    }
                    _ => return Err(failure("private control order/direction")),
                }
            }
        }
        if self.close_received {
            return Ok(());
        }
        if control.idle() {
            let wanted = self.bridge.wanted();
            if self.hello_ack
                && self.close.is_none()
                && self.challenge.is_none()
                && wanted[0] != 0
                && self.sent_fence != Some(wanted)
                && !self.control_failed
            {
                let mut header = Header::control(
                    Kind::Fence,
                    self.identity,
                    self.sequences[0].next().map_err(failure)?,
                );
                header.bytes += 16;
                let mut payload = [0; 16];
                payload[..8].copy_from_slice(&wanted[0].to_le_bytes());
                payload[8..].copy_from_slice(&wanted[1].to_le_bytes());
                control.send(header, &payload)?;
                self.sent_fence = Some(wanted);
            }
        }
        if control.idle() {
            let next = if !self.hello_sent {
                self.hello_sent = true;
                Some((Kind::Hello, None))
            } else if self.hello_ack {
                self.close
                    .take()
                    .map(|n| (Kind::Close, Some(n)))
                    .or_else(|| self.challenge.take().map(|n| (Kind::Challenge, Some(n))))
            } else {
                None
            };
            if let Some((kind, number)) = next {
                let mut header = Header::control(
                    kind,
                    self.identity,
                    self.sequences[0].next().map_err(failure)?,
                );
                if let Some(number) = number {
                    header.bytes += 8;
                    control.send(header, &number.to_le_bytes())?;
                    if kind == Kind::Close {
                        self.close_sent = true;
                    }
                } else {
                    control.send(header, &[])?;
                }
            }
        }
        if self.control_failed || self.bulk_failed {
            return Ok(());
        }
        // Owner challenges/close/fences have priority over coalesced operator UI.
        if control.idle() && self.hello_ack && !self.close_sent {
            let shared = self.bridge.shared.lock().unwrap();
            if let Some(bytes) = &shared.feedback
                && self.sent_feedback.as_ref() != Some(bytes)
            {
                let mut header = Header::control(
                    Kind::ControlFeedback,
                    self.identity,
                    self.sequences[0].next().map_err(failure)?,
                );
                header.bytes += bytes.len();
                control.send(header, bytes)?;
                self.sent_feedback = Some(bytes.clone());
            }
        }
        self.in_bulk = true;
        // A bulk deadline never prevents the independent control path from
        // servicing the already queued close/owner challenge on this turn.
        if self
            .writing
            .as_ref()
            .is_some_and(|t| t.started.elapsed() >= std::time::Duration::from_secs(2))
        {
            return Err(failure("incomplete TX snapshot deadline"));
        }
        // One chunk completion/submission per turn; snapshot stalls never gate control.
        if snapshot.poll_send()?
            && let Some(writing) = &mut self.writing
        {
            writing.offset += writing.chunk;
        }
        if !snapshot.idle() || !self.hello_ack {
            return Ok(());
        }
        let Ok(mut shared) = self.bridge.shared.try_lock() else {
            return Ok(());
        };
        if let Some(writing) = &self.writing
            && writing.offset == shared.pool.slots[writing.index].len
        {
            shared
                .pool
                .complete_write(writing.index, writing.stream)
                .map_err(failure)?;
            self.next_stream = 1 - writing.stream;
            self.writing = None;
        }
        if self.writing.is_none() {
            for stream in [self.next_stream, 1 - self.next_stream] {
                if self.waiting[stream].is_some() {
                    continue;
                }
                // Document admission receipt fences reads for its exact epoch.
                if stream == 1
                    && !shared.pool.slots.iter().any(|slot| {
                        slot.credit == Credit::Latest(1)
                            && Some(slot.epoch) == self.applied_epoch
                            && slot.source >= self.bridge.wanted()[1]
                    })
                {
                    continue;
                }
                if let Some(index) = shared.pool.start_write(stream) {
                    self.writing = Some(Transfer {
                        index,
                        stream,
                        offset: 0,
                        chunk: 0,
                        started: Instant::now(),
                    });
                    break;
                }
            }
        }
        if let Some(writing) = &mut self.writing {
            let slot = &shared.pool.slots[writing.index];
            let count = (slot.len - writing.offset).min(CHUNK - HEADER);
            let header = Header {
                kind: if writing.stream == 0 {
                    Kind::Document
                } else {
                    Kind::Observations
                },
                bytes: HEADER + count,
                total: slot.len,
                identity: self.identity,
                sequence: self.sequences[1].next().map_err(failure)?,
                epoch: slot.epoch,
                snapshot: slot.snapshot,
                offset: writing.offset,
                source: slot.source,
            };
            snapshot.send(header, &slot.bytes[writing.offset..writing.offset + count])?;
            // A peer receipt can arrive before we poll our own TX completion.
            // Fence it now, while retaining Writing credit until that completion.
            if writing.offset + count == slot.len {
                self.waiting[writing.stream] = Some(header);
            }
            writing.chunk = count;
        }
        Ok(())
    }
}
impl Backend for IpcBackend {
    fn service(&mut self) -> std::result::Result<(), Error> {
        let result = self.pump();
        if result.is_err() {
            if self.in_bulk {
                self.bulk_failed = true;
            } else {
                self.control_failed = true;
            }
        }
        self.publish(result.as_ref().err().map(|e| e.operation));
        result
    }
    fn prepare(&mut self, launch: &Launch, incarnation: u64) -> std::result::Result<(), Error> {
        launch.validate()?;
        if self.job.is_some()
            || self.control.is_some()
            || self.snapshot.is_some()
            || launch.arguments.len() > 1
        {
            return Err(failure("unretired private episode or fixture arguments"));
        }
        self.identity.incarnation = incarnation;
        let id: String = self
            .identity
            .workbench
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let base = format!(
            r"\\.\pipe\lab-workbench-{}-{id}-{incarnation}",
            std::process::id()
        );
        let control = format!("{base}-control");
        let snapshot = format!("{base}-snapshot");
        let pid = std::process::id().to_string();
        let incarnation_arg = incarnation.to_string();
        let mode = launch.arguments.first().map_or("normal", String::as_str);
        self.job = Some(Job::new(launch.limits)?);
        self.job.as_mut().unwrap().prepare(
            &launch.executable,
            &[&control, &snapshot, &id, &incarnation_arg, &pid, mode],
        )?;
        // Child is already atomically Job-contained and suspended. Failure of
        // either server creation leaves that exact child for startup rollback.
        self.control = Some(Framed::new(Pipe::server(&control)?));
        self.snapshot = Some(Framed::new(Pipe::server(&snapshot)?));
        #[cfg(test)]
        if let Some(fault) = &self.snapshot_fault {
            self.snapshot.as_mut().unwrap().inject_fault(fault.clone());
        }
        self.connected = [false; 2];
        self.hello_sent = false;
        self.hello_ack = false;
        self.sequences = Default::default();
        self.applied = [0; 2];
        self.waiting = [None; 2];
        self.applied_epoch = None;
        self.progress = None;
        self.child_read_current = false;
        self.renderer = None;
        self.challenge = None;
        self.close = None;
        self.close_sent = false;
        self.close_received = false;
        self.window_close_requested = false;
        self.control_failed = false;
        self.bulk_failed = false;
        self.sent_fence = None;
        self.retiring = false;
        self.pending_retirement = false;
        self.diagnostic_burst = 0;
        self.sent_feedback = None;
        {
            let mut shared = self.bridge.shared.lock().unwrap();
            shared.intent = None;
            shared.feedback = None;
        }
        self.publish(None);
        Ok(())
    }
    fn resume(&mut self) -> std::result::Result<(), Error> {
        self.job.as_mut().unwrap().resume()
    }
    fn has_process(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|job| job.objects().processes != 0)
    }
    fn poll_exit(&mut self) -> std::result::Result<Option<u32>, Error> {
        self.job.as_ref().unwrap().poll_exit()
    }
    fn challenge(&mut self, incarnation: u64, sequence: u64) -> std::result::Result<(), Error> {
        if incarnation != self.identity.incarnation {
            return Err(failure("challenge incarnation"));
        }
        self.challenge = Some(sequence);
        Ok(())
    }
    fn progress(&mut self) -> std::result::Result<Option<Progress>, Error> {
        Ok(self.progress.take())
    }
    fn close(&mut self, incarnation: u64, sequence: u64) -> std::result::Result<(), Error> {
        if incarnation != self.identity.incarnation {
            return Err(failure("close incarnation"));
        }
        self.close = Some(sequence);
        Ok(())
    }
    fn terminate(&mut self, rollback: bool) -> std::result::Result<(), Error> {
        if rollback {
            self.job.as_ref().unwrap().rollback(77)
        } else {
            self.job.as_ref().unwrap().terminate(77)
        }
    }
    fn retire(&mut self) -> std::result::Result<Reading, Error> {
        if self.has_process() && self.job.as_ref().unwrap().poll_exit()?.is_none() {
            return Err(failure("IPC retirement before process exit"));
        }
        self.retiring = true;
        self.pending_retirement = false;
        // Cancellation of each independent channel is requested even if the
        // other channel's cancellation/release reports an error.
        let canceled_control = self.control.as_mut().map_or(Ok(()), |s| s.pipe.cancel());
        let canceled_snapshot = self.snapshot.as_mut().map_or(Ok(()), |s| s.pipe.cancel());
        canceled_control?;
        canceled_snapshot?;
        #[cfg(test)]
        if self
            .retirement_gate
            .as_ref()
            .is_some_and(|gate| !gate.load(Ordering::Acquire))
            && (self.control.is_some() || self.snapshot.is_some())
        {
            self.pending_retirement = true;
            self.publish(None);
            return Err(failure("IPC completion pending"));
        }
        let control = match &mut self.control {
            Some(s) => s.pipe.retire()?,
            None => true,
        };
        let snapshot = match &mut self.snapshot {
            Some(s) => s.pipe.retire()?,
            None => true,
        };
        if !control || !snapshot {
            self.pending_retirement = true;
            self.publish(None);
            return Err(failure("IPC completion pending"));
        }
        self.control = None;
        self.snapshot = None;
        // Only now may borrowed encoded slots re-enter the reusable pool.
        let mut shared = self.bridge.shared.lock().unwrap();
        if let Some(writing) = self.writing.take() {
            shared
                .pool
                .complete_write(writing.index, writing.stream)
                .map_err(failure)?;
        }
        shared.pool.restore().map_err(failure)?;
        drop(shared);
        let reading = if let Some(job) = &mut self.job {
            let reading = job.verify_limits()?;
            job.retire()?;
            reading
        } else {
            Reading::default()
        };
        self.job = None;
        self.publish(None);
        Ok(reading)
    }
    fn retirement_pending(&self) -> bool {
        self.pending_retirement
    }
    fn objects(&self) -> Objects {
        self.job.as_ref().map_or(Objects::default(), Job::objects)
    }
}

#[cfg(test)]
impl IpcBackend {
    pub(crate) fn hold_retirement(&mut self, gate: Arc<AtomicBool>) {
        self.retirement_gate = Some(gate);
    }
    pub(crate) fn hold_io_observation(&mut self, gate: Arc<AtomicBool>) {
        self.io_gate = Some(gate);
    }
    pub(super) fn inject_snapshot_fault(&mut self, fault: Arc<std::sync::atomic::AtomicU8>) {
        self.snapshot_fault = Some(fault);
    }
    pub(super) fn witness(&self) -> std::result::Result<lab_windows_job::ProcessWitness, Error> {
        self.job.as_ref().unwrap().witness()
    }
}

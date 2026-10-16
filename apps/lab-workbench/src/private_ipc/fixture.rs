//! Compiled only into the separately launched qualification fixture. This entry
//! creates no Runtime client, dispatcher, GUI, Preview, GL context or Job handle.
use super::{
    Replica,
    protocol::*,
    transport::{Framed, failure},
};
use lab_windows_job::pipe::Pipe;
use std::time::{Duration, Instant};

pub(crate) fn run() -> std::result::Result<(), lab_windows_job::Error> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 6 || args.iter().any(|s| s.len() > 256) {
        return Err(failure("fixture argument bounds"));
    }
    if args[2].len() != 32 {
        return Err(failure("fixture identity"));
    }
    let mut workbench = [0; 16];
    for (index, byte) in workbench.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&args[2][index * 2..index * 2 + 2], 16)
            .map_err(|_| failure("fixture identity hex"))?;
    }
    let identity = Identity {
        workbench,
        incarnation: args[3]
            .parse()
            .map_err(|_| failure("fixture incarnation"))?,
    };
    let parent = args[4]
        .parse()
        .map_err(|_| failure("fixture parent identity"))?;
    let mode = args[5].as_str();
    if ![
        "normal",
        "slow",
        "slow-after-read",
        "snapshot-close",
        "hang",
        "crash",
        "flood",
        "wrong-incarnation",
        "wrong-server",
        "version",
        "variant",
        "oversize",
        "length",
        "truncated",
        "duplicate",
        "order",
        "truncate-snapshot",
        "handoff",
        "handoff-crash",
    ]
    .contains(&mode)
    {
        return Err(failure("fixture mode"));
    }
    let mut control = Framed::<CONTROL>::new(Pipe::client(&args[0])?);
    let mut snapshot = Framed::<CHUNK>::new(Pipe::client(&args[1])?);
    let expected_parent = if mode == "wrong-server" {
        parent + 1
    } else {
        parent
    };
    if !control.pipe.connected(expected_parent)? || !snapshot.pipe.connected(expected_parent)? {
        return Err(failure("fixture server identity"));
    }
    let mut replica = Replica::new();
    let mut sequence = Sequence::default();
    let mut hello = false;
    let mut hello_ack = false;
    let mut pending: [Option<(Header, bool)>; 2] = [None; 2];
    let mut challenge = None;
    let mut closing = false;
    let mut closed_sent = false;
    let mut counter = 0u64;
    let mut snapshot_closed = false;
    let mut handoff_diagnostic = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if Instant::now() >= deadline {
            return Err(failure("fixture lifetime deadline"));
        }
        let complete = control.poll_send()?;
        if complete && closed_sent {
            break;
        }
        if mode == "hang" && hello_ack {
            std::thread::yield_now();
            continue;
        }
        let mut received_bytes = 0;
        if let Some((header, payload)) = control.receive(identity, Instant::now())? {
            received_bytes += header.bytes;
            match header.kind {
                Kind::Hello if !hello && payload.is_empty() => hello = true,
                Kind::Challenge | Kind::Close if hello_ack && payload.len() == 8 => {
                    replica.parent_progress(Instant::now());
                    challenge = Some(u64::from_le_bytes(payload.try_into().unwrap()));
                    closing |= header.kind == Kind::Close;
                }
                Kind::Fence if hello_ack && payload.len() == 16 => {
                    replica
                        .fence(
                            u64::from_le_bytes(payload[..8].try_into().unwrap()),
                            u64::from_le_bytes(payload[8..].try_into().unwrap()),
                            Instant::now(),
                        )
                        .map_err(failure)?;
                }
                _ => return Err(failure("fixture control direction/order")),
            }
        }
        let snapshot_paused = match mode {
            "handoff" | "handoff-crash" => replica.document_candidate().is_some(),
            "slow" => replica.accepted(0).is_some(),
            "slow-after-read" => replica.accepted(1).is_some(),
            "snapshot-close" => snapshot_closed,
            _ => false,
        };
        if hello_ack && !closing && !snapshot_paused {
            if let Some((header, payload)) =
                snapshot.receive_bounded(identity, Instant::now(), OWNER_BYTES - received_bytes)?
            {
                if mode == "truncate-snapshot" {
                    std::process::exit(74);
                }
                if replica
                    .chunk(header, payload, Instant::now())
                    .map_err(failure)?
                {
                    let stream = header.kind.stream().unwrap();
                    if stream == 0 {
                        if replica.accepted(0).is_some()
                            && matches!(mode, "handoff" | "handoff-crash")
                        {
                            if mode == "handoff-crash" {
                                std::process::exit(73);
                            }
                            // Complete Assembly stays in its original credit. No
                            // parsed admission or Applied while the consumer waits.
                            handoff_diagnostic = Some(header.snapshot);
                            continue;
                        }
                        let decision = replica.admit_encoded_document().map_err(failure)?;
                        pending[0] = Some((decision.header, decision.applied));
                        continue;
                    }
                    pending[stream] = Some((
                        header,
                        replica
                            .accepted(stream)
                            .is_some_and(|h| h.snapshot == header.snapshot),
                    ));
                }
            }
            replica.expire(Instant::now()).map_err(failure)?;
        }
        if mode == "snapshot-close" && replica.accepted(0).is_some() && !snapshot_closed {
            snapshot_closed = snapshot.pipe.retire()?;
        }
        if mode == "crash" && replica.accepted(0).is_some() {
            std::process::exit(73);
        }
        if control.idle() && hello {
            if !hello_ack {
                let header =
                    Header::control(Kind::HelloAck, identity, sequence.next().map_err(failure)?);
                if [
                    "wrong-incarnation",
                    "version",
                    "variant",
                    "oversize",
                    "length",
                    "truncated",
                    "duplicate",
                    "order",
                ]
                .contains(&mode)
                {
                    let count = if mode == "duplicate" {
                        2 * HEADER
                    } else if mode == "truncated" {
                        HEADER / 2
                    } else {
                        HEADER
                    };
                    control.pipe.write_with(count, |out| {
                        let mut bytes = [0; HEADER];
                        header.encode(&mut bytes).map_err(failure)?;
                        match mode {
                            "wrong-incarnation" => bytes[32..40]
                                .copy_from_slice(&(identity.incarnation + 1).to_le_bytes()),
                            "version" => bytes[4..6].copy_from_slice(&u16::MAX.to_le_bytes()),
                            "variant" => bytes[6..8].copy_from_slice(&255u16.to_le_bytes()),
                            "oversize" => {
                                bytes[8..12].copy_from_slice(&((CONTROL + 1) as u32).to_le_bytes())
                            }
                            "length" => {
                                bytes[8..12].copy_from_slice(&((HEADER - 1) as u32).to_le_bytes())
                            }
                            "order" => bytes[40..48].copy_from_slice(&2u64.to_le_bytes()),
                            _ => {}
                        }
                        if mode == "duplicate" {
                            out[..HEADER].copy_from_slice(&bytes);
                            out[HEADER..].copy_from_slice(&bytes);
                        } else {
                            out.copy_from_slice(&bytes[..count]);
                        }
                        Ok(())
                    })?;
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while control.pipe.poll_write()?.is_none() {
                        if Instant::now() >= deadline {
                            return Err(failure("fault TX receipt deadline"));
                        }
                        std::thread::yield_now();
                    }
                    if mode == "truncated" {
                        std::process::exit(74);
                    }
                } else {
                    control.send(header, &[])?;
                }
                hello_ack = true;
            } else if closing {
                let mut header =
                    Header::control(Kind::Closed, identity, sequence.next().map_err(failure)?);
                header.bytes += 32;
                let mut payload = [0; 32];
                payload[..8].copy_from_slice(&challenge.unwrap_or(0).to_le_bytes());
                counter += 1;
                payload[8..16].copy_from_slice(&counter.to_le_bytes());
                payload[16..24]
                    .copy_from_slice(&replica.accepted(1).map_or(0, |h| h.snapshot).to_le_bytes());
                payload[24] = u8::from(replica.read_is_current(Instant::now()));
                control.send(header, &payload)?;
                closed_sent = true;
            } else if let Some(stream) = pending.iter().position(Option::is_some) {
                let (applied, accepted) = pending[stream].take().unwrap();
                let mut header = Header::control(
                    if accepted {
                        Kind::Applied
                    } else {
                        Kind::Skipped
                    },
                    identity,
                    sequence.next().map_err(failure)?,
                );
                header.bytes += 1;
                header.epoch = applied.epoch;
                header.snapshot = applied.snapshot;
                header.source = applied.source;
                control.send(header, &[stream as u8])?;
            } else if let Some(snapshot) = handoff_diagnostic.take() {
                let mut header = Header::control(
                    Kind::Diagnostic,
                    identity,
                    sequence.next().map_err(failure)?,
                );
                header.bytes += 8;
                control.send(header, &snapshot.to_le_bytes())?;
            } else if let Some(challenge) = challenge.take() {
                let mut header =
                    Header::control(Kind::Status, identity, sequence.next().map_err(failure)?);
                header.bytes += 32;
                let mut payload = [0; 32];
                payload[..8].copy_from_slice(&challenge.to_le_bytes());
                counter += 1;
                payload[8..16].copy_from_slice(&counter.to_le_bytes());
                payload[16..24]
                    .copy_from_slice(&replica.accepted(1).map_or(0, |h| h.snapshot).to_le_bytes());
                payload[24] = u8::from(replica.read_is_current(Instant::now()));
                control.send(header, &payload)?;
            } else if mode == "flood" {
                let mut header = Header::control(
                    Kind::Diagnostic,
                    identity,
                    sequence.next().map_err(failure)?,
                );
                header.bytes += 1;
                control.send(header, &[0])?;
            }
        }
        std::thread::yield_now();
    }
    // Each pipe has its own actual completion observations, independent of the
    // future process exit witnessed by the parent.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let a = control.pipe.retire()?;
        let b = snapshot.pipe.retire()?;
        if a && b {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(failure("fixture IPC retirement unconfirmed"));
        }
        std::thread::yield_now();
    }
}

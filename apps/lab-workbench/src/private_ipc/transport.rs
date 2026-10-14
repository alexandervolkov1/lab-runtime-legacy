//! Bounded framing over one original pair of overlapped operations.
use super::protocol::*;
use lab_windows_job::{Error, pipe::Pipe};
use std::time::{Duration, Instant};

pub(crate) fn failure(operation: &'static str) -> Error {
    Error { operation, code: 0 }
}
pub(crate) struct Framed<const N: usize> {
    pub(crate) pipe: Pipe<N>,
    rx: usize,
    header: Option<Header>,
    rx_pending: bool,
    complete: bool,
    delivered: bool,
    started: Option<Instant>,
    received: Sequence,
    tx: usize,
    sent: usize,
    #[cfg(test)]
    fault: Option<std::sync::Arc<std::sync::atomic::AtomicU8>>,
}
impl<const N: usize> Framed<N> {
    pub(crate) fn new(pipe: Pipe<N>) -> Self {
        Self {
            pipe,
            rx: 0,
            header: None,
            rx_pending: false,
            complete: false,
            delivered: false,
            started: None,
            received: Sequence::default(),
            tx: 0,
            sent: 0,
            #[cfg(test)]
            fault: None,
        }
    }
    pub(crate) fn idle(&self) -> bool {
        self.tx == 0
    }
    pub(crate) fn send(
        &mut self,
        header: Header,
        payload: &[u8],
    ) -> std::result::Result<(), Error> {
        if !self.idle() || header.bytes != HEADER + payload.len() || header.bytes > N {
            return Err(failure("framed TX bound/state"));
        }
        self.pipe.write_with(header.bytes, |out| {
            header.encode(out).map_err(failure)?;
            out[HEADER..].copy_from_slice(payload);
            #[cfg(test)]
            if let Some(fault) = &self.fault {
                match fault.swap(0, std::sync::atomic::Ordering::AcqRel) {
                    1 => out[12..16].copy_from_slice(&((SNAPSHOT + 1) as u32).to_le_bytes()),
                    2 => out[8..12].copy_from_slice(&((CHUNK + 1) as u32).to_le_bytes()),
                    3 => out[64..72]
                        .copy_from_slice(&header.epoch.revision.saturating_sub(1).to_le_bytes()),
                    4 => out[80..84].copy_from_slice(&1u32.to_le_bytes()),
                    5 => out[HEADER] = b'!',
                    _ => {}
                }
            }
            Ok(())
        })?;
        self.tx = header.bytes;
        self.sent = 0;
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn inject_fault(&mut self, fault: std::sync::Arc<std::sync::atomic::AtomicU8>) {
        self.fault = Some(fault);
    }
    pub(crate) fn poll_send(&mut self) -> std::result::Result<bool, Error> {
        if self.tx == 0 {
            return Ok(false);
        }
        let Some(count) = self.pipe.poll_write()? else {
            return Ok(false);
        };
        if count == 0 || count > self.tx - self.sent {
            return Err(failure("partial TX count"));
        }
        self.sent += count;
        if self.sent == self.tx {
            self.tx = 0;
            return Ok(true);
        }
        self.pipe.write_remainder(self.sent, self.tx - self.sent)?;
        Ok(false)
    }
    /// At most one OS completion and one submission per turn. Declared frame
    /// length is validated while only HEADER bytes occupy the fixed RX buffer.
    pub(crate) fn receive(
        &mut self,
        identity: Identity,
        now: Instant,
    ) -> std::result::Result<Option<(Header, &[u8])>, Error> {
        self.receive_bounded(identity, now, N)
    }
    /// A completed frame can wait in the same RX buffer until the owner has byte
    /// credit. It is neither dropped nor copied into another assembly buffer.
    pub(crate) fn receive_bounded(
        &mut self,
        identity: Identity,
        now: Instant,
        budget: usize,
    ) -> std::result::Result<Option<(Header, &[u8])>, Error> {
        if self.complete && self.delivered {
            self.complete = false;
            self.delivered = false;
            self.rx = 0;
            self.header = None;
            self.started = None;
        }
        if self
            .started
            .is_some_and(|start| now.saturating_duration_since(start) >= Duration::from_secs(2))
        {
            return Err(failure("incomplete frame deadline"));
        }
        if self.rx_pending {
            let Some(bytes) = self.pipe.poll_read()? else {
                return Ok(None);
            };
            if bytes.is_empty() {
                return Err(failure("truncated private frame"));
            }
            self.rx_pending = false;
            self.rx += bytes.len();
            self.started.get_or_insert(now);
        }
        if self.rx == HEADER && self.header.is_none() {
            let header = Header::decode(self.pipe.received(HEADER)?, identity, N == CHUNK)
                .map_err(failure)?;
            self.received.accept(header.sequence).map_err(failure)?;
            self.header = Some(header);
        }
        let total = self.header.map_or(HEADER, |h| h.bytes);
        if self.rx == total
            && let Some(header) = self.header
        {
            self.complete = true;
            if header.bytes > budget {
                return Ok(None);
            }
            self.delivered = true;
            return Ok(Some((header, &self.pipe.received(total)?[HEADER..])));
        }
        self.pipe.read_at(self.rx, total - self.rx)?;
        self.rx_pending = true;
        Ok(None)
    }
}

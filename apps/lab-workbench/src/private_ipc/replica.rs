//! Assembly and parsed admission are distinct. A complete document keeps its
//! existing Assembly credit until the consumer explicitly resolves admission.
//! The encoded-only consumer retains zero parsed documents; a paired renderer
//! owner may retain one, and must release it before the next parsed admission.
use super::{DocumentOwner, document_owner::AdmissionKey, protocol::*, reads};
use crate::gui::readonly_v2::{host_session::HostSession, texture_owner::FrameBackend};
use crate::presentation::v2;
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Transfer {
    index: usize,
    header: Header,
    filled: usize,
    started: Instant,
}
pub(crate) struct Replica {
    pool: Pool,
    incoming: [Option<Transfer>; 2],
    accepted: [Option<Header>; 2],
    parsed_peak: usize,
    required: [u64; 2],
    fence_seen: Option<Instant>,
    read_invalidated: bool,
    renderer: Option<Rc<AdmissionKey>>,
    document_attempt: Option<Header>,
    stopped: bool,
}
/// Only this post-admission result can authorize an Applied/Skipped receipt.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub(crate) struct DocumentDecision {
    pub(crate) header: Header,
    pub(crate) applied: bool,
}
impl Replica {
    pub(crate) fn selected_provider(
        &mut self,
        now: Instant,
    ) -> Result<Option<super::selected_provider::SelectedSnapshot<'_>>> {
        let current = self.read_is_current(now);
        self.snapshot(1)
            .map(|(header, bytes)| {
                super::selected_provider::SelectedSnapshot::decode(header, bytes, current)
            })
            .transpose()
    }
    pub(crate) fn snapshot(&self, stream: usize) -> Option<(Header, &[u8])> {
        let header = self.accepted(stream)?;
        let slot = self
            .pool
            .slots
            .iter()
            .find(|s| s.credit == Credit::Current(stream) && s.snapshot == header.snapshot)?;
        Some((header, &slot.bytes[..slot.len]))
    }
    pub(crate) fn accepted(&self, stream: usize) -> Option<Header> {
        self.accepted.get(stream).copied().flatten()
    }
    #[cfg(test)]
    pub(super) fn pool(&self) -> &Pool {
        &self.pool
    }
    #[cfg(test)]
    pub(super) fn admission_peak(&self) -> usize {
        self.parsed_peak
    }
    pub(crate) fn new() -> Self {
        Self {
            pool: Pool::new(),
            incoming: [None; 2],
            accepted: [None; 2],
            parsed_peak: 0,
            required: [0; 2],
            fence_seen: None,
            read_invalidated: true,
            renderer: None,
            document_attempt: None,
            stopped: false,
        }
    }
    pub(crate) fn renderer<B: FrameBackend>(
        session: &mut HostSession<B>,
    ) -> (Self, DocumentOwner<'_, B>) {
        let key = Rc::new(AdmissionKey);
        let mut replica = Self::new();
        replica.renderer = Some(key.clone());
        (
            replica,
            DocumentOwner {
                key,
                document: None,
                identity: None,
                session,
            },
        )
    }
    /// One complete candidate, never a parsed document or additional byte copy.
    pub(crate) fn document_candidate(&self) -> Option<Header> {
        self.incoming[0]
            .filter(|t| t.filled == t.header.total)
            .map(|t| t.header)
    }
    pub(crate) fn candidate_bytes(&self) -> Option<&[u8]> {
        let transfer = self.incoming[0].filter(|t| t.filled == t.header.total)?;
        Some(&self.pool.slots[transfer.index].bytes[..transfer.filled])
    }
    /// Stop is admission closure, not a cancellation/completion or release
    /// receipt. Keep the bounded episode and every credit until root retirement.
    pub(crate) fn stop(&mut self) {
        self.stopped = true;
        self.read_invalidated = true;
    }
    pub(crate) fn admit_encoded_document(&mut self) -> Result<DocumentDecision> {
        if self.renderer.is_some() {
            return Err("renderer requires paired document owner");
        }
        self.admit_document_with(|document, _| drop(document))
    }
    pub(crate) fn admit_renderer_document<B: FrameBackend>(
        &mut self,
        owner: &mut DocumentOwner<'_, B>,
    ) -> Result<DocumentDecision> {
        if self
            .renderer
            .as_ref()
            .is_none_or(|key| !Rc::ptr_eq(key, &owner.key))
        {
            return Err("unpaired renderer document owner");
        }
        if owner.document.is_some() {
            return Err("old parsed document remains alive");
        }
        owner.ensure_retired()?;
        self.admit_document_with(|document, header| {
            owner.document = Some(document);
            owner.identity = Some(header);
        })
    }
    fn admit_document_with(
        &mut self,
        retain: impl FnOnce(v2::AdmittedV2Document, Header),
    ) -> Result<DocumentDecision> {
        if self.stopped {
            return Err("document admission stopped");
        }
        let transfer = self.incoming[0]
            .filter(|t| t.filled == t.header.total)
            .ok_or("no complete document candidate")?;
        let header = transfer.header;
        // Consume this exact attempt even on malformed input. A duplicate cannot
        // trigger another parsed admission, and there is no rejection backlog.
        self.document_attempt = Some(header);
        let parsed = v2::admit(&self.pool.slots[transfer.index].bytes[..header.total])
            .map_err(|_| "canonical document admission")
            .and_then(|document| {
                self.parsed_peak = self.parsed_peak.max(document.allocation().accounted_bytes);
                if document.canonical_bytes()
                    != &self.pool.slots[transfer.index].bytes[..header.total]
                {
                    Err("noncanonical document bytes")
                } else {
                    Ok(document)
                }
            });
        let document = match parsed {
            Ok(document) => document,
            Err(error) => {
                self.pool.slots[transfer.index].credit = Credit::Free;
                self.incoming[0] = None;
                return Err(error);
            }
        };
        let applied = header.epoch.revision >= self.required[0];
        if applied {
            retain(document, header);
        } else {
            drop(document);
        }
        self.commit(0, applied);
        Ok(DocumentDecision { header, applied })
    }
    /// Sequential restoration only: the candidate must be resolved first, and
    /// current control fences must still permit the accepted epoch/source.
    pub(crate) fn restore_renderer_document<B: FrameBackend>(
        &mut self,
        owner: &mut DocumentOwner<'_, B>,
    ) -> Result<()> {
        if self.stopped
            || self.incoming[0].is_some()
            || owner.document.is_some()
            || self
                .renderer
                .as_ref()
                .is_none_or(|key| !Rc::ptr_eq(key, &owner.key))
        {
            return Err("renderer restoration fenced");
        }
        owner.ensure_retired()?;
        let (header, bytes) = self.snapshot(0).ok_or("no accepted document")?;
        if header.epoch.revision < self.required[0] {
            return Err("accepted epoch superseded by parent fence");
        }
        let document = v2::admit(bytes).map_err(|_| "accepted document re-admission")?;
        self.parsed_peak = self.parsed_peak.max(document.allocation().accounted_bytes);
        owner.document = Some(document);
        owner.identity = Some(header);
        // Restoring display does not restore observation freshness.
        self.read_invalidated = true;
        Ok(())
    }
    pub(crate) fn fence(&mut self, revision: u64, source: u64, now: Instant) -> Result<()> {
        if revision == 0 || revision < self.required[0] || source < self.required[1] {
            return Err("stale parent fence");
        }
        self.parent_progress(now);
        self.required = [revision, source];
        if self.accepted[1].is_none_or(|h| h.epoch.revision != revision || h.source < source) {
            self.read_invalidated = true;
        }
        Ok(())
    }
    pub(crate) fn parent_progress(&mut self, now: Instant) {
        if self
            .fence_seen
            .is_some_and(|seen| now.saturating_duration_since(seen) >= Duration::from_secs(2))
        {
            self.read_invalidated = true;
        }
        self.fence_seen = Some(now);
    }
    /// This predicate can only downgrade validated original read freshness.
    /// Timeout remains latched until another complete checked read snapshot.
    pub(crate) fn read_is_current(&mut self, now: Instant) -> bool {
        if self
            .fence_seen
            .is_none_or(|seen| now.saturating_duration_since(seen) >= Duration::from_secs(2))
        {
            self.read_invalidated = true;
        }
        !self.read_invalidated
            && self.accepted[1].is_some_and(|h| {
                h.epoch.revision == self.required[0] && h.source >= self.required[1]
            })
    }
    pub(crate) fn expire(&self, now: Instant) -> Result<()> {
        if self.incoming.iter().flatten().any(|t| {
            t.filled < t.header.total
                && now.saturating_duration_since(t.started) >= Duration::from_secs(2)
        }) {
            return Err("incomplete snapshot deadline");
        }
        Ok(())
    }
    pub(crate) fn chunk(&mut self, header: Header, bytes: &[u8], now: Instant) -> Result<bool> {
        if self.stopped {
            return Err("replica admission stopped");
        }
        self.expire(now)?;
        let stream = header.kind.stream().ok_or("non-snapshot replica message")?;
        if bytes.len() + HEADER != header.bytes
            || header.total > SNAPSHOT
            || header.total == 0
            || header.bytes > CHUNK
            || header
                .offset
                .checked_add(bytes.len())
                .is_none_or(|n| n > header.total)
        {
            return Err("replica chunk bound");
        }
        if stream == 0
            && self.document_attempt.is_some_and(|old| {
                header.snapshot <= old.snapshot
                    || header.epoch.owner != old.epoch.owner
                    || header.epoch.revision <= old.epoch.revision
                    || header.source < old.source
            })
        {
            return Err("repeated document admission attempt");
        }
        if let Some(old) = self.accepted[stream] {
            if header.snapshot <= old.snapshot
                || header.epoch.owner != old.epoch.owner
                || header.epoch.revision < old.epoch.revision
                || header.source < old.source
            {
                return Err("stale snapshot identity/source/epoch");
            }
            if stream == 0 && header.epoch.revision <= old.epoch.revision {
                return Err("stale document revision");
            }
        }
        if stream == 1 && self.accepted[0].is_none_or(|doc| doc.epoch != header.epoch) {
            return Err("read without exact admitted document epoch");
        }
        if self.incoming[stream].is_none() {
            if header.offset != 0 {
                return Err("snapshot missing initial chunk");
            }
            let index = self
                .pool
                .slots
                .iter()
                .position(|s| s.credit == Credit::Free)
                .ok_or("replica credits exhausted")?;
            self.pool.slots[index].credit = Credit::Assembly(stream);
            self.incoming[stream] = Some(Transfer {
                index,
                header,
                filled: 0,
                started: now,
            });
        }
        let transfer = self.incoming[stream].as_mut().unwrap();
        if transfer.filled == transfer.header.total {
            return Err("complete candidate already awaiting handoff");
        }
        if transfer.header.snapshot != header.snapshot
            || transfer.header.epoch != header.epoch
            || transfer.header.source != header.source
            || transfer.header.total != header.total
            || transfer.filled != header.offset
        {
            return Err("interleaved snapshot");
        }
        let slot = &mut self.pool.slots[transfer.index];
        slot.bytes[transfer.filled..transfer.filled + bytes.len()].copy_from_slice(bytes);
        transfer.filled += bytes.len();
        if transfer.filled != header.total {
            return Ok(false);
        }
        slot.len = header.total;
        slot.epoch = header.epoch;
        slot.snapshot = header.snapshot;
        slot.source = header.source;
        // Document admission is explicitly deferred, including for the no-GPU
        // fixture. Completing bytes is not validation, publication or Applied.
        if stream == 0 {
            return Ok(true);
        } else {
            reads::validate(&slot.bytes[..header.total])?;
            if let Some(old) = self.accepted[1] {
                let current = self
                    .pool
                    .slots
                    .iter()
                    .find(|s| s.credit == Credit::Current(1))
                    .ok_or("read credit invariant")?;
                let incoming = &self.pool.slots[transfer.index];
                reads::successor(
                    &current.bytes[..current.len],
                    &incoming.bytes[..header.total],
                    header.source > old.source,
                )?;
            }
        }
        let applied =
            header.epoch.revision >= self.required[0] && header.source >= self.required[1];
        self.commit(stream, applied);
        Ok(true)
    }
    fn commit(&mut self, stream: usize, applied: bool) {
        let transfer = self.incoming[stream]
            .take()
            .expect("complete Assembly transfer");
        let index = transfer.index;
        let header = transfer.header;
        if !applied {
            self.pool.slots[index].credit = Credit::Free;
            return;
        }
        for slot in &mut self.pool.slots {
            if slot.credit == Credit::Current(stream) {
                slot.credit = Credit::Free;
            }
        }
        self.pool.slots[index].credit = Credit::Current(stream);
        self.accepted[stream] = Some(header);
        if stream == 1 {
            self.read_invalidated = false;
        }
        if stream == 0 {
            // Read bytes from an older document epoch cannot accompany the new one.
            self.accepted[1] = None;
            self.read_invalidated = true;
            for slot in &mut self.pool.slots {
                if slot.credit == Credit::Current(1) {
                    slot.credit = Credit::Free;
                }
            }
        }
    }
}

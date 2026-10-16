//! Legacy Custom Glow pairing stays outside the neutral replica and stock host.
use super::{
    DocumentOwner,
    protocol::*,
    replica::{AdmissionKey, DocumentDecision, Replica},
};
use crate::{
    gui::readonly_v2::{host_session::HostSession, texture_owner::FrameBackend},
    presentation::v2,
};
use std::rc::Rc;
impl Replica {
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
}

//! One parsed renderer document, separate from the four encoded credits. The
//! paired admission key cannot be cloned or manufactured by a consumer. This
//! owner has no native Device, deletion operation, or shutdown authority.
use super::protocol::*;
use crate::{
    gui::readonly_v2::{
        host_session::{CleanupOutcome, HostSession},
        texture_owner::FrameBackend,
    },
    presentation::v2,
};
use std::rc::Rc;

pub(super) struct AdmissionKey;

/// Trusted outer-root ownership, like HostSession: retain this owner on Pending.
/// Borrows prevent replacement while Preview lives; arbitrary destruction of
/// the whole unresolved root is outside the native lifecycle contract.
#[must_use = "retain this owner and its exact session while retirement is unresolved"]
pub(crate) struct DocumentOwner<'s, B: FrameBackend> {
    pub(super) key: Rc<AdmissionKey>,
    pub(super) document: Option<v2::AdmittedV2Document>,
    pub(super) identity: Option<Header>,
    pub(super) session: &'s mut HostSession<B>,
}
impl<B: FrameBackend> DocumentOwner<'_, B> {
    pub(crate) fn document(&self) -> Option<&v2::AdmittedV2Document> {
        self.document.as_ref()
    }
    pub(crate) fn identity(&self) -> Option<Header> {
        self.identity
    }
    /// Disjoint borrows for a Preview, tied to this owner's exact session. While
    /// the Preview lives, releasing/replacing the document is a borrow error.
    pub(crate) fn parts(
        &mut self,
    ) -> Option<(&v2::AdmittedV2Document, Header, &mut HostSession<B>)> {
        Some((self.document.as_ref()?, self.identity?, self.session))
    }
    pub(crate) fn session(&self) -> &HostSession<B> {
        self.session
    }
    pub(crate) fn session_mut(&mut self) -> &mut HostSession<B> {
        self.session
    }
    /// Preview's borrow must end before this call. The real session must already
    /// have proved retirement; this method never waits, deletes, or closes it.
    /// Pending preserves the old parsed document as well as the session payload.
    pub(super) fn ensure_retired(&self) -> Result<()> {
        if !matches!(
            self.session.status(),
            CleanupOutcome::Idle | CleanupOutcome::Retired(_)
        ) || self.session.owned_keys() != 0
            || self.session.retained_context()
            || self.session.retained_cpu_frame()
        {
            return Err("renderer retirement unresolved");
        }
        Ok(())
    }
    pub(crate) fn release_retired(&mut self) -> Result<()> {
        self.ensure_retired()?;
        self.document = None;
        self.identity = None;
        Ok(())
    }
}

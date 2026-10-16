//! One immutable CPU document for a stock UI. No graphics owner or completion API.
use super::{
    protocol::*,
    replica::{AdmissionKey, DocumentDecision, Replica},
};
use crate::presentation::v2;
use std::rc::Rc;

pub(crate) struct UiDocument {
    key: Rc<AdmissionKey>,
    document: Option<v2::AdmittedV2Document>,
    identity: Option<Header>,
}
impl UiDocument {
    pub(crate) fn document(&self) -> Option<&v2::AdmittedV2Document> {
        self.document.as_ref()
    }
    pub(crate) fn identity(&self) -> Option<Header> {
        self.identity
    }
}
impl Replica {
    pub(crate) fn stock_ui() -> (Self, UiDocument) {
        let key = Rc::new(AdmissionKey);
        let mut replica = Self::new();
        replica.renderer = Some(key.clone());
        (
            replica,
            UiDocument {
                key,
                document: None,
                identity: None,
            },
        )
    }
    /// Must run between UI calls. Immutable borrows prevent dropping a document
    /// while painting. Old parsed storage is released before the existing parser.
    pub(crate) fn admit_ui_document(&mut self, owner: &mut UiDocument) -> Result<DocumentDecision> {
        if self
            .renderer
            .as_ref()
            .is_none_or(|key| !Rc::ptr_eq(key, &owner.key))
        {
            return Err("unpaired stock UI document owner");
        }
        if self.document_candidate().is_none() {
            return Err("no complete document candidate");
        }
        owner.document = None;
        owner.identity = None;
        self.admit_document_with(|document, header| {
            owner.document = Some(document);
            owner.identity = Some(header);
        })
    }
}

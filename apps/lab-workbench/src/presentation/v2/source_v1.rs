//! Read-only source schema for offline conversion; no persistence or owner access.

pub(super) use crate::presentation::{
    ConfigurationOwner, Control, ControlKind, DocumentError, PanelKind, PresentationDocument,
    RuntimeRef,
};

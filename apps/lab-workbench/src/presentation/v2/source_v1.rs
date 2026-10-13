//! Shared v1 syntax for offline import/export; no persistence or owner access.

pub(super) use crate::presentation::{
    AxisOptions, ConfigurationOwner, Control, ControlKind, DocumentError, MAX_CONTROLS, Panel,
    PanelKind, Plot, PresentationDocument, PresentationTab, PresentationWindow, RuntimeRef, Trace,
    TraceStyle,
};

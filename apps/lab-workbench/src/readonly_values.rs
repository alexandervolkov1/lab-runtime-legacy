//! Canonical safe read-only values shared by the parent and renderer consumer.
//! No cache, Runtime client, mutable model, transport or authority is defined here.
#![allow(dead_code, reason = "shared private read-only consumer boundary")]

use crate::presentation::RuntimeRef;

/// Maximum live display points retained by the parent for one trace.
pub(crate) const LIVE_TRACE_POINTS: usize = 4_096;

/// Freshness of a rebuildable observation set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Freshness {
    /// No observation has been obtained in this client lifetime.
    #[default]
    Unknown,
    /// A reconnect/resnapshot rebuild is in progress.
    Rebuilding,
    /// Values are based on the current attached boot/session/event stream.
    Fresh,
    /// Values are cached and must not be presented as current.
    Stale,
}

/// One display-only live point; it is not Recorder data or physical evidence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LivePoint {
    /// Client/display timestamp expressed in seconds.
    pub(crate) time_seconds: f64,
    /// Observed numeric value.
    pub(crate) value: f64,
}

/// A finite semantic selector understood by Workbench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadBinding {
    SignalSample,
    ConfigurationPropertyCurrent,
    ReferenceValue,
    ControllerState,
    RecorderState,
}

impl ReadBinding {
    /// Selects only meanings fixed by an existing Runtime target category.
    pub(crate) fn for_target(target: &RuntimeRef) -> Option<Self> {
        match target {
            RuntimeRef::Signal { .. } => Some(Self::SignalSample),
            RuntimeRef::ConfigurationProperty { .. } => Some(Self::ConfigurationPropertyCurrent),
            RuntimeRef::Reference { .. } => Some(Self::ReferenceValue),
            RuntimeRef::Controller { .. } => Some(Self::ControllerState),
            RuntimeRef::Recorder => Some(Self::RecorderState),
            RuntimeRef::Instrument { .. }
            | RuntimeRef::Resource { .. }
            | RuntimeRef::Component { .. } => None,
        }
    }
}

/// Exact scalar type, independent of its JSON representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScalarKind {
    Boolean,
    Integer,
    Number,
    Text,
    Enum,
}

impl ScalarKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Text => "text",
            Self::Enum => "enum",
        }
    }
}

/// Sample quality is independent of observation-cache freshness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadQuality {
    Good,
    Unavailable,
    NotObserved,
}

/// Explicit absence or incompatibility; none of these carries a default value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadAbsence {
    MissingObservation,
    MissingValue,
    Unavailable,
    NotObserved,
    MissingMetadata,
    IncompatibleMetadata,
    Unsupported,
}

impl ReadAbsence {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::MissingObservation => "no Runtime observation",
            Self::MissingValue => "value absent",
            Self::Unavailable => "unavailable",
            Self::NotObserved => "not observed",
            Self::MissingMetadata => "typed metadata absent",
            Self::IncompatibleMetadata => "value and metadata are incompatible",
            Self::Unsupported => "unsupported typed value",
        }
    }
}

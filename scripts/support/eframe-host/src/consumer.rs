//! Read-only canonical consumer; no client, model, Preview or graphics custody.
#![allow(
    dead_code,
    unused_imports,
    reason = "shared bounded admission and selected-read surface"
)]
#[path = "../../../../apps/lab-workbench/src/gui/readonly_v2/observation_source.rs"]
pub(crate) mod observation_source;
#[path = "../../../../apps/lab-native-child/src/presentation.rs"]
pub(crate) mod presentation;
#[path = "../../../../apps/lab-workbench/src/readonly_limits.rs"]
pub(crate) mod readonly_limits;
#[path = "../../../../apps/lab-workbench/src/readonly_values.rs"]
pub(crate) mod readonly_values;
pub(crate) use crate::private_ipc::protocol::Epoch as PresentationEpoch;

//! Exact shared safe consumer source. No parent model, client or authority.
#[path = "presentation.rs"]
pub(crate) mod presentation;
#[path = "private_ipc.rs"]
pub(crate) mod private_ipc;
#[path = "../../lab-workbench/src/readonly_limits.rs"]
pub(crate) mod readonly_limits;
#[path = "../../lab-workbench/src/gui/readonly_v2/mod.rs"]
pub(crate) mod readonly_v2;
#[path = "../../lab-workbench/src/readonly_values.rs"]
pub(crate) mod readonly_values;

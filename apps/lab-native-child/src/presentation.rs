//! Immutable reference and offline admission stages only; no owner or v1 schema.
#[path = "../../lab-workbench/src/presentation/reference.rs"]
mod reference;
pub(crate) use reference::{ConfigurationOwner, RuntimeRef};
#[path = "../../lab-workbench/src/presentation/v2/consumer.rs"]
pub(crate) mod v2;

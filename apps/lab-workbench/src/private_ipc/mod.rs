//! Inactive private read-only host seam. The active v1 host does not use it.
#![allow(
    dead_code,
    reason = "private no-GPU host qualification before activation"
)]

mod backend;
mod document_owner;
pub(crate) mod fixture;
pub(crate) mod parent_bridge;
mod protocol;
mod reads;
mod replica;
mod selected_provider;
mod transport;
pub(crate) use document_owner::DocumentOwner;
pub(crate) use replica::Replica;

#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
pub(crate) mod native_tests;
#[cfg(test)]
mod provider_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod selected_preview_tests;

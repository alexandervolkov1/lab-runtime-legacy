//! Inactive private read-only host seam. The active v1 host does not use it.
#![allow(
    dead_code,
    reason = "private no-GPU host qualification before activation"
)]

mod backend;
pub(crate) mod fixture;
pub(crate) mod parent_bridge;
mod protocol;
mod reads;
mod replica;
mod transport;

#[cfg(test)]
pub(crate) mod native_tests;
#[cfg(test)]
mod tests;

//! Experimental read-only Windows renderer child. Not an activated product host.
#![allow(
    dead_code,
    reason = "private renderer qualification surface; activation remains false"
)]

#[cfg(windows)]
#[path = "../../../scripts/support/native-glow-backend.rs"]
mod backend;
#[cfg(windows)]
mod consumer;
#[cfg(windows)]
#[path = "../../../scripts/support/native-glow-device.rs"]
mod device;
#[cfg(windows)]
mod drawable;
#[cfg(windows)]
mod entry;
#[cfg(windows)]
mod fault;
#[cfg(windows)]
pub(crate) use consumer::{
    presentation, private_ipc, readonly_limits, readonly_v2, readonly_values,
};
#[cfg(windows)]
mod gui {
    pub(crate) use crate::readonly_v2;
}

fn main() {
    #[cfg(windows)]
    if let Err(error) = entry::run() {
        eprintln!("Read-only renderer child startup: {error}; ACTIVATION_QUALIFIED=false");
        std::process::exit(91);
    }
    #[cfg(not(windows))]
    eprintln!("Windows qualification target only; ACTIVATION_QUALIFIED=false");
}

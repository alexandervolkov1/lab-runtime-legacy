//! Inert Windows native child packaging proof. No event loop or renderer starts.
#![allow(
    dead_code,
    reason = "compile-only capsule pending independent integration review"
)]

#[cfg(windows)]
mod consumer;
#[cfg(windows)]
#[path = "../../../scripts/support/native-glow-device.rs"]
mod device;
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
    let _native_owner_layout = std::mem::size_of::<device::Device>();
    #[cfg(windows)]
    let _consumer_layout = (
        std::mem::size_of::<readonly_v2::Preview<'_>>(),
        std::mem::size_of::<private_ipc::Replica>(),
        std::mem::size_of::<presentation::v2::I64Decimal>(),
        std::mem::size_of::<presentation::v2::U64Decimal>(),
    );
    eprintln!("Native capsule compatibility target only; ACTIVATION_QUALIFIED=false");
}

//! Inert Windows native child packaging proof. No event loop or renderer starts.
#![allow(
    dead_code,
    reason = "compile-only capsule pending independent integration review"
)]

#[cfg(windows)]
#[path = "../../../scripts/support/native-glow-device.rs"]
mod device;
#[cfg(windows)]
mod readonly_v2;

fn main() {
    #[cfg(windows)]
    let _native_owner_layout = std::mem::size_of::<device::Device>();
    eprintln!("Native capsule compatibility target only; ACTIVATION_QUALIFIED=false");
}

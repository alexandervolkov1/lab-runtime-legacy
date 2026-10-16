//! Experimental standard eframe host. One read-only private IPC consumer; no Runtime client.
#![forbid(unsafe_code)]

#[cfg(windows)]
mod consumer;
#[cfg(windows)]
mod controls;
#[cfg(windows)]
#[path = "../../../../apps/lab-workbench/src/time_context.rs"]
mod time_context;
#[cfg(windows)]
pub(crate) use consumer::{presentation, readonly_limits, readonly_values};
#[cfg(windows)]
mod ipc_app;
#[cfg(windows)]
mod private_ipc;
#[cfg(windows)]
mod time_axis;
#[cfg(windows)]
mod ui;
#[cfg(windows)]
mod gui {
    pub(crate) use crate::consumer as readonly_v2;
}
#[cfg(windows)]
mod app;
#[cfg(windows)]
mod controller;
#[cfg(windows)]
mod diagnostics;
#[cfg(windows)]
#[path = "../../../../apps/lab-workbench/src/gui/plot_ui.rs"]
mod plot_ui;

#[cfg(windows)]
type Result<T> = std::result::Result<T, String>;

#[cfg(windows)]
fn write(path: &std::path::Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

#[cfg(windows)]
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let result = match args.as_slice() {
        [_, mode, renderer, dir] if mode == "--launch" =>
            controller::run(renderer, std::path::Path::new(dir)),
        [_, mode, renderer, destination, dir] if mode == "--renderer" && destination == "--receipts" =>
            app::run(renderer, std::path::Path::new(dir)),
        [_, control, snapshot, identity, incarnation, parent, mode] if control.starts_with(r"\\.\pipe\lab-workbench-") => ipc_app::run(control, snapshot, identity, incarnation, parent, mode),
        _ => Err("test-only usage: --launch wgpu|glow NEW_ABSOLUTE_DIRECTORY; child: --renderer wgpu|glow --receipts DIRECTORY".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    panic!("EFRAME-1 launcher qualification is Windows/MSVC only");
}

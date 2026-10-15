//! Test-only standard eframe host. No Runtime client or private IPC endpoint.
#![forbid(unsafe_code)]

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

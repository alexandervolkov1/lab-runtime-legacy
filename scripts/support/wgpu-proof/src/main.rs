//! Isolated RENDER-2A test executable. Never a Workbench or Runtime client.
#![deny(unsafe_code)]

#[cfg(windows)]
mod controller;
#[cfg(windows)]
mod fixtures;
#[cfg(windows)]
mod glow_proof;
#[cfg(windows)]
mod gpu;

// Exact shared consumer and GL capsule; no Preview/document/Runtime owner.
#[cfg(windows)]
#[allow(
    dead_code,
    unused_imports,
    reason = "unchanged shared native qualification sources"
)]
#[path = "../../../../apps/lab-native-child/src/consumer.rs"]
mod consumer;
#[cfg(windows)]
use consumer::{presentation, readonly_limits, readonly_v2, readonly_values};
#[cfg(windows)]
mod gui {
    pub(crate) use crate::readonly_v2;
}
#[cfg(windows)]
#[allow(dead_code, reason = "unchanged audited native GL capsule")]
#[path = "../../native-glow-device.rs"]
mod device;

#[cfg(windows)]
type Result<T> = std::result::Result<T, String>;

#[cfg(windows)]
fn write(path: &std::path::Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

/// Keep the caller's original root alive until its owning Job contains it.
#[cfg(windows)]
fn contained_failure(dir: &std::path::Path, error: impl std::fmt::Display) -> ! {
    write(
        &dir.join("failure.json"),
        &serde_json::json!({"error": error.to_string(), "root_retained": true}),
    );
    wait_for_containment()
}

#[cfg(windows)]
fn wait_for_containment() -> ! {
    loop {
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.as_slice() {
        [_, mode, dir] if mode == "--controller" => {
            controller::run(std::path::Path::new(dir)).unwrap()
        }
        [_, mode, case, dir] if mode == "--child" => {
            let dir = std::path::Path::new(dir);
            let panic_path = dir.join("panic.json");
            std::panic::set_hook(Box::new(move |info| {
                // Job launch intentionally does not inherit arbitrary std handles.
                // Retain the original panic text before abort closes the process.
                let diagnostic = info.to_string();
                let _ = std::fs::write(&panic_path, serde_json::to_vec_pretty(&serde_json::json!({"panic":diagnostic.chars().take(8192).collect::<String>(),"policy":"abort; no GPU-root unwind"})).unwrap());
            }));
            write(
                &dir.join("entered.json"),
                &serde_json::json!({"case": case, "pid": std::process::id()}),
            );
            if case == "glow" {
                glow_proof::run(dir);
            } else if let Err(error) = gpu::run(case, dir) {
                // Root::new has not published a GPU root or issued queue writes.
                // Initialization references may have dropped: do not describe
                // this as retention or as native memory cleanup.
                write(
                    &dir.join("failure.json"),
                    &serde_json::json!({"error":error,"stage":"initialization before published root","root_retained":false,"native_memory_cleanup":false}),
                );
                wait_for_containment();
            }
        }
        _ => panic!("test-only usage: --controller ABSOLUTE_EVIDENCE_DIRECTORY"),
    }
}

#[cfg(not(windows))]
fn main() {
    panic!("RENDER-2A is Windows/MSVC only");
}

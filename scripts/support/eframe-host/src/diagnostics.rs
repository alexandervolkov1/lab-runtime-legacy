//! Bounded upstream diagnostics. No graphics API calls in the application.
use std::{fs::File, io::Write, sync::Mutex};

struct Logger(Mutex<(File, usize)>);
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info || metadata.target().starts_with("egui_glow")
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut output = self.0.lock().unwrap();
        if output.1 < 4096 {
            let text: String = format!("{} {} {}", record.level(), record.target(), record.args())
                .chars()
                .take(4096)
                .collect();
            let _ = writeln!(output.0, "{text}");
            output.1 += 1;
        }
    }
    fn flush(&self) {
        let _ = self.0.lock().unwrap().0.flush();
    }
}

pub(super) fn install(dir: &std::path::Path) -> crate::Result<()> {
    let file = File::create(dir.join("framework.log")).map_err(|e| e.to_string())?;
    log::set_boxed_logger(Box::new(Logger(Mutex::new((file, 0))))).map_err(|e| e.to_string())?;
    log::set_max_level(log::LevelFilter::Debug);
    let path = dir.join("panic.json");
    std::panic::set_hook(Box::new(move |info| {
        let message: String = info.to_string().chars().take(8192).collect();
        crate::write(&path, &serde_json::json!({"panic":message}));
    }));
    Ok(())
}

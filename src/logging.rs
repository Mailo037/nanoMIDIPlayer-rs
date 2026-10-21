//! File logging to `Documents/nanoMIDIPlayer/logs` (newest 10 files kept) and the
//! optional debug console from the Settings tab.

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

static CONSOLE_OPEN: AtomicBool = AtomicBool::new(false);

struct Logger {
    file: Mutex<Option<File>>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target().starts_with("nanomidiplayer") || metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} [{}] {}: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
            record.args()
        );
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.write_all(line.as_bytes());
        }
        if cfg!(debug_assertions) || CONSOLE_OPEN.load(Ordering::Relaxed) {
            print!("{line}");
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().as_mut() {
            let _ = f.flush();
        }
    }
}

pub fn init() {
    let dir = crate::config::base_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut logs: Vec<_> = rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with("nanoMIDIPlayer_") && n.ends_with(".log")
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        logs.sort();
        if logs.len() > 9 {
            for (_, path) in &logs[..logs.len() - 9] {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let path = dir.join(format!(
        "nanoMIDIPlayer_{}.log",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    ));
    let logger = Box::leak(Box::new(Logger {
        file: Mutex::new(File::create(&path).ok()),
    }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    log::info!("Logging initialized -> {}", path.display());
}

pub fn open_console() {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::System::Console::AllocConsole();
    }
    CONSOLE_OPEN.store(true, Ordering::Relaxed);
    log::info!("Console opened and all loggers linked.");
}

pub fn close_console() {
    log::info!("Console closed");
    CONSOLE_OPEN.store(false, Ordering::Relaxed);
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::System::Console::FreeConsole();
    }
}

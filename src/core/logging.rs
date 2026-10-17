use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use log::{Level, Metadata, Record};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

use crate::core::config;

/// Outside the logger, so the file can be opened again in append mode.
static FILE: Mutex<Option<File>> = Mutex::new(None);

struct FileLogger {
    level: Level,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let target = record.target().rsplit("::").next().unwrap_or("wincraft");
        let line = format!(
            "{} {:<5} {}: {}\n",
            now(),
            record.level(),
            target,
            record.args()
        );
        if let Ok(mut guard) = FILE.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = FILE.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

fn now() -> String {
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

pub fn init() {
    start(&config::log_path(), false);
}

/// For a WinCraft started by an update, or started again after one failed:
/// the version before it still writes wincraft.log, so the file is added to
/// and never emptied.
pub fn init_appending() {
    start(&config::log_path(), true);
}

/// Logs to `path`, emptied first. `--install` writes its own file, because
/// the WinCraft it starts empties wincraft.log a moment later.
pub fn init_to(path: &Path) {
    start(path, false);
}

/// The old version that waits for its update keeps writing while the new
/// one writes too. Both must add at the end, or each overwrites the other.
pub fn reopen_appending() {
    if let Some(file) = open(&config::log_path(), true) {
        if let Ok(mut guard) = FILE.lock() {
            *guard = Some(file);
        }
    }
}

fn open(path: &Path, append: bool) -> Option<File> {
    fs::create_dir_all(path.parent()?).ok()?;
    if append {
        File::options().append(true).create(true).open(path).ok()
    } else {
        File::create(path).ok()
    }
}

fn start(path: &Path, append: bool) {
    let debug = std::env::var("WINCRAFT_DEBUG")
        .map(|v| v == "1")
        .unwrap_or(false);
    let level = if debug { Level::Debug } else { Level::Info };

    if let Ok(mut guard) = FILE.lock() {
        *guard = open(path, append);
    }
    if log::set_boxed_logger(Box::new(FileLogger { level })).is_ok() {
        log::set_max_level(level.to_level_filter());
    }
}

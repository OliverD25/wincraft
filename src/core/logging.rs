use std::fs::{self, File};
use std::io::Write;
use std::sync::Mutex;

use log::{Level, Metadata, Record};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

use crate::core::config;

struct FileLogger {
    file: Mutex<Option<File>>,
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
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
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
    let debug = std::env::var("WINCRAFT_DEBUG")
        .map(|v| v == "1")
        .unwrap_or(false);
    let level = if debug { Level::Debug } else { Level::Info };

    let path = config::log_path();
    let file = path
        .parent()
        .and_then(|dir| fs::create_dir_all(dir).ok())
        .and_then(|()| File::create(&path).ok());

    let logger = FileLogger {
        file: Mutex::new(file),
        level,
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level.to_level_filter());
    }
}

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

/// Starts a new wincraft.log. The log of the run before is kept as
/// wincraft.prev.log, so a restart or a reboot does not wipe the record of
/// what happened just before it.
pub fn init() {
    let path = config::log_path();
    keep_previous(&path);
    start(&path, false);
}

/// `wincraft.prev.log` next to `wincraft.log`.
pub fn previous_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("prev.log")
}

/// Moves the log to its `previous_path`, over the one kept before, so only
/// one older run is kept and the two files together stay about two runs
/// long. Nothing happens when there is no log yet; when the move fails the
/// log is emptied as before, which is better than not logging.
fn keep_previous(path: &Path) {
    if path.exists() {
        let _ = fs::rename(path, previous_path(path));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_start_keeps_one_older_log() {
        let dir = std::env::temp_dir().join(format!("wincraft-test-log-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("wincraft.log");
        let prev = previous_path(&log);
        assert_eq!(prev, dir.join("wincraft.prev.log"));

        keep_previous(&log);
        assert!(!prev.exists(), "no log yet, nothing to keep");

        fs::write(&log, "first run\n").unwrap();
        keep_previous(&log);
        assert!(!log.exists());
        assert_eq!(fs::read_to_string(&prev).unwrap(), "first run\n");

        fs::write(&log, "second run\n").unwrap();
        keep_previous(&log);
        assert_eq!(
            fs::read_to_string(&prev).unwrap(),
            "second run\n",
            "only the run just before is kept"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wincraft-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn opening_a_log_makes_its_folder_and_either_empties_or_adds() {
        let dir = scratch("open");
        let path = dir.join("deeper").join("wincraft.log");
        {
            let mut file = open(&path, false).expect("a new log");
            file.write_all(b"one\n").unwrap();
        }
        {
            let mut file = open(&path, true).expect("adding to it");
            file.write_all(b"two\n").unwrap();
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        drop(open(&path, false).expect("emptied again"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_logger_writes_one_line_per_record_at_or_above_its_level() {
        use log::Log;
        let dir = scratch("lines");
        let path = dir.join("wincraft.log");
        *FILE.lock().unwrap() = open(&path, false);
        let logger = FileLogger { level: Level::Info };
        let record = |level, target: &str, text: &str| {
            logger.log(
                &Record::builder()
                    .level(level)
                    .target(target)
                    .args(format_args!("{text}"))
                    .build(),
            )
        };
        record(Level::Info, "wincraft::core::thing", "hello there");
        record(Level::Debug, "wincraft::core::thing", "too chatty");
        record(Level::Warn, "plain", "careful");
        logger.flush();
        *FILE.lock().unwrap() = None;
        record(Level::Error, "wincraft::lost", "nowhere to go");
        logger.flush();
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(
            lines[0].ends_with(" INFO  thing: hello there"),
            "{}",
            lines[0]
        );
        assert!(lines[1].ends_with(" WARN  plain: careful"), "{}", lines[1]);
        let stamp = &lines[0][..19];
        assert_eq!(stamp.as_bytes()[4], b'-');
        assert_eq!(stamp.as_bytes()[10], b' ');
        assert_eq!(stamp.as_bytes()[13], b':');
        assert!(logger.enabled(&Metadata::builder().level(Level::Error).build()));
        assert!(!logger.enabled(&Metadata::builder().level(Level::Trace).build()));
        let _ = fs::remove_dir_all(&dir);
    }
}

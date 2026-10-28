//! Writes a settings or state file on a thread of its own, so the host
//! thread never waits for the disk. A write through `config::write_atomic`
//! flushes the file to disk, which takes 1–2 ms on a quiet PC and far more
//! while the disk or a virus scanner is busy.
//!
//! The newest text for a path always wins: when several writes for one path
//! queue up, only the newest is written. `write_now` is for the end of a
//! session, when the process may end right after.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::core::config;

/// The newest text waiting for each path.
static PENDING: Mutex<BTreeMap<PathBuf, String>> = Mutex::new(BTreeMap::new());
/// Held while a file is written, so two writes never cross.
static WRITING: Mutex<()> = Mutex::new(());

/// Writes `text` to `path` soon, on a thread of its own. `what` names the
/// file in the log if the write fails.
pub fn write_later(path: PathBuf, text: String, what: &'static str) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.insert(path.clone(), text);
    }
    std::thread::spawn(move || {
        if let Err(err) = flush(&path) {
            log::error!("could not save {what}: {err}");
        }
    });
}

/// Writes `text` to `path` now, after any write already under way, and
/// drops any older text still waiting for it.
pub fn write_now(path: &Path, text: &str) -> Result<(), String> {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut pending) = PENDING.lock() {
        pending.remove(path);
    }
    config::write_atomic(path, text)
}

/// Writes the newest text waiting for `path`, if a newer thread has not
/// written it already.
fn flush(path: &Path) -> Result<(), String> {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let text = PENDING
        .lock()
        .ok()
        .and_then(|mut pending| pending.remove(path));
    match text {
        Some(text) => config::write_atomic(path, &text),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wincraft-write-later-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("file.json")
    }

    /// Waits until nothing is waiting for `path` and no write is under way.
    fn settle(path: &Path) {
        for _ in 0..500 {
            let waiting = PENDING.lock().unwrap().contains_key(path);
            if !waiting {
                drop(WRITING.lock().unwrap_or_else(|p| p.into_inner()));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("the write never happened");
    }

    #[test]
    fn a_later_write_reaches_the_file() {
        let path = scratch("one");
        write_later(path.clone(), "first".to_string(), "a test file");
        settle(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_newest_of_several_writes_wins() {
        let path = scratch("many");
        {
            // Hold the disk, as a slow write would, while the texts queue.
            let _busy = WRITING.lock().unwrap_or_else(|p| p.into_inner());
            for n in 0..5 {
                write_later(path.clone(), format!("text {n}"), "a test file");
            }
        }
        settle(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "text 4");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_write_now_is_not_undone_by_an_older_write_still_waiting() {
        let path = scratch("now");
        {
            let busy = WRITING.lock().unwrap_or_else(|p| p.into_inner());
            write_later(path.clone(), "older".to_string(), "a test file");
            drop(busy);
            write_now(&path, "at the end").unwrap();
        }
        settle(&path);
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "at the end");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

//! The detailed log's writer thread and its files: `detailed-<date>.log`,
//! then `detailed-<date>-2.log` and so on when one reaches `MAX_BYTES`, at
//! most `KEEP` files in all; the oldest go first.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

/// The most one file holds.
pub const MAX_BYTES: u64 = 100 * 1024 * 1024;
/// The most files kept.
pub const KEEP: usize = 5;

pub enum Msg {
    /// One finished line, without its line break.
    Line(String),
    /// Closes the file; the next line opens one again.
    Close,
    /// Writes out what is buffered, then answers.
    Flush(Sender<()>),
    /// Closes the file, deletes every file, answers with how many went.
    DeleteAll(Sender<usize>),
}

/// `detailed-2026-10-28.log` for the first part of a day, then
/// `detailed-2026-10-28-2.log`.
pub fn file_name(date: &str, part: u32) -> String {
    if part <= 1 {
        format!("detailed-{date}.log")
    } else {
        format!("detailed-{date}-{part}.log")
    }
}

/// The date and part of a detailed log file's name; None for other files.
pub fn parse_name(name: &str) -> Option<(String, u32)> {
    let rest = name.strip_prefix("detailed-")?.strip_suffix(".log")?;
    if rest.len() < 10 {
        return None;
    }
    let (date, tail) = rest.split_at(10);
    let digits = |text: &str| text.chars().all(|ch| ch.is_ascii_digit());
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 || parts.iter().any(|part| !digits(part)) {
        return None;
    }
    let part = match tail {
        "" => 1,
        _ => {
            let number = tail.strip_prefix('-')?;
            if number.is_empty() || !digits(number) {
                return None;
            }
            number.parse().ok()?
        }
    };
    Some((date.to_string(), part))
}

/// The detailed log files in `dir`, oldest first.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<((String, u32), PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            parse_name(&name).map(|key| (key, entry.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

/// The files to delete so that at most `keep` remain, from a list oldest
/// first.
pub fn beyond_keep(files: &[PathBuf], keep: usize) -> &[PathBuf] {
    &files[..files.len().saturating_sub(keep)]
}

/// Deletes every detailed log file in `dir`; returns how many went.
pub fn delete_all(dir: &Path) -> usize {
    files_in(dir)
        .iter()
        .filter(|path| fs::remove_file(path).is_ok())
        .count()
}

struct Open {
    file: BufWriter<File>,
    bytes: u64,
    date: String,
    part: u32,
}

pub struct Writer {
    dir: PathBuf,
    cap: u64,
    open: Option<Open>,
}

impl Writer {
    pub fn new(dir: PathBuf, cap: u64) -> Self {
        Writer {
            dir,
            cap,
            open: None,
        }
    }

    /// Writes one line; its first ten characters are its date.
    pub fn line(&mut self, line: &str) {
        let date = line.get(..10).unwrap_or("0000-00-00").to_string();
        let full = self
            .open
            .as_ref()
            .is_some_and(|open| open.bytes >= self.cap);
        let other_day = self.open.as_ref().is_some_and(|open| open.date != date);
        if other_day {
            self.close();
        } else if full {
            let part = self.open.as_ref().map_or(1, |open| open.part + 1);
            self.close();
            self.open_part(&date, part, true);
        }
        if self.open.is_none() {
            self.open_part(&date, 0, false);
        }
        if let Some(open) = self.open.as_mut() {
            if open.file.write_all(line.as_bytes()).is_ok() && open.file.write_all(b"\n").is_ok() {
                open.bytes += line.len() as u64 + 1;
            }
        }
    }

    /// Opens `date`'s file `part`. Part 0 means "the last part of that day
    /// that still has room, or a new one". `fresh` starts the part empty.
    fn open_part(&mut self, date: &str, part: u32, fresh: bool) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let mut part = part;
        if part == 0 {
            let last = files_in(&self.dir)
                .iter()
                .filter_map(|path| parse_name(&path.file_name()?.to_string_lossy()))
                .filter(|(day, _)| day == date)
                .map(|(_, part)| part)
                .max()
                .unwrap_or(1);
            let size = fs::metadata(self.dir.join(file_name(date, last)))
                .map(|meta| meta.len())
                .unwrap_or(0);
            part = if size >= self.cap { last + 1 } else { last };
        }
        let path = self.dir.join(file_name(date, part));
        let file = if fresh {
            File::create(&path)
        } else {
            File::options().append(true).create(true).open(&path)
        };
        let Ok(file) = file else {
            return;
        };
        let bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        self.open = Some(Open {
            file: BufWriter::new(file),
            bytes,
            date: date.to_string(),
            part,
        });
        for old in beyond_keep(&files_in(&self.dir), KEEP) {
            let _ = fs::remove_file(old);
        }
    }

    pub fn flush(&mut self) {
        if let Some(open) = self.open.as_mut() {
            let _ = open.file.flush();
        }
    }

    pub fn close(&mut self) {
        self.flush();
        self.open = None;
    }

    pub fn delete_all(&mut self) -> usize {
        self.close();
        delete_all(&self.dir)
    }
}

/// The writer thread: writes what comes, and flushes whenever the channel
/// is empty, so a crash loses at most the lines of the last moment.
pub fn run(mut writer: Writer, inbox: Receiver<Msg>) {
    while let Ok(first) = inbox.recv() {
        let mut next = Some(first);
        while let Some(msg) = next {
            match msg {
                Msg::Line(line) => writer.line(&line),
                Msg::Close => writer.close(),
                Msg::Flush(done) => {
                    writer.flush();
                    let _ = done.send(());
                }
                Msg::DeleteAll(done) => {
                    let _ = done.send(writer.delete_all());
                }
            }
            next = inbox.try_recv().ok();
        }
        writer.flush();
    }
}

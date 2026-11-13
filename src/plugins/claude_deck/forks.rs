//! Forks running in a terminal. "Fork in a terminal" starts a new chat in
//! a Windows Terminal window with a title WinCraft chose and a session id
//! WinCraft chose. Each start is written to a small file; while the
//! window is open, the fork shows as a row under its chat.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};

use crate::core::claude_sessions::{self, Age};
use crate::core::claude_turns::End;
use crate::core::{config, instance, windows_list};

use super::model::{project_folder, Chat, Mark};
use super::servers::status::Parsed;
use super::status::{full_text, line_text};

#[cfg(test)]
mod tests;

/// The id of a terminal fork's row.
pub const ID_PREFIX: &str = "fork/";
const MAX_RECORDS: usize = 20;
const HOUR_MS: u64 = 60 * 60 * 1000;
const KEEP_MS: u64 = 24 * HOUR_MS;
/// A fork whose window has not shown up yet is kept this long.
const GRACE_MS: u64 = 30_000;
const TITLE_MAX: usize = 60;
const SHORT: usize = 8;
const READ_TAIL: u64 = 256 * 1024;

/// One fork that was started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The new chat's session id.
    pub id: String,
    /// The chat it was forked from, as ClaudeDeck knows it.
    pub parent: String,
    /// The fork's name: "Fork of <chat>".
    pub title: String,
    /// The folder it runs in (on the server for a server fork).
    pub folder: String,
    #[serde(default)]
    pub server: Option<String>,
    /// Unix ms.
    pub started: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    records: Vec<Record>,
}

pub fn path() -> PathBuf {
    config::plugins_dir().join("claude_deck.forks.json")
}

/// The records in the file; none when it is missing or broken.
pub fn load(path: &Path) -> Vec<Record> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Stored>(config::strip_bom(&text))
        .map(|stored| stored.records)
        .unwrap_or_default()
}

fn save(path: &Path, records: &[Record]) {
    let stored = Stored {
        records: records.to_vec(),
    };
    let written = serde_json::to_string_pretty(&stored)
        .map_err(|err| err.to_string())
        .and_then(|text| std::fs::write(path, text).map_err(|err| err.to_string()));
    if let Err(err) = written {
        log::warn!("ClaudeDeck: could not write {}: {err}", path.display());
    }
}

/// The records with a new one, at most `MAX_RECORDS`: the oldest go first.
pub fn add(mut records: Vec<Record>, record: Record) -> Vec<Record> {
    records.push(record);
    records.sort_by_key(|record| record.started);
    let extra = records.len().saturating_sub(MAX_RECORDS);
    records.drain(..extra);
    records
}

/// A random version 4 UUID from the bytes given.
pub fn uuid_from(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn new_uuid() -> Option<String> {
    let mut bytes = [0u8; 16];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            16,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Some(uuid_from(bytes))
    } else {
        log::warn!("ClaudeDeck: Windows gave no random numbers ({status:#x})");
        None
    }
}

/// Words that are safe on the Windows Terminal command line: no quotes,
/// no `;` (Windows Terminal splits its tabs there), no control characters.
pub fn clean(words: &str, most: usize) -> String {
    words
        .chars()
        .filter(|c| !matches!(c, ';' | '"' | '\'' | '\\') && !c.is_control())
        .take(most)
        .collect::<String>()
        .trim()
        .to_string()
}

fn window_title(name: &str, id: &str) -> String {
    let short: String = id.chars().take(SHORT).collect();
    format!("{name} \u{b7} {short}")
}

/// A fork being started: its new session id and its name.
pub struct Launch {
    pub id: String,
    name: String,
}

impl Launch {
    pub fn new(parent_title: &str) -> Option<Launch> {
        new_uuid().map(|id| Launch::with_id(&id, parent_title))
    }

    pub fn with_id(id: &str, parent_title: &str) -> Launch {
        let room = TITLE_MAX - SHORT - 3;
        Launch {
            id: id.to_string(),
            name: clean(&format!("Fork of {parent_title}"), room),
        }
    }

    /// The Windows Terminal options before the command: a new window, with
    /// a title Claude cannot change.
    pub fn wt_prefix(&self) -> Vec<String> {
        vec![
            "-w".to_string(),
            "new".to_string(),
            "--title".to_string(),
            window_title(&self.name, &self.id),
            "--suppressApplicationTitle".to_string(),
        ]
    }

    /// Writes the start down, so the window shows as a row.
    pub fn remember(&self, parent: &str, folder: &str, server: Option<&str>) {
        if cfg!(test) || instance::is_test() {
            return;
        }
        let record = Record {
            id: self.id.clone(),
            parent: parent.to_string(),
            title: self.name.clone(),
            folder: folder.to_string(),
            server: server.map(str::to_string),
            started: claude_sessions::now_ms(),
        };
        let path = path();
        save(&path, &add(load(&path), record));
    }
}

/// What the end of a fork's transcript says.
pub struct Transcript {
    pub parsed: Parsed,
    /// Since the transcript was last written.
    pub quiet_ms: u64,
}

fn mark_of(transcript: &Transcript) -> Mark {
    let turn = &transcript.parsed.turn;
    let recent = transcript.quiet_ms < HOUR_MS;
    if turn.question.is_some() {
        Mark::AsksYou
    } else if recent && matches!(turn.end, End::UserMessage | End::ToolCalled | End::ToolDone) {
        Mark::Working
    } else if recent && turn.end == End::Interrupted {
        Mark::Stopped
    } else if recent && turn.end == End::Failed {
        Mark::Failed
    } else {
        Mark::Idle(Age::of(transcript.quiet_ms))
    }
}

fn row(
    record: &Record,
    window: isize,
    chats: &[Chat],
    now: u64,
    transcript: Option<Transcript>,
) -> Chat {
    let on_pc = record.server.is_none();
    let parent = chats
        .iter()
        .find(|chat| on_pc && chat.id == record.parent)
        .map(|chat| chat.id.clone());
    let mut chat = Chat {
        id: format!("{ID_PREFIX}{}", record.id),
        title: record.title.clone(),
        folder: record
            .server
            .is_none()
            .then(|| project_folder(&record.folder))
            .flatten(),
        branch: None,
        model: None,
        parent,
        created: record.started,
        mark: Mark::Idle(Age::Fresh),
        since: Some(record.started),
        jobs: 0,
        window: Some(window),
        text: None,
        full: None,
        fork: None,
        terminal: Some(match &record.server {
            Some(server) => format!("{server}:{}", record.folder),
            None => record.folder.clone(),
        }),
        news: false,
        job_on: None,
    };
    match (&record.server, transcript) {
        (Some(server), _) => {
            let name = server.strip_prefix("rde").unwrap_or(server);
            chat.text = Some(format!("Runs on {name} in a terminal"));
        }
        (None, Some(transcript)) => {
            chat.mark = mark_of(&transcript);
            chat.since = Some(now.saturating_sub(transcript.quiet_ms));
            chat.text = line_text(chat.mark, Some(&transcript.parsed.turn));
            chat.full = full_text(chat.mark, Some(&transcript.parsed.turn));
            if let Some(title) = transcript.parsed.title {
                chat.title = title;
            }
        }
        (None, None) => {}
    }
    chat
}

/// The rows of the forks whose window is open (`windows` are the open
/// windows' handles and titles), and the records to keep: those with a
/// window, and those started a moment ago whose window has not opened yet.
pub fn rows(
    records: &[Record],
    windows: &[(isize, String)],
    chats: &[Chat],
    now: u64,
    read: &dyn Fn(&str) -> Option<Transcript>,
) -> (Vec<Chat>, Vec<Record>) {
    let mut shown = Vec::new();
    let mut kept = Vec::new();
    for record in records {
        let age = now.saturating_sub(record.started);
        let wanted = window_title(&record.title, &record.id);
        let window = windows.iter().find(|(_, title)| title.contains(&wanted));
        match window {
            Some((hwnd, _)) if age < KEEP_MS => {
                shown.push(row(record, *hwnd, chats, now, read(&record.id)));
                kept.push(record.clone());
            }
            None if age < GRACE_MS => kept.push(record.clone()),
            _ => {}
        }
    }
    (shown, kept)
}

/// The end of the transcript `<id>.jsonl` in any project folder.
fn read_transcript(id: &str) -> Option<Transcript> {
    let projects = std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join(".claude").join("projects"))?;
    let name = format!("{id}.jsonl");
    let file = std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|folder| folder.path().join(&name))
        .find(|path| path.is_file())?;
    let mut open = std::fs::File::open(&file).ok()?;
    let length = open.metadata().ok()?.len();
    let written = open.metadata().ok()?.modified().ok()?;
    open.seek(SeekFrom::Start(length.saturating_sub(READ_TAIL)))
        .ok()?;
    let mut bytes = Vec::new();
    open.take(READ_TAIL).read_to_end(&mut bytes).ok()?;
    let quiet = written
        .elapsed()
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    Some(Transcript {
        parsed: super::servers::status::parse_tail(&bytes, length <= READ_TAIL),
        quiet_ms: quiet,
    })
}

/// Adds the rows of the open terminal forks to the chats, and forgets the
/// forks whose window is gone. Reads nothing when no fork was started.
pub fn add_rows(chats: &mut Vec<Chat>, now: u64) {
    let path = path();
    let records = load(&path);
    if records.is_empty() {
        return;
    }
    let windows: Vec<(isize, String)> = windows_list::app_window_records(&|_| true)
        .into_iter()
        .map(|(hwnd, record)| (hwnd as isize, record.title))
        .collect();
    let (shown, kept) = rows(&records, &windows, chats, now, &read_transcript);
    if kept != records && !cfg!(test) && !instance::is_test() {
        save(&path, &kept);
    }
    chats.extend(shown);
}

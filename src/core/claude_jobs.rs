//! Background jobs of a Claude Code chat, for Custom WindowDeck's
//! "Background" status. The session file does not list them, but the chat's
//! transcript does: `%USERPROFILE%\.claude\projects\<folder>\<cliSessionId>.jsonl`.
//! A job starts with a tool result that holds `"backgroundTaskId":"<id>"`
//! (a background command) or `"status":"async_launched"` with
//! `"agentId":"<id>"` (a background subagent). A subagent that finished and
//! is sent a new message works again: its result holds
//! `"resumedAgentId":"<id>"`, and the same id starts once more. A job ends
//! with a task notice
//! (`<task-id><id></task-id>` and a `<status>`), or when the chat stops it
//! with the `TaskStop` tool.
//!
//! The same pass keeps the chat's last records for ClaudeDeck
//! (`claude_turns`): how its last turn ended and what Claude said last.
//!
//! The transcript can be 20 MB or more, so it is read once and then only
//! what was appended (Principle 5). Text is searched, not parsed as JSON,
//! so a line of an unknown shape means "no job", never a wrong status.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::core::claude_launches::{Launch, Launches};
use crate::core::claude_turns::{Replies, Reply, Tail, Turn};

/// A job that started this long ago and never ended is ignored: the app may
/// have been closed while it ran, and then no end notice is ever written.
pub const MAX_JOB_AGE_MS: u64 = 3 * 60 * 60 * 1000;

/// A transcript is read this much at a time. A first read of a 200 MB
/// transcript must not hold all of it in memory at once (Principle 5).
const READ_CHUNK: usize = 1024 * 1024;

/// How far after a `<task-id>` its `<status>` may be, in bytes.
const STATUS_REACH: usize = 400;

/// What was read of one transcript: how far, and which jobs are open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Bytes of the file already read.
    offset: u64,
    /// The unfinished last line, kept for the next read.
    partial: Vec<u8>,
    /// Open jobs: id and start time in Unix milliseconds.
    open: HashMap<String, u64>,
    /// Jobs that ended. A later line that only quotes such an id (a grep
    /// result, a log) must not open it again.
    ended: HashSet<String>,
}

impl Scan {
    /// Reads bytes appended to the transcript. `seen_ms` is the start time
    /// of a job whose line has no `"timestamp"`.
    #[cfg(test)]
    pub fn feed(&mut self, bytes: &[u8], seen_ms: u64) {
        self.feed_with(bytes, seen_ms, |_| {});
    }

    /// The same, and hands each whole line to `each` as well.
    pub fn feed_with(&mut self, bytes: &[u8], seen_ms: u64, mut each: impl FnMut(&str)) {
        self.offset += bytes.len() as u64;
        self.partial.extend_from_slice(bytes);
        let mut start = 0;
        while let Some(end) = self.partial[start..].iter().position(|b| *b == b'\n') {
            let line = String::from_utf8_lossy(&self.partial[start..start + end]).into_owned();
            self.line(&line, seen_ms);
            each(&line);
            start += end + 1;
        }
        self.partial.drain(..start);
    }

    /// Forgets everything, for a file that got shorter.
    pub fn reset(&mut self) {
        *self = Scan::default();
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    fn line(&mut self, line: &str, seen_ms: u64) {
        let started = line.contains("backgroundTaskId")
            || line.contains("async_launched")
            || line.contains("resumedAgentId");
        let ended = line.contains("<task-id>") || line.contains("TaskStop");
        if !started && !ended {
            return;
        }
        // A result may hold its JSON as an escaped string.
        let text = line.replace("\\\"", "\"");
        let at = timestamp_ms(&text).unwrap_or(seen_ms);
        if started {
            for id in values_after(&text, "\"backgroundTaskId\":\"") {
                self.start(id, at);
            }
            if text.contains("\"status\":\"async_launched\"") {
                for id in values_after(&text, "\"agentId\":\"") {
                    self.start(id, at);
                }
            }
            for id in values_after(&text, "\"resumedAgentId\":\"") {
                self.resume(id, at);
            }
        }
        if text.contains("\"name\":\"TaskStop\"") {
            for id in values_after(&text, "\"task_id\":\"") {
                self.end(&id);
            }
        }
        let mut rest = text.as_str();
        while let Some(found) = rest.find("<task-id>") {
            rest = &rest[found + "<task-id>".len()..];
            let Some(close) = rest.find("</task-id>") else {
                break;
            };
            let id = &rest[..close];
            let after = &rest[close..];
            let mut reach = after.len().min(STATUS_REACH);
            while !after.is_char_boundary(reach) {
                reach -= 1;
            }
            let window = &after[..reach];
            let window = window
                .find("<task-id>")
                .map_or(window, |next| &window[..next]);
            if window.contains("<status>") && is_id(id) {
                self.end(id);
            }
        }
    }

    fn start(&mut self, id: String, at: u64) {
        if !self.ended.contains(&id) {
            self.open.entry(id).or_insert(at);
        }
    }

    /// A finished subagent that was sent a new message runs again.
    fn resume(&mut self, id: String, at: u64) {
        self.ended.remove(&id);
        self.open.insert(id, at);
    }

    fn end(&mut self, id: &str) {
        self.open.remove(id);
        self.ended.insert(id.to_string());
    }

    /// The jobs still running at `now`: how many, and the start of the
    /// oldest. Jobs older than `MAX_JOB_AGE_MS` are ignored.
    pub fn running(&self, now: u64) -> (usize, Option<u64>) {
        self.open
            .values()
            .filter(|start| now.saturating_sub(**start) < MAX_JOB_AGE_MS)
            .fold((0, None::<u64>), |(count, oldest), start| {
                (count + 1, Some(oldest.map_or(*start, |o| o.min(*start))))
            })
    }
}

fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The ids written right after `key` (which ends with an opening quote).
fn values_after(text: &str, key: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(key) {
        rest = &rest[at + key.len()..];
        let end = rest.find('"').unwrap_or(rest.len());
        let id = &rest[..end];
        if is_id(id) {
            found.push(id.to_string());
        }
    }
    found
}

/// The line's `"timestamp":"2026-11-11T08:30:00.123Z"` as Unix ms.
pub(crate) fn timestamp_ms(text: &str) -> Option<u64> {
    let key = "\"timestamp\":\"";
    let at = text.find(key)? + key.len();
    let stamp = text.get(at..at + 19)?;
    let num = |from: usize, to: usize| -> Option<i64> { stamp.get(from..to)?.parse().ok() };
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let millis = text
        .get(at + 19..)
        .and_then(|s| s.strip_prefix('.'))
        .and_then(|s| s.get(..3))
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    // Days since 1970-01-01 (the civil-date algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second;
    u64::try_from(secs * 1000 + millis).ok()
}

/// A transcript found on disk, with what was read of it.
#[derive(Default)]
struct Entry {
    path: Option<PathBuf>,
    scan: Scan,
    tail: Tail,
    /// The last turn, and the offset it was worked out at.
    turn: Option<(u64, Turn)>,
    replies: Replies,
    /// The finished replies, and the offset they were taken at.
    shown: Option<(u64, Arc<Vec<Reply>>)>,
    /// The ssh calls (`claude_launches`), and those of the last week as
    /// taken at an offset.
    launches: Launches,
    sent: Option<(u64, Arc<Vec<Launch>>)>,
}

/// What the reader thread has read so far. Only `refresh` touches it.
static READ: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
/// The open jobs of each chat, written whole after each read. A read of
/// ClaudeDeck's ~300 chats can take seconds, and WindowDeck's Background
/// chip must keep its count all that time.
static OPEN: Mutex<Option<HashMap<String, Scan>>> = Mutex::new(None);
static WANTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static WANTED_BY_DECK: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The last turns of ClaudeDeck's chats, written whole after each read, so
/// the UI thread never sees a read half done.
static TURNS: Mutex<Option<HashMap<String, Turn>>> = Mutex::new(None);
/// The finished replies of ClaudeDeck's chats, for its News tab, written
/// whole after each read like the turns.
static REPLIES: Mutex<Option<HashMap<String, Arc<Vec<Reply>>>>> = Mutex::new(None);
/// The ssh calls of ClaudeDeck's chats, written whole like the turns.
static LAUNCHES: Mutex<Option<HashMap<String, Arc<Vec<Launch>>>>> = Mutex::new(None);

/// Names the chats whose windows are shown now (their `cliSessionId`s);
/// only these are read at the next `refresh`.
pub fn want(ids: Vec<String>) {
    if let Ok(mut wanted) = WANTED.lock() {
        *wanted = ids;
    }
}

/// The same as `want`, for ClaudeDeck's list. The two lists are kept
/// apart, so neither window drops the other's chats.
pub fn want_for_deck(ids: Vec<String>) {
    if let Ok(mut wanted) = WANTED_BY_DECK.lock() {
        *wanted = ids;
    }
}

/// Both lists, each id once.
fn wanted() -> Vec<String> {
    let mut ids = WANTED
        .lock()
        .map(|wanted| wanted.clone())
        .unwrap_or_default();
    if let Ok(deck) = WANTED_BY_DECK.lock() {
        for id in deck.iter() {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
    }
    ids
}

fn projects_folder() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude").join("projects"))
}

/// `<projects>\*\<cli_session_id>.jsonl`; the folder name can change, so
/// every folder is looked in.
fn find_transcript(projects: &Path, cli_session_id: &str) -> Option<PathBuf> {
    let name = format!("{cli_session_id}.jsonl");
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|folder| folder.path().join(&name))
        .find(|path| path.is_file())
}

/// Reads what was appended to the transcript of each wanted chat, and
/// forgets the chats not wanted. Runs on the session reader's thread. A
/// missing transcript means no jobs.
pub fn refresh(now: u64) {
    let ids = wanted();
    let Some(projects) = projects_folder() else {
        return;
    };
    let mut entries = READ
        .lock()
        .ok()
        .and_then(|mut cache| cache.take())
        .unwrap_or_default();
    entries.retain(|id, _| ids.contains(id));
    for id in ids.iter().filter(|id| !id.is_empty()) {
        let entry = entries.entry(id.clone()).or_default();
        if entry.path.as_ref().is_none_or(|path| !path.is_file()) {
            entry.path = find_transcript(&projects, id);
            entry.scan.reset();
            entry.tail.reset();
            entry.turn = None;
            entry.replies.reset();
            entry.shown = None;
            entry.launches.reset();
            entry.sent = None;
        }
        let Some(path) = entry.path.clone() else {
            continue;
        };
        if let Err(err) = read_new(&path, entry, now) {
            log::debug!("Custom WindowDeck: could not read a chat transcript: {err}");
        }
    }
    let deck = WANTED_BY_DECK
        .lock()
        .map(|wanted| wanted.clone())
        .unwrap_or_default();
    let mut turns = HashMap::new();
    let mut replies = HashMap::new();
    let mut launches = HashMap::new();
    for id in &deck {
        let Some(entry) = entries.get_mut(id) else {
            continue;
        };
        let offset = entry.scan.offset();
        if entry.turn.as_ref().is_none_or(|(at, _)| *at != offset) {
            entry.turn = Some((offset, entry.tail.turn()));
        }
        if let Some((_, turn)) = &entry.turn {
            turns.insert(id.clone(), turn.clone());
        }
        if entry.shown.as_ref().is_none_or(|(at, _)| *at != offset) {
            entry.shown = Some((offset, Arc::new(entry.replies.all())));
        }
        if let Some((_, shown)) = &entry.shown {
            replies.insert(id.clone(), Arc::clone(shown));
        }
        if entry.sent.as_ref().is_none_or(|(at, _)| *at != offset) {
            entry.sent = Some((offset, Arc::new(entry.launches.recent(now))));
        }
        if let Some((_, sent)) = &entry.sent {
            launches.insert(id.clone(), Arc::clone(sent));
        }
    }
    publish_open(&entries);
    if let Ok(mut read) = READ.lock() {
        *read = Some(entries);
    }
    if let Ok(mut latest) = TURNS.lock() {
        *latest = Some(turns);
    }
    if let Ok(mut latest) = REPLIES.lock() {
        *latest = Some(replies);
    }
    if let Ok(mut latest) = LAUNCHES.lock() {
        *latest = Some(launches);
    }
}

fn publish_open(entries: &HashMap<String, Entry>) {
    let open = entries
        .iter()
        .map(|(id, entry)| {
            let scan = Scan {
                open: entry.scan.open.clone(),
                ..Scan::default()
            };
            (id.clone(), scan)
        })
        .collect();
    if let Ok(mut latest) = OPEN.lock() {
        *latest = Some(open);
    }
}

fn read_new(path: &Path, entry: &mut Entry, now: u64) -> std::io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len < entry.scan.offset() {
        entry.scan.reset();
        entry.tail.reset();
        entry.replies.reset();
        entry.launches.reset();
    }
    if len == entry.scan.offset() {
        return Ok(());
    }
    let from = entry.scan.offset();
    file.seek(SeekFrom::Start(from))?;
    let mut rest = file.take(len - from);
    let mut chunk = vec![0u8; READ_CHUNK];
    let tail = &mut entry.tail;
    let replies = &mut entry.replies;
    let launches = &mut entry.launches;
    loop {
        let got = rest.read(&mut chunk)?;
        if got == 0 {
            return Ok(());
        }
        entry.scan.feed_with(&chunk[..got], now, |line| {
            tail.line(line);
            replies.line(line);
            launches.line(line, now);
        });
    }
}

/// How the last turn of ClaudeDeck's chat with this `cliSessionId` ended,
/// as read last. None before its transcript was read.
pub fn turn(cli_session_id: &str) -> Option<Turn> {
    TURNS.lock().ok()?.as_ref()?.get(cli_session_id).cloned()
}

/// The finished replies of ClaudeDeck's chat with this `cliSessionId`,
/// oldest first, as read last. None before its transcript was read.
pub fn replies(cli_session_id: &str) -> Option<Arc<Vec<Reply>>> {
    REPLIES.lock().ok()?.as_ref()?.get(cli_session_id).cloned()
}

/// The ssh calls of the last week of ClaudeDeck's chat with this
/// `cliSessionId`, oldest first, as read last.
pub fn launches(cli_session_id: &str) -> Option<Arc<Vec<Launch>>> {
    LAUNCHES.lock().ok()?.as_ref()?.get(cli_session_id).cloned()
}

/// The running jobs of the chat with this `cliSessionId`, as read last:
/// how many, and the start of the oldest.
pub fn running(cli_session_id: &str, now: u64) -> (usize, Option<u64>) {
    OPEN.lock()
        .ok()
        .and_then(|open| {
            open.as_ref()
                .and_then(|scans| scans.get(cli_session_id))
                .map(|scan| scan.running(now))
        })
        .unwrap_or((0, None))
}

#[cfg(test)]
mod tests;

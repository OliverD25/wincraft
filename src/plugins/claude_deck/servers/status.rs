//! A server chat's row, from the end of its transcript and what runs on
//! the server. The rules:
//! - its Remote Control unit failed or keeps restarting: Failed, with
//!   "Remote Control stopped";
//! - not alive (no Remote Control unit and no `claude` process in its
//!   folder): Stopped;
//! - an open question card: Asks you;
//! - an open tool call, or a write in the last 60 s: Working;
//! - else Idle, as old as the last write.
//!
//! Needs approval cannot be told: on the PC a hook says so, and nothing is
//! installed on the servers.

use serde_json::Value;

use crate::core::claude_sessions::Age;
use crate::core::claude_turns::{End, Tail, Turn};
use crate::plugins::claude_deck::model::{Chat, Mark};
use crate::plugins::claude_deck::status::{full_text, line_text};

use super::reply::RawChat;

/// The line of a chat whose Remote Control service is broken.
pub const RC_STOPPED: &str = "Remote Control stopped";

/// A write this recent means the chat works, in seconds.
const RECENT_WRITE: u64 = 60;

/// What the end of a transcript tells, kept while it does not change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub turn: Turn,
    /// The folder the chat runs in, from its records.
    pub cwd: Option<String>,
    /// The chat's title, when the transcript names one.
    pub title: Option<String>,
}

/// Reads the end of a transcript. `whole` is false when the file was
/// longer than what was sent: its first line is then cut and is skipped.
pub fn parse_tail(bytes: &[u8], whole: bool) -> Parsed {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    if !whole {
        lines.next();
    }
    let mut tail = Tail::default();
    let mut parsed = Parsed::default();
    for line in lines {
        tail.line(line);
        if let Some(cwd) = string_field(line, "cwd") {
            parsed.cwd = Some(cwd);
        }
        for key in ["customTitle", "aiTitle", "summary"] {
            if let Some(title) = title_of(line, key) {
                parsed.title = Some(title);
            }
        }
    }
    parsed.turn = tail.turn();
    parsed
}

/// A string field of a JSON line, read only when the key is in it.
fn string_field(line: &str, key: &str) -> Option<String> {
    if !line.contains(&format!("\"{key}\":\"")) {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// A title record (`custom-title`, `ai-title` or `summary`); never a
/// field of an ordinary message.
fn title_of(line: &str, key: &str) -> Option<String> {
    if line.starts_with("{\"parentUuid\"") {
        return None;
    }
    let title = string_field(line, key)?;
    Some(title.chars().take(120).collect())
}

/// Claude's folder name for a path: every character that is not a letter
/// or a digit becomes `-`.
pub fn encode(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// What runs on the server, to tell a live chat.
pub struct Running<'a> {
    pub rc: &'a [String],
    pub procs: &'a [String],
    pub rc_failed: &'a [String],
}

/// Whether a transcript folder belongs to one of these unit folders.
fn unit_of(folders: &[String], dir: &str) -> bool {
    folders
        .iter()
        .any(|folder| dir.ends_with(&format!("-projects-{}", encode(folder))))
}

impl Running<'_> {
    /// The Remote Control unit of this transcript folder, if one runs.
    pub fn rc_of(&self, dir: &str) -> bool {
        unit_of(self.rc, dir)
    }

    /// Its Remote Control unit failed or keeps restarting.
    pub fn rc_failed_of(&self, dir: &str) -> bool {
        unit_of(self.rc_failed, dir)
    }

    fn proc_in(&self, dir: &str, cwd: Option<&str>) -> bool {
        self.procs
            .iter()
            .any(|proc| encode(proc) == dir || Some(proc.as_str()) == cwd)
    }
}

/// The last part of a folder path.
fn base_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
}

/// The row of one server chat. `server_now` is the server's clock and
/// `now` the PC's, both in ms, so a clock that is off does not matter.
pub fn chat(
    server: &str,
    raw: &RawChat,
    parsed: &Parsed,
    running: &Running,
    server_now: u64,
    now: u64,
) -> Chat {
    let rc = running.rc_of(&raw.dir);
    let alive = rc || running.proc_in(&raw.dir, parsed.cwd.as_deref());
    let quiet_ms = server_now.saturating_sub(raw.mtime * 1000);
    let written = now.saturating_sub(quiet_ms);
    let turn = &parsed.turn;
    let open_tool = turn.end == End::ToolCalled;
    let rc_failed = running.rc_failed_of(&raw.dir);
    let mark = if rc_failed {
        Mark::Failed
    } else if !alive {
        Mark::Stopped
    } else if turn.question.is_some() {
        Mark::AsksYou
    } else if open_tool || quiet_ms < RECENT_WRITE * 1000 {
        Mark::Working
    } else {
        Mark::Idle(Age::of(quiet_ms))
    };
    // The whole path: the hover panel shows it and "Copy the folder path"
    // copies it.
    let folder = parsed.cwd.clone();
    let title = parsed
        .title
        .clone()
        .or_else(|| parsed.cwd.as_deref().map(|cwd| base_name(cwd).to_string()))
        .unwrap_or_else(|| raw.dir.clone());
    Chat {
        id: format!("{server}/{}", raw.dir),
        title,
        folder,
        branch: None,
        model: None,
        parent: None,
        created: raw.mtime * 1000,
        mark,
        since: Some(written),
        jobs: 0,
        window: None,
        text: if rc_failed {
            Some(RC_STOPPED.to_string())
        } else {
            line_text(mark, Some(turn))
        },
        full: full_text(mark, Some(turn)),
    }
}

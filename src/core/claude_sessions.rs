//! What each Claude Code chat in the Claude desktop app is doing, and how
//! old it is, for Custom WindowDeck's Claude cards. Read only, from the
//! app's own session files:
//! `%APPDATA%\Claude\claude-code-sessions\*\*\local_*.json`, and
//! `%APPDATA%\Claude\session-popouts.json` for the titles of popped-out
//! windows. The app may change these files at any update: a missing field
//! means "no status" for that chat, never a crash or a wrong colour.
//!
//! The files are big (some hold hundreds of KB of tool lists), so they are
//! read on a thread of their own, at most every few seconds, and only when
//! WindowDeck asks (Principle 5). A file is parsed again only when its
//! modified time changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

/// The files are read again at most this often.
pub const READ_EVERY: Duration = Duration::from_secs(3);

const MINUTE: u64 = 60_000;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

/// One chat, as far as its session file says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub previous_titles: Vec<String>,
    /// The chat's last activity, in Unix milliseconds.
    pub last_activity: Option<u64>,
    /// The last time a turn started or ended.
    pub last_turn_edge: Option<u64>,
    /// The last message the user sent.
    pub latest_user: Option<u64>,
    pub archived: bool,
}

/// The fields of a session file this module reads. Everything else is
/// skipped without being kept.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    previous_titles: Option<Vec<Option<String>>>,
    #[serde(default)]
    last_activity_at: Option<f64>,
    #[serde(default)]
    last_turn_edge_at: Option<f64>,
    #[serde(default)]
    latest_user_frame_at: Option<f64>,
    #[serde(default)]
    is_archived: Option<bool>,
}

fn millis(value: Option<f64>) -> Option<u64> {
    value
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| v as u64)
}

/// Reads one session file's text. None for text that is not a JSON object
/// of the expected shape; a field of the wrong type counts as missing.
pub fn parse(text: &str) -> Option<Session> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if !value.is_object() {
        return None;
    }
    let field = |key: &str| value.get(key).cloned().unwrap_or(serde_json::Value::Null);
    let number = |key: &str| field(key).as_f64();
    let file = SessionFile {
        session_id: field("sessionId").as_str().map(str::to_string),
        title: field("title").as_str().map(str::to_string),
        previous_titles: field("previousTitles").as_array().map(|list| {
            list.iter()
                .map(|title| title.as_str().map(str::to_string))
                .collect()
        }),
        last_activity_at: number("lastActivityAt"),
        last_turn_edge_at: number("lastTurnEdgeAt"),
        latest_user_frame_at: number("latestUserFrameAt"),
        is_archived: field("isArchived").as_bool(),
    };
    Some(from_file(file))
}

/// The same as `parse`, but faster on a big file: fields are read straight
/// into their places and the rest is skipped. Falls back to `parse` when a
/// field has an unexpected type.
fn parse_fast(text: &str) -> Option<Session> {
    match serde_json::from_str::<SessionFile>(text) {
        Ok(file) => Some(from_file(file)),
        Err(_) => parse(text),
    }
}

fn from_file(file: SessionFile) -> Session {
    Session {
        id: file.session_id.unwrap_or_default(),
        title: file.title.unwrap_or_default().trim().to_string(),
        previous_titles: file
            .previous_titles
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty())
            .collect(),
        last_activity: millis(file.last_activity_at),
        last_turn_edge: millis(file.last_turn_edge_at),
        latest_user: millis(file.latest_user_frame_at),
        archived: file.is_archived.unwrap_or(false),
    }
}

/// A popped-out chat window, as `session-popouts.json` lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Popout {
    pub session_id: String,
    /// The title the window had when it opened; may be empty.
    pub title: String,
    /// The window's saved size in pixels; 0 when not known.
    pub width: i32,
    pub height: i32,
}

/// The popped-out windows. Empty for text of another shape.
pub fn parse_popouts(text: &str) -> Vec<Popout> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    value
        .get("popouts")
        .and_then(|list| list.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|popout| {
                    let id = popout.get("sessionId")?.as_str()?.trim();
                    let title = popout.get("title").and_then(|t| t.as_str()).unwrap_or("");
                    let size = |key: &str| {
                        popout
                            .get("bounds")
                            .and_then(|b| b.get(key))
                            .and_then(|v| v.as_f64())
                            .filter(|v| v.is_finite() && *v > 0.0)
                            .map_or(0, |v| v.round() as i32)
                    };
                    (!id.is_empty()).then(|| Popout {
                        session_id: id.to_string(),
                        title: title.trim().to_string(),
                        width: size("width"),
                        height: size("height"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What a chat is doing now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The chat asked the user something (the attention hook said so).
    Waiting,
    /// Claude works on the user's last message.
    Working,
    /// The turn ended and nothing waits.
    Idle,
}

/// How much later than the user's message the edge that starts its turn
/// may be written. Seen on 2026-11-11: the start edge comes 0 to 1.1 s after
/// the message, the end edge 5 s or more after it (the shortest turn seen
/// took 5.6 s).
pub const START_EDGE_MS: u64 = 3000;

/// The status from the session's two times. `lastTurnEdgeAt` is written
/// when a turn starts and again when it ends, so the chat is working while
/// the last edge is the one that started the turn for the user's last
/// message: not more than `START_EDGE_MS` after it. None when a time is
/// missing.
pub fn status(session: &Session) -> Option<Status> {
    let user = session.latest_user?;
    let edge = session.last_turn_edge?;
    Some(if user + START_EDGE_MS > edge {
        Status::Working
    } else {
        Status::Idle
    })
}

/// What a Claude window's card shows: its status, since when (Unix ms),
/// and the chat's last activity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Card {
    pub status: Option<Status>,
    /// When the status began: the waiting mark's time, or the user's last
    /// message for a working chat. None for idle.
    pub since: Option<u64>,
    pub last_activity: Option<u64>,
}

/// The card for a window that shows `session` (None when no session
/// matched), with `waiting_since` when the attention hook marked it as
/// waiting. A waiting mark wins over the session's own status.
pub fn card(session: Option<&Session>, waiting_since: Option<u64>) -> Card {
    let last_activity = session.and_then(|s| s.last_activity);
    if let Some(since) = waiting_since {
        return Card {
            status: Some(Status::Waiting),
            since: Some(since),
            last_activity,
        };
    }
    let status = session.and_then(status);
    let since = match status {
        Some(Status::Working) => session.and_then(|s| s.latest_user),
        _ => None,
    };
    Card {
        status,
        since,
        last_activity,
    }
}

/// Whether a window titled `window_title` shows this session: the title
/// is the session's title, one it had before, or a popped-out window's
/// title for it (`extra`).
pub fn shows(session: &Session, extra: &[&str], window_title: &str) -> bool {
    let wanted = window_title.trim();
    !wanted.is_empty()
        && (session.title == wanted
            || session.previous_titles.iter().any(|title| title == wanted)
            || extra.iter().any(|title| title.trim() == wanted))
}

/// The session a window titled `window_title` shows. A session whose
/// current title matches beats one that only had that title before; among
/// equals the one with the latest activity wins. Archived sessions never
/// match.
pub fn find<'a>(
    sessions: &'a [Session],
    popouts: &[Popout],
    window_title: &str,
) -> Option<&'a Session> {
    let wanted = window_title.trim();
    sessions
        .iter()
        .filter(|session| !session.archived)
        .filter_map(|session| {
            let extra: Vec<&str> = popouts
                .iter()
                .filter(|p| !p.title.is_empty() && p.session_id == session.id)
                .map(|p| p.title.as_str())
                .collect();
            shows(session, &extra, wanted).then(|| {
                let current = session.title == wanted || extra.contains(&wanted);
                (session, current)
            })
        })
        .max_by_key(|(session, current)| (*current, session.last_activity.unwrap_or(0)))
        .map(|(session, _)| session)
}

/// How far a window's size may be from a popout's saved size and still be
/// that popout: 3 %, as the attention hook allows.
pub const SIZE_SLACK: f64 = 0.03;

/// A Claude window as `match_by_size` sees it: its handle, title, size in
/// physical pixels, and its monitor's scale (1.5 at 150 %).
#[derive(Clone, Debug, PartialEq)]
pub struct ShownWindow {
    pub hwnd: isize,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

/// How far a window is from a popout's saved size, as a share of it. The
/// saved size is in scaled pixels (2576 x 1408 for a window of 3864 x 2112
/// at 150 %).
fn size_off(window: &ShownWindow, popout: &Popout) -> f64 {
    let scale = if window.scale > 0.0 {
        window.scale
    } else {
        1.0
    };
    let want_w = f64::from(popout.width) * scale;
    let want_h = f64::from(popout.height) * scale;
    let off_w = (f64::from(window.width) - want_w).abs() / want_w;
    let off_h = (f64::from(window.height) - want_h).abs() / want_h;
    off_w.max(off_h)
}

/// Popped-out windows whose title is in no session file (seen: "Verdict
/// AI" for a session titled "VerdictAI - UA Legal AI platform"), matched
/// by size as the attention hook does it: for each popout that no window
/// shows by title, the windows a session or a popout claims by title are
/// set aside (and the main window, titled "Claude"), and of the rest the
/// ones within `SIZE_SLACK` of the popout's saved size are candidates.
///
/// Unlike the hook, which knows which session it looks for, a guess here
/// could give a card a wrong status, so a match counts only when it is
/// sure: the popout has exactly one candidate window, and that window is a
/// candidate of no other popout. Several maximized windows of the same size
/// therefore match nothing. Returns (window, the popout's session id).
pub fn match_by_size(
    windows: &[ShownWindow],
    sessions: &[Session],
    popouts: &[Popout],
) -> Vec<(isize, String)> {
    let mut claimed: Vec<&str> = vec!["Claude"];
    claimed.extend(popouts.iter().map(|p| p.title.as_str()));
    let free: Vec<&ShownWindow> = windows
        .iter()
        .filter(|w| !claimed.contains(&w.title.trim()))
        .filter(|w| find(sessions, popouts, &w.title).is_none())
        .collect();
    let candidates: Vec<(&Popout, Vec<isize>)> = popouts
        .iter()
        .filter(|p| p.width > 0 && p.height > 0)
        .filter_map(|popout| {
            let session = sessions
                .iter()
                .find(|s| !s.archived && s.id == popout.session_id)?;
            let shown = windows
                .iter()
                .any(|w| shows(session, &[popout.title.as_str()], &w.title));
            if shown {
                return None;
            }
            let near: Vec<isize> = free
                .iter()
                .filter(|w| size_off(w, popout) <= SIZE_SLACK)
                .map(|w| w.hwnd)
                .collect();
            Some((popout, near))
        })
        .collect();
    candidates
        .iter()
        .filter_map(|(popout, near)| {
            let [hwnd] = near.as_slice() else {
                return None;
            };
            let wanted_by = candidates
                .iter()
                .filter(|(_, other)| other.contains(hwnd))
                .count();
            (wanted_by == 1).then(|| (*hwnd, popout.session_id.clone()))
        })
        .collect()
}

/// How old a chat is, by the time since its last activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    /// Under 1 hour: the picture as always.
    Fresh,
    /// 1 hour to 1 day: the picture at 85 %.
    Cooling,
    /// 1 to 3 days: the picture at 60 %, a light blue tint.
    Cold,
    /// 3 days or more: the picture at 40 %, a snowflake.
    Frozen,
}

impl Age {
    pub fn of(elapsed_ms: u64) -> Age {
        match elapsed_ms {
            ms if ms < HOUR => Age::Fresh,
            ms if ms < DAY => Age::Cooling,
            ms if ms < 3 * DAY => Age::Cold,
            _ => Age::Frozen,
        }
    }

    /// The picture's opacity.
    pub fn picture_opacity(self) -> f32 {
        match self {
            Age::Fresh => 1.0,
            Age::Cooling => 0.85,
            Age::Cold => 0.6,
            Age::Frozen => 0.4,
        }
    }

    /// Idle for 1 day or more: the "Cold" counter and the Board's Cold
    /// column.
    pub fn is_cold(self) -> bool {
        matches!(self, Age::Cold | Age::Frozen)
    }
}

/// The age under a card's title: "now", "12 min", "5 h", "1 day", "2 days".
pub fn age_text(elapsed_ms: u64) -> String {
    if elapsed_ms < MINUTE {
        "now".to_string()
    } else {
        span_text(elapsed_ms)
    }
}

/// How long a status has lasted, for the chip: "40 s", "4 min", "2 h",
/// "1 day".
pub fn duration_text(elapsed_ms: u64) -> String {
    if elapsed_ms < MINUTE {
        format!("{} s", elapsed_ms / 1000)
    } else {
        span_text(elapsed_ms)
    }
}

fn span_text(elapsed_ms: u64) -> String {
    match elapsed_ms {
        ms if ms < HOUR => format!("{} min", ms / MINUTE),
        ms if ms < DAY => format!("{} h", ms / HOUR),
        ms if ms < 2 * DAY => "1 day".to_string(),
        ms => format!("{} days", ms / DAY),
    }
}

/// Now in Unix milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// An `Instant` in the past as Unix milliseconds.
pub fn instant_ms(at: Instant) -> u64 {
    now_ms().saturating_sub(at.elapsed().as_millis() as u64)
}

/// The sessions read so far, and the files they came from.
#[derive(Default)]
struct Cache {
    files: HashMap<PathBuf, (SystemTime, Option<Session>)>,
    popouts: Vec<Popout>,
    read_at: Option<Instant>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
static READING: AtomicBool = AtomicBool::new(false);

/// The Claude app's folder under `%APPDATA%`.
fn app_folder() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|folder| PathBuf::from(folder).join("Claude"))
}

/// The sessions read last, without archived ones, and the popped-out
/// windows. Empty until the first read has finished.
pub fn sessions() -> (Vec<Session>, Vec<Popout>) {
    let Ok(cache) = CACHE.lock() else {
        return (Vec::new(), Vec::new());
    };
    let Some(cache) = cache.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    let sessions = cache
        .files
        .values()
        .filter_map(|(_, session)| session.clone())
        .filter(|session| !session.archived)
        .collect();
    (sessions, cache.popouts.clone())
}

/// Starts reading the files again on a thread of its own, unless a read
/// runs now or the last one is younger than `READ_EVERY`. Never waits.
pub fn read_soon() {
    let recent = CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().and_then(|c| c.read_at))
        .is_some_and(|at| at.elapsed() < READ_EVERY);
    if recent || READING.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(folder) = app_folder() else {
        READING.store(false, Ordering::Release);
        return;
    };
    let started = std::thread::Builder::new()
        .name("claude-sessions".into())
        .spawn(move || {
            read_all(&folder);
            READING.store(false, Ordering::Release);
        });
    if started.is_err() {
        READING.store(false, Ordering::Release);
    }
}

/// Reads every session file whose modified time changed, and the popouts.
fn read_all(folder: &Path) {
    let started = Instant::now();
    let known: HashMap<PathBuf, SystemTime> = CACHE
        .lock()
        .ok()
        .and_then(|cache| {
            cache.as_ref().map(|c| {
                c.files
                    .iter()
                    .map(|(path, (modified, _))| (path.clone(), *modified))
                    .collect()
            })
        })
        .unwrap_or_default();
    let mut files: HashMap<PathBuf, (SystemTime, Option<Option<Session>>)> = HashMap::new();
    let mut parsed = 0usize;
    for path in session_files(&folder.join("claude-code-sessions")) {
        let Some(modified) = std::fs::metadata(&path).and_then(|m| m.modified()).ok() else {
            continue;
        };
        if known.get(&path) == Some(&modified) {
            files.insert(path, (modified, None));
            continue;
        }
        let session = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| parse_fast(&text));
        parsed += 1;
        files.insert(path, (modified, Some(session)));
    }
    let popouts = std::fs::read_to_string(folder.join("session-popouts.json"))
        .map(|text| parse_popouts(&text))
        .unwrap_or_default();
    let Ok(mut cache) = CACHE.lock() else {
        return;
    };
    let cache = cache.get_or_insert_with(Cache::default);
    let mut old = std::mem::take(&mut cache.files);
    for (path, (modified, fresh)) in files {
        let session = match fresh {
            Some(session) => session,
            None => old.remove(&path).and_then(|(_, session)| session),
        };
        cache.files.insert(path, (modified, session));
    }
    cache.popouts = popouts;
    cache.read_at = Some(Instant::now());
    if parsed > 0 {
        log::info!(
            "Custom WindowDeck: read {parsed} changed Claude session files of {} in {} ms",
            cache.files.len(),
            started.elapsed().as_millis()
        );
    }
}

/// `<root>\*\*\local_*.json`.
fn session_files(root: &Path) -> Vec<PathBuf> {
    let folders = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|entry| entry.path())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut found = Vec::new();
    for account in folders(root) {
        for space in folders(&account) {
            let Ok(entries) = std::fs::read_dir(&space) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("local_") && name.ends_with(".json") {
                    found.push(entry.path());
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests;

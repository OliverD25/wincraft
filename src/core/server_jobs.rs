//! The jobs PC chats started on the homelab servers, as ClaudeDeck linked
//! them last, for other parts of WinCraft. ClaudeDeck reads the servers
//! and publishes a small snapshot here; WindowDeck's Claude board reads it
//! to give an idle chat with a job on a server the status "Job". A
//! snapshot older than `FRESH_MS` counts as nothing, so a closed ClaudeDeck
//! or a plugin switched off leaves no old jobs behind.

use std::sync::Mutex;

use crate::core::claude_sessions::{self, Card, Status};

/// How long a snapshot counts: two minutes.
pub const FRESH_MS: u64 = 2 * 60 * 1000;

/// One PC chat's jobs on the servers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// The chat's `sessionId`, as `claude_sessions::Session::id`.
    pub chat_id: String,
    /// The servers' short names ("home2"), each once.
    pub servers: Vec<String>,
    pub jobs: usize,
    /// When the oldest job started, in Unix ms.
    pub oldest: Option<u64>,
    /// One of the jobs uses the CPU now.
    pub busy: bool,
}

/// Where a chat's job runs, as its card shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnServer {
    /// "home2", or "2 servers".
    pub place: String,
    pub busy: bool,
}

struct Snapshot {
    read_at: u64,
    entries: Vec<Entry>,
}

static LATEST: Mutex<Option<Snapshot>> = Mutex::new(None);

/// ClaudeDeck linked the jobs again at `read_at` (Unix ms).
pub fn publish(entries: Vec<Entry>, read_at: u64) {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = Some(Snapshot { read_at, entries });
    }
}

/// Nothing is linked any more: no server is listed.
pub fn clear() {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = None;
    }
}

/// Whether a snapshot made at `read_at` still counts at `now`.
pub fn fresh(read_at: u64, now: u64) -> bool {
    now.saturating_sub(read_at) <= FRESH_MS
}

/// The chat's jobs, when the snapshot is fresh.
pub fn of(chat_id: &str, now: u64) -> Option<Entry> {
    let latest = LATEST.lock().ok()?;
    let snapshot = latest.as_ref()?;
    pick(&snapshot.entries, snapshot.read_at, chat_id, now).cloned()
}

/// The chat's entry in a snapshot made at `read_at`, when it still counts.
pub fn pick<'a>(entries: &'a [Entry], read_at: u64, chat_id: &str, now: u64) -> Option<&'a Entry> {
    if !fresh(read_at, now) {
        return None;
    }
    entries.iter().find(|entry| entry.chat_id == chat_id)
}

/// "45 s", "12 min", "1 h 48 m", "2 d 3 h".
pub fn run_time(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => match (s / 3600, s % 3600 / 60) {
            (h, 0) => format!("{h} h"),
            (h, m) => format!("{h} h {m} m"),
        },
        s => match (s / 86_400, s % 86_400 / 3600) {
            (d, 0) => format!("{d} d"),
            (d, h) => format!("{d} d {h} h"),
        },
    }
}

/// "home2" for one server, "2 servers" for more.
pub fn place_text(servers: &[String]) -> String {
    match servers {
        [one] => one.clone(),
        many => format!("{} servers", many.len()),
    }
}

/// The card with its server job: an Idle chat with a job of its own on a
/// server becomes "Job" since the oldest job started. Waiting, Working
/// and Background win over it, as in ClaudeDeck.
pub fn apply(mut card: Card, entry: Option<&Entry>) -> Card {
    let Some(entry) = entry.filter(|entry| entry.jobs > 0) else {
        return card;
    };
    if card.status != Some(Status::Idle) {
        return card;
    }
    card.status = Some(Status::ServerJob);
    card.since = entry.oldest;
    card.jobs = entry.jobs;
    card.server_job = Some(OnServer {
        place: place_text(&entry.servers),
        busy: entry.busy,
    });
    card
}

/// The card of a chat with this id, with its server job when the
/// snapshot is fresh.
pub fn with_job(card: Card, chat_id: Option<&str>) -> Card {
    let entry = chat_id.and_then(|id| of(id, claude_sessions::now_ms()));
    apply(card, entry.as_ref())
}

#[cfg(test)]
mod tests;

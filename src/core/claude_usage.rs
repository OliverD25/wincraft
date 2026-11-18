//! How many tokens each Claude Code chat used, from its transcripts on
//! this PC: `%USERPROFILE%\.claude\projects\<folder>\<session>.jsonl` and
//! its subagents' `<session>\subagents\*.jsonl`. Each assistant line holds
//! its `message.usage` and `message.model`.
//!
//! A `Reader` reads them on a thread of its own at low priority, never on
//! the host or UI thread. Each pass reads only the bytes added since the
//! last one, and only files changed in the last 8 days. What was read is
//! saved to `claude_deck.usage.json`, so a restart does not read the
//! transcripts again. A pass runs every 60 s, or at once when asked, and
//! only while someone asked for the numbers in the last 90 s (Principle 5).
//! The UI reads the newest summary with `try_lock`, so it never waits.

pub mod files;
pub mod parse;
pub mod prices;
pub mod tally;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::core::{clock, config, write_later};

use files::{Book, FileState, Slot, BOOK_VERSION};
pub use parse::Tokens;
pub use tally::{ChatUse, Span};

pub const FILE: &str = "claude_deck.usage.json";
/// Files and buckets older than this many days are dropped.
pub const KEEP_DAYS: i32 = 8;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
pub const EVERY: Duration = Duration::from_secs(60);
/// A pass runs only while the numbers were asked for this recently.
pub const WANTED_FOR: Duration = Duration::from_secs(90);
const STEP: Duration = Duration::from_millis(200);

/// How far the first pass has got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

/// What the reader knows, for the UI.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Today, as a local day number.
    pub today: i32,
    pub today_span: Span,
    pub week: Span,
    /// While the first pass reads: how far it is.
    pub counting: Option<Progress>,
}

/// The local day number of a local date: days since 1970-01-01.
pub fn day_number(year: i32, month: u32, day: u32) -> i32 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i32;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The weekday of a day number, 0 for Monday.
pub fn weekday(day: i32) -> u32 {
    // 1970-01-01 was a Thursday.
    (day + 3).rem_euclid(7) as u32
}

/// The local day and hour of a moment in Unix ms.
pub fn local_slot(at_ms: u64) -> Slot {
    match clock::local((at_ms / 1000) as i64) {
        Some(t) => (
            day_number(i32::from(t.year), u32::from(t.month), u32::from(t.day)),
            t.hour.min(23) as u8,
        ),
        None => ((at_ms / DAY_MS) as i32, ((at_ms / 3_600_000) % 24) as u8),
    }
}

/// The summary of a book for today and the 7 days up to today.
pub fn summary(book: &Book, today: i32) -> Snapshot {
    Snapshot {
        today,
        today_span: tally::span(book.files.values(), today, today),
        week: tally::span(book.files.values(), today - 6, today),
        counting: None,
    }
}

/// One pass over the transcripts: reads what changed, drops what is too
/// old. `progress` hears how far it got. True when anything changed.
pub fn pass(
    book: &mut Book,
    projects: &Path,
    now_ms: u64,
    slot: &dyn Fn(u64) -> Slot,
    progress: &mut dyn FnMut(Progress),
    stop: &AtomicBool,
) -> bool {
    let since = now_ms.saturating_sub(KEEP_DAYS as u64 * DAY_MS);
    let found = files::list(projects, since);
    let mut changed = false;
    let before = book.files.len();
    let listed: std::collections::HashSet<String> = found
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    book.files.retain(|path, _| listed.contains(path));
    changed |= book.files.len() != before;
    let todo: Vec<_> = found
        .iter()
        .filter(|f| files::changed(book.files.get(&*f.path.to_string_lossy()), f))
        .collect();
    let total = todo.len();
    for (at, file) in todo.into_iter().enumerate() {
        if stop.load(Ordering::SeqCst) {
            return changed;
        }
        progress(Progress { done: at, total });
        let state = book
            .files
            .entry(file.path.to_string_lossy().into_owned())
            .or_insert_with(|| FileState {
                session: file.session.clone(),
                folder: file.folder.clone(),
                subagent: file.subagent,
                ..FileState::default()
            });
        if let Err(err) = files::read_new(&file.path, state, slot) {
            log::debug!(
                "ClaudeDeck stats: could not read {}: {err}",
                file.path.display()
            );
        }
        changed = true;
    }
    progress(Progress { done: total, total });
    let first_day = slot(now_ms).0 - KEEP_DAYS;
    for state in book.files.values_mut() {
        state.drop_before(first_day);
    }
    changed
}

fn projects_folder() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude").join("projects"))
}

pub fn book_path() -> PathBuf {
    config::plugins_dir().join(FILE)
}

/// The saved book; an empty one when the file is missing, broken or from
/// another version (then everything is read again).
pub fn load(path: &Path) -> Book {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Book>(&text).ok())
        .filter(|book| book.version == BOOK_VERSION)
        .unwrap_or_default()
}

/// Lowers this thread's CPU and disk priority, so the first pass does not
/// slow the PC.
fn background() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
    };
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

struct Inner {
    stop: AtomicBool,
    /// Ms since the reader started when the numbers were last asked for.
    wanted_at: AtomicU64,
    asked_now: AtomicBool,
    latest: Mutex<Option<Arc<Snapshot>>>,
    started: Instant,
}

impl Inner {
    fn since_start(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn publish(&self, snapshot: Snapshot) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(Arc::new(snapshot));
        }
    }
}

/// Reads the transcripts on its own thread until dropped.
pub struct Reader {
    inner: Arc<Inner>,
}

impl Reader {
    /// Starts reading the real transcripts into the real book file.
    pub fn start() -> Option<Reader> {
        let projects = projects_folder()?;
        Some(Self::start_with(projects, book_path()))
    }

    pub fn start_with(projects: PathBuf, book_file: PathBuf) -> Reader {
        let inner = Arc::new(Inner {
            stop: AtomicBool::new(false),
            wanted_at: AtomicU64::new(0),
            asked_now: AtomicBool::new(true),
            latest: Mutex::new(None),
            started: Instant::now(),
        });
        let worker = Arc::clone(&inner);
        let spawned = std::thread::Builder::new()
            .name("deck-usage".into())
            .spawn(move || run(&worker, &projects, &book_file));
        if let Err(err) = spawned {
            log::warn!("ClaudeDeck: could not start counting tokens: {err}");
        }
        Reader { inner }
    }

    /// Someone looks at the numbers: passes go on for `WANTED_FOR`.
    pub fn want(&self) {
        self.inner
            .wanted_at
            .store(self.inner.since_start().max(1), Ordering::SeqCst);
    }

    /// Asks for a pass now, not at the next minute.
    pub fn now_please(&self) {
        self.want();
        self.inner.asked_now.store(true, Ordering::SeqCst);
    }

    /// The newest summary, without waiting: None while the worker holds
    /// it or before the first one.
    pub fn latest(&self) -> Option<Arc<Snapshot>> {
        self.inner.latest.try_lock().ok().and_then(|l| l.clone())
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::SeqCst);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn run(inner: &Inner, projects: &Path, book_file: &Path) {
    background();
    let mut book = load(book_file);
    let mut first = true;
    let mut last_pass: Option<Instant> = None;
    while !inner.stop.load(Ordering::SeqCst) {
        let wanted = inner
            .since_start()
            .saturating_sub(inner.wanted_at.load(Ordering::SeqCst))
            < WANTED_FOR.as_millis() as u64
            && inner.wanted_at.load(Ordering::SeqCst) > 0;
        let due = last_pass.is_none_or(|at| at.elapsed() >= EVERY)
            || inner.asked_now.swap(false, Ordering::SeqCst);
        if !(wanted && due) {
            std::thread::sleep(STEP);
            continue;
        }
        let started = Instant::now();
        let now = now_ms();
        let today = local_slot(now).0;
        let mut shown = summary(&book, today);
        let mut last_shown = Instant::now();
        let changed = pass(
            &mut book,
            projects,
            now,
            &local_slot,
            &mut |progress| {
                if first && last_shown.elapsed() >= Duration::from_millis(500) {
                    shown.counting = Some(progress);
                    inner.publish(shown.clone());
                    last_shown = Instant::now();
                }
            },
            &inner.stop,
        );
        if inner.stop.load(Ordering::SeqCst) {
            return;
        }
        inner.publish(summary(&book, today));
        log::info!(
            "ClaudeDeck stats: {} pass over {} files took {} ms",
            if first { "first" } else { "a" },
            book.files.len(),
            started.elapsed().as_millis()
        );
        if changed {
            book.version = BOOK_VERSION;
            match serde_json::to_string(&book) {
                Ok(text) => {
                    write_later::write_later(book_file.to_path_buf(), text, "ClaudeDeck stats")
                }
                Err(err) => log::warn!("ClaudeDeck stats: could not save: {err}"),
            }
        }
        first = false;
        last_pass = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests;

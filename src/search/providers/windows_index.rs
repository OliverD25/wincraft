//! File search in the Windows Search index, for when Everything is not
//! running. The question is plain SQL for the index's OLE DB provider (see
//! `oledb`), asked on a thread of its own because the index is slower than
//! Everything: a search that matches little can scan the whole index.
//!
//! This file holds the parts that need no index: the SQL and its escaping,
//! the turning of a row into a hit, and the one-slot queue that waits for
//! typing to pause and drops a search a newer one replaced.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, Once, PoisonError};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use super::everything_ipc::Hit;
use super::oledb::{Connection, Failure, RawRow, COLUMNS};

/// Rows asked for. They are ranked by the caller, as Everything's are.
pub const CANDIDATES: usize = 300;
/// A search starts this long after the last keystroke.
pub const DEBOUNCE: Duration = Duration::from_millis(120);
/// How long the index may take. A search that matches little scans all of
/// it, which takes about 0.6 s on a large index.
pub const TIMEOUT: Duration = Duration::from_millis(800);
/// More words than this are ignored, to keep the SQL short.
const MOST_TERMS: usize = 8;

/// The text of a `LIKE` pattern for `term`. In this SQL `%` and `_` are
/// wildcards, which `[ ]` turns into plain characters; `[` is plain only
/// inside its own brackets; and `'` is doubled.
pub fn like_text(term: &str) -> String {
    let mut text = String::with_capacity(term.len() + 8);
    for letter in term.chars() {
        match letter {
            '\'' => text.push_str("''"),
            '%' => text.push_str("[%]"),
            '_' => text.push_str("[_]"),
            '[' => text.push_str("[[]"),
            other => text.push(other),
        }
    }
    text
}

/// The words of a search. Control characters, which a pasted text can carry
/// and the SQL parser would choke on, are dropped.
fn terms(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|letter| !letter.is_control())
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .take(MOST_TERMS)
        .collect()
}

/// The index's SQL for a typed search, or None when it holds no word. Every
/// word must be somewhere in the file name, the way Everything reads a
/// search; a word with a slash in it is looked for in the whole path
/// instead, which the index keeps as `file:C:/folder/name`. Recently
/// changed items come first among the rows the limit cuts off.
pub fn sql(text: &str, top: usize) -> Option<String> {
    let words = terms(text);
    if words.is_empty() {
        return None;
    }
    let mut query = format!("SELECT TOP {top} {COLUMNS} FROM SystemIndex WHERE SCOPE='file:'");
    for word in &words {
        let (column, word) = if word.contains(['\\', '/']) {
            ("System.ItemUrl", word.replace('\\', "/"))
        } else {
            ("System.FileName", word.clone())
        };
        query.push_str(&format!(" AND {column} LIKE '%{}%'", like_text(&word)));
    }
    query.push_str(" ORDER BY System.DateModified DESC");
    Some(query)
}

/// The path an item's `file:` address stands for: the index writes
/// `file:C:/Users/me/a b.txt` with the characters as they are, and
/// `file://server/share/x` for a network share.
fn path_of(url: &str) -> Option<String> {
    if !url.get(..5)?.eq_ignore_ascii_case("file:") {
        return None;
    }
    let rest = &url[5..];
    let rest = rest.strip_prefix("///").unwrap_or(rest);
    let path = rest.replace('/', "\\");
    (!path.is_empty()).then_some(path)
}

/// A hit from a row of the index; None for an item that is not a file or
/// folder with a name, such as a drive's root.
pub fn hit_of(row: &RawRow) -> Option<Hit> {
    let path = path_of(row.url.as_deref()?)?;
    let trimmed = path.trim_end_matches('\\');
    let (folder_path, name) = trimmed.rsplit_once('\\')?;
    if name.is_empty() {
        return None;
    }
    Some(Hit {
        name: name.to_string(),
        path: folder_path.to_string(),
        folder: row
            .kind
            .as_deref()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("Directory")),
        drive: false,
        size: None,
        modified: row.modified,
    })
}

/// What a worker run found.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Hits(Vec<Hit>),
    /// The index service is off, or the provider is missing.
    Unavailable,
    /// The index could not answer this search.
    Failed,
    TimedOut,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    pub id: u32,
    pub outcome: Outcome,
}

/// What the worker does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    Run(u32, String),
    /// A search is waiting for typing to pause, for this much longer.
    Wait(Duration),
    Idle,
}

struct Posted {
    id: u32,
    text: String,
    at: Instant,
}

/// The newest search wanted. A newer one replaces an older one that has not
/// started, and starts the wait for a pause in typing again.
pub struct Pending {
    slot: Mutex<Option<Posted>>,
    wake: Condvar,
}

impl Pending {
    pub const fn new() -> Self {
        Self {
            slot: Mutex::new(None),
            wake: Condvar::new(),
        }
    }

    pub fn post(&self, id: u32, text: &str, now: Instant) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(Posted {
                id,
                text: text.to_string(),
                at: now,
            });
        }
        self.wake.notify_one();
    }

    /// The search to run now, if typing has paused; takes it.
    #[cfg(test)]
    fn poll(&self, now: Instant) -> Poll {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        due(&mut slot, now)
    }

    /// Blocks until a search is due.
    fn next(&self) -> (u32, String) {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match due(&mut slot, Instant::now()) {
                Poll::Run(id, text) => return (id, text),
                Poll::Wait(time) => {
                    slot = self
                        .wake
                        .wait_timeout(slot, time)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                Poll::Idle => {
                    slot = self.wake.wait(slot).unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
    }
}

fn due(slot: &mut Option<Posted>, now: Instant) -> Poll {
    let Some(posted) = slot.as_ref() else {
        return Poll::Idle;
    };
    let waited = now.saturating_duration_since(posted.at);
    if waited < DEBOUNCE {
        return Poll::Wait(DEBOUNCE - waited);
    }
    slot.take()
        .map_or(Poll::Idle, |posted| Poll::Run(posted.id, posted.text))
}

static PENDING: Pending = Pending::new();
static DONE: Mutex<Vec<Done>> = Mutex::new(Vec::new());
static START: Once = Once::new();
/// Set while the last try at the index found it unreachable.
static DOWN: AtomicBool = AtomicBool::new(false);

/// Whether the last try found the index unreachable.
pub fn is_down() -> bool {
    DOWN.load(Ordering::SeqCst)
}

/// Asks for a search; it runs once typing has paused for [`DEBOUNCE`].
pub fn submit(id: u32, text: &str) {
    START.call_once(|| {
        if let Err(error) = std::thread::Builder::new()
            .name("windows-index".to_string())
            .spawn(worker)
        {
            log::warn!("could not start the Windows index thread: {error}");
        }
    });
    PENDING.post(id, text, Instant::now());
}

/// The searches that finished since the last call.
pub fn take_done() -> Vec<Done> {
    DONE.lock()
        .map(|mut done| std::mem::take(&mut *done))
        .unwrap_or_default()
}

fn worker() {
    unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    let mut connection: Option<Connection> = None;
    loop {
        let (id, text) = PENDING.next();
        let outcome = run(&mut connection, &text);
        if let Ok(mut done) = DONE.lock() {
            // Nobody read the old ones; only the newest search matters.
            if done.len() > 8 {
                done.remove(0);
            }
            done.push(Done { id, outcome });
        }
    }
}

/// Asks the index, opening the connection first if there is none. A
/// connection that failed is thrown away so the next search opens a new one.
fn run(connection: &mut Option<Connection>, text: &str) -> Outcome {
    let Some(sql) = sql(text, CANDIDATES) else {
        return Outcome::Hits(Vec::new());
    };
    let deadline = Instant::now() + TIMEOUT;
    if connection.is_none() {
        match Connection::open() {
            Ok(opened) => *connection = Some(opened),
            Err(failure) => return note_unreachable(failure),
        }
    }
    let Some(open) = connection.as_ref() else {
        return Outcome::Failed;
    };
    match open.query(&sql, CANDIDATES, deadline) {
        Ok(rows) => {
            DOWN.store(false, Ordering::SeqCst);
            Outcome::Hits(rows.iter().filter_map(hit_of).collect())
        }
        Err(Failure::TimedOut) => Outcome::TimedOut,
        Err(failure) => {
            *connection = None;
            log::debug!("the Windows index failed a search: {failure}");
            match failure {
                Failure::Unavailable(_) => note_unreachable(failure),
                _ => Outcome::Failed,
            }
        }
    }
}

/// Logs the first time in a row that the index cannot be reached.
fn note_unreachable(failure: Failure) -> Outcome {
    if !DOWN.swap(true, Ordering::SeqCst) {
        log::warn!("file search: the Windows index cannot be used, {failure}");
    }
    Outcome::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIKE_START: &str = "SELECT TOP 300 System.ItemUrl, System.ItemType, System.DateModified FROM SystemIndex WHERE SCOPE='file:'";
    const ORDER: &str = " ORDER BY System.DateModified DESC";

    #[test]
    fn a_word_becomes_a_like_on_the_file_name() {
        assert_eq!(
            sql("pswds", 300).unwrap(),
            format!("{LIKE_START} AND System.FileName LIKE '%pswds%'{ORDER}")
        );
    }

    #[test]
    fn every_word_must_match_and_extra_space_is_ignored() {
        assert_eq!(
            sql("  annual   report ", 50).unwrap(),
            "SELECT TOP 50 System.ItemUrl, System.ItemType, System.DateModified FROM SystemIndex WHERE SCOPE='file:' AND System.FileName LIKE '%annual%' AND System.FileName LIKE '%report%' ORDER BY System.DateModified DESC"
        );
    }

    #[test]
    fn an_empty_search_asks_nothing() {
        assert_eq!(sql("", 300), None);
        assert_eq!(sql("   \t ", 300), None);
        assert_eq!(sql("\u{1}\u{7}", 300), None, "only control characters");
    }

    #[test]
    fn quotes_are_doubled_so_a_word_cannot_end_the_string() {
        assert_eq!(like_text("it's"), "it''s");
        let query = sql("x' OR 1=1 --", 300).unwrap();
        assert!(query.contains("LIKE '%x''%'"), "{query}");
    }

    #[test]
    fn like_wildcards_are_made_plain_characters() {
        assert_eq!(like_text("100%"), "100[%]");
        assert_eq!(like_text("my_file"), "my[_]file");
        assert_eq!(like_text("[draft]"), "[[]draft]");
        assert_eq!(like_text("a%_[b"), "a[%][_][[]b");
        assert_eq!(
            sql("my_file", 300).unwrap(),
            format!("{LIKE_START} AND System.FileName LIKE '%my[_]file%'{ORDER}")
        );
    }

    #[test]
    fn cyrillic_letters_and_other_scripts_pass_through_whole() {
        assert_eq!(like_text("звіт_2024"), "звіт[_]2024");
        assert!(sql("Фінансова звітність", 300)
            .unwrap()
            .contains("LIKE '%Фінансова%' AND System.FileName LIKE '%звітність%'"));
    }

    #[test]
    fn control_characters_are_dropped_from_a_word() {
        assert_eq!(
            sql("re\u{0}port\n", 300).unwrap(),
            format!("{LIKE_START} AND System.FileName LIKE '%report%'{ORDER}")
        );
    }

    #[test]
    fn a_word_with_a_slash_is_looked_for_in_the_whole_address() {
        assert_eq!(
            sql(r"Users\Admin\my_docs", 300).unwrap(),
            format!("{LIKE_START} AND System.ItemUrl LIKE '%Users/Admin/my[_]docs%'{ORDER}")
        );
        assert!(sql("a/b c", 300)
            .unwrap()
            .contains("System.ItemUrl LIKE '%a/b%' AND System.FileName LIKE '%c%'"));
    }

    #[test]
    fn only_the_first_words_are_used() {
        let many = (0..20)
            .map(|n| format!("w{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let query = sql(&many, 300).unwrap();
        assert_eq!(query.matches(" LIKE ").count(), MOST_TERMS);
    }

    fn row(url: &str, kind: &str, modified: Option<u64>) -> RawRow {
        RawRow {
            url: Some(url.to_string()),
            kind: Some(kind.to_string()),
            modified,
        }
    }

    #[test]
    fn a_folder_row_becomes_a_folder_hit_with_a_real_path() {
        let hit = hit_of(&row(
            "file:D:/Windows_OneDrive/OneDrive/pswds_OneDrive",
            "Directory",
            Some(133_000_000_000_000_000),
        ))
        .unwrap();
        assert!(hit.folder && !hit.drive);
        assert_eq!(hit.name, "pswds_OneDrive");
        assert_eq!(hit.path, r"D:\Windows_OneDrive\OneDrive");
        assert_eq!(
            hit.full_path(),
            r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive"
        );
        assert_eq!(hit.modified, Some(133_000_000_000_000_000));
        assert_eq!(hit.size, None);
    }

    #[test]
    fn a_file_row_keeps_spaces_percent_signs_and_cyrillic_as_they_are() {
        let hit = hit_of(&row(
            "file:C:/Users/Admin/Downloads/Telegram Desktop/Матвеев 30% оплата #2.pdf",
            ".pdf",
            None,
        ))
        .unwrap();
        assert!(!hit.folder);
        assert_eq!(hit.name, "Матвеев 30% оплата #2.pdf");
        assert_eq!(hit.path, r"C:\Users\Admin\Downloads\Telegram Desktop");
        assert_eq!(hit.modified, None);
    }

    #[test]
    fn a_network_share_keeps_its_double_backslash() {
        let hit = hit_of(&row("file://nas/media/Films/a.mkv", ".mkv", None)).unwrap();
        assert_eq!(hit.full_path(), r"\\nas\media\Films\a.mkv");
        let hit = hit_of(&row("file:///C:/x/y.txt", ".txt", None)).unwrap();
        assert_eq!(hit.full_path(), r"C:\x\y.txt");
    }

    #[test]
    fn rows_that_are_not_named_files_are_left_out() {
        assert_eq!(hit_of(&RawRow::default()), None, "no address");
        assert_eq!(hit_of(&row("winrt://{S-1}/LS/mail/3", "", None)), None);
        assert_eq!(hit_of(&row("file:", "", None)), None);
        assert_eq!(hit_of(&row("file:C:/", "Directory", None)), None);
        assert_eq!(hit_of(&row("mapi15://x", ".msg", None)), None);
        assert!(hit_of(&row("FILE:C:/a.txt", ".txt", None)).is_some());
        assert!(
            hit_of(&row("file:C:/a.txt", "DIRECTORY", None))
                .unwrap()
                .folder
        );
    }

    #[test]
    fn a_search_waits_for_a_pause_in_typing() {
        let pending = Pending::new();
        let t0 = Instant::now();
        assert_eq!(pending.poll(t0), Poll::Idle);
        pending.post(1, "pswds", t0);
        assert_eq!(
            pending.poll(t0 + Duration::from_millis(50)),
            Poll::Wait(Duration::from_millis(70))
        );
        assert_eq!(
            pending.poll(t0 + DEBOUNCE),
            Poll::Run(1, "pswds".to_string())
        );
        assert_eq!(pending.poll(t0 + DEBOUNCE), Poll::Idle, "it runs once");
    }

    #[test]
    fn a_newer_search_replaces_one_that_has_not_started_and_restarts_the_wait() {
        let pending = Pending::new();
        let t0 = Instant::now();
        pending.post(1, "psw", t0);
        pending.post(2, "pswd", t0 + Duration::from_millis(100));
        // The first would be due now, but it was replaced.
        assert_eq!(
            pending.poll(t0 + Duration::from_millis(130)),
            Poll::Wait(Duration::from_millis(90))
        );
        assert_eq!(
            pending.poll(t0 + Duration::from_millis(220)),
            Poll::Run(2, "pswd".to_string())
        );
        assert_eq!(pending.poll(t0 + Duration::from_secs(5)), Poll::Idle);
    }

    #[test]
    fn a_clock_that_went_back_only_waits_longer() {
        let pending = Pending::new();
        let t0 = Instant::now() + Duration::from_secs(1);
        pending.post(1, "ab", t0);
        assert_eq!(
            pending.poll(t0 - Duration::from_millis(1)),
            Poll::Wait(DEBOUNCE)
        );
    }

    #[test]
    fn the_worker_waits_for_a_search_and_runs_it_once_typing_pauses() {
        let pending = std::sync::Arc::new(Pending::new());
        let waiting = std::sync::Arc::clone(&pending);
        let thread = std::thread::spawn(move || waiting.next());
        std::thread::sleep(Duration::from_millis(30));
        let posted = Instant::now();
        pending.post(7, "ab", posted);
        pending.post(8, "abc", Instant::now());
        let (id, text) = thread.join().unwrap();
        assert_eq!((id, text.as_str()), (8, "abc"));
        assert!(posted.elapsed() >= DEBOUNCE);
    }

    #[test]
    fn the_timeout_is_longer_than_the_debounce_and_shorter_than_a_second() {
        assert!(TIMEOUT > DEBOUNCE);
        assert!(TIMEOUT < Duration::from_secs(1));
    }
}

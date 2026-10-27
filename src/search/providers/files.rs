//! Files and folders from every disk, through Everything (voidtools), or
//! through the Windows Search index when Everything is not running.
//!
//! Everything keeps an index of all file names and answers in milliseconds.
//! WinCraft asks it over its window messages (see `everything_ipc`), on a
//! thread of its own, so a slow or stuck Everything never freezes the
//! palette. The Windows index is asked the same way, by `windows_index` on
//! its own thread, and has a client of its own so that the two sources never
//! share an answer. The palette sees the usual async provider: `query`
//! answers from the last reply, `waiting` and `has_news` say when a new one
//! arrives.

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW,
    KillTimer, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageTimeoutW, SetTimer,
    TranslateMessage, HWND_MESSAGE, MSG, SMTO_ABORTIFHUNG, WM_APP, WM_CLOSE, WM_COPYDATA,
    WM_DESTROY, WM_TIMER, WM_USER, WNDCLASSW,
};

use super::everything_ipc::{
    self as ipc, Hit, ParseError, Query2Header, Reply as Answer, REQUEST_FIELDS,
};
use super::windows_index::{self, Outcome};
use crate::core::config::SearchConfig;
use crate::core::wide;
use crate::search::{
    usage, Action, Choice, Completion, Context, Glyph, IconRef, Query, Reply, ResultItem,
    SearchProvider, Tone,
};

/// Shorter searches would list most of the disk.
const MIN_CHARS: usize = 2;
/// Matches asked for. They are ranked here, so the best few are in the set
/// even when the name order puts them late.
const CANDIDATES: u32 = 300;
/// Rows in the blended list, folders first.
const BLENDED_ROWS: usize = 6;
/// Rows after the `.` prefix, where the list scrolls.
const PREFIXED_ROWS: usize = 50;
/// Of those, kept for files when there are any, so folders cannot fill all.
const BLENDED_FILE_ROWS: usize = 2;
/// A search starts this long after the last keystroke.
const DEBOUNCE_MS: u32 = 120;
/// How long Everything may take to take a search in.
const SEND_TIMEOUT_MS: u32 = 500;
/// The palette stops waiting for a reply after this long.
const GIVE_UP: Duration = Duration::from_millis((DEBOUNCE_MS + SEND_TIMEOUT_MS + 100) as u64);
/// The biggest reply read; a bigger one is a mistake.
const MOST_REPLY_BYTES: usize = 8 * 1024 * 1024;

const WM_JOB: u32 = WM_APP + 1;
const DEBOUNCE_TIMER: usize = 1;
const CLASS_NAME: &str = "WinCraftEverythingReply";

/// Why there are no rows, in words for the person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotRunning,
    /// Everything older than 1.4 has no `QUERY2`.
    TooOld,
    /// The 1.5 alpha has an IPC of its own, which this does not speak.
    Alpha15,
    /// Running, but still reading the disks for its index.
    Loading,
    NotAnswering,
    Refused,
    /// File search is off in the settings.
    TurnedOff,
    /// Not an error: Everything is not there, so the Windows index answers.
    UsingIndex,
    /// Neither Everything nor the Windows index can be asked.
    IndexUnavailable,
    IndexTimedOut,
    IndexFailed,
}

impl Status {
    fn title(self) -> &'static str {
        match self {
            Status::NotRunning => "File search needs Everything (voidtools). It is not running.",
            Status::TooOld => "File search needs Everything 1.4 or newer.",
            Status::Alpha15 => "Everything 1.5 alpha is running. WinCraft cannot talk to it yet.",
            Status::Loading => "Everything is still reading its index.",
            Status::NotAnswering => "Everything did not answer in time.",
            Status::Refused => "Everything refused the search.",
            Status::TurnedOff => "File search is turned off.",
            Status::UsingIndex => {
                "Searching the Windows index (Everything is not running; it is faster)"
            }
            Status::IndexUnavailable => "File search needs Everything or Windows Search",
            Status::IndexTimedOut => "The Windows index did not answer in time.",
            Status::IndexFailed => "The Windows index refused the search.",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Status::NotRunning => "Start Everything, then type again",
            Status::TooOld => "Update Everything from voidtools.net",
            Status::Alpha15 => "Everything 1.4 works",
            Status::Loading => "Type again in a moment",
            Status::NotAnswering | Status::Refused => "Type again, or restart Everything",
            Status::TurnedOff => "Turn on File search in Settings, General",
            Status::UsingIndex => "Start Everything for instant results",
            Status::IndexUnavailable => "Start Everything, or start the Windows Search service",
            Status::IndexTimedOut | Status::IndexFailed => {
                "Type again, or start Everything for faster results"
            }
        }
    }

    fn tone(self) -> Tone {
        match self {
            Status::UsingIndex => Tone::Normal,
            _ => Tone::Warning,
        }
    }
}

/// Where a search goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Everything,
    /// The Windows index; `down` when the last try found it unreachable.
    Index {
        down: bool,
    },
    /// Nowhere, and why.
    Unavailable(Status),
}

/// Everything when it is running, else the Windows index, unless the
/// settings say otherwise. `presence` is why Everything cannot be asked, if
/// it cannot; its 1.5 alpha counts as not running, because its messages are
/// not the ones this code speaks.
fn choose(search: &SearchConfig, presence: Option<Status>, index_down: bool) -> Source {
    if !search.everything {
        return Source::Unavailable(Status::TurnedOff);
    }
    match presence {
        None => Source::Everything,
        Some(_) if search.file_search_fallback => Source::Index { down: index_down },
        Some(status) => Source::Unavailable(status),
    }
}

/// The second line of a row from the Windows index starts with this.
const VIA_INDEX: &str = "Windows index";

/// What the worker thread sends back to the palette side.
enum Incoming {
    Reply(u32, Vec<u8>),
    /// Rows the Windows index found.
    Hits(u32, Vec<Hit>),
    Failed(u32, Status),
}

/// What the Windows index's thread found, as the client takes it.
fn incoming_of(done: windows_index::Done) -> Incoming {
    let id = done.id;
    match done.outcome {
        Outcome::Hits(hits) => Incoming::Hits(id, hits),
        Outcome::Unavailable => Incoming::Failed(id, Status::IndexUnavailable),
        Outcome::Failed => Incoming::Failed(id, Status::IndexFailed),
        Outcome::TimedOut => Incoming::Failed(id, Status::IndexTimedOut),
    }
}

struct Job {
    id: u32,
    text: String,
}

/// The newest search wanted. A newer one replaces an older one that has not
/// been sent yet.
static JOB: Mutex<Option<Job>> = Mutex::new(None);
static INCOMING: Mutex<Vec<Incoming>> = Mutex::new(Vec::new());
/// The worker's window; 0 until the thread has made it.
static WORKER_WINDOW: AtomicIsize = AtomicIsize::new(0);
static START: Once = Once::new();

/// Makes the thread once, and tells it about the job in `JOB`.
fn wake_worker() {
    START.call_once(|| {
        if let Err(error) = std::thread::Builder::new()
            .name("everything".to_string())
            .spawn(worker)
        {
            log::warn!("could not start the Everything thread: {error}");
        }
    });
    let window = WORKER_WINDOW.load(Ordering::SeqCst);
    if window != 0 {
        unsafe { PostMessageW(window as HWND, WM_JOB, 0, 0) };
    }
}

fn worker() {
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let name = wide(CLASS_NAME);
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(worker_proc);
    class.hInstance = hinstance;
    class.lpszClassName = name.as_ptr();
    unsafe { RegisterClassW(&class) };
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            name.as_ptr(),
            wide("").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        log::warn!("could not make the Everything reply window");
        return;
    }
    WORKER_WINDOW.store(hwnd as isize, Ordering::SeqCst);
    // A search asked for while the window was being made.
    unsafe { PostMessageW(hwnd, WM_JOB, 0, 0) };
    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    WORKER_WINDOW.store(0, Ordering::SeqCst);
}

unsafe extern "system" fn worker_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // Setting the timer again restarts it: the search goes out once the
        // typing has paused.
        WM_JOB => {
            unsafe { SetTimer(hwnd, DEBOUNCE_TIMER, DEBOUNCE_MS, None) };
            0
        }
        WM_TIMER if wparam == DEBOUNCE_TIMER => {
            unsafe { KillTimer(hwnd, DEBOUNCE_TIMER) };
            let job = JOB.lock().ok().and_then(|mut job| job.take());
            if let Some(job) = job {
                send(hwnd, job);
            }
            0
        }
        WM_COPYDATA => {
            let data = lparam as *const COPYDATASTRUCT;
            if let Some(data) = unsafe { data.as_ref() } {
                let size = data.cbData as usize;
                if size > 0 && size <= MOST_REPLY_BYTES && !data.lpData.is_null() {
                    let bytes =
                        unsafe { std::slice::from_raw_parts(data.lpData as *const u8, size) };
                    push(Incoming::Reply(data.dwData as u32, bytes.to_vec()));
                }
            }
            // Everything wants TRUE back.
            1
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn push(incoming: Incoming) {
    if let Ok(mut queue) = INCOMING.lock() {
        // Nobody read the old ones; only the newest search matters.
        if queue.len() > 8 {
            queue.remove(0);
        }
        queue.push(incoming);
    }
}

fn find(class: &str) -> Option<HWND> {
    let window = unsafe { FindWindowW(wide(class).as_ptr(), std::ptr::null()) };
    (!window.is_null()).then_some(window)
}

/// Everything's answer to a plain `SendMessage(WM_USER, what)`.
fn ask_number(everything: HWND, what: usize) -> Option<usize> {
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            everything,
            WM_USER,
            what,
            0,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            &mut result,
        )
    };
    (sent != 0).then_some(result)
}

/// Sends one search to Everything. Runs on the worker thread.
fn send(reply_window: HWND, job: Job) {
    let fail = |status| push(Incoming::Failed(job.id, status));
    let Some(everything) = find(ipc::WINDOW_CLASS) else {
        fail(if find(ipc::WINDOW_CLASS_15A).is_some() {
            Status::Alpha15
        } else {
            Status::NotRunning
        });
        return;
    };
    let (Some(major), Some(minor)) = (
        ask_number(everything, ipc::GET_MAJOR_VERSION),
        ask_number(everything, ipc::GET_MINOR_VERSION),
    ) else {
        fail(Status::NotAnswering);
        return;
    };
    if (major, minor) < (1, 4) {
        fail(Status::TooOld);
        return;
    }
    match ask_number(everything, ipc::IS_DB_LOADED) {
        Some(0) => {
            fail(Status::Loading);
            return;
        }
        None => {
            fail(Status::NotAnswering);
            return;
        }
        Some(_) => {}
    }
    let header = Query2Header {
        reply_hwnd: reply_window as usize as u32,
        reply_copydata_message: job.id,
        search_flags: 0,
        offset: 0,
        max_results: CANDIDATES,
        request_flags: REQUEST_FIELDS,
        sort_type: ipc::SORT_NAME_ASCENDING,
    };
    let bytes = ipc::build_query(&header, &job.text);
    let data = COPYDATASTRUCT {
        dwData: ipc::COPYDATA_QUERY2W,
        cbData: bytes.len() as u32,
        lpData: bytes.as_ptr() as *mut std::ffi::c_void,
    };
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            everything,
            WM_COPYDATA,
            reply_window as WPARAM,
            &data as *const COPYDATASTRUCT as LPARAM,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            &mut result,
        )
    };
    if sent == 0 {
        fail(Status::NotAnswering);
    } else if result == 0 {
        fail(Status::Refused);
    }
}

/// Whether there is an Everything to ask, from one cheap look for its window.
fn presence() -> Option<Status> {
    if find(ipc::WINDOW_CLASS).is_some() {
        None
    } else if find(ipc::WINDOW_CLASS_15A).is_some() {
        Some(Status::Alpha15)
    } else {
        Some(Status::NotRunning)
    }
}

/// What the palette side does next about a typed search.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// The answer for this text is in hand, or it failed.
    Show,
    /// Already asked; the reply has not come.
    Wait,
    Send(u32),
}

struct Wanted {
    id: u32,
    text: String,
    since: Instant,
}

/// The palette side of the conversation: which search is wanted, and the last
/// answer. Holds no window and no thread, so it is tested on its own.
struct Client {
    last_id: u32,
    wanted: Option<Wanted>,
    shown: Option<(String, Answer)>,
    failed: Option<(String, Status)>,
    news: bool,
    /// How long a search may stay unanswered, and what is said when it does.
    give_up: Duration,
    expired: Status,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(GIVE_UP, Status::NotAnswering)
    }
}

impl Client {
    fn new(give_up: Duration, expired: Status) -> Self {
        Self {
            last_id: 0,
            wanted: None,
            shown: None,
            failed: None,
            news: false,
            give_up,
            expired,
        }
    }

    fn ask(&mut self, text: &str, now: Instant) -> Next {
        if self.shown.as_ref().is_some_and(|(shown, _)| shown == text)
            || self
                .failed
                .as_ref()
                .is_some_and(|(failed, _)| failed == text)
        {
            return Next::Show;
        }
        if self
            .wanted
            .as_ref()
            .is_some_and(|wanted| wanted.text == text)
        {
            return Next::Wait;
        }
        // Id 0 is never used, so a zeroed message cannot match a search.
        self.last_id = self.last_id.wrapping_add(1).max(1);
        self.wanted = Some(Wanted {
            id: self.last_id,
            text: text.to_string(),
            since: now,
        });
        Next::Send(self.last_id)
    }

    /// A reply for an older search than the wanted one is dropped.
    fn receive(&mut self, incoming: Incoming) {
        let (id, outcome) = match incoming {
            Incoming::Reply(id, bytes) => (id, ipc::parse_list2(&bytes).map_err(refusal)),
            Incoming::Hits(id, hits) => (
                id,
                Ok(Answer {
                    total: hits.len() as u32,
                    hits,
                }),
            ),
            Incoming::Failed(id, status) => (id, Err(status)),
        };
        let Some(wanted) = self.wanted.take_if(|wanted| wanted.id == id) else {
            return;
        };
        match outcome {
            Ok(answer) => {
                self.failed = None;
                self.shown = Some((wanted.text, answer));
            }
            Err(status) => {
                self.shown = None;
                self.failed = Some((wanted.text, status));
            }
        }
        self.news = true;
    }

    fn expire(&mut self, now: Instant) {
        if let Some(wanted) = self
            .wanted
            .take_if(|wanted| now.duration_since(wanted.since) >= self.give_up)
        {
            log::debug!("{:?} was not answered in time", wanted.text);
            self.shown = None;
            self.failed = Some((wanted.text, self.expired));
            self.news = true;
        }
    }

    fn waiting(&self) -> bool {
        self.wanted.is_some()
    }

    fn take_news(&mut self) -> bool {
        std::mem::take(&mut self.news)
    }

    fn answer(&self, text: &str) -> Option<&Answer> {
        self.shown
            .as_ref()
            .filter(|(shown, _)| shown == text)
            .map(|(_, answer)| answer)
    }

    fn failure(&self, text: &str) -> Option<Status> {
        self.failed
            .as_ref()
            .filter(|(failed, _)| failed == text)
            .map(|(_, status)| *status)
    }
}

fn refusal(error: ParseError) -> Status {
    log::warn!("Everything's reply could not be read: {error}");
    Status::Refused
}

/// The name without its last extension.
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(at) if at > 0 => &name[..at],
        _ => name,
    }
}

/// How well a name answers the typed text. Files stay below the apps and
/// commands that match as well: the best a file gets is 50, and an exact app
/// name scores 10000.
fn match_score(text: &str, hit: &Hit) -> i32 {
    let query = text.trim().to_lowercase();
    let name = hit.name.to_lowercase();
    let at_word_start = || {
        name.match_indices(&query).any(|(at, _)| {
            name[..at]
                .chars()
                .next_back()
                .is_none_or(|before| !before.is_alphanumeric())
        })
    };
    let tier = if name == query || stem(&name) == query {
        45
    } else if name.starts_with(&query) {
        35
    } else if at_word_start() {
        28
    } else if name.contains(&query) {
        20
    } else if query.split_whitespace().all(|term| name.contains(term)) {
        15
    } else {
        // Matched on the folder, or by Everything's own syntax.
        8
    };
    // A shorter name is nearer to what was typed.
    tier + 5 - (name.chars().count() / 8).min(5) as i32
}

struct Ranked<'a> {
    score: i32,
    hit: &'a Hit,
}

/// Places that only hold copies or shortcuts of what lives elsewhere, which
/// would crowd out the real thing: the recycle bin, and the Recent list whose
/// shortcuts carry the name of every folder opened lately.
fn is_clutter(hit: &Hit) -> bool {
    let path = hit.path.to_lowercase();
    path.contains("$recycle.bin") || path.ends_with(r"\microsoft\windows\recent")
}

/// Best first; equal scores go to the most recently changed.
fn ranked<'a>(text: &str, hits: impl Iterator<Item = &'a Hit>) -> Vec<Ranked<'a>> {
    let mut list: Vec<Ranked> = hits
        .filter(|hit| !is_clutter(hit))
        .map(|hit| Ranked {
            score: match_score(text, hit),
            hit,
        })
        .collect();
    list.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.hit.folder.cmp(&a.hit.folder))
            .then_with(|| b.hit.modified.cmp(&a.hit.modified))
            .then_with(|| a.hit.name.cmp(&b.hit.name))
    });
    list
}

/// Folders first, then files, `rows` in all, as the blended list shows them.
/// Files always score below the best folder, so the group order follows.
fn blended<'a>(text: &str, answer: &'a Answer, rows: usize) -> (Vec<Ranked<'a>>, Vec<Ranked<'a>>) {
    let mut folders = ranked(text, answer.hits.iter().filter(|hit| hit.folder));
    let mut files = ranked(text, answer.hits.iter().filter(|hit| !hit.folder));
    let file_rows = files.len().min(BLENDED_FILE_ROWS);
    folders.truncate(rows - file_rows);
    files.truncate(rows - folders.len());
    if let (Some(folder), Some(file)) = (folders.first(), files.first()) {
        let shift = file.score - folder.score + 1;
        if shift > 0 {
            for file in &mut files {
                file.score -= shift;
            }
        }
    }
    (folders, files)
}

/// The `.` list: everything in one group, best match first, folders ahead of
/// files that match as well.
fn prefixed_list<'a>(text: &str, answer: &'a Answer, rows: usize) -> Vec<Ranked<'a>> {
    let mut list = ranked(text, answer.hits.iter());
    list.truncate(rows);
    list
}

/// The one row that says why there is nothing to list.
fn hint(status: Status) -> ResultItem {
    ResultItem {
        group: "Files".to_string(),
        title: status.title().to_string(),
        subtitle: status.subtitle().to_string(),
        icon: IconRef::Glyph(Glyph::Search),
        tone: status.tone(),
        ..Default::default()
    }
}

/// A row for a hit. `via` names the source when it is not Everything.
fn row(ranked: Ranked, group: &str, via: Option<&str>) -> ResultItem {
    let hit = ranked.hit;
    let path = hit.full_path();
    let place = if hit.drive {
        "Drive".to_string()
    } else {
        hit.path.clone()
    };
    ResultItem {
        group: group.to_string(),
        title: hit.name.clone(),
        subtitle: match via {
            Some(via) => format!("{via} \u{b7} {place}"),
            None => place,
        },
        icon: IconRef::Path(PathBuf::from(&path)),
        score: ranked.score,
        usage_key: Some(usage::path_key(&path)),
        enter: Some(Choice {
            label: "Open".to_string(),
            action: Action::Open(PathBuf::from(&path)),
        }),
        shift_enter: Some(Choice {
            label: "Show in folder".to_string(),
            action: Action::Reveal(PathBuf::from(&path)),
        }),
        ctrl_enter: Some(Choice {
            label: "Copy path".to_string(),
            action: Action::Copy(path.clone()),
        }),
        tab_action: hit.folder.then(|| Choice {
            label: "Browse".to_string(),
            action: Action::Provider {
                provider: "files".to_string(),
                command: format!("{BROWSE} {path}"),
            },
        }),
        tab: hit.folder.then(|| Completion::quiet(&hit.name)),
        bookmark: super::bookmarks::choice(&path, &hit.name),
        ..Default::default()
    }
}

/// The command on a folder row's Tab action, followed by the folder.
const BROWSE: &str = "browse";

pub struct Files {
    client: Client,
    index: Client,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            client: Client::default(),
            index: Client::new(
                windows_index::DEBOUNCE + windows_index::TIMEOUT + Duration::from_millis(100),
                Status::IndexTimedOut,
            ),
        }
    }
}

impl Files {
    pub fn new() -> Self {
        Self::default()
    }

    fn drain(&mut self) {
        let incoming = INCOMING
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default();
        for incoming in incoming {
            self.client.receive(incoming);
        }
        for done in windows_index::take_done() {
            self.index.receive(incoming_of(done));
        }
    }
}

impl SearchProvider for Files {
    fn id(&self) -> &'static str {
        "files"
    }

    fn name(&self) -> &'static str {
        "Files"
    }

    fn description(&self) -> &'static str {
        "Find files and folders on every disk with Everything (voidtools), or the Windows index"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some(".")
    }

    fn blended(&self) -> bool {
        true
    }

    fn placeholder(&self) -> &'static str {
        "Type part of a file or folder name\u{2026}"
    }

    fn waiting(&self) -> bool {
        self.client.waiting() || self.index.waiting()
    }

    fn has_news(&mut self) -> bool {
        self.drain();
        let now = Instant::now();
        self.client.expire(now);
        self.index.expire(now);
        let everything = self.client.take_news();
        let index = self.index.take_news();
        everything || index
    }

    fn act(&mut self, command: &str, _context: &Context) -> Reply {
        match command.strip_prefix(BROWSE).map(str::trim) {
            Some(folder) if !folder.is_empty() => Reply::Switch {
                provider: "paths",
                text: if folder.ends_with('\\') {
                    folder.to_string()
                } else {
                    format!("{folder}\\")
                },
            },
            _ => Reply::Stay,
        }
    }

    fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem> {
        let presence = if context.search.everything {
            presence()
        } else {
            None
        };
        let source = choose(context.search, presence, windows_index::is_down());
        let (rows, job) = self.rows(query, source, Instant::now());
        if let Some(job) = job {
            if matches!(source, Source::Index { .. }) {
                windows_index::submit(job.id, &job.text);
            } else {
                if let Ok(mut slot) = JOB.lock() {
                    *slot = Some(job);
                }
                wake_worker();
            }
        }
        rows
    }
}

impl Files {
    /// The rows for a query, and the search to send when it is a new one.
    /// After the `.` prefix, why there is nothing to list is said in a row;
    /// in the blended list it is silent. With the Windows index as the
    /// source, the row that says so also stands in for an empty list.
    fn rows(
        &mut self,
        query: &Query,
        source: Source,
        now: Instant,
    ) -> (Vec<ResultItem>, Option<Job>) {
        let prefixed = query.prefix.is_some();
        let hint_for = |status| {
            if prefixed {
                vec![hint(status)]
            } else {
                Vec::new()
            }
        };
        let (client, via, notice) = match source {
            Source::Unavailable(status) => return (hint_for(status), None),
            Source::Everything => (&mut self.client, None, None),
            Source::Index { down } => {
                let notice = if down {
                    Status::IndexUnavailable
                } else {
                    Status::UsingIndex
                };
                (&mut self.index, Some(VIA_INDEX), Some(notice))
            }
        };
        let text = query.text.trim();
        if text.chars().count() < MIN_CHARS {
            return (notice.map_or_else(Vec::new, hint_for), None);
        }
        let job = match client.ask(text, now) {
            Next::Send(id) => Some(Job {
                id,
                text: text.to_string(),
            }),
            Next::Show | Next::Wait => None,
        };
        let Some(answer) = client.answer(text) else {
            let rows = client
                .failure(text)
                .or(notice)
                .map_or_else(Vec::new, hint_for);
            return (rows, job);
        };
        let rows: Vec<ResultItem> = if prefixed {
            let list = prefixed_list(text, answer, PREFIXED_ROWS.min(query.limit));
            list.into_iter()
                .map(|r| row(r, "Files and folders", via))
                .collect()
        } else {
            let (folders, files) = blended(text, answer, BLENDED_ROWS.min(query.limit));
            let mut rows: Vec<ResultItem> = folders
                .into_iter()
                .map(|r| row(r, "Folders", via))
                .collect();
            rows.extend(files.into_iter().map(|r| row(r, "Files", via)));
            rows
        };
        if rows.is_empty() {
            return (notice.map_or_else(Vec::new, hint_for), job);
        }
        (rows, job)
    }
}

#[cfg(test)]
mod tests;

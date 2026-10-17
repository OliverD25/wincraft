//! Files and folders from every disk, through Everything (voidtools).
//!
//! Everything keeps an index of all file names and answers in milliseconds.
//! WinCraft asks it over its window messages (see `everything_ipc`), on a
//! thread of its own, so a slow or stuck Everything never freezes the
//! palette. The palette sees the usual async provider: `query` answers from
//! the last reply, `waiting` and `has_news` say when a new one arrives.

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
use crate::core::wide;
use crate::search::{
    usage, Action, Choice, Completion, Context, IconRef, Query, Reply, ResultItem, SearchProvider,
};

/// Shorter searches would list most of the disk.
const MIN_CHARS: usize = 2;
/// Matches asked for. They are ranked here, so the best few are in the set
/// even when the name order puts them late.
const CANDIDATES: u32 = 300;
/// Rows in the blended list, folders first.
const BLENDED_ROWS: usize = 6;
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
}

/// What the worker thread sends back to the palette side.
enum Incoming {
    Reply(u32, Vec<u8>),
    Failed(u32, Status),
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
#[derive(Default)]
struct Client {
    last_id: u32,
    wanted: Option<Wanted>,
    shown: Option<(String, Answer)>,
    failed: Option<(String, Status)>,
    news: bool,
}

impl Client {
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
            .take_if(|wanted| now.duration_since(wanted.since) >= GIVE_UP)
        {
            log::debug!("Everything did not answer {:?} in time", wanted.text);
            self.shown = None;
            self.failed = Some((wanted.text, Status::NotAnswering));
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

fn row(ranked: Ranked, group: &str) -> ResultItem {
    let hit = ranked.hit;
    let path = hit.full_path();
    ResultItem {
        group: group.to_string(),
        title: hit.name.clone(),
        subtitle: if hit.drive {
            "Drive".to_string()
        } else {
            hit.path.clone()
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
        ..Default::default()
    }
}

/// The command on a folder row's Tab action, followed by the folder.
const BROWSE: &str = "browse";

#[derive(Default)]
pub struct Files {
    client: Client,
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
        "Find files and folders on every disk with Everything (voidtools)"
    }

    fn blended(&self) -> bool {
        true
    }

    fn placeholder(&self) -> &'static str {
        "Type part of a file or folder name\u{2026}"
    }

    fn waiting(&self) -> bool {
        self.client.waiting()
    }

    fn has_news(&mut self) -> bool {
        self.drain();
        self.client.expire(Instant::now());
        self.client.take_news()
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

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        let text = query.text.trim();
        if text.chars().count() < MIN_CHARS || presence().is_some() {
            return Vec::new();
        }
        if let Next::Send(id) = self.client.ask(text, Instant::now()) {
            if let Ok(mut job) = JOB.lock() {
                *job = Some(Job {
                    id,
                    text: text.to_string(),
                });
            }
            wake_worker();
        }
        let Some(answer) = self.client.answer(text) else {
            return Vec::new();
        };
        let (folders, files) = blended(text, answer, BLENDED_ROWS.min(query.limit));
        let mut rows: Vec<ResultItem> = folders.into_iter().map(|r| row(r, "Folders")).collect();
        rows.extend(files.into_iter().map(|r| row(r, "Files")));
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::test_context;

    fn hit(name: &str, path: &str, folder: bool, modified: u64) -> Hit {
        Hit {
            name: name.to_string(),
            path: path.to_string(),
            folder,
            drive: false,
            size: None,
            modified: Some(modified),
        }
    }

    fn answer(hits: Vec<Hit>) -> Answer {
        Answer {
            total: hits.len() as u32,
            hits,
        }
    }

    fn names<'a>(list: &[Ranked<'a>]) -> Vec<&'a str> {
        list.iter().map(|ranked| ranked.hit.name.as_str()).collect()
    }

    #[test]
    fn an_exact_name_beats_a_prefix_beats_a_word_beats_a_fragment() {
        let exact = hit("pswds", r"D:\a", true, 0);
        let exact_file = hit("pswds.txt", r"D:\a", false, 0);
        let prefix = hit("pswds_OneDrive", r"D:\a", true, 0);
        let word = hit("old pswds", r"D:\a", true, 0);
        let fragment = hit("mypswdsold", r"D:\a", true, 0);
        let path_only = hit("misc", r"D:\pswds\misc", true, 0);
        let all = [&exact, &exact_file, &prefix, &word, &fragment, &path_only];
        let scores: Vec<i32> = all.iter().map(|h| match_score("pswds", h)).collect();
        assert!(scores[0] > scores[2], "{scores:?}");
        assert!(
            scores[1] > scores[2],
            "an extension does not spoil an exact name"
        );
        assert!(scores[2] > scores[3], "{scores:?}");
        assert!(scores[3] > scores[4], "{scores:?}");
        assert!(scores[4] > scores[5], "{scores:?}");
    }

    #[test]
    fn a_shorter_name_beats_a_longer_one_of_the_same_kind() {
        let short = hit("pswds_a", r"D:\a", true, 0);
        let long = hit("pswds_a_very_long_folder_name_indeed", r"D:\a", true, 0);
        assert!(match_score("pswds", &short) > match_score("pswds", &long));
    }

    #[test]
    fn a_file_never_scores_like_an_exact_app_name() {
        let exact = hit("chrome.exe", r"C:\x", false, 0);
        assert!(match_score("chrome", &exact) < 60);
    }

    #[test]
    fn folders_come_first_and_files_score_below_them() {
        // The file is the better match, but the folder still leads.
        let reply = answer(vec![
            hit("report.docx", r"D:\a", false, 5),
            hit("old report archive", r"D:\a", true, 5),
        ]);
        let (folders, files) = blended("report", &reply, 6);
        assert_eq!(names(&folders), ["old report archive"]);
        assert_eq!(names(&files), ["report.docx"]);
        assert!(files[0].score < folders[0].score);
    }

    #[test]
    fn folders_cannot_take_the_rows_files_are_kept() {
        let mut hits: Vec<Hit> = (0..9)
            .map(|n| hit(&format!("proj{n}"), r"D:\a", true, n))
            .collect();
        hits.push(hit("proj.txt", r"D:\a", false, 1));
        hits.push(hit("proj2.txt", r"D:\a", false, 2));
        hits.push(hit("proj3.txt", r"D:\a", false, 3));
        let reply = answer(hits);
        let (folders, files) = blended("proj", &reply, 6);
        assert_eq!(folders.len(), 4);
        assert_eq!(files.len(), 2);
        // With no files all six rows are folders.
        let only: Vec<Hit> = (0..9)
            .map(|n| hit(&format!("proj{n}"), r"D:\a", true, n))
            .collect();
        let reply = answer(only);
        let (folders, files) = blended("proj", &reply, 6);
        assert_eq!((folders.len(), files.len()), (6, 0));
    }

    #[test]
    fn equal_matches_put_the_newest_first() {
        let reply = answer(vec![
            hit("proj1", r"D:\a", true, 10),
            hit("proj2", r"D:\a", true, 30),
            hit("proj3", r"D:\a", true, 20),
        ]);
        let (folders, _) = blended("proj", &reply, 6);
        assert_eq!(names(&folders), ["proj2", "proj3", "proj1"]);
    }

    #[test]
    fn the_recycle_bin_and_recent_shortcuts_are_left_out() {
        let reply = answer(vec![
            hit("pswds", r"D:\$RECYCLE.BIN\S-1-5-21", true, 1),
            hit(
                "pswds.lnk",
                r"C:\Users\Admin\AppData\Roaming\Microsoft\Windows\Recent",
                false,
                1,
            ),
            hit("pswds", r"D:\Real", true, 1),
        ]);
        let (folders, files) = blended("pswds", &reply, 6);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].hit.path, r"D:\Real");
        assert!(files.is_empty());
    }

    #[test]
    fn a_folder_row_opens_shows_copies_and_browses() {
        let folder = hit("pswds_OneDrive", r"D:\Windows_OneDrive\OneDrive", true, 1);
        let item = row(
            Ranked {
                score: 40,
                hit: &folder,
            },
            "Folders",
        );
        let path = r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive";
        assert_eq!(item.title, "pswds_OneDrive");
        assert_eq!(item.subtitle, r"D:\Windows_OneDrive\OneDrive");
        assert_eq!(item.usage_key.as_deref(), Some(&*path.to_lowercase()));
        assert_eq!(item.icon, IconRef::Path(PathBuf::from(path)));
        assert_eq!(
            item.enter.as_ref().map(|c| &c.action),
            Some(&Action::Open(PathBuf::from(path)))
        );
        assert_eq!(
            item.shift_enter.as_ref().map(|c| &c.action),
            Some(&Action::Reveal(PathBuf::from(path)))
        );
        assert_eq!(
            item.ctrl_enter.as_ref().map(|c| &c.action),
            Some(&Action::Copy(path.to_string()))
        );
        let tab = item.tab_action.expect("a folder can be browsed");
        assert_eq!(
            tab.action,
            Action::Provider {
                provider: "files".to_string(),
                command: format!("browse {path}"),
            }
        );
        let mut files = Files::new();
        let Action::Provider { command, .. } = tab.action else {
            unreachable!()
        };
        assert_eq!(
            files.act(&command, &test_context()),
            Reply::Switch {
                provider: "paths",
                text: format!("{path}\\"),
            }
        );
    }

    #[test]
    fn a_file_row_has_no_browse() {
        let file = hit("notes.txt", r"D:\a", false, 1);
        let item = row(
            Ranked {
                score: 1,
                hit: &file,
            },
            "Files",
        );
        assert!(item.tab_action.is_none());
        assert!(item.tab.is_none());
    }

    #[test]
    fn a_drive_row_says_so_and_ends_with_a_backslash() {
        let mut drive = hit("D:", "", true, 0);
        drive.drive = true;
        let item = row(
            Ranked {
                score: 1,
                hit: &drive,
            },
            "Folders",
        );
        assert_eq!(item.subtitle, "Drive");
        assert_eq!(
            item.enter.map(|c| c.action),
            Some(Action::Open(PathBuf::from(r"D:\")))
        );
    }

    fn failure_of(client: &Client, text: &str) -> Option<Status> {
        client
            .failed
            .as_ref()
            .filter(|(failed, _)| failed == text)
            .map(|(_, status)| *status)
    }

    fn reply_for(id: u32, name: &str) -> Incoming {
        let reply = answer(vec![hit(name, r"D:\a", true, 1)]);
        // Written back out in Everything's layout, and read by the real parser.
        Incoming::Reply(id, encode(&reply))
    }

    /// An answer in the layout of `EVERYTHING_IPC_LIST2`, for the client.
    fn encode(reply: &Answer) -> Vec<u8> {
        let field = |text: &str| {
            let units: Vec<u16> = text.encode_utf16().collect();
            let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
            units
                .iter()
                .for_each(|unit| bytes.extend_from_slice(&unit.to_le_bytes()));
            bytes.extend_from_slice(&[0, 0]);
            bytes
        };
        let mut bytes = Vec::new();
        for value in [reply.total, reply.hits.len() as u32, 0, REQUEST_FIELDS, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let table = bytes.len();
        bytes.resize(table + reply.hits.len() * ipc::ITEM2_SIZE, 0);
        for (index, hit) in reply.hits.iter().enumerate() {
            let data_at = bytes.len() as u32;
            let at = table + index * ipc::ITEM2_SIZE;
            let flags = if hit.folder { ipc::ITEM_FOLDER } else { 0 };
            bytes[at..at + 4].copy_from_slice(&flags.to_le_bytes());
            bytes[at + 4..at + 8].copy_from_slice(&data_at.to_le_bytes());
            bytes.extend(field(&hit.name));
            bytes.extend(field(&hit.path));
            bytes.extend_from_slice(&u64::MAX.to_le_bytes());
            bytes.extend_from_slice(&hit.modified.unwrap_or(0).to_le_bytes());
        }
        bytes
    }

    #[test]
    fn a_search_is_sent_once_and_shown_when_its_reply_comes() {
        let mut client = Client::default();
        let now = Instant::now();
        let Next::Send(id) = client.ask("pswds", now) else {
            panic!("a new search is sent");
        };
        assert!(client.waiting());
        assert_eq!(client.ask("pswds", now), Next::Wait, "not sent twice");
        assert!(client.answer("pswds").is_none());
        client.receive(reply_for(id, "pswds_OneDrive"));
        assert!(!client.waiting());
        assert!(client.take_news());
        assert!(!client.take_news(), "news is told once");
        assert_eq!(client.ask("pswds", now), Next::Show);
        assert_eq!(
            client.answer("pswds").unwrap().hits[0].name,
            "pswds_OneDrive"
        );
    }

    #[test]
    fn a_newer_search_supersedes_an_older_reply() {
        let mut client = Client::default();
        let now = Instant::now();
        let Next::Send(first) = client.ask("psw", now) else {
            panic!()
        };
        let Next::Send(second) = client.ask("pswd", now) else {
            panic!()
        };
        assert_ne!(first, second);
        // The old reply arrives late: it must not show under the new text.
        client.receive(reply_for(first, "psw_old"));
        assert!(!client.take_news());
        assert!(client.answer("psw").is_none());
        assert!(client.answer("pswd").is_none());
        assert!(client.waiting());
        client.receive(reply_for(second, "pswd_new"));
        assert!(client.take_news());
        assert_eq!(client.answer("pswd").unwrap().hits[0].name, "pswd_new");
    }

    #[test]
    fn a_reply_that_does_not_come_is_given_up() {
        let mut client = Client::default();
        let now = Instant::now();
        assert!(matches!(client.ask("pswds", now), Next::Send(_)));
        client.expire(now + GIVE_UP - Duration::from_millis(1));
        assert!(client.waiting());
        client.expire(now + GIVE_UP);
        assert!(!client.waiting());
        assert!(client.take_news());
        assert_eq!(failure_of(&client, "pswds"), Some(Status::NotAnswering));
        // The same text is not sent again in a loop; other text is.
        assert_eq!(client.ask("pswds", now + GIVE_UP), Next::Show);
        assert!(matches!(client.ask("pswd", now + GIVE_UP), Next::Send(_)));
    }

    #[test]
    fn a_refusal_from_the_worker_becomes_a_status() {
        let mut client = Client::default();
        let now = Instant::now();
        let Next::Send(id) = client.ask("pswds", now) else {
            panic!()
        };
        client.receive(Incoming::Failed(id, Status::Loading));
        assert!(client.take_news());
        assert_eq!(failure_of(&client, "pswds"), Some(Status::Loading));
        assert!(client.answer("pswds").is_none());
        // A later good answer for other text clears it.
        let Next::Send(id) = client.ask("pswd", now) else {
            panic!()
        };
        client.receive(reply_for(id, "pswd"));
        assert_eq!(failure_of(&client, "pswd"), None);
    }

    #[test]
    fn an_unreadable_reply_is_a_refusal_not_a_crash() {
        let mut client = Client::default();
        let Next::Send(id) = client.ask("pswds", Instant::now()) else {
            panic!()
        };
        client.receive(Incoming::Reply(id, vec![1, 2, 3]));
        assert_eq!(failure_of(&client, "pswds"), Some(Status::Refused));
    }

    #[test]
    fn ids_are_never_zero_even_after_wrapping() {
        let mut client = Client {
            last_id: u32::MAX,
            ..Client::default()
        };
        assert_eq!(client.ask("ab", Instant::now()), Next::Send(1));
    }

    #[test]
    fn a_short_search_asks_nobody() {
        let mut files = Files::new();
        let query = Query {
            text: "p".to_string(),
            prefix: None,
            limit: 8,
        };
        assert!(files.query(&query, &test_context()).is_empty());
        assert!(!files.waiting());
    }

    /// Asks the Everything that runs on this PC. Read-only: it searches and
    /// opens nothing. Run it with `cargo test -- --ignored live`.
    #[test]
    #[ignore = "needs Everything running with the pswds folder on D:"]
    fn live_pswds_is_found() {
        let mut files = Files::new();
        let query = Query {
            text: "pswds".to_string(),
            prefix: None,
            limit: 8,
        };
        let context = test_context();
        let started = Instant::now();
        let mut rows = files.query(&query, &context);
        while rows.is_empty() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
            files.has_news();
            rows = files.query(&query, &context);
        }
        println!("answered in {} ms", started.elapsed().as_millis());
        for item in &rows {
            println!(
                "{} | {} | {} | {}",
                item.group, item.title, item.subtitle, item.score
            );
        }
        assert!(
            rows.iter().any(|item| {
                matches!(
                    &item.enter,
                    Some(Choice { action: Action::Open(path), .. })
                        if path.as_os_str().eq_ignore_ascii_case(r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive")
                )
            }),
            "the pswds folder is in the rows"
        );
    }
}

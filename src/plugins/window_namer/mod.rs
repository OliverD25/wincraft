mod guard;
mod rules;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_ACCESS_DENIED, ERROR_TIMEOUT, HWND, LPARAM, RECT, WPARAM,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowPlacement, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindow, KillTimer, SendMessageTimeoutW, SetTimer, CHILDID_SELF, EVENT_OBJECT_NAMECHANGE,
    OBJID_WINDOW, SMTO_ABORTIFHUNG, USER_TIMER_MINIMUM, WINDOWPLACEMENT, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WM_SETTEXT, WM_TIMER,
};

use crate::core::desktop_manager::{self, DesktopManager};
use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    WinCraftPlugin,
};
use crate::core::ui_bridge::{ArrangeAction, Prompt};
use crate::core::window_names::{self, Names};
use crate::core::{appid, host, instance, wide, windows_list};
use guard::{Guard, Verdict};
use rules::{Reason, Rule, RuleState, TitleStyle, Wanted, Window};

/// Timer ids on the shared host window, spelling "WN" to stay clear of the
/// other plugins' ids.
const TICK_TIMER: usize = 0x574E_0001;
const CHANGE_TIMER: usize = 0x574E_0002;
const TICK_MS: u32 = 1000;
/// Saved names wait for their window; they are matched every other tick.
const MATCH_EVERY: u64 = 2;
/// A new app title is saved within this many ticks; a new name at once.
const SAVE_EVERY: u64 = 30;
/// Setting a title sends the app a message. An app that hangs must not hang
/// WinCraft with it.
const SET_TIMEOUT_MS: u32 = 500;
const MAX_NAME_CHARS: usize = 200;

const ID: &str = "window_namer";
const ACTION_RENAME_ACTIVE: u32 = 1;
const COMMAND_CLEAR_ALL: u32 = 1;
const PROMPT_RENAME: u32 = 1;
/// Win+Alt+N was taken on the PC this was built on (ShortcutDetector said
/// "Taken by another app"); E, for editing the title, was free.
const VK_E: u32 = b'E' as u32;

thread_local! {
    /// Windows whose title changed since the last look, noted by the event
    /// hook and handled on a timer.
    static CHANGED: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
    static HOST_WINDOW: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
}

/// Windows calls this on the host thread for every title change in a process
/// that has a named window, and it may do so while the host thread waits on
/// a message it sent, in the middle of other work. So it only notes the
/// window; a timer does the rest from the message loop.
unsafe extern "system" fn on_name_change(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if hwnd.is_null() || object != OBJID_WINDOW || child != CHILDID_SELF as i32 {
        return;
    }
    let noted = CHANGED.with(|changed| {
        changed
            .try_borrow_mut()
            .map(|mut list| {
                if !list.contains(&(hwnd as isize)) {
                    list.push(hwnd as isize);
                }
            })
            .is_ok()
    });
    let host_window = HOST_WINDOW.with(Cell::get);
    if noted && !host_window.is_null() {
        unsafe { SetTimer(host_window, CHANGE_TIMER, USER_TIMER_MINIMUM, None) };
    }
}

/// Why a title could not be set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetError {
    /// Windows keeps apps from sending messages to an app that runs as
    /// administrator.
    Elevated,
    NotResponding,
    /// The message went through and the title stayed as it was.
    Kept,
    Other(u32),
}

impl SetError {
    fn message(self) -> String {
        match self {
            SetError::Elevated => "Cannot rename: that app runs as administrator".to_string(),
            SetError::NotResponding => "Cannot rename: that app is not responding".to_string(),
            SetError::Kept => "Cannot rename: that app keeps its own title".to_string(),
            SetError::Other(code) => format!("Cannot rename: Windows error {code}"),
        }
    }
}

/// Whether the title bar took `wanted`. Titles are read into a 512-unit
/// buffer, so a very long one comes back cut short.
fn took(shown: &str, wanted: &str) -> bool {
    shown == wanted || (shown.encode_utf16().count() >= 500 && wanted.starts_with(shown))
}

fn set_text(hwnd: HWND, text: &str) -> Result<(), SetError> {
    let text_wide = wide(text);
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_SETTEXT,
            0,
            text_wide.as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            SET_TIMEOUT_MS,
            &mut result,
        )
    };
    if sent == 0 {
        return Err(match unsafe { GetLastError() } {
            ERROR_ACCESS_DENIED => SetError::Elevated,
            ERROR_TIMEOUT => SetError::NotResponding,
            code => SetError::Other(code),
        });
    }
    if !took(&windows_list::raw_window_text(hwnd), text) {
        return Err(SetError::Kept);
    }
    Ok(())
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// Where the window is, or for a minimized one where it comes back to: a
/// minimized window reports a position far off screen.
fn rect_of(hwnd: HWND) -> rules::Rect {
    let mut place: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    let rect =
        if unsafe { IsIconic(hwnd) } != 0 && unsafe { GetWindowPlacement(hwnd, &mut place) } != 0 {
            place.rcNormalPosition
        } else {
            let mut rect: RECT = unsafe { std::mem::zeroed() };
            unsafe { GetWindowRect(hwnd, &mut rect) };
            rect
        };
    [rect.left, rect.top, rect.right, rect.bottom]
}

fn group_of(hwnd: HWND, pid: u32) -> String {
    appid::group_key(
        appid::read(hwnd).id.as_deref(),
        &windows_list::exe_path(pid),
    )
}

/// The window still exists and belongs to the same process, so its handle
/// was not handed to a new window.
fn alive(hwnd: isize, pid: u32) -> bool {
    let exists = unsafe { IsWindow(hwnd as HWND) } != 0;
    exists && pid_of(hwnd as HWND) == pid
}

/// The window the user is working in. When that is one of WinCraft's own,
/// as when the command runs from the palette, or no app window at all,
/// the topmost app window of another program.
fn active_window() -> Option<isize> {
    let own = unsafe { GetCurrentProcessId() };
    let front = unsafe { GetForegroundWindow() };
    let usable = |hwnd: HWND| {
        !hwnd.is_null()
            && pid_of(hwnd) != own
            && windows_list::is_app_window(&windows_list::read(hwnd, &|_| true))
    };
    if usable(front) {
        return Some(front as isize);
    }
    windows_list::app_windows(&|_| false)
        .into_iter()
        .find(|hwnd| pid_of(*hwnd) != own)
        .map(|hwnd| hwnd as isize)
}

/// "notepad" for C:\Windows\notepad.exe.
fn program_of(pid: u32) -> String {
    let path = windows_list::exe_path(pid);
    let file = path.rsplit('\\').next().unwrap_or("");
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    stem.to_lowercase()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

/// One line, no longer than a title bar can reasonably show.
fn clean_name(name: &str) -> String {
    let line: String = name
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    line.trim().chars().take(MAX_NAME_CHARS).collect::<String>()
}

/// A window that carries a name now.
struct Live {
    pid: u32,
    names: Names,
    guard: Guard,
}

/// A saved name and the window it is on, if any.
struct Saved {
    rule: Rule,
    window: Option<isize>,
    /// It was on a window earlier in this session and that window closed.
    rested: bool,
}

impl Saved {
    fn state(&self) -> RuleState {
        match (self.window, self.rested) {
            (Some(_), _) => RuleState::Bound,
            (None, true) => RuleState::Rested,
            (None, false) => RuleState::Free,
        }
    }
}

/// What a window is, read when it is renamed.
struct Target {
    pid: u32,
    app_title: String,
    group: String,
    rect: rules::Rect,
}

fn read_target(hwnd: HWND) -> Result<Target, String> {
    if unsafe { IsWindow(hwnd) } == 0 {
        return Err("Cannot rename: that window is closed".to_string());
    }
    let pid = pid_of(hwnd);
    if pid == unsafe { GetCurrentProcessId() } {
        return Err("WinCraft does not rename its own windows".to_string());
    }
    Ok(Target {
        pid,
        app_title: windows_list::raw_window_text(hwnd),
        group: group_of(hwnd, pid),
        rect: rect_of(hwnd),
    })
}

pub struct WindowNamer {
    host_hwnd: Option<HWND>,
    keep: bool,
    style: TitleStyle,
    live: BTreeMap<isize, Live>,
    /// One title-change hook per process with a named window.
    hooks: BTreeMap<u32, HWINEVENTHOOK>,
    saved: Vec<Saved>,
    /// Each window's taskbar group, read once per window: it costs a COM
    /// call and does not change.
    groups: HashMap<isize, (u32, String)>,
    file: PathBuf,
    dirty: bool,
    ticks: u64,
    /// A test instance matches saved names by title only, never as an
    /// app's only window, so it cannot name one of the user's own windows.
    titles_only: bool,
}

impl Default for WindowNamer {
    fn default() -> Self {
        Self {
            host_hwnd: None,
            keep: true,
            style: TitleStyle::default(),
            live: BTreeMap::new(),
            hooks: BTreeMap::new(),
            saved: Vec::new(),
            groups: HashMap::new(),
            file: PathBuf::new(),
            dirty: false,
            ticks: 0,
            titles_only: false,
        }
    }
}

impl WindowNamer {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_settings(&mut self, settings: &serde_json::Value) {
        self.keep = settings
            .get("keep_names")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        self.style = TitleStyle::from_setting(settings.get("title_shows").and_then(|v| v.as_str()));
    }

    fn title_for(&self, names: &Names) -> String {
        self.style.title(&names.custom, &names.app_title)
    }

    fn saved_of(&mut self, hwnd: isize) -> Option<&mut Saved> {
        self.saved
            .iter_mut()
            .find(|saved| saved.window == Some(hwnd))
    }

    fn changed(&self) {
        host::plugin_changed();
        host::refresh_arrange();
    }

    /// Gives the window a name, or a new one; an empty name takes it away.
    fn rename(&mut self, hwnd: isize, name: &str) {
        let name = clean_name(name);
        if name.is_empty() {
            self.clear(hwnd);
            return;
        }
        if let Some(live) = self.live.get_mut(&hwnd) {
            let old = std::mem::replace(&mut live.names.custom, name.clone());
            let names = live.names.clone();
            window_names::set(hwnd, names.clone());
            if let Err(err) = set_text(hwnd as HWND, &self.title_for(&names)) {
                log::warn!("\"{old}\" renamed to \"{name}\", but {}", err.message());
            } else {
                log::info!("\"{old}\" renamed to \"{name}\"");
            }
            if let Some(saved) = self.saved_of(hwnd) {
                saved.rule.name = name;
            }
            self.save_now();
            self.changed();
            return;
        }
        let target = match read_target(hwnd as HWND) {
            Ok(target) => target,
            Err(why) => {
                log::warn!("{why}");
                host::notify("WindowNamer", &why);
                return;
            }
        };
        if let Err(err) = self.start(hwnd, target.pid, &name, &target.app_title) {
            log::warn!(
                "could not name \"{}\" \"{name}\": {}",
                target.app_title,
                err.message()
            );
            host::notify("WindowNamer", &err.message());
            return;
        }
        log::info!("\"{}\" is now called \"{name}\"", target.app_title);
        // A saved name of this app that found no window is what the user
        // just gave by hand; kept, it would later compete with the new one
        // and neither would be put back.
        self.saved.retain(|saved| {
            saved.window.is_some() || saved.rule.group != target.group || saved.rule.name != name
        });
        let now = unix_now();
        self.saved.push(Saved {
            rule: Rule {
                name,
                group: target.group,
                app_title: target.app_title,
                rect: target.rect,
                created: now,
                last_used: now,
            },
            window: Some(hwnd),
            rested: false,
        });
        self.save_now();
        self.changed();
    }

    /// Opens the palette's rename box for the window.
    fn ask_name(&self, hwnd: isize) {
        let target = hwnd as HWND;
        if unsafe { IsWindow(target) } == 0 {
            return;
        }
        let pid = pid_of(target);
        if pid == unsafe { GetCurrentProcessId() } {
            log::info!("WinCraft does not rename its own windows");
            return;
        }
        let names = self.live.get(&hwnd).map(|live| live.names.clone());
        let app_title = names
            .as_ref()
            .map(|names| names.app_title.clone())
            .unwrap_or_else(|| windows_list::raw_window_text(target));
        let program = program_of(pid);
        host::ask(Prompt {
            plugin: ID.to_string(),
            id: PROMPT_RENAME,
            target: hwnd,
            chip: "Rename".to_string(),
            placeholder: format!("Name for \u{201c}{app_title}\u{201d}"),
            empty_action: names.as_ref().map(|_| "Clear the name".to_string()),
            text: names.map(|names| names.custom).unwrap_or_default(),
            action: "Rename to".to_string(),
            subtitle: if program.is_empty() {
                app_title
            } else {
                format!("{program} \u{b7} {app_title}")
            },
        });
    }

    /// Puts the name on the window and starts keeping it there.
    fn start(
        &mut self,
        hwnd: isize,
        pid: u32,
        name: &str,
        app_title: &str,
    ) -> Result<(), SetError> {
        let names = Names {
            custom: name.to_string(),
            app_title: app_title.to_string(),
        };
        // In the registry before the title changes, so nothing reads the
        // name back as the app's own title.
        window_names::set(hwnd, names.clone());
        if let Err(err) = set_text(hwnd as HWND, &self.title_for(&names)) {
            window_names::remove(hwnd);
            return Err(err);
        }
        self.watch(pid);
        self.live.insert(
            hwnd,
            Live {
                pid,
                names,
                guard: Guard::default(),
            },
        );
        Ok(())
    }

    /// Takes the name off the window and forgets it.
    fn clear(&mut self, hwnd: isize) {
        let Some(live) = self.live.remove(&hwnd) else {
            return;
        };
        self.put_back(hwnd, &live.names);
        window_names::remove(hwnd);
        self.unwatch_if_unused(live.pid);
        self.saved.retain(|saved| saved.window != Some(hwnd));
        log::info!(
            "\"{}\" has no name any more; it shows \"{}\" again",
            live.names.custom,
            live.names.app_title
        );
        self.save_now();
        self.changed();
    }

    fn clear_all(&mut self) {
        let count = self.saved.len();
        self.restore_all();
        self.saved.clear();
        self.save_now();
        log::info!("all {count} saved names cleared");
        self.changed();
    }

    /// Every named window shows its app's own title again.
    fn restore_all(&mut self) -> usize {
        let live = std::mem::take(&mut self.live);
        for (hwnd, entry) in &live {
            self.put_back(*hwnd, &entry.names);
            window_names::remove(*hwnd);
        }
        for hook in std::mem::take(&mut self.hooks).into_values() {
            unsafe { UnhookWinEvent(hook) };
        }
        for saved in &mut self.saved {
            if saved.window.take().is_some() {
                saved.rule.last_used = unix_now();
            }
        }
        live.len()
    }

    /// Gives the window its app's own title back, unless the app has set a
    /// title since, in a pause or in a change not handled yet: that one is
    /// newer than any title WinCraft remembers.
    fn put_back(&self, hwnd: isize, names: &Names) {
        let shown = windows_list::raw_window_text(hwnd as HWND);
        if !took(&shown, &self.title_for(names)) {
            log::debug!("\"{}\" already shows a title of its app's", names.custom);
            return;
        }
        if let Err(err) = set_text(hwnd as HWND, &names.app_title) {
            log::warn!(
                "could not put \"{}\" back: {}",
                names.app_title,
                err.message()
            );
        }
    }

    fn watch(&mut self, pid: u32) {
        if self.hooks.contains_key(&pid) {
            return;
        }
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_NAMECHANGE,
                EVENT_OBJECT_NAMECHANGE,
                std::ptr::null_mut(),
                Some(on_name_change),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_null() {
            log::warn!(
                "could not watch the titles of process {pid}; checking once a second instead"
            );
            return;
        }
        self.hooks.insert(pid, hook);
    }

    fn unwatch_if_unused(&mut self, pid: u32) {
        if self.live.values().any(|live| live.pid == pid) {
            return;
        }
        if let Some(hook) = self.hooks.remove(&pid) {
            unsafe { UnhookWinEvent(hook) };
        }
    }

    /// A named window's title changed, or might have: when it is not the
    /// name, the app set a title of its own, which is noted and covered
    /// with the name again.
    fn check_title(&mut self, hwnd: isize, now: u64) {
        let style = self.style;
        let Some(live) = self.live.get_mut(&hwnd) else {
            return;
        };
        let shown = windows_list::raw_window_text(hwnd as HWND);
        if took(
            &shown,
            &style.title(&live.names.custom, &live.names.app_title),
        ) {
            return;
        }
        let app_title = style.app_part(&live.names.custom, &shown).to_string();
        if app_title != live.names.app_title {
            log::debug!("a named window's app title is now \"{app_title}\"");
            live.names.app_title = app_title.clone();
            window_names::set(hwnd, live.names.clone());
            self.dirty = true;
        }
        match live.guard.check(now) {
            Verdict::Apply => {
                let title = style.title(&live.names.custom, &live.names.app_title);
                if let Err(err) = set_text(hwnd as HWND, &title) {
                    log::debug!(
                        "could not put \"{}\" back on: {}",
                        live.names.custom,
                        err.message()
                    );
                }
            }
            Verdict::Pause if live.guard.pauses() == 1 => log::warn!(
                "the window called \"{}\" keeps changing its title; putting the name back pauses for {} s",
                live.names.custom,
                guard::PAUSE_MS / 1000
            ),
            Verdict::Pause => log::debug!("\"{}\" paused again", live.names.custom),
            Verdict::Wait => {}
        }
        if let Some(saved) = self.saved_of(hwnd) {
            saved.rule.app_title = app_title;
        }
    }

    fn handle_changes(&mut self) {
        if let Some(hwnd) = self.host_hwnd {
            unsafe { KillTimer(hwnd, CHANGE_TIMER) };
        }
        let changed = CHANGED.with(|changed| std::mem::take(&mut *changed.borrow_mut()));
        let now = now_ms();
        for hwnd in changed {
            self.check_title(hwnd, now);
        }
    }

    fn tick(&mut self) {
        self.ticks += 1;
        self.forget_closed();
        // A title change the hook missed, or the end of a pause.
        let now = now_ms();
        let named: Vec<isize> = self.live.keys().copied().collect();
        for hwnd in named {
            self.check_title(hwnd, now);
        }
        let waiting = self.saved.iter().any(|saved| saved.window.is_none());
        if waiting && self.ticks.is_multiple_of(MATCH_EVERY) {
            self.apply_saved();
        }
        if self.dirty && self.ticks.is_multiple_of(SAVE_EVERY) {
            self.save_now();
        }
    }

    /// A closed window loses its name, but the saved name stays for the
    /// next time the app opens it.
    fn forget_closed(&mut self) {
        let closed: Vec<isize> = self
            .live
            .iter()
            .filter(|(hwnd, live)| !alive(**hwnd, live.pid))
            .map(|(hwnd, _)| *hwnd)
            .collect();
        if closed.is_empty() {
            self.groups.retain(|hwnd, (pid, _)| alive(*hwnd, *pid));
            return;
        }
        for hwnd in closed {
            let Some(live) = self.live.remove(&hwnd) else {
                continue;
            };
            window_names::remove(hwnd);
            self.unwatch_if_unused(live.pid);
            if let Some(saved) = self.saved_of(hwnd) {
                saved.window = None;
                saved.rested = true;
                saved.rule.last_used = unix_now();
            }
            log::info!(
                "the window called \"{}\" closed; the name is kept for when it opens again",
                live.names.custom
            );
            self.dirty = true;
        }
        self.groups.retain(|hwnd, (pid, _)| alive(*hwnd, *pid));
        self.changed();
    }

    /// Puts saved names back on the windows they belong to.
    fn apply_saved(&mut self) {
        let own = unsafe { GetCurrentProcessId() };
        let desktops = DesktopManager::new().ok();
        let on_a_desktop = |hwnd: HWND| {
            desktops
                .as_ref()
                .and_then(|manager| manager.desktop_of(hwnd))
                .is_some_and(|desktop| !desktop_manager::is_null(&desktop))
        };
        let mut windows = Vec::new();
        let mut pids = Vec::new();
        for (hwnd, record) in windows_list::app_window_records(&on_a_desktop) {
            let key = hwnd as isize;
            let pid = pid_of(hwnd);
            if pid == own {
                continue;
            }
            let group = match self.groups.get(&key) {
                Some((known, group)) if *known == pid => group.clone(),
                _ => {
                    let group = group_of(hwnd, pid);
                    self.groups.insert(key, (pid, group.clone()));
                    group
                }
            };
            windows.push(Window {
                hwnd: key,
                group,
                title: record.title,
                rect: rect_of(hwnd),
                named: self.live.contains_key(&key),
            });
            pids.push(pid);
        }
        let wanted: Vec<Wanted> = self
            .saved
            .iter()
            .map(|saved| Wanted {
                rule: &saved.rule,
                state: saved.state(),
            })
            .collect();
        let pairs = rules::match_rules(&wanted, &windows, !self.titles_only);
        if pairs.is_empty() {
            return;
        }
        for (index, at, reason) in pairs {
            let window = &windows[at];
            let name = self.saved[index].rule.name.clone();
            // A window that still shows the name has lost its own title.
            let app_title = if window.title == name {
                self.saved[index].rule.app_title.clone()
            } else {
                window.title.clone()
            };
            match self.start(window.hwnd, pids[at], &name, &app_title) {
                Ok(()) => {
                    let saved = &mut self.saved[index];
                    saved.window = Some(window.hwnd);
                    saved.rested = false;
                    saved.rule.app_title = app_title;
                    saved.rule.last_used = unix_now();
                    self.dirty = true;
                    let how = match reason {
                        Reason::Title => "same title",
                        Reason::Page => "same page",
                        Reason::Lone => "the app's only window",
                    };
                    log::info!("\"{}\" is called \"{name}\" again ({how})", window.title);
                }
                Err(err) => log::warn!(
                    "could not name \"{}\" \"{name}\" again: {}",
                    window.title,
                    err.message()
                ),
            }
        }
        self.changed();
    }

    /// Writes the saved names; with "Keep names after restart" off, writes
    /// none, so names from before cannot come back later.
    fn save_now(&mut self) {
        self.dirty = false;
        if !self.keep && !self.file.exists() {
            return;
        }
        let rules: Vec<Rule> = if self.keep {
            self.saved.iter().map(|saved| saved.rule.clone()).collect()
        } else {
            Vec::new()
        };
        if let Err(err) = rules::save(&self.file, &rules) {
            log::error!("could not save the window names: {err}");
        }
    }

    fn restyle(&mut self) {
        for (hwnd, live) in &self.live {
            let title = self.style.title(&live.names.custom, &live.names.app_title);
            if let Err(err) = set_text(*hwnd as HWND, &title) {
                log::debug!(
                    "could not restyle \"{}\": {}",
                    live.names.custom,
                    err.message()
                );
            }
        }
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl WinCraftPlugin for WindowNamer {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: ID,
            name: "WindowNamer",
            description: "Give any window a name of your own. The taskbar, Alt+Tab and the title bar show it, and it comes back after a restart.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "keep_names": true,
            "title_shows": TitleStyle::CHOICES[0],
        })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: "keep_names",
                label: "Keep names after restart",
                help: "Save each name and put it back on its window when WinCraft or the app starts again.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: "title_shows",
                label: "Title shows",
                help: "What the taskbar, Alt+Tab and the title bar show for a renamed window.",
                kind: FieldKind::Choice(TitleStyle::CHOICES),
            },
        ]
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.apply_settings(ctx.settings);
        self.titles_only = instance::is_test();
        self.file = rules::path();
        let mut loaded = if self.keep {
            rules::load(&self.file)
        } else {
            Vec::new()
        };
        let expired = rules::expire(&mut loaded, unix_now());
        if expired > 0 {
            log::info!(
                "{} unused for {} days forgotten",
                plural(expired, "name", "names"),
                rules::KEEP_SECONDS / 86_400
            );
            self.dirty = true;
        }
        self.saved = loaded
            .into_iter()
            .map(|rule| Saved {
                rule,
                window: None,
                rested: false,
            })
            .collect();
        if unsafe { SetTimer(ctx.hwnd, TICK_TIMER, TICK_MS, None) } == 0 {
            return Err("could not start its timer".to_string());
        }
        self.host_hwnd = Some(ctx.hwnd);
        HOST_WINDOW.with(|cell| cell.set(ctx.hwnd));
        if self.titles_only {
            log::info!(
                "test instance: saved names are matched by title only, never as an app's only window"
            );
        }
        log::info!(
            "{} saved",
            plural(self.saved.len(), "window name", "window names")
        );
        if !self.saved.is_empty() {
            self.apply_saved();
        }
        Ok(())
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        let (keep, style) = (self.keep, self.style);
        self.apply_settings(settings);
        if self.style != style {
            self.restyle();
        }
        if self.keep != keep {
            self.save_now();
        }
        host::plugin_changed();
        true
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![HotkeyAction {
            id: ACTION_RENAME_ACTIVE,
            name: "rename_active",
            label: "Rename the active window",
            default: Hotkey {
                modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
                vk: VK_E,
            },
        }]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        if action_id != ACTION_RENAME_ACTIVE {
            return;
        }
        match active_window() {
            Some(hwnd) => self.ask_name(hwnd),
            None => log::info!("no window to rename"),
        }
    }

    fn on_prompt_answer(&mut self, prompt_id: u32, target: isize, text: &str) {
        if prompt_id == PROMPT_RENAME {
            self.rename(target, text);
        }
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: COMMAND_CLEAR_ALL,
            label: "Clear all names",
            hint: "",
        }]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id == COMMAND_CLEAR_ALL {
            self.clear_all();
        }
    }

    fn status(&self) -> Option<String> {
        let now = plural(self.live.len(), "window renamed now", "windows renamed now");
        let saved = if self.keep {
            format!("{} saved", plural(self.saved.len(), "name", "names"))
        } else {
            "names are not kept after a restart".to_string()
        };
        Some(format!("{now} \u{b7} {saved}"))
    }

    fn on_arrange_action(&mut self, action: &ArrangeAction) {
        match action {
            ArrangeAction::Rename {
                hwnd,
                name: Some(name),
            } => self.rename(*hwnd, name),
            ArrangeAction::Rename { hwnd, name: None } => self.clear(*hwnd),
            ArrangeAction::AskName(hwnd) => self.ask_name(*hwnd),
            _ => {}
        }
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, _lparam: LPARAM) {
        if msg != WM_TIMER {
            return;
        }
        match wparam {
            TICK_TIMER => self.tick(),
            CHANGE_TIMER => self.handle_changes(),
            _ => {}
        }
    }

    fn teardown(&mut self) {
        if let Some(hwnd) = self.host_hwnd.take() {
            unsafe {
                KillTimer(hwnd, TICK_TIMER);
                KillTimer(hwnd, CHANGE_TIMER);
            }
        }
        HOST_WINDOW.with(|cell| cell.set(std::ptr::null_mut()));
        let restored = self.restore_all();
        if restored > 0 {
            log::info!(
                "put the apps' own titles back on {}",
                plural(restored, "window", "windows")
            );
        }
        self.save_now();
        self.saved.clear();
        self.groups.clear();
        CHANGED.with(|changed| changed.borrow_mut().clear());
        self.ticks = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_keeping_names_and_showing_the_name_alone() {
        let mut plugin = WindowNamer::new();
        plugin.apply_settings(&serde_json::json!({}));
        assert!(plugin.keep);
        assert_eq!(plugin.style, TitleStyle::NameOnly);
        plugin.apply_settings(&serde_json::json!({
            "keep_names": false,
            "title_shows": "Name \u{2014} app title",
        }));
        assert!(!plugin.keep);
        assert_eq!(plugin.style, TitleStyle::NameAndApp);
    }

    #[test]
    fn a_name_is_one_trimmed_line_of_reasonable_length() {
        assert_eq!(clean_name("  Work notes \n"), "Work notes");
        assert_eq!(clean_name("two\nlines"), "two lines");
        assert_eq!(clean_name("\t"), "");
        assert_eq!(clean_name(&"x".repeat(500)).chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn a_title_counts_as_taken_even_when_read_back_cut_short() {
        assert!(took("Mail", "Mail"));
        assert!(!took("Inbox", "Mail"));
        assert!(!took("Ma", "Mail"));
        let long = "y".repeat(700);
        assert!(took(&long[..511], &long));
    }

    #[test]
    fn a_saved_name_knows_whether_it_is_on_a_window() {
        let rule = Rule {
            name: "A".to_string(),
            group: "g".to_string(),
            app_title: "t".to_string(),
            rect: [0, 0, 1, 1],
            created: 0,
            last_used: 0,
        };
        let mut saved = Saved {
            rule,
            window: None,
            rested: false,
        };
        assert_eq!(saved.state(), RuleState::Free);
        saved.rested = true;
        assert_eq!(saved.state(), RuleState::Rested);
        saved.window = Some(5);
        assert_eq!(saved.state(), RuleState::Bound);
    }

    #[test]
    fn the_status_counts_live_and_saved_names() {
        let plugin = WindowNamer::new();
        assert_eq!(
            plugin.status().as_deref(),
            Some("0 windows renamed now \u{b7} 0 names saved")
        );
        let off = WindowNamer {
            keep: false,
            ..WindowNamer::new()
        };
        assert_eq!(
            off.status().as_deref(),
            Some("0 windows renamed now \u{b7} names are not kept after a restart")
        );
    }

    #[test]
    fn the_hotkey_is_a_win_alt_one_as_every_default_must_be() {
        let actions = WindowNamer::new().hotkey_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            crate::core::hotkeys::format(actions[0].default),
            "Win+Alt+E"
        );
    }

    #[test]
    fn errors_say_what_to_do_about_them() {
        assert_eq!(
            SetError::Elevated.message(),
            "Cannot rename: that app runs as administrator"
        );
        assert_eq!(
            SetError::Other(87).message(),
            "Cannot rename: Windows error 87"
        );
    }
}

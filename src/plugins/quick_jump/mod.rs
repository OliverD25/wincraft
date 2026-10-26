//! QuickJump: pin up to ten windows to the digits and jump back to one
//! from anywhere, like Harpoon in Neovim. Win+Alt+Q, then a digit, jumps;
//! Win+Alt+Shift+Q, then a digit, pins the window in front; Win+Alt+Q twice
//! opens the pin list.
//!
//! The plugin keeps a hidden window of its own: the next-key wait posts to
//! it, its timers run the strip's countdown and the notice, and the pin
//! list on the UI thread wakes it with requests. Everything else it gets
//! from the host.

mod keys;
mod list;
mod live;
mod look;
mod matching;
mod notice;
mod picker;
mod pictures;
mod pins;
mod provider;
mod shared;
mod strip;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_SHIFT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, KillTimer, PostMessageW,
    RegisterClassW, SetTimer, HWND_MESSAGE, WM_APP, WM_TIMER, WNDCLASSW,
};

use crate::core::traits::{
    HostContext, Hotkey, HotkeyAction, PluginMetadata, PluginWindow, TrayAction, WinCraftPlugin,
};
use crate::core::ui_bridge::ActionKind;
use crate::core::{chord, config, host, instance, shortcuts, wide};
use crate::search::actions;
use crate::search::icons::{self, Pixels};
use crate::search::{Action, IconRef, SearchProvider};

use keys::Key;
use matching::{find, slot_views, Live, SlotView, State};
use notice::{Clock, Message, Notice};
use pins::{Pin, Pinned, Slots, Undo};
use shared::{App, Handle, Request};
use strip::{Mode, Strip};

pub const ID: &str = "quick_jump";
const NAME: &str = "QuickJump";
const CLASS_NAME: &str = "WinCraftQuickJump";

const ACTION_JUMP: u32 = 1;
const ACTION_PIN: u32 = 2;
const TRAY_LIST: u32 = 1;

/// Posted by the next-key wait.
const WM_APP_KEY: u32 = WM_APP + 40;
/// The strip's countdown and the notice's clock; only runs while one shows
/// or an app is starting.
const TICK_TIMER: usize = 0x514A_0001;
const TICK_MS: u32 = 50;
/// How long to wait for an app QuickJump started to open a window.
const START_WAIT: Duration = Duration::from_secs(15);
const START_POLL: Duration = Duration::from_millis(500);
/// A second jump press this soon after the strip closed on it still opens
/// the list; longer when the shortcut is a chord, whose own wait comes first.
const AGAIN_PLAIN: Duration = Duration::from_millis(600);
const AGAIN_CHORD: Duration = Duration::from_secs(4);

pub struct QuickJump {
    shared: Handle,
}

impl QuickJump {
    pub fn new() -> Self {
        Self {
            shared: shared::new(),
        }
    }
}

impl Default for QuickJump {
    fn default() -> Self {
        Self::new()
    }
}

fn state_path() -> PathBuf {
    config::plugins_dir().join("quick_jump.state.json")
}

/// What the strip waits for, and for which window.
struct Waiting {
    mode: Mode,
    strip: Option<Strip>,
    started: Instant,
    /// The window in front when the strip opened.
    front: isize,
    /// The window the pin strip pins.
    target: Option<Live>,
}

struct Shown {
    notice: Option<Notice>,
    clock: Clock,
    undo: Option<(Undo, String)>,
}

struct Starting {
    slot: usize,
    name: String,
    until: Instant,
    next: Instant,
}

struct Engine {
    hwnd: HWND,
    shared: Handle,
    slots: Slots,
    path: PathBuf,
    jump_keys: String,
    pin_keys: String,
    waiting: Option<Waiting>,
    /// When and how the strip last closed because another key came.
    closed_by_key: Option<(Instant, Mode)>,
    shown: Option<Shown>,
    /// The Ctrl+Z wait runs for the notice.
    undo_keys: bool,
    starting: Option<Starting>,
    icons: HashMap<String, Option<Pixels>>,
    live: Vec<Live>,
    epoch: Instant,
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// Runs `f` on the engine; false when it is off or already busy, as when a
/// message arrives while the engine is working.
fn with_engine(f: impl FnOnce(&mut Engine)) -> bool {
    ENGINE.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => match slot.as_mut() {
            Some(engine) => {
                f(engine);
                true
            }
            None => true,
        },
        Err(_) => false,
    })
}

impl Engine {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn save(&self) {
        if let Err(err) = config::write_atomic(&self.path, &self.slots.to_json()) {
            log::warn!("QuickJump: could not save the pins: {err}");
        }
    }

    /// Reads the open windows, ties each pin to its window and hands the
    /// windows a fresh view.
    fn refresh(&mut self) {
        self.live = live::read();
        let live = &self.live;
        for (_, pin) in self.slots.iter_mut() {
            pin.hwnd = find(pin, live).map(|window| window.hwnd);
        }
        self.publish();
    }

    fn views(&self, front: isize) -> Vec<SlotView> {
        slot_views(&self.slots, &self.live, front)
    }

    fn publish(&mut self) {
        let front = unsafe { GetForegroundWindow() } as isize;
        let slots = self.views(front);
        let mut apps: Vec<App> = Vec::new();
        let pinned = self.slots.iter().map(|(_, pin)| pin);
        let running = self.live.iter().map(|live| App {
            exe: live.exe.clone(),
            app_id: live.app_id.clone(),
            name: live.app_name(),
        });
        let saved = pinned.map(|pin| App {
            exe: pin.exe.clone(),
            app_id: pin.app_id.clone(),
            name: pin.app_name.clone(),
        });
        for app in running.chain(saved) {
            if !apps
                .iter()
                .any(|have| have.exe.eq_ignore_ascii_case(&app.exe) && have.app_id == app.app_id)
            {
                apps.push(app);
            }
        }
        if let Ok(mut shared) = self.shared.lock() {
            shared.view = shared::View {
                slots,
                pins: (0..pins::SLOTS)
                    .map(|slot| self.slots.get(slot).cloned())
                    .collect(),
                windows: self.live.clone(),
                apps,
                jump_keys: self.jump_keys.clone(),
                pin_keys: self.pin_keys.clone(),
            };
        }
        host::plugin_changed();
    }

    fn icon_key(view_exe: &str, app_id: Option<&str>) -> String {
        format!("{}|{}", view_exe.to_lowercase(), app_id.unwrap_or_default())
    }

    /// The app's icon, read once per app and kept.
    fn load_icon(&mut self, exe: &str, app_id: Option<&str>, hwnd: Option<isize>) {
        let key = Self::icon_key(exe, app_id);
        if self.icons.contains_key(&key) {
            return;
        }
        let packaged = app_id.filter(|id| id.contains('!'));
        let reference = match (hwnd, packaged) {
            (Some(hwnd), _) => IconRef::Window {
                hwnd,
                exe: PathBuf::from(exe),
            },
            (None, Some(id)) => IconRef::Path(PathBuf::from(format!(r"shell:AppsFolder\{id}"))),
            (None, None) => IconRef::Path(PathBuf::from(exe)),
        };
        self.icons.insert(key, icons::load(&reference));
    }

    fn icon(&self, exe: &str, app_id: Option<&str>) -> Option<&Pixels> {
        self.icons
            .get(&Self::icon_key(exe, app_id))
            .and_then(Option::as_ref)
    }

    fn start_ticks(&self) {
        unsafe { SetTimer(self.hwnd, TICK_TIMER, TICK_MS, None) };
    }

    fn stop_ticks_if_idle(&self) {
        if self.waiting.is_none() && self.shown.is_none() && self.starting.is_none() {
            unsafe { KillTimer(self.hwnd, TICK_TIMER) };
        }
    }

    // ---- The strip ---------------------------------------------------

    fn jump_pressed(&mut self) {
        let again = match (&self.waiting, self.closed_by_key) {
            (Some(waiting), _) => waiting.mode == Mode::Jump,
            (None, Some((at, Mode::Jump))) => {
                let chord = shortcuts::parse(&self.jump_keys).is_some_and(|keys| keys.is_chord());
                at.elapsed() <= if chord { AGAIN_CHORD } else { AGAIN_PLAIN }
            }
            _ => false,
        };
        if again {
            log::info!("QuickJump: the jump shortcut twice opens the pin list");
            self.close_strip(false);
            self.closed_by_key = None;
            self.open_list(None);
            return;
        }
        self.open_strip(Mode::Jump);
    }

    fn pin_pressed(&mut self) {
        self.open_strip(Mode::Pin);
    }

    fn open_strip(&mut self, mode: Mode) {
        self.close_strip(false);
        self.close_notice();
        let foreground = unsafe { GetForegroundWindow() };
        self.refresh();
        let front = foreground as isize;
        let target = match mode {
            Mode::Jump => None,
            Mode::Pin => {
                // From the palette the window in front is WinCraft's own, so
                // the one under it is meant.
                let own = live::is_own(foreground);
                let found = self
                    .live
                    .iter()
                    .find(|window| window.hwnd == front)
                    .or_else(|| own.then(|| self.live.first()).flatten())
                    .cloned();
                if found.is_none() {
                    self.notify(notice::nothing_to_pin(), None);
                    return;
                }
                found
            }
        };
        let listening = chord::listen(
            self.hwnd as isize,
            WM_APP_KEY,
            shortcuts::CHORD_WAIT,
            keys::rule(&self.jump_keys, &self.pin_keys),
        );
        if !listening {
            self.notify(notice::no_keys(), None);
            self.open_list(None);
            return;
        }
        let views = self.views(target.as_ref().map_or(front, |window| window.hwnd));
        for view in views.iter().filter(|view| view.is_pinned()) {
            self.load_icon(&view.exe, view.app_id.as_deref(), view.hwnd);
        }
        if let Some(window) = &target {
            self.load_icon(&window.exe, window.app_id.as_deref(), Some(window.hwnd));
        }
        let strip = {
            let icons_list = views
                .iter()
                .map(|view| {
                    view.is_pinned()
                        .then(|| self.icon(&view.exe, view.app_id.as_deref()))
                        .flatten()
                })
                .collect();
            let target_view = target.as_ref().map(|window| strip::Target {
                name: self
                    .slots
                    .slot_of_window(window.hwnd)
                    .and_then(|slot| self.slots.get(slot))
                    .map_or_else(|| window.app_name(), |pin| pin.name().to_string()),
                place: match window.place() {
                    place if place.is_empty() => window.shown.clone(),
                    place => place,
                },
                icon: self.icon(&window.exe, window.app_id.as_deref()),
                slot: self.slots.slot_of_window(window.hwnd),
            });
            let target_name = target_view
                .as_ref()
                .map(|target| target.name.clone())
                .unwrap_or_default();
            let content = strip::Content {
                mode,
                slots: &views,
                icons: icons_list,
                target: target_view,
                hint: strip::hint(mode, &self.jump_keys, &target_name),
            };
            Strip::show(&content)
        };
        log::info!(
            "QuickJump: {} strip open, waiting 3 s for a digit",
            match mode {
                Mode::Jump => "jump",
                Mode::Pin => "pin",
            }
        );
        self.waiting = Some(Waiting {
            mode,
            strip,
            started: Instant::now(),
            front,
            target,
        });
        self.undo_keys = false;
        self.start_ticks();
    }

    /// Takes the strip away. `by_key` says another key closed it, which a
    /// second jump press right after still counts as "twice".
    fn close_strip(&mut self, by_key: bool) {
        if let Some(waiting) = self.waiting.take() {
            chord::stop_for(self.hwnd as isize);
            if by_key {
                self.closed_by_key = Some((Instant::now(), waiting.mode));
            }
        }
        self.stop_ticks_if_idle();
    }

    fn on_keys(&mut self) {
        let news = chord::take_for(self.hwnd as isize);
        if self.undo_keys {
            if !news.taken.is_empty() {
                self.undo();
            }
            if news.ended || !news.taken.is_empty() {
                self.undo_keys = false;
                chord::stop_for(self.hwnd as isize);
            }
            return;
        }
        let Some(mode) = self.waiting.as_ref().map(|waiting| waiting.mode) else {
            return;
        };
        for stroke in news.taken {
            match keys::kind(stroke, &[]) {
                Key::Digit(digit) => {
                    let Some(slot) = pins::slot_of_digit(digit) else {
                        continue;
                    };
                    let waiting = self.waiting.take();
                    chord::stop_for(self.hwnd as isize);
                    match (mode, waiting) {
                        (Mode::Jump, Some(waiting)) => self.jump(slot, waiting.front),
                        (Mode::Pin, Some(waiting)) => {
                            if let Some(target) = waiting.target {
                                self.pin_window(slot, &target);
                            }
                        }
                        _ => {}
                    }
                    self.stop_ticks_if_idle();
                    return;
                }
                Key::Cancel => {
                    self.close_strip(false);
                    return;
                }
                Key::Again | Key::Other => {}
            }
        }
        if news.ended {
            self.close_strip(true);
        }
    }

    fn tick(&mut self) {
        if let Some(waiting) = &mut self.waiting {
            let wait = shortcuts::CHORD_WAIT.as_secs_f32();
            let left = 1.0 - waiting.started.elapsed().as_secs_f32() / wait;
            if let Some(strip) = &mut waiting.strip {
                strip.tick(left);
            }
        }
        let now = self.now_ms();
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.clock.expired(now))
        {
            self.close_notice();
        }
        if let Some(starting) = &mut self.starting {
            if Instant::now() >= starting.next {
                starting.next = Instant::now() + START_POLL;
                let (slot, name, late) = (
                    starting.slot,
                    starting.name.clone(),
                    Instant::now() >= starting.until,
                );
                self.refresh();
                let found = self
                    .slots
                    .get(slot)
                    .and_then(|pin| find(pin, &self.live))
                    .map(|window| window.hwnd);
                if let Some(hwnd) = found {
                    self.starting = None;
                    self.close_notice();
                    self.activate(hwnd);
                } else if late {
                    self.starting = None;
                    self.notify(notice::not_started(&name), None);
                }
            }
        }
        self.stop_ticks_if_idle();
    }

    // ---- Jumping and pinning -------------------------------------------

    fn activate(&self, hwnd: isize) {
        if instance::is_test() {
            log::info!("read-only test instance: switching to window {hwnd:#x} skipped");
            return;
        }
        actions::perform(&Action::Activate(hwnd), self.hwnd);
    }

    fn jump(&mut self, slot: usize, front: isize) {
        let Some(pin) = self.slots.get(slot).cloned() else {
            log::info!("QuickJump: slot {} is empty", pins::digit_of_slot(slot));
            self.notify(notice::empty(slot, &self.pin_keys), None);
            return;
        };
        match find(&pin, &self.live).map(|window| window.hwnd) {
            Some(hwnd) if hwnd == front => {
                log::info!("QuickJump: {} is in front already", pin.name());
            }
            Some(hwnd) => {
                log::info!("QuickJump: jump to slot {}", pins::digit_of_slot(slot));
                self.activate(hwnd);
            }
            None if pin.start => self.start(slot, &pin),
            None => self.notify(notice::not_open(pin.name(), slot), None),
        }
    }

    fn start(&mut self, slot: usize, pin: &Pin) {
        let target = match pin.app_id.as_deref().filter(|id| id.contains('!')) {
            Some(id) => format!(r"shell:AppsFolder\{id}"),
            None => pin.exe.clone(),
        };
        self.notify(notice::starting(pin.name(), slot), None);
        if instance::is_test() {
            log::info!("read-only test instance: starting {target} skipped");
            return;
        }
        log::info!(
            "QuickJump: starting {target} for slot {}",
            pins::digit_of_slot(slot)
        );
        actions::perform(&Action::Open(PathBuf::from(&target)), self.hwnd);
        self.starting = Some(Starting {
            slot,
            name: pin.name().to_string(),
            until: Instant::now() + START_WAIT,
            next: Instant::now() + START_POLL,
        });
        self.start_ticks();
    }

    fn pin_window(&mut self, slot: usize, window: &Live) {
        let (done, undo) = self.slots.pin(slot, window.to_pin());
        let name = self
            .slots
            .get(slot)
            .map_or_else(|| window.app_name(), |pin| pin.name().to_string());
        let message = notice::pinned(&name, slot, &done);
        log::info!("QuickJump: {}", message.text);
        if done != Pinned::Same {
            self.save();
            self.publish();
        }
        let undo = (!undo.is_empty()).then(|| (undo, message.text.clone()));
        self.notify(message, undo);
    }

    fn undo(&mut self) {
        let Some((undo, what)) = self.shown.as_mut().and_then(|shown| shown.undo.take()) else {
            return;
        };
        self.slots.undo(undo);
        self.save();
        self.refresh();
        log::info!("QuickJump: undone: {what}");
        self.notify(notice::undone(&what), None);
    }

    // ---- The notice ----------------------------------------------------

    fn notify(&mut self, message: Message, undo: Option<(Undo, String)>) {
        self.close_notice();
        let notice = Notice::show(&message, self.hwnd);
        let has_undo = message.undo && undo.is_some();
        self.shown = Some(Shown {
            notice,
            clock: Clock::new(self.now_ms(), has_undo),
            undo,
        });
        if has_undo && self.waiting.is_none() {
            self.undo_keys = chord::listen(
                self.hwnd as isize,
                WM_APP_KEY,
                Duration::from_millis(notice::UNDO_MS * 8),
                keys::undo_rule,
            );
        }
        self.start_ticks();
    }

    fn close_notice(&mut self) {
        if self.shown.take().is_some() && self.undo_keys {
            self.undo_keys = false;
            chord::stop_for(self.hwnd as isize);
        }
        self.stop_ticks_if_idle();
    }

    fn notice_hover(&mut self, on: bool) {
        let now = self.now_ms();
        if let Some(shown) = &mut self.shown {
            shown.clock.hover(now, on);
        }
    }

    fn notice_click(&mut self, lparam: LPARAM) {
        let point = notice::click_point(lparam);
        let on_undo = self
            .shown
            .as_ref()
            .and_then(|shown| shown.notice.as_ref())
            .is_some_and(|notice| notice.on_undo(point.x, point.y));
        if on_undo {
            self.undo();
        } else {
            self.close_notice();
        }
    }

    // ---- The pin list ----------------------------------------------------

    fn open_list(&mut self, select: Option<usize>) {
        self.refresh();
        if let Ok(mut shared) = self.shared.lock() {
            shared.list_open = true;
            shared.select = select;
        }
        host::show_window(ID, list::WINDOW_ID);
    }

    fn requests(&mut self) {
        let requests = match self.shared.lock() {
            Ok(mut shared) => shared.take_requests(),
            Err(_) => return,
        };
        for request in requests {
            self.request(request);
        }
    }

    fn request(&mut self, request: Request) {
        match request {
            Request::Refresh => self.refresh(),
            Request::Jump(slot) => {
                self.refresh();
                let front = unsafe { GetForegroundWindow() } as isize;
                self.jump(slot, front);
            }
            Request::Pin { slot, hwnd } => {
                self.refresh();
                match self.live.iter().find(|window| window.hwnd == hwnd).cloned() {
                    Some(window) => self.pin_window(slot, &window),
                    None => log::info!("QuickJump: the window to pin has closed"),
                }
            }
            Request::Move { from, to } => {
                if let Some(undo) = self.slots.move_to(from, to) {
                    let name = self
                        .slots
                        .get(to)
                        .map(|pin| pin.name().to_string())
                        .unwrap_or_default();
                    let message = notice::moved(&name, from, to);
                    self.save();
                    self.publish();
                    let text = message.text.clone();
                    self.notify(message, Some((undo, text)));
                }
            }
            Request::Remove(slot) => {
                if let Some((_, undo)) = self.slots.remove(slot) {
                    let message = notice::removed(slot);
                    self.save();
                    self.publish();
                    let text = message.text.clone();
                    self.notify(message, Some((undo, text)));
                }
            }
            Request::Update { slot, pin } => {
                let changed = self.slots.get(slot).is_some_and(|old| *old != pin);
                if !changed {
                    return;
                }
                let name = pin.name().to_string();
                let undo = self.slots.update(slot, |old| {
                    *old = Pin { hwnd: None, ..pin };
                });
                if let Some(undo) = undo {
                    self.save();
                    self.refresh();
                    let message = notice::changed(&name, slot);
                    let text = message.text.clone();
                    self.notify(message, Some((undo, text)));
                }
            }
        }
    }
}

unsafe extern "system" fn engine_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled = match msg {
        WM_APP_KEY => Some(with_engine(Engine::on_keys)),
        shared::WM_APP_REQUEST => Some(with_engine(Engine::requests)),
        notice::WM_APP_NOTICE_HOVER => Some(with_engine(|engine| engine.notice_hover(wparam != 0))),
        notice::WM_APP_NOTICE_CLICK => Some(with_engine(|engine| engine.notice_click(lparam))),
        WM_TIMER if wparam == chord::TIMER_ID => Some(with_engine(|engine| {
            chord::on_timer(engine.hwnd as isize);
            if engine.waiting.is_some() {
                log::info!("QuickJump: no digit came, the strip closed");
            }
            engine.undo_keys = false;
            engine.close_strip(false);
        })),
        WM_TIMER if wparam == TICK_TIMER => {
            with_engine(Engine::tick);
            Some(true)
        }
        _ => None,
    };
    match handled {
        // Busy: the message comes back once the engine is free.
        Some(false) if msg != WM_TIMER => {
            unsafe { PostMessageW(hwnd, msg, wparam, lparam) };
            0
        }
        Some(_) => 0,
        None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn create_window() -> Result<HWND, String> {
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let name = wide(&instance::name(CLASS_NAME));
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(engine_proc);
    class.hInstance = hinstance;
    class.lpszClassName = name.as_ptr();
    // Fails harmlessly with "class already exists" after a restart.
    unsafe { RegisterClassW(&class) };
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            name.as_ptr(),
            wide(NAME).as_ptr(),
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
        return Err("could not create its window".to_string());
    }
    Ok(hwnd)
}

impl WinCraftPlugin for QuickJump {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: ID,
            name: NAME,
            description: "Pin windows to the digits and jump back to one from anywhere.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
            category: "Windows",
            icon: Some("pin"),
            thumbnail: None,
        }
    }

    fn init(&mut self, _ctx: &HostContext) -> Result<(), String> {
        let hwnd = create_window()?;
        let path = state_path();
        let slots = std::fs::read_to_string(&path)
            .map(|text| Slots::from_json(&text))
            .unwrap_or_default();
        log::info!("QuickJump: {} pins read", slots.count());
        if let Ok(mut shared) = self.shared.lock() {
            shared.plugin_window = hwnd as isize;
        }
        let engine = Engine {
            hwnd,
            shared: self.shared.clone(),
            slots,
            path,
            jump_keys: String::new(),
            pin_keys: String::new(),
            waiting: None,
            closed_by_key: None,
            shown: None,
            undo_keys: false,
            starting: None,
            icons: HashMap::new(),
            live: Vec::new(),
            epoch: Instant::now(),
        };
        ENGINE.with(|cell| *cell.borrow_mut() = Some(engine));
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        let q = 0x51;
        vec![
            HotkeyAction {
                id: ACTION_JUMP,
                name: "jump",
                label: "Jump to a pinned window",
                default: Hotkey {
                    modifiers: MOD_WIN | MOD_ALT,
                    vk: q,
                },
            },
            HotkeyAction {
                id: ACTION_PIN,
                name: "pin",
                label: "Pin this window\u{2026}",
                default: Hotkey {
                    modifiers: MOD_WIN | MOD_ALT | MOD_SHIFT,
                    vk: q,
                },
            },
        ]
    }

    fn on_hotkeys_bound(&mut self, keys: &[(u32, String)]) {
        let text = |id: u32| {
            keys.iter()
                .find(|(action, _)| *action == id)
                .map(|(_, keys)| keys.clone())
                .unwrap_or_default()
        };
        with_engine(|engine| {
            engine.jump_keys = text(ACTION_JUMP);
            engine.pin_keys = text(ACTION_PIN);
            engine.refresh();
        });
    }

    fn on_hotkey(&mut self, action_id: u32) {
        with_engine(|engine| match action_id {
            ACTION_JUMP => engine.jump_pressed(),
            ACTION_PIN => engine.pin_pressed(),
            _ => {}
        });
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: TRAY_LIST,
            label: "Open pin list",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == TRAY_LIST {
            with_engine(|engine| engine.open_list(None));
        }
    }

    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        let count = self
            .shared
            .lock()
            .map(|shared| shared.view.slots.iter().filter(|v| v.is_pinned()).count())
            .unwrap_or(0);
        match (kind, action_id) {
            (ActionKind::Hotkey, ACTION_PIN) => {
                Some("Choose a slot for the window in front".to_string())
            }
            (ActionKind::Hotkey, ACTION_JUMP) => Some("Then press the slot's number".to_string()),
            (ActionKind::Tray, TRAY_LIST) => Some(matching::pinned_line(count)),
            _ => None,
        }
    }

    fn status(&self) -> Option<String> {
        let shared = self.shared.lock().ok()?;
        let count = shared.view.slots.iter().filter(|v| v.is_pinned()).count();
        let open = shared
            .view
            .slots
            .iter()
            .filter(|v| matches!(v.state, State::Open | State::InFront))
            .count();
        Some(format!(
            "{}, {open} of them open",
            matching::pinned_line(count)
        ))
    }

    fn search_providers(&self) -> Vec<Box<dyn SearchProvider>> {
        vec![Box::new(provider::Slots {
            shared: self.shared.clone(),
        })]
    }

    fn windows(&self) -> Vec<Box<dyn PluginWindow>> {
        vec![
            Box::new(list::PinList::new(self.shared.clone())),
            Box::new(picker::Picker::new(self.shared.clone())),
        ]
    }

    fn teardown(&mut self) {
        let engine = ENGINE.with(|cell| cell.borrow_mut().take());
        if let Some(mut engine) = engine {
            engine.close_strip(false);
            engine.close_notice();
            engine.starting = None;
            chord::stop_for(engine.hwnd as isize);
            unsafe {
                KillTimer(engine.hwnd, TICK_TIMER);
                DestroyWindow(engine.hwnd);
            }
        }
        if let Ok(mut shared) = self.shared.lock() {
            shared.plugin_window = 0;
            shared.list_open = false;
            shared.picker = None;
            shared.take_requests();
        }
    }
}

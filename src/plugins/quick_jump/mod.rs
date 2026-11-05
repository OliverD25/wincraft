//! QuickJump: keep up to ten windows in a numbered list and jump back to
//! one from anywhere, like Harpoon in Neovim. Win+`, then a digit, jumps;
//! Win+`, then A, adds the window in front; Win+` twice opens Quick access.
//!
//! The plugin keeps a hidden window of its own: the next-key wait posts to
//! it, its timer runs the notice's clock, and the Quick access window on
//! the UI thread wakes it with requests. Everything else, the chord tip
//! among it, it gets from the host.

mod keys;
mod list;
mod live;
mod matching;
mod notice;
mod picker;
mod pictures;
mod pins;
mod provider;
mod shared;
mod spot;
mod wait;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_WIN;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, KillTimer, PostMessageW,
    RegisterClassW, SetTimer, HWND_MESSAGE, WM_APP, WM_TIMER, WNDCLASSW,
};

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, PluginWindow,
    SettingField, TrayAction, WinCraftPlugin,
};
use crate::core::ui_bridge::ActionKind;
use crate::core::{chord, chord_tip, config, host, instance, shortcuts, wide};
use crate::search::actions;
use crate::search::{Action, SearchProvider};
use serde_json::Value;

use matching::{find, slot_views, SlotView, State};
use notice::{Clock, Message, Notice};
use pins::{Pin, Slots, Undo};
use shared::{App, Handle, Request};
use wait::Mode;

pub const ID: &str = "quick_jump";
const NAME: &str = "QuickJump";
const CLASS_NAME: &str = "WinCraftQuickJump";

const ACTION_JUMP: u32 = 1;
const ACTION_PIN: u32 = 2;
const JUMP_ACTION: &str = "jump";
const PIN_ACTION: &str = "pin";
/// The defaults before 0.20.74. Win+` took the jump's place, and the pin
/// shortcut has no keys by default now: Win+` then Alt and a digit does it.
const OLD_JUMP_KEYS: &str = "Win+Alt+Q";
const JUMP_KEYS: &str = "Win+`";
const OLD_PIN_KEYS: &str = "Win+Alt+Shift+Q";
/// Written once the old defaults were looked at, so keys the user picks
/// later stay.
const MOVED_NOTE: &str = "keys_moved_0_20_74";
const TRAY_LIST: u32 = 1;
const PALETTE_ADD: u32 = 1;

/// Posted by the next-key wait.
const WM_APP_KEY: u32 = WM_APP + 40;
/// The notice's clock; only runs while a notice shows or an app is
/// starting.
const TICK_TIMER: usize = 0x514A_0001;
const TICK_MS: u32 = 50;
/// How long to wait for an app QuickJump started to open a window.
const START_WAIT: Duration = Duration::from_secs(15);
const START_POLL: Duration = Duration::from_millis(500);
/// A second jump press this soon after another key ended the wait still
/// opens Quick access; longer when the shortcut is a chord, whose own wait
/// comes first.
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

/// The stored jump key equal to the old default was never a choice, so it
/// takes the new default, and the old pin default goes; once. Returns
/// whether anything moved.
fn move_old_defaults(hotkeys: &mut BTreeMap<String, String>, settings: &mut Value) -> bool {
    if settings.get(MOVED_NOTE).and_then(Value::as_bool) == Some(true) {
        return false;
    }
    if let Some(object) = settings.as_object_mut() {
        object.insert(MOVED_NOTE.to_string(), Value::Bool(true));
    }
    let mut moved = false;
    for (action, old, new) in [
        (JUMP_ACTION, OLD_JUMP_KEYS, JUMP_KEYS),
        (PIN_ACTION, OLD_PIN_KEYS, ""),
    ] {
        if hotkeys
            .get(action)
            .is_some_and(|keys| shortcuts::same_text(keys, old))
        {
            hotkeys.insert(action.to_string(), new.to_string());
            moved = true;
        }
    }
    moved
}

/// What the wait after the shortcut is for, and the window it acts on.
struct Waiting {
    mode: Mode,
    /// The window in front when the wait began.
    front: isize,
    /// The window A, U and Alt with a digit act on.
    target: Option<matching::Live>,
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
    /// When and how the wait last ended because another key came.
    closed_by_key: Option<(Instant, Mode)>,
    shown: Option<Shown>,
    /// The Ctrl+Z wait runs for the notice.
    undo_keys: bool,
    starting: Option<Starting>,
    live: Vec<matching::Live>,
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
                pins: self.slots.iter().map(|(_, pin)| pin.clone()).collect(),
                windows: self.live.clone(),
                apps,
                jump_keys: self.jump_keys.clone(),
                pin_keys: self.pin_keys.clone(),
            };
        }
        host::plugin_changed();
    }

    fn start_ticks(&self) {
        unsafe { SetTimer(self.hwnd, TICK_TIMER, TICK_MS, None) };
    }

    fn stop_ticks_if_idle(&self) {
        if self.waiting.is_none() && self.shown.is_none() && self.starting.is_none() {
            unsafe { KillTimer(self.hwnd, TICK_TIMER) };
        }
    }

    // ---- The wait after the shortcut ------------------------------------

    fn jump_pressed(&mut self) {
        if !self.twice() {
            self.open_wait(Mode::Jump);
        }
    }

    fn pin_pressed(&mut self) {
        self.open_wait(Mode::Pin);
    }

    fn tick(&mut self) {
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

    /// Jumps to the pin at `slot`. Returns its name when the window is in
    /// front now; None when a notice says what happened instead.
    fn jump(&mut self, slot: usize, front: isize) -> Option<String> {
        let Some(pin) = self.slots.get(slot).cloned() else {
            log::info!("QuickJump: number {} is empty", pins::number_of_slot(slot));
            self.notify(notice::empty(slot, &self.jump_keys), None);
            return None;
        };
        match find(&pin, &self.live).map(|window| window.hwnd) {
            Some(hwnd) if hwnd == front => {
                log::info!("QuickJump: {} is in front already", pin.name());
                Some(pin.name().to_string())
            }
            Some(hwnd) => {
                log::info!("QuickJump: jump to number {}", pins::number_of_slot(slot));
                self.activate(hwnd);
                Some(pin.name().to_string())
            }
            None if pin.start => {
                self.start(slot, &pin);
                None
            }
            None => {
                self.notify(notice::not_open(pin.name(), slot), None);
                None
            }
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
            "QuickJump: starting {target} for number {}",
            pins::number_of_slot(slot)
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
        let notice = Notice::show(&message, &instance::name(notice::CLASS_NAME), self.hwnd);
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

    // ---- Quick access ----------------------------------------------------

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
            Request::Add(hwnd) => {
                self.refresh();
                match self.live.iter().find(|window| window.hwnd == hwnd).cloned() {
                    Some(window) => self.add_window(&window),
                    None => log::info!("QuickJump: the window to add has closed"),
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
            Request::Remove(slot) => self.unpin(slot),
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
            engine.undo_keys = false;
            engine.wait_timed_out();
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
            description:
                "Keep up to ten windows in a numbered list and jump back to one from anywhere.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
            category: "Windows",
            icon: Some("pin"),
            thumbnail: None,
        }
    }

    fn default_settings(&self) -> Value {
        serde_json::json!({ spot::CENTRE: true })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![SettingField {
            key: spot::CENTRE,
            label: "Open Quick access in the middle of the screen",
            help: "Turn off to open it where you last left it.",
            kind: FieldKind::Toggle,
        }]
    }

    fn on_settings_changed(&mut self, settings: &Value) -> bool {
        if let Ok(mut shared) = self.shared.lock() {
            shared.keep_spot = !spot::centre_on(settings);
        }
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        if let Ok(mut shared) = self.shared.lock() {
            shared.keep_spot = !spot::centre_on(ctx.settings);
            shared.spot = spot::load(&spot::path());
        }
        let hwnd = create_window()?;
        let path = state_path();
        let slots = Slots::load(&path, &config::plugins_dir().join(pins::BACKUP_NAME));
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
            live: Vec::new(),
            epoch: Instant::now(),
        };
        ENGINE.with(|cell| *cell.borrow_mut() = Some(engine));
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![
            HotkeyAction {
                id: ACTION_JUMP,
                name: JUMP_ACTION,
                label: "Jump to a pinned window",
                default: Hotkey {
                    modifiers: MOD_WIN,
                    vk: keys::BACKTICK,
                },
            },
            HotkeyAction {
                id: ACTION_PIN,
                name: PIN_ACTION,
                label: "Pin this window\u{2026}",
                default: Hotkey::NONE,
            },
        ]
    }

    fn migrate_hotkeys(&self, hotkeys: &mut BTreeMap<String, String>, settings: &mut Value) {
        if move_old_defaults(hotkeys, settings) {
            log::info!(
                "QuickJump: the old default {OLD_JUMP_KEYS} is now {JUMP_KEYS}; {OLD_PIN_KEYS} is free"
            );
        }
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

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: PALETTE_ADD,
            label: "Add a window to Quick access\u{2026}",
            hint: "",
            words: "pin quickjump",
            confirm: "",
            list: false,
            toggle: None,
        }]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id == PALETTE_ADD {
            with_engine(|engine| engine.refresh());
            host::show_window(ID, picker::WINDOW_ID);
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: TRAY_LIST,
            label: "Open quick access",
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
            .map(|shared| shared.view.slots.len())
            .unwrap_or(0);
        match (kind, action_id) {
            (ActionKind::Hotkey, ACTION_PIN) => {
                Some("Choose a number for the window in front".to_string())
            }
            (ActionKind::Hotkey, ACTION_JUMP) => Some("Then press the window's number".to_string()),
            (ActionKind::Tray, TRAY_LIST) => Some(matching::pinned_line(count)),
            (ActionKind::Palette, PALETTE_ADD) => {
                Some("Choose an open window for the end of the list".to_string())
            }
            _ => None,
        }
    }

    fn status(&self) -> Option<String> {
        let shared = self.shared.lock().ok()?;
        let count = shared.view.slots.len();
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
            engine.close_wait(false);
            chord_tip::hide(engine.hwnd as isize);
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

#[cfg(test)]
mod tests;

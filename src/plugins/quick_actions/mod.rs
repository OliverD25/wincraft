//! QuickActions: Windows tools in the palette, VS Code style. "Window:
//! Center", "Theme: Toggle Dark Mode" and the rest are palette commands of
//! this plugin; none has a hotkey until the user binds one with Ctrl+K.
//!
//! Every change shows WinCraft's notice with Undo (Principle 1). The
//! plugin keeps a hidden window of its own: the notice reports the mouse
//! to it, and its timer runs the notice's clock. Windows itself is only
//! reached through `system::System`, so the tests use a fake PC and a test
//! instance only logs what it would change.

mod actions;
mod audio;
mod geometry;
mod inspect;
mod network;
mod system;
mod toggles;
mod tools;

use std::cell::RefCell;
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, KillTimer, PostMessageW, RegisterClassW,
    SetTimer, HWND_MESSAGE, WM_TIMER, WNDCLASSW,
};

use crate::core::notice::{self, Clock, Message, Notice};
use crate::core::traits::{
    HostContext, PaletteCommand, PluginMetadata, TrayToggle, WinCraftPlugin,
};
use crate::core::ui_bridge::{ActionKind, CommandDetails};
use crate::core::{host, instance, wide};

use actions::{Awake, Outcome, Undo, ENTRIES, KEEP_AWAKE};
use system::{Live, ReadOnly, System, WM_APP_BIN_DONE};

pub const ID: &str = "quick_actions";
const NAME: &str = "QuickActions";
const CLASS_NAME: &str = "WinCraftQuickActions";
const NOTICE_CLASS: &str = "WinCraftQuickActionsNotice";

/// The tray menu's "Keep awake" tick.
const TRAY_KEEP_AWAKE: u32 = 1;

/// The notice's clock; runs only while a notice shows.
const TICK_TIMER: usize = 0x5141_0001;
const TICK_MS: u32 = 100;

#[derive(Default)]
pub struct QuickActions {
    /// Whether keep awake is on, for the tray tick and the palette, which
    /// the host asks without a way into the engine's state.
    awake_since: std::rc::Rc<std::cell::Cell<Option<Instant>>>,
}

impl QuickActions {
    pub fn new() -> Self {
        Self::default()
    }
}

struct Shown {
    notice: Option<Notice>,
    clock: Clock,
    undo: Option<(Undo, String)>,
}

struct Engine {
    hwnd: HWND,
    system: Box<dyn System>,
    awake: Awake,
    awake_since: std::rc::Rc<std::cell::Cell<Option<Instant>>>,
    shown: Option<Shown>,
    epoch: Instant,
    /// The UUID the inspector showed, which Copy New UUID copies.
    next_uuid: Option<String>,
    /// What the Recycle Bin held while it empties in the background.
    emptying: Option<String>,
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// Runs `f` on the engine; false when it is busy, as when a message
/// arrives while the engine is working.
fn with_engine(f: impl FnOnce(&mut Engine)) -> bool {
    ENGINE.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            if let Some(engine) = slot.as_mut() {
                f(engine);
            }
            true
        }
        Err(_) => false,
    })
}

/// Like `with_engine`, for a call that answers; None when the engine is
/// off or busy.
fn ask_engine<R>(f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
    ENGINE.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

impl Engine {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn run(&mut self, id: u32) {
        let Some(entry) = actions::entry(id) else {
            return;
        };
        let now = self.now_ms();
        let outcome = actions::run(
            entry.command,
            self.system.as_mut(),
            &mut self.awake,
            &mut self.next_uuid,
            now,
        );
        if entry.command == actions::Command::Tool(tools::Tool::EmptyBin) {
            if let Some(outcome) = outcome
                .as_ref()
                .filter(|o| o.glyph == notice::Glyph::Loader)
            {
                self.emptying = outcome.detail.clone();
            }
        }
        match outcome {
            Some(outcome) => self.after(outcome),
            None => self.close_notice(),
        }
    }

    fn details(&mut self, id: u32) -> Option<CommandDetails> {
        let entry = actions::entry(id)?;
        let now = self.now_ms();
        Some(inspect::details(
            entry.command,
            self.system.as_ref(),
            &self.awake,
            &mut self.next_uuid,
            now,
        ))
    }

    fn bin_done(&mut self, ok: bool) {
        let what = self.emptying.take().unwrap_or_default();
        self.after(tools::bin_finished(ok, &what));
    }

    fn undo(&mut self) {
        let Some((undo, what)) = self.shown.as_mut().and_then(|shown| shown.undo.take()) else {
            return;
        };
        let now = self.now_ms();
        let outcome = actions::undo(undo, &what, self.system.as_mut(), &mut self.awake, now);
        self.after(outcome);
    }

    fn after(&mut self, outcome: Outcome) {
        let was = self.awake_since.get().is_some();
        if was != self.awake.is_on() {
            self.awake_since.set(self.awake.is_on().then(Instant::now));
        }
        host::plugin_changed();
        self.notify(outcome);
    }

    fn notify(&mut self, outcome: Outcome) {
        self.close_notice();
        let mut message = Message::new(NAME, outcome.glyph, outcome.text.clone());
        if let Some(detail) = outcome.detail {
            message = message.detail(detail);
        }
        if outcome.undo.is_some() {
            message = message.with_undo();
        }
        let notice = Notice::show(&message, &instance::name(NOTICE_CLASS), self.hwnd);
        let undo = outcome.undo.map(|undo| (undo, outcome.text));
        self.shown = Some(Shown {
            notice,
            clock: Clock::new(self.now_ms(), undo.is_some()),
            undo,
        });
        unsafe { SetTimer(self.hwnd, TICK_TIMER, TICK_MS, None) };
    }

    fn close_notice(&mut self) {
        self.shown = None;
        unsafe { KillTimer(self.hwnd, TICK_TIMER) };
    }

    fn tick(&mut self) {
        let now = self.now_ms();
        if self
            .shown
            .as_ref()
            .is_none_or(|shown| shown.clock.expired(now))
        {
            self.close_notice();
        }
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

    /// Ends keep awake, as when the plugin is switched off or WinCraft quits.
    fn stop(&mut self) {
        if self.awake.is_on() {
            if let Err(reason) = self.system.keep_awake(false) {
                log::warn!("QuickActions: could not end keep awake: {reason}");
            }
            self.awake = Awake::default();
            self.awake_since.set(None);
            log::info!("QuickActions: keep awake ended");
        }
        self.close_notice();
    }
}

unsafe extern "system" fn engine_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let handled = match msg {
        notice::WM_APP_NOTICE_HOVER => Some(with_engine(|engine| engine.notice_hover(wparam != 0))),
        notice::WM_APP_NOTICE_CLICK => Some(with_engine(|engine| engine.notice_click(lparam))),
        WM_APP_BIN_DONE => Some(with_engine(|engine| engine.bin_done(wparam != 0))),
        WM_TIMER if wparam == TICK_TIMER => {
            with_engine(Engine::tick);
            Some(true)
        }
        _ => None,
    };
    match handled {
        // Busy: the message comes back once the engine is free.
        Some(false) => {
            unsafe { PostMessageW(hwnd, msg, wparam, lparam) };
            0
        }
        Some(true) => 0,
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

impl QuickActions {
    fn awake_line(&self) -> Option<String> {
        self.awake_since
            .get()
            .map(|since| actions::awake_line(since.elapsed().as_millis() as u64))
    }
}

impl WinCraftPlugin for QuickActions {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: ID,
            name: NAME,
            description:
                "Windows tools in the palette: window size, dark mode, keep awake and more.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
            category: "System",
            icon: Some("settings"),
            thumbnail: None,
        }
    }

    fn init(&mut self, _ctx: &HostContext) -> Result<(), String> {
        let hwnd = create_window()?;
        let live = Live {
            owner: hwnd as isize,
        };
        let system: Box<dyn System> = if instance::is_test() {
            Box::new(ReadOnly::new(live))
        } else {
            Box::new(live)
        };
        self.awake_since.set(None);
        let engine = Engine {
            hwnd,
            system,
            awake: Awake::default(),
            awake_since: self.awake_since.clone(),
            shown: None,
            epoch: Instant::now(),
            next_uuid: None,
            emptying: None,
        };
        ENGINE.with(|cell| *cell.borrow_mut() = Some(engine));
        Ok(())
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        ENTRIES
            .iter()
            .map(|entry| PaletteCommand {
                id: entry.id,
                label: entry.label,
                hint: "",
                words: entry.words,
                confirm: entry.confirm,
            })
            .collect()
    }

    fn on_palette_command(&mut self, id: u32) {
        with_engine(|engine| engine.run(id));
    }

    fn command_details(&mut self, kind: ActionKind, action_id: u32) -> Option<CommandDetails> {
        if kind != ActionKind::Palette {
            return None;
        }
        ask_engine(|engine| engine.details(action_id)).flatten()
    }

    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        if kind != ActionKind::Palette {
            return None;
        }
        if action_id == KEEP_AWAKE {
            if let Some(line) = self.awake_line() {
                return Some(format!("{line} \u{b7} Enter turns it off"));
            }
        }
        actions::entry(action_id).map(|entry| entry.subtitle.to_string())
    }

    fn tray_toggles(&self) -> Vec<TrayToggle> {
        vec![TrayToggle {
            id: TRAY_KEEP_AWAKE,
            label: "Keep awake",
            on: self.awake_since.get().is_some(),
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == TRAY_KEEP_AWAKE {
            with_engine(|engine| engine.run(KEEP_AWAKE));
        }
    }

    fn status(&self) -> Option<String> {
        Some(match self.awake_line() {
            Some(line) => format!("Keep awake: {}", line.to_lowercase()),
            None => format!(
                "{} actions in the palette \u{b7} Keep awake is off",
                ENTRIES.len()
            ),
        })
    }

    fn page_action(&self) -> Option<u32> {
        Some(KEEP_AWAKE)
    }

    fn teardown(&mut self) {
        let engine = ENGINE.with(|cell| cell.borrow_mut().take());
        if let Some(mut engine) = engine {
            engine.stop();
            unsafe { DestroyWindow(engine.hwnd) };
        }
        self.awake_since.set(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_lists_every_action_with_its_words_and_subtitle() {
        let plugin = QuickActions::new();
        let commands = plugin.palette_commands();
        assert_eq!(commands.len(), ENTRIES.len());
        for command in &commands {
            assert!(!command.words.is_empty(), "{}", command.label);
            let subtitle = plugin
                .palette_subtitle(ActionKind::Palette, command.id)
                .expect("a subtitle");
            assert!(!subtitle.is_empty());
        }
        assert!(plugin.palette_subtitle(ActionKind::Hotkey, 1).is_none());
        assert!(plugin.palette_subtitle(ActionKind::Palette, 999).is_none());
        assert!(plugin.hotkey_actions().is_empty(), "no default hotkeys");
    }

    #[test]
    fn keep_awake_shows_in_the_tray_status_and_palette() {
        let plugin = QuickActions::new();
        assert!(!plugin.tray_toggles()[0].on);
        assert!(plugin
            .status()
            .expect("a status")
            .contains("Keep awake is off"));
        plugin.awake_since.set(Some(Instant::now()));
        assert!(plugin.tray_toggles()[0].on);
        assert_eq!(
            plugin.status().as_deref(),
            Some("Keep awake: on for less than a minute")
        );
        assert_eq!(
            plugin
                .palette_subtitle(ActionKind::Palette, KEEP_AWAKE)
                .as_deref(),
            Some("On for less than a minute \u{b7} Enter turns it off")
        );
        assert_eq!(plugin.page_action(), Some(KEEP_AWAKE));
    }

    #[test]
    fn the_metadata_names_the_plugin_and_an_icon_wincraft_draws() {
        let meta = QuickActions::new().metadata();
        assert_eq!(meta.id, ID);
        assert_eq!(meta.name, "QuickActions");
        assert!(crate::ui::widgets::icons::Icon::named(meta.icon.expect("an icon")).is_some());
    }
}

//! ClaudeDeck: a small window that lists the user's Claude Code chats,
//! grouped by project, with forks under the chat they came from and a mark
//! for what each chat is doing. It only reads: a click or Enter brings a
//! chat's own window to the front, nothing else (Principle 1).
//!
//! The list is drawn on the UI thread (`window.rs`). While it is open it
//! asks the host every few seconds to read the chats again
//! (`HostRequest::ClaudeDeckRefresh`); when it is closed nothing runs.

mod arrange;
mod data;
mod drop;
mod edges;
mod edit;
mod emit;
mod fade;
mod fork;
mod forks;
mod header;
mod hover;
mod keys;
mod labels;
mod list;
mod marks;
mod menu;
mod mini;
mod model;
mod modes;
mod place;
mod quick;
mod rows;
mod select;
mod servers;
mod status;
mod store;
mod taskbar;
mod tree;
mod window;

use std::sync::{Arc, Mutex};

use serde_json::Value;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, PluginWindow,
    SettingField, TrayAction, WinCraftPlugin,
};
use crate::core::ui_bridge::ArrangeAction;
use crate::core::ui_bridge::{ActionKind, ListRow, SubList};
use crate::core::{claude_windows, host, instance, window_actions};

const ID: &str = "claude_deck";
const ACTION_OPEN: u32 = 1;
const ACTION_MINI: u32 = 2;
/// The palette command that lists the servers to open a terminal on.
const COMMAND_TERMINAL: u32 = 1;
/// The palette command that opens the window with its quick settings.
const COMMAND_QUICK: u32 = 2;
/// The `host::post_to_plugins` id the window sends when the tray count
/// changed.
pub(crate) const TRAY_POST_ID: usize = 0xC1A0_DEC0;

pub(crate) const STAY_ON_TOP_KEY: &str = "stay_on_top";
pub(crate) const ONLY_OPEN_KEY: &str = "only_open_windows";
const FOLD_COLD_KEY: &str = "fold_cold_chats";
const MARK_KEY: &str = "status_mark";
const TIME_KEY: &str = "show_time";
const PROJECT_KEY: &str = "show_project";
const JOBS_KEY: &str = "show_jobs";
const MODEL_KEY: &str = "show_model";
const TEXT_KEY: &str = "show_text";
const FADE_KEY: &str = "fade_when_away";
const TRAY_KEY: &str = "tray_count";
const SERVERS_KEY: &str = "servers";
/// Not on the settings page: the window's mini mode, kept across restarts.
pub(crate) const MINI_KEY: &str = "mini_mode";

const MARK_SHAPES: &str = "Shapes";
const MARK_WORDS: &str = "Shapes and words";
const MARK_OFF: &str = "Off";

/// A test instance opens the window at start with this flag, so a scene
/// can show it without a hotkey.
const SCENE_FLAG: &str = "--open-claude-deck";
/// The same in mini mode, which is not saved.
const SCENE_MINI_FLAG: &str = "--open-claude-deck=mini";

/// How the status mark is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkStyle {
    #[default]
    Shapes,
    ShapesAndWords,
    Off,
}

/// The settings, as the window reads them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub stay_on_top: bool,
    pub only_open: bool,
    pub fold_cold: bool,
    pub mark: MarkStyle,
    pub time: bool,
    pub project: bool,
    pub jobs: bool,
    pub model: bool,
    /// The line of text under the name.
    pub text: bool,
    pub fade: bool,
    pub tray_count: bool,
    /// The narrow column of marks instead of the list.
    pub mini: bool,
    /// The ssh names of the servers to read; empty is off.
    pub servers: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            stay_on_top: true,
            only_open: false,
            fold_cold: true,
            mark: MarkStyle::Shapes,
            time: true,
            project: true,
            jobs: true,
            model: false,
            text: true,
            fade: true,
            tray_count: true,
            mini: false,
            servers: Vec::new(),
        }
    }
}

impl Settings {
    fn read(settings: &Value) -> Self {
        let defaults = Settings::default();
        let flag = |key: &str, default: bool| {
            settings
                .get(key)
                .and_then(Value::as_bool)
                .unwrap_or(default)
        };
        let mark = match settings.get(MARK_KEY).and_then(Value::as_str) {
            Some(MARK_WORDS) => MarkStyle::ShapesAndWords,
            Some(MARK_OFF) => MarkStyle::Off,
            _ => MarkStyle::Shapes,
        };
        Settings {
            stay_on_top: flag(STAY_ON_TOP_KEY, defaults.stay_on_top),
            only_open: flag(ONLY_OPEN_KEY, defaults.only_open),
            fold_cold: flag(FOLD_COLD_KEY, defaults.fold_cold),
            mark,
            time: flag(TIME_KEY, defaults.time),
            project: flag(PROJECT_KEY, defaults.project),
            jobs: flag(JOBS_KEY, defaults.jobs),
            model: flag(MODEL_KEY, defaults.model),
            text: flag(TEXT_KEY, defaults.text),
            fade: flag(FADE_KEY, defaults.fade),
            tray_count: flag(TRAY_KEY, defaults.tray_count),
            mini: flag(MINI_KEY, defaults.mini),
            servers: servers::parse_servers(
                settings
                    .get(SERVERS_KEY)
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
        }
    }
}

/// What the plugin and its window share.
#[derive(Default)]
pub struct Shared {
    pub open: bool,
    pub settings: Settings,
    /// The hotkey opened the window: the window that was in front then,
    /// which Esc gives the keyboard back to.
    pub by_hotkey: Option<isize>,
    /// What the tray tooltip should say; the plugin sets it on the host
    /// thread.
    pub tray_tip: Option<String>,
    /// The next `open` only brings the window forward: the mini mode
    /// hotkey never closes it.
    pub keep_open: bool,
    /// The palette asked for the quick settings menu.
    pub quick_wanted: bool,
}

pub type Handle = Arc<Mutex<Shared>>;

#[derive(Default)]
pub struct ClaudeDeck {
    shared: Handle,
    /// The tooltip this plugin set last, so it only takes back its own.
    tip_set: Option<String>,
}

impl ClaudeDeck {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the window. A second press brings it to the front, or closes
    /// it when it is already in front (the window decides, on its thread).
    fn open(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.open = true;
        }
        host::show_window(ID, window::WINDOW_ID);
    }

    /// Sets the tray tooltip to what the window asked for, when it changed.
    fn show_tip(&mut self) {
        let wanted = self
            .shared
            .lock()
            .ok()
            .and_then(|shared| shared.tray_tip.clone());
        if wanted != self.tip_set {
            self.tip_set = wanted.clone();
            host::set_tray_tip(wanted);
        }
    }

    fn servers(&self) -> Vec<String> {
        self.shared
            .lock()
            .map(|shared| shared.settings.servers.clone())
            .unwrap_or_default()
    }

    fn apply(&mut self, settings: &Value) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.settings = Settings::read(settings);
        }
    }
}

const TERMINAL_COMMAND: &str = "ClaudeDeck: Open a terminal on a server";

/// The palette's rows for "Open a terminal on a server": one per listed
/// server, "ClaudeDeck: Open a terminal on home2".
fn terminal_list(servers: &[String]) -> SubList {
    SubList {
        title: "Open a terminal on".to_string(),
        rows: servers
            .iter()
            .map(|server| ListRow {
                id: server.clone(),
                title: format!(
                    "ClaudeDeck: Open a terminal on {}",
                    servers::group_name(server)
                ),
                detail: format!("ssh {server}, in the home folder"),
                ..ListRow::default()
            })
            .collect(),
        empty: "No servers. List them in ClaudeDeck's settings, under Homelab.".to_string(),
    }
}

/// Whether a click may bring this window forward: it must be one of the
/// Claude windows the host listed last. A test copy never does it.
fn may_activate(hwnd: isize, listed: &[claude_windows::ChatWindow], test: bool) -> bool {
    !test && hwnd != 0 && listed.iter().any(|window| window.hwnd == hwnd)
}

impl WinCraftPlugin for ClaudeDeck {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: ID,
            name: "ClaudeDeck",
            description: "A small list of your Claude Code chats by project, with what each one is doing. A click brings a chat's window to the front.",
            author: "community",
            version: "0.1.0",
            readme: include_str!("README.md"),
            category: "Windows",
            icon: Some("sparkles"),
            thumbnail: None,
        }
    }

    fn default_settings(&self) -> Value {
        serde_json::json!({
            STAY_ON_TOP_KEY: true,
            ONLY_OPEN_KEY: false,
            FOLD_COLD_KEY: true,
            MARK_KEY: MARK_SHAPES,
            TIME_KEY: true,
            PROJECT_KEY: true,
            JOBS_KEY: true,
            MODEL_KEY: false,
            TEXT_KEY: true,
            FADE_KEY: true,
            TRAY_KEY: true,
            SERVERS_KEY: "",
        })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: STAY_ON_TOP_KEY,
                label: "Stay on top",
                help: "Keep ClaudeDeck above other windows. The pin in its header does the same.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: ONLY_OPEN_KEY,
                label: "Only open windows",
                help: "List only the chats that have a window open.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: FADE_KEY,
                label: "Fade when the mouse is away",
                help: "ClaudeDeck turns half see-through a few seconds after the mouse leaves it. It is solid again when the mouse comes back, when it has the keyboard, or when a chat starts to wait for you.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: FOLD_COLD_KEY,
                label: "Fold chats idle for over 1 day",
                help: "Old idle chats go into one grey line at the end.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: TRAY_KEY,
                label: "Show a count on the tray icon",
                help: "While ClaudeDeck is open, the tray icon's tooltip says how many chats need approval or ask you, for example \"WinCraft \u{2014} 2 chats wait for you\". Closed, ClaudeDeck reads nothing, so the tooltip is plain again.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: MARK_KEY,
                label: "Status mark",
                help: "A shape for each status, a shape and a short word, or no mark.",
                kind: FieldKind::Choice(&[MARK_SHAPES, MARK_WORDS, MARK_OFF]),
            },
            SettingField {
                key: TEXT_KEY,
                label: "A line of text (the question, or what it does)",
                help: "A second line under the name: the question when the chat waits, else what it does or what it said last. Resting the mouse on a chat shows all of it.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: TIME_KEY,
                label: "Time in the status",
                help: "How long the chat has been in its status, at the right edge.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: PROJECT_KEY,
                label: "Project and branch",
                help: "The chat's git branch after its name.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: JOBS_KEY,
                label: "Job count",
                help: "How many background jobs a chat runs.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: MODEL_KEY,
                label: "Model name",
                help: "The Claude model the chat uses.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: SERVERS_KEY,
                label: "Servers (ssh names)",
                help: "For example rdehome1, rdehome2. Empty: no servers are read. WinCraft reads them only while the ClaudeDeck window is open, every 30 s, and never changes anything on them.",
                kind: FieldKind::Text,
            },
        ]
    }

    fn settings_group(&self, key: &str) -> Option<&'static str> {
        match key {
            MARK_KEY | TEXT_KEY | TIME_KEY | PROJECT_KEY | JOBS_KEY | MODEL_KEY => {
                Some("On each row")
            }
            SERVERS_KEY => Some("Homelab"),
            _ => None,
        }
    }

    fn on_settings_changed(&mut self, settings: &Value) -> bool {
        self.apply(settings);
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.apply(ctx.settings);
        let scene = |flag: &str| std::env::args().skip(1).any(|arg| arg == flag);
        if instance::is_test() && scene(SCENE_MINI_FLAG) {
            if let Ok(mut shared) = self.shared.lock() {
                shared.settings.mini = true;
            }
        }
        if instance::is_test() && (scene(SCENE_FLAG) || scene(SCENE_MINI_FLAG)) {
            self.open();
        }
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![
            HotkeyAction {
                id: ACTION_OPEN,
                name: "open",
                label: "Open ClaudeDeck",
                default: Hotkey {
                    modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
                    vk: u32::from(b'X'),
                },
            },
            HotkeyAction {
                id: ACTION_MINI,
                name: "mini",
                label: "ClaudeDeck mini mode",
                default: Hotkey::NONE,
            },
        ]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        match action_id {
            ACTION_OPEN => {
                // The window in front now gets the keyboard back on Esc.
                let before = unsafe { GetForegroundWindow() } as isize;
                if let Ok(mut shared) = self.shared.lock() {
                    shared.by_hotkey = Some(before);
                }
                self.open();
            }
            ACTION_MINI => {
                let mini = match self.shared.lock() {
                    Ok(mut shared) => {
                        shared.settings.mini = !shared.settings.mini;
                        shared.keep_open = true;
                        Some(shared.settings.mini)
                    }
                    Err(_) => None,
                };
                if let Some(mini) = mini {
                    host::save_setting(ID, MINI_KEY, Value::Bool(mini));
                }
                self.open();
            }
            _ => {}
        }
    }

    fn on_windows_message(&mut self, msg: u32, wparam: usize, _lparam: isize) {
        if msg == host::PLUGIN_POST && wparam == TRAY_POST_ID {
            self.show_tip();
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: ACTION_OPEN,
            label: "ClaudeDeck",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == ACTION_OPEN {
            self.open();
        }
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![
            PaletteCommand {
                id: COMMAND_TERMINAL,
                label: TERMINAL_COMMAND,
                hint: "",
                words: "ssh server homelab shell",
                confirm: "",
                list: true,
                toggle: None,
            },
            PaletteCommand {
                id: COMMAND_QUICK,
                label: "ClaudeDeck: Quick settings",
                hint: "Fade, stay on top, mini mode and what each row shows",
                words: "options preferences fade opacity",
                confirm: "",
                list: false,
                toggle: None,
            },
        ]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id != COMMAND_QUICK {
            return;
        }
        if let Ok(mut shared) = self.shared.lock() {
            shared.quick_wanted = true;
            shared.keep_open = true;
        }
        self.open();
    }

    fn command_list(&mut self, kind: ActionKind, action_id: u32) -> Option<SubList> {
        (kind == ActionKind::Palette && action_id == COMMAND_TERMINAL)
            .then(|| terminal_list(&self.servers()))
    }

    fn on_list_pick(&mut self, kind: ActionKind, action_id: u32, row: &str) {
        let listed = self.servers().iter().any(|server| server == row);
        if kind == ActionKind::Palette && action_id == COMMAND_TERMINAL && listed {
            servers::open_terminal(row, None);
        }
    }

    fn page_action(&self) -> Option<u32> {
        Some(ACTION_OPEN)
    }

    fn windows(&self) -> Vec<Box<dyn PluginWindow>> {
        vec![Box::new(window::DeckWindow::new(self.shared.clone()))]
    }

    fn on_arrange_action(&mut self, action: &ArrangeAction) {
        let ArrangeAction::Activate(hwnd) = action else {
            return;
        };
        if instance::is_test() {
            log::info!("read-only test instance: switching to a Claude chat skipped");
            return;
        }
        if may_activate(*hwnd, &claude_windows::latest(), false) {
            window_actions::activate(*hwnd as HWND);
        }
    }

    fn teardown(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.open = false;
            shared.tray_tip = None;
        }
        self.show_tip();
    }
}

#[cfg(test)]
mod tests;

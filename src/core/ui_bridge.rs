use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::core::config::{SearchConfig, ThemeChoice, ThemeName};
use crate::core::tips::TipsConfig;
use crate::core::traits::FieldKind;

/// Posted to the host window when the UI thread has queued a request. The host
/// drains the channel inside its normal borrow, exactly like a tray click.
pub const WM_APP_UI: u32 = WM_APP + 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    General,
    Plugins,
    /// Every command's shortcut.
    Shortcuts,
    /// The windows LayoutKeeper has saved.
    Layout,
    /// The palette's bookmarks.
    Bookmarks,
    Store,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Hotkey,
    Tray,
    Palette,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostCommand {
    OpenSettings,
    OpenStore,
    OpenAbout,
    OpenLayout,
    OpenLog,
    OpenConfig,
    /// Check now, and install a newer release at once.
    UpdateWinCraft,
    TogglePlugin(usize),
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    Host(HostCommand),
    Plugin {
        index: usize,
        kind: ActionKind,
        action: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostSetting {
    StartWithWindows(bool),
    Theme(ThemeName),
    Mode(ThemeChoice),
    PaletteHotkey(String),
    /// The palette search settings, sent whole after one of them changed.
    Search(SearchConfig),
    CheckUpdatesDaily(bool),
    /// The tips' switch and what each tip has done, sent whole.
    Tips(TipsConfig),
}

/// One window in the Arrange strip.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeWindow {
    /// The window handle as a number: the strip draws its live picture from
    /// it and sends it back to say which window to act on.
    pub hwnd: isize,
    pub label: String,
    /// The id of the virtual desktop the window is on, as the registry spells
    /// it; empty when unknown or shown on every desktop.
    pub desktop: String,
    /// Which monitor the window is on ("Left", "Right"), the one it returns
    /// to when minimized; None on the primary monitor or with just one.
    pub monitor: Option<String>,
    /// The monitor the window is on now (for a minimized window, the one it
    /// returns to), as a place in the snapshot's `monitors`.
    pub on_monitor: Option<usize>,
    /// The monitor of the window in the last saved layout, as a place in
    /// `monitors`; None when the window has not been saved yet.
    pub saved_monitor: Option<usize>,
    /// The label is a name the user gave the window with WindowNamer.
    pub renamed: bool,
    /// Another window of the app has the same label, so a restore cannot
    /// tell them apart.
    pub same_title: bool,
    /// The window flashed for attention and was not activated since, so its
    /// taskbar button is orange. The host fills it in; plugins leave it
    /// false.
    pub attention: bool,
}

/// A monitor for the strip's columns: every connected one in the order
/// Windows lists them, then the disconnected ones that still have saved
/// windows.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeMonitor {
    /// The GDI device name, such as "\\.\DISPLAY1": a move names the
    /// target monitor by it.
    pub device: String,
    /// The column's name: its place ("Left", "Centre", "Right").
    pub label: String,
    /// The name the monitor gives itself, such as "DELL U2720Q"; can be empty.
    pub name: String,
    /// The whole screen in physical pixels; all zero when disconnected.
    pub rect: [i32; 4],
    /// The screen without the taskbar, the same way.
    pub work: [i32; 4],
    pub primary: bool,
    pub connected: bool,
}

/// How the strip groups a desktop's windows: one column per monitor, or one
/// wrapping row as before 0.13. Auto is By monitor with two or more
/// monitors connected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrangeView {
    #[default]
    Auto,
    ByMonitor,
    Flat,
}

impl ArrangeView {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "monitor" => Self::ByMonitor,
            "flat" => Self::Flat,
            _ => Self::Auto,
        }
    }

    pub fn setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ByMonitor => "monitor",
            Self::Flat => "flat",
        }
    }

    /// The view to draw with `connected` monitors: never Auto.
    pub fn resolve(self, connected: usize) -> Self {
        match self {
            Self::Auto if connected >= 2 => Self::ByMonitor,
            Self::Auto => Self::Flat,
            view => view,
        }
    }
}

/// One taskbar group, windows in thumbnail order. Windows groups buttons by
/// AppUserModelID, so one program can have several (Chrome and each of its
/// installed web apps).
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeGroup {
    pub key: String,
    pub label: String,
    pub windows: Vec<ArrangeWindow>,
}

impl ArrangeGroup {
    /// The group a scene asks for: by its place in the list ("0" is the
    /// first) or by its key, in any case.
    pub fn pick(groups: &[ArrangeGroup], wanted: &str) -> Option<usize> {
        let wanted = wanted.trim();
        if let Ok(index) = wanted.parse::<usize>() {
            return (index < groups.len()).then_some(index);
        }
        groups
            .iter()
            .position(|group| group.key.eq_ignore_ascii_case(wanted))
    }
}

/// A virtual desktop, in Task View order.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeDesktop {
    pub id: String,
    pub name: String,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeSnapshot {
    pub plugin: String,
    pub groups: Vec<ArrangeGroup>,
    pub desktops: Vec<ArrangeDesktop>,
    /// The group to show when the strip opens.
    pub focus: usize,
    /// The exe names being watched, for the strip's empty state.
    pub watched: Vec<String>,
    /// Whether resting the mouse on a card shows that window on its monitor.
    pub preview: bool,
    /// WindowNamer is on, so the card menu offers Rename.
    pub can_rename: bool,
    pub monitors: Vec<ArrangeMonitor>,
    /// The view the user picked last, kept in the plugin's settings.
    pub view: ArrangeView,
    /// The taskbar button the mouse rests on, when a hover opened the strip
    /// (left, top, right, bottom in physical pixels). The strip then sits
    /// above it and leaves the keyboard with the window in front.
    pub anchor: Option<[i32; 4]>,
}

static ARRANGE_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// The UI thread says which window is the Arrange strip, or 0 once it is
/// gone.
pub fn set_arrange_window(hwnd: isize) {
    ARRANGE_WINDOW.store(hwnd, Ordering::Relaxed);
}

/// The Arrange strip's window while it is on screen.
pub fn arrange_window() -> Option<isize> {
    Some(ARRANGE_WINDOW.load(Ordering::Relaxed)).filter(|hwnd| *hwnd != 0)
}

/// What the strip asks the plugin to do.
#[derive(Clone, Debug, PartialEq)]
pub enum ArrangeAction {
    Activate(isize),
    /// A taskbar group's windows in their new order.
    Reorder {
        group: String,
        order: Vec<isize>,
    },
    MoveToDesktop {
        hwnd: isize,
        desktop: String,
    },
    /// Move the window to the monitor with this device name now, keeping
    /// its size and place relative to the screen.
    MoveToMonitor {
        hwnd: isize,
        monitor: String,
    },
    Close(isize),
    /// Give the window a name, or take its name away with None. Names
    /// belong to WindowNamer, whichever plugin draws the strip.
    Rename {
        hwnd: isize,
        name: Option<String>,
    },
    /// Ask for a name for the window, in the palette.
    AskName(isize),
}

impl ArrangeAction {
    /// The host sends these to WindowNamer instead of the strip's plugin.
    pub fn is_about_names(&self) -> bool {
        matches!(
            self,
            ArrangeAction::Rename { .. } | ArrangeAction::AskName(_)
        )
    }
}

/// A saved window, named by its place in the plugin's list. `title` is the
/// saved title, checked before an edit so a stale list cannot change the
/// wrong window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutRef {
    pub exe: String,
    pub index: usize,
    pub title: String,
}

/// One saved window on the Layout page.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutEntry {
    pub entry: LayoutRef,
    /// What the window is called: its WindowNamer name when open and named.
    pub title: String,
    /// The saved desktop's id, as the registry spells it; empty when the
    /// window was not on a desktop of its own.
    pub desktop: String,
    /// The saved desktop no longer exists.
    pub desktop_removed: bool,
    pub monitor: Option<String>,
    /// The window handle when the window is open now.
    pub hwnd: Option<isize>,
    /// The day the window was last seen open, when it is closed.
    pub closed_since: Option<String>,
    /// Another saved window of the app has the same title.
    pub same_title: bool,
}

/// The saved windows of one taskbar group.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutApp {
    pub key: String,
    pub label: String,
    pub windows: Vec<LayoutEntry>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutSnapshot {
    pub plugin: String,
    /// "14:05", when this session saved at all.
    pub last_saved: Option<String>,
    pub desktops: Vec<ArrangeDesktop>,
    pub apps: Vec<LayoutApp>,
    /// WindowNamer is on, so an open window can be given a name.
    pub can_rename: bool,
}

/// What the Layout page asks the plugin to do.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutAction {
    /// Save another desktop for the window and, when it is open (`hwnd`),
    /// move it there now.
    SetDesktop {
        entry: LayoutRef,
        desktop: String,
        hwnd: Option<isize>,
    },
    /// Drop a closed window from the saved layout.
    Forget(LayoutRef),
    RestoreNow,
    SaveNow,
}

/// A search provider's question names its prompt `search:<provider id>`; the
/// palette answers it itself, with no round trip through the host.
pub const SEARCH_PROMPT: &str = "search:";

/// A question the palette asks for a plugin: one line of text, handed back
/// to the plugin's `on_prompt_answer` when the user presses Enter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prompt {
    /// The plugin that gets the answer. A search provider's question names
    /// it as `search:<provider id>` and is answered in the palette itself.
    pub plugin: String,
    /// The plugin's own number for the question.
    pub id: u32,
    /// What the answer is about, such as a window handle.
    pub target: isize,
    /// The chip before the box, such as "Rename"; also the Enter key's label.
    pub chip: String,
    pub placeholder: String,
    /// Put in the box and selected, so typing replaces it.
    pub text: String,
    /// The row under the box says what Enter does with the text typed, such
    /// as "Rename to" …
    pub action: String,
    /// … and with the box empty, or None when Enter then does nothing.
    pub empty_action: Option<String>,
    /// The row's second line.
    pub subtitle: String,
    /// What a search provider needs back with the answer; unused by plugins.
    pub payload: String,
}

impl Prompt {
    /// A search provider's question as a prompt.
    pub fn from_ask(ask: crate::search::Ask) -> Self {
        Self {
            plugin: format!("{SEARCH_PROMPT}{}", ask.provider),
            chip: ask.chip,
            placeholder: ask.placeholder,
            text: ask.text,
            action: ask.action,
            subtitle: ask.subtitle,
            payload: ask.payload,
            ..Default::default()
        }
    }
}

/// Screen rectangle in physical pixels, as Win32 reports it.
#[derive(Clone, Copy, Debug, Default)]
pub struct MonitorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub enum UiCommand {
    ShowPalette(MonitorRect),
    /// The palette as a one-line box that answers a plugin's question.
    ShowPrompt(Prompt, MonitorRect),
    ShowSettings(Page),
    /// Test instances only: a plugin's own page in the settings window.
    ShowPluginPage(String),
    /// The palette forgets every pick it recorded.
    ClearPaletteUsage,
    /// Test instances only: text put in the palette's box as if typed,
    /// prefix and all. Nothing runs.
    TypeInPalette(String),
    /// Test instances only: the strip shows the peek for one card, as if the
    /// mouse rested on it.
    PeekCard(PeekCard),
    ShowArrange(ArrangeSnapshot),
    /// A fresh list for the Arrange window, if it is still open.
    ArrangeUpdate(ArrangeSnapshot),
    /// The strip just opened for a taskbar button: pick the window with
    /// this number (from 1) and take the keyboard.
    ArrangeSelect(usize),
    /// A short line the open strip shows for a moment.
    ArrangeNote(String),
    /// The strip just opened for the window in front: pick its card.
    ArrangeSelectWindow(isize),
    HideArrange,
    /// The Layout page's data; None when no plugin keeps a layout.
    LayoutUpdate(Option<LayoutSnapshot>),
    HideAll,
    Snapshot(Box<UiSnapshot>),
    ThemeChanged,
    Quit,
}

/// Which card a test scene peeks at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeekCard {
    /// The card at this place, counting from 0 across the rows.
    Index(usize),
    /// The first card with a monitor chip, else the first card.
    Chip,
}

/// Which settings page a test scene opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsTarget {
    Page(Page),
    Plugin(String),
}

pub enum HostRequest {
    UiReady {
        palette_hwnd: isize,
    },
    RunCommand(CommandId),
    /// A palette row's action that needs the host thread.
    RunAction(crate::search::Action),
    /// The text typed in answer to a `Prompt`.
    PromptAnswer {
        plugin: String,
        id: u32,
        target: isize,
        text: String,
    },
    Arrange {
        plugin: String,
        action: ArrangeAction,
    },
    /// The Layout page wants fresh data.
    LayoutRefresh,
    Layout {
        plugin: String,
        action: LayoutAction,
    },
    /// The strip's own window, which only the host thread may bring forward
    /// right after the hotkey that opened it.
    FocusWindow(isize),
    /// A digit typed in the strip: the strip for that taskbar button.
    ArrangeDigit(u8),
    /// Settings' "Clear usage history": the palette forgets every pick.
    ClearPaletteUsage,
    SetPluginEnabled {
        id: String,
        enabled: bool,
    },
    SetHotkey {
        plugin: String,
        action: String,
        binding: String,
    },
    SetSetting {
        plugin: String,
        key: String,
        value: Value,
    },
    ResetPlugin(String),
    /// From the Keyboard shortcuts page, all at once: Replace changes two.
    SetShortcuts(Vec<ShortcutChange>),
    SetHostSetting(HostSetting),
    /// About's "Check for updates".
    CheckForUpdates,
    /// About's "Update now" for the newer release the host knows of.
    UpdateNow,
    OpenPath(PathBuf),
    Exit,
}

/// A new shortcut for one command: `Some("")` removes it, None puts the
/// default back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutChange {
    pub id: String,
    pub binding: Option<String>,
}

/// One row of the Keyboard shortcuts page: a command the palette lists.
#[derive(Clone, Debug, PartialEq)]
pub struct ShortcutInfo {
    /// Stable across restarts: "wincraft.open-settings",
    /// "screen_dimmer.toggle_monitor_1".
    pub id: String,
    /// None for the palette's own hotkey, which is not a palette command.
    pub command: Option<CommandId>,
    /// "WinCraft" or the plugin's name.
    pub group: String,
    pub plugin: Option<String>,
    /// The plugin's category, for its icon; empty for WinCraft.
    pub category: String,
    pub label: String,
    pub description: String,
    /// "Win+Alt+K, S"; empty for none.
    pub binding: String,
    pub default_binding: String,
    /// False when another app holds the first key press.
    pub registered: bool,
    /// The palette's own hotkey takes one key press only.
    pub one_press: bool,
}

#[derive(Clone, Debug)]
pub struct HotkeyInfo {
    pub action: String,
    pub label: String,
    pub binding: String,
    pub default_binding: String,
    pub registered: bool,
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub key: String,
    pub label: String,
    pub help: String,
    pub kind: FieldKind,
    pub value: Value,
}

#[derive(Clone, Debug, Default)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub readme: String,
    /// The word that groups the plugin, from its metadata.
    pub category: String,
    pub enabled: bool,
    pub config_path: PathBuf,
    pub hotkeys: Vec<HotkeyInfo>,
    pub fields: Vec<FieldInfo>,
    pub status: Option<String>,
    /// A button on the plugin's page: its label and what it runs.
    pub page_action: Option<(String, CommandId)>,
}

#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub id: CommandId,
    pub group: String,
    pub label: String,
    pub hint: String,
    /// The plugin is off: the entry is shown dimmed and opens the plugin's page.
    pub disabled: bool,
    pub plugin: Option<String>,
    /// A second line under the label, such as a monitor's state.
    pub subtitle: Option<String>,
    /// The command's id on the Keyboard shortcuts page, and its keys there.
    pub shortcut_id: String,
    pub shortcut: String,
}

/// Everything the windows draw. Rebuilt whole after every change: a plugin's
/// worth of it is well under a kilobyte, so diffing would cost more code than
/// it saves.
#[derive(Clone, Debug, Default)]
pub struct UiSnapshot {
    pub version: String,
    pub start_with_windows: bool,
    /// Set when this copy is not the installed one and so cannot change the
    /// logon task; shown in place of the setting's help text.
    pub autostart_note: Option<String>,
    pub theme: ThemeName,
    pub mode: ThemeChoice,
    pub palette_hotkey: String,
    pub palette_hotkey_registered: bool,
    pub config_path: PathBuf,
    pub log_path: PathBuf,
    pub plugins: Vec<PluginInfo>,
    pub commands: Vec<PaletteEntry>,
    pub shortcuts: Vec<ShortcutInfo>,
    pub search: SearchConfig,
    pub tips: TipsConfig,
    pub update: UpdateView,
}

/// What the About page and the tray menu know about updates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateView {
    pub check_daily: bool,
    pub status: UpdateStatus,
    /// A release newer than the running version.
    pub available: Option<NewRelease>,
    /// Why Update now cannot run in this copy; None when it can.
    pub blocked: Option<String>,
    /// "Update to 0.17.0 failed, kept 0.16.0", after a rollback.
    pub failed: Option<String>,
    /// The release notes of the installed version, Markdown, when they are
    /// known: About shows them under "What's new".
    pub installed_notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewRelease {
    pub tag: String,
    /// Markdown.
    pub notes: String,
    pub page: String,
}

/// The line beside About's "Check for updates".
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    UpToDate,
    NoRelease,
    Offline,
    /// Percent, when the server said how much there is.
    Downloading(Option<u8>),
    /// Swapping the files and restarting.
    Installing,
    Error(String),
}

/// Host → UI. The egui context arrives once the UI thread is up; until then
/// commands still queue, they are simply drawn on the next heartbeat.
pub struct UiChannel {
    tx: Sender<UiCommand>,
    ctx: Mutex<Option<egui::Context>>,
}

impl UiChannel {
    pub fn send(&self, command: UiCommand) {
        if self.tx.send(command).is_err() {
            return;
        }
        // A hidden eframe window only wakes every 100 ms, so without this the
        // palette would appear a tenth of a second after the hotkey.
        if let Ok(guard) = self.ctx.lock() {
            if let Some(ctx) = guard.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    pub fn attach(&self, ctx: egui::Context) {
        if let Ok(mut guard) = self.ctx.lock() {
            *guard = Some(ctx);
        }
    }
}

/// UI → host.
pub struct HostChannel {
    tx: Sender<HostRequest>,
    hwnd: AtomicIsize,
}

impl HostChannel {
    pub fn send(&self, request: HostRequest) {
        if self.tx.send(request).is_err() {
            return;
        }
        let hwnd = self.hwnd.load(Ordering::Relaxed);
        if hwnd != 0 {
            unsafe { PostMessageW(hwnd as HWND, WM_APP_UI, 0, 0) };
        }
    }

    pub fn set_host_window(&self, hwnd: HWND) {
        self.hwnd.store(hwnd as isize, Ordering::Relaxed);
    }
}

pub struct Bridge {
    pub to_ui: Arc<UiChannel>,
    pub to_host: Arc<HostChannel>,
    pub ui_rx: Receiver<UiCommand>,
    pub host_rx: Receiver<HostRequest>,
}

pub fn create() -> Bridge {
    let (ui_tx, ui_rx) = channel();
    let (host_tx, host_rx) = channel();
    Bridge {
        to_ui: Arc::new(UiChannel {
            tx: ui_tx,
            ctx: Mutex::new(None),
        }),
        to_host: Arc::new(HostChannel {
            tx: host_tx,
            hwnd: AtomicIsize::new(0),
        }),
        ui_rx,
        host_rx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(key: &str) -> ArrangeGroup {
        ArrangeGroup {
            key: key.to_string(),
            label: key.to_string(),
            windows: Vec::new(),
        }
    }

    #[test]
    fn auto_is_by_monitor_with_two_monitors_or_more() {
        assert_eq!(ArrangeView::Auto.resolve(1), ArrangeView::Flat);
        assert_eq!(ArrangeView::Auto.resolve(0), ArrangeView::Flat);
        assert_eq!(ArrangeView::Auto.resolve(2), ArrangeView::ByMonitor);
        assert_eq!(ArrangeView::Flat.resolve(3), ArrangeView::Flat);
        assert_eq!(ArrangeView::ByMonitor.resolve(1), ArrangeView::ByMonitor);
        for view in [ArrangeView::Auto, ArrangeView::ByMonitor, ArrangeView::Flat] {
            assert_eq!(ArrangeView::from_setting(view.setting()), view);
        }
        assert_eq!(ArrangeView::from_setting("sideways"), ArrangeView::Auto);
    }

    #[test]
    fn a_group_is_picked_by_place_or_by_key() {
        let groups = [
            group("Chrome.UserData.Profile3"),
            group("Telegram.TelegramDesktop"),
        ];
        assert_eq!(ArrangeGroup::pick(&groups, "0"), Some(0));
        assert_eq!(ArrangeGroup::pick(&groups, "1"), Some(1));
        assert_eq!(ArrangeGroup::pick(&groups, "2"), None);
        assert_eq!(
            ArrangeGroup::pick(&groups, "telegram.telegramdesktop"),
            Some(1)
        );
        assert_eq!(
            ArrangeGroup::pick(&groups, " Chrome.UserData.Profile3 "),
            Some(0)
        );
        assert_eq!(ArrangeGroup::pick(&groups, "Viber"), None);
        assert_eq!(ArrangeGroup::pick(&[], "0"), None);
    }
}

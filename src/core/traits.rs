use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};

use crate::core::config::PluginConfig;
use crate::core::ui_bridge::HostChannel;
use crate::core::ui_bridge::{
    ActionKind, ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeMonitor, ArrangeView,
    LayoutAction, LayoutSnapshot,
};
use crate::search::SearchProvider;

pub struct PluginMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub author: &'static str,
    pub version: &'static str,
    /// `include_str!("README.md")`, so a plugin without one fails to compile.
    pub readme: &'static str,
    /// One short word that groups the plugin on the Plugins page and in the
    /// store, such as "Windows" or "Input". Reuse a word another plugin
    /// already uses when it fits.
    pub category: &'static str,
    /// A 16:9 picture for the plugin's tile, as a path inside the repository.
    /// `None` shows the category's icon instead.
    pub thumbnail: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
}

pub struct HotkeyAction {
    pub id: u32,
    pub name: &'static str,
    pub label: &'static str,
    pub default: Hotkey,
}

pub struct TrayAction {
    pub id: u32,
    pub label: &'static str,
}

/// An on/off entry with a tick in the tray menu. Choosing it calls
/// `on_tray_action` with its id, which shares the tray ids' space.
pub struct TrayToggle {
    pub id: u32,
    pub label: &'static str,
    pub on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldKind {
    Toggle,
    Slider { min: f64, max: f64, step: f64 },
    Number { min: f64, max: f64 },
    Text,
    Choice(&'static [&'static str]),
    Path,
}

impl FieldKind {
    /// Listed on the About page so a plugin author can see what this build can
    /// draw without reading the source.
    pub const ALL: &'static [FieldKind] = &[
        FieldKind::Toggle,
        FieldKind::Slider {
            min: 0.0,
            max: 1.0,
            step: 0.05,
        },
        FieldKind::Number {
            min: 0.0,
            max: 100.0,
        },
        FieldKind::Text,
        FieldKind::Choice(&[]),
        FieldKind::Path,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            FieldKind::Toggle => "Toggle",
            FieldKind::Slider { .. } => "Slider",
            FieldKind::Number { .. } => "Number",
            FieldKind::Text => "Text",
            FieldKind::Choice(_) => "Choice",
            FieldKind::Path => "Path",
        }
    }
}

/// One editable option. The settings window draws the control from this, so a
/// plugin author never writes any UI code.
pub struct SettingField {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: FieldKind,
}

/// A setting whose key and text are only known while the plugin runs, such
/// as one per connected monitor. The key is a JSON pointer into the
/// plugin's settings ("/monitors/PHL 323E7/mode", each step made with
/// `config::pointer_token`), and the plugin says the current value itself.
pub struct DynamicField {
    pub key: String,
    pub label: String,
    pub help: String,
    pub kind: FieldKind,
    pub value: serde_json::Value,
}

pub struct PaletteCommand {
    pub id: u32,
    pub label: &'static str,
    pub hint: &'static str,
}

/// What a plugin that keeps a window order shows in the Arrange strip.
pub struct WindowGroups {
    pub groups: Vec<ArrangeGroup>,
    pub desktops: Vec<ArrangeDesktop>,
    /// The group to show first, usually the front window's program.
    pub focus: usize,
    pub watched: Vec<String>,
    pub monitors: Vec<ArrangeMonitor>,
}

/// How the Arrange strip looks, from the plugin that decides when it opens.
pub struct StripLook {
    /// Whether resting the mouse on a card shows that window on its monitor.
    pub preview: bool,
    /// The view the user picked last. The strip saves a new pick as this
    /// plugin's `arrange_view` setting.
    pub view: ArrangeView,
}

/// A window a plugin draws itself with egui, such as a list it keeps. It
/// lives on the UI thread, like a search provider: share data with the
/// plugin through an `Arc<Mutex<…>>`, and wake the plugin with a message to
/// a window of its own.
pub trait PluginWindow: Send {
    /// Unique within the plugin; `host::show_window` names it.
    fn id(&self) -> &'static str;

    /// The plugin asked for the window with `host::show_window`.
    fn open(&mut self);

    /// The plugin asked for the window to go, or was switched off.
    fn close(&mut self);

    fn is_open(&self) -> bool;

    /// Draws the window's own viewport, every frame while it is open.
    fn show(&mut self, ctx: &egui::Context, frame: &WindowFrame);
}

/// What the UI thread gives a plugin's window to draw with.
pub struct WindowFrame {
    pub icon: Option<std::sync::Arc<egui::IconData>>,
    /// The palette's window, which the search for a viewport's own window
    /// skips (see `ui::settings::find_window`).
    pub palette_hwnd: isize,
    /// For the requests only the host thread can do, such as bringing the
    /// window to the front right after a hotkey.
    pub to_host: std::sync::Arc<HostChannel>,
}

pub struct HostContext<'a> {
    pub hwnd: HWND,
    pub settings: &'a serde_json::Value,
}

pub trait WinCraftPlugin {
    fn metadata(&self) -> PluginMetadata;

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    /// Rewrites stored settings from an older version, before the plugin
    /// starts. Called at every start after missing keys were filled in from
    /// `default_settings`; the host saves the file afterwards.
    fn migrate_settings(&self, settings: &mut serde_json::Value) {
        let _ = settings;
    }

    /// Rewrites stored hotkeys from an older version, as `migrate_settings`
    /// does for settings: `hotkeys` maps each action's name to its keys as
    /// config text. A plugin that keeps a note that it already did this
    /// writes it to `settings`. The host saves the file afterwards.
    fn migrate_hotkeys(
        &self,
        hotkeys: &mut std::collections::BTreeMap<String, String>,
        settings: &mut serde_json::Value,
    ) {
        let _ = (hotkeys, settings);
    }

    /// The plugin this one was split out of, if any. The first time this
    /// plugin starts, when it has no settings file yet, `take_over` gets
    /// that plugin's stored settings and hotkeys.
    fn split_from(&self) -> Option<&'static str> {
        None
    }

    /// Moves what this plugin now owns out of `old`, the stored config of
    /// the `split_from` plugin, into `new`, and decides whether `new` starts
    /// on. Runs once, before `migrate_hotkeys`; the host saves both files.
    /// Returns whether anything was moved.
    fn take_over(&self, old: &mut PluginConfig, new: &mut PluginConfig) -> bool {
        let _ = (old, new);
        false
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String>;

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        Vec::new()
    }

    fn on_hotkey(&mut self, action_id: u32) {
        let _ = action_id;
    }

    /// The keys each hotkey action has, as the user set them ("Win+Alt+Q",
    /// "Win+Alt+K, J", or empty for none), right after `init`. A plugin
    /// that names its keys on screen keeps them; a change starts the plugin
    /// again, so this comes again too.
    fn on_hotkeys_bound(&mut self, keys: &[(u32, String)]) {
        let _ = keys;
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        let _ = (msg, wparam, lparam);
    }

    /// Explorer restarted and built a new taskbar. It is still being filled
    /// when this arrives, so wait a few seconds before touching its buttons.
    fn on_taskbar_created(&mut self) {}

    fn tray_actions(&self) -> Vec<TrayAction> {
        Vec::new()
    }

    fn on_tray_action(&mut self, action_id: u32) {
        let _ = action_id;
    }

    /// Ticked entries for the tray menu, shown after the plugin's first
    /// tray action. For a setting people switch often.
    fn tray_toggles(&self) -> Vec<TrayToggle> {
        Vec::new()
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        Vec::new()
    }

    /// Settings drawn after `settings_fields`, built from what the plugin
    /// sees now. Asked each time the settings window gets fresh data.
    fn dynamic_fields(&self) -> Vec<DynamicField> {
        Vec::new()
    }

    /// Called with the whole settings object after the user changes one value.
    /// Return true if the change was applied live; returning false tells the
    /// host to stop and start the plugin instead.
    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        let _ = settings;
        false
    }

    /// Extra palette entries. Hotkey and tray actions are listed automatically,
    /// so most plugins need nothing here.
    fn palette_commands(&self) -> Vec<PaletteCommand> {
        Vec::new()
    }

    fn on_palette_command(&mut self, id: u32) {
        let _ = id;
    }

    /// Extra sources of palette results, such as a list of bookmarks. Asked
    /// once at startup whether the plugin is on or not; the palette asks a
    /// provider for rows only while its plugin is on. The provider is moved
    /// to the UI thread and queried there on every keystroke.
    fn search_providers(&self) -> Vec<Box<dyn SearchProvider>> {
        Vec::new()
    }

    /// Windows the plugin draws itself, asked once at startup whether the
    /// plugin is on or not, and moved to the UI thread. `host::show_window`
    /// opens one; the host closes them all when the plugin is switched off.
    fn windows(&self) -> Vec<Box<dyn PluginWindow>> {
        Vec::new()
    }

    /// One line shown under the description on the plugin's page, for state
    /// the user should see without opening the log. Call
    /// `host::plugin_changed()` when it changes.
    fn status(&self) -> Option<String> {
        None
    }

    /// A second line for one of the plugin's palette entries, such as the
    /// state it would change. Asked each time the palette's list is rebuilt;
    /// call `host::plugin_changed()` when the answer changes.
    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        let _ = (kind, action_id);
        None
    }

    /// One of the plugin's hotkey actions, or failing that one of its
    /// palette commands, shown as a button on its page.
    fn page_action(&self) -> Option<u32> {
        None
    }

    /// Groups for the Arrange strip, for a plugin that keeps a window order,
    /// or that lists the windows itself while none is on. The host asks a
    /// plugin that keeps an order first (it is not the one that answers
    /// `strip_look`), when the strip opens and after every change;
    /// `host::open_arrange()` opens it from a plugin callback.
    fn window_groups(&mut self) -> Option<WindowGroups> {
        None
    }

    /// How the strip looks, for the plugin that decides when it opens. The
    /// strip opens only while a plugin that answers this is on.
    fn strip_look(&self) -> Option<StripLook> {
        None
    }

    /// The strip asked for something to be done with one window.
    fn on_arrange_action(&mut self, action: &ArrangeAction) {
        let _ = action;
    }

    /// The saved windows for the Layout page, for a plugin that keeps a
    /// layout. The host asks while the page is open.
    fn layout_snapshot(&mut self) -> Option<LayoutSnapshot> {
        None
    }

    /// The Layout page asked for something to be done.
    fn on_layout_action(&mut self, action: &LayoutAction) {
        let _ = action;
    }

    /// The answer to a question the plugin asked with `host::ask`: the
    /// prompt's id and target, and the text typed, which may be empty.
    fn on_prompt_answer(&mut self, prompt_id: u32, target: isize, text: &str) {
        let _ = (prompt_id, target, text);
    }

    fn teardown(&mut self) {}
}

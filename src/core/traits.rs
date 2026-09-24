use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};

use crate::core::ui_bridge::{ArrangeAction, ArrangeDesktop, ArrangeGroup};

pub struct PluginMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub author: &'static str,
    pub version: &'static str,
    /// `include_str!("README.md")`, so a plugin without one fails to compile.
    pub readme: &'static str,
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

    fn init(&mut self, ctx: &HostContext) -> Result<(), String>;

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        Vec::new()
    }

    fn on_hotkey(&mut self, action_id: u32) {
        let _ = action_id;
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        let _ = (msg, wparam, lparam);
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        Vec::new()
    }

    fn on_tray_action(&mut self, action_id: u32) {
        let _ = action_id;
    }

    fn settings_fields(&self) -> Vec<SettingField> {
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

    /// One line shown under the description on the plugin's page, for state
    /// the user should see without opening the log. Call
    /// `host::plugin_changed()` when it changes.
    fn status(&self) -> Option<String> {
        None
    }

    /// One of the plugin's hotkey actions, shown as a button on its page.
    fn page_action(&self) -> Option<u32> {
        None
    }

    /// Groups for the Arrange strip, for a plugin that keeps a window order.
    /// The host asks when the strip opens and after every change;
    /// `host::open_arrange()` opens it from a plugin callback.
    fn window_groups(&mut self) -> Option<WindowGroups> {
        None
    }

    /// The strip asked for something to be done with one window.
    fn on_arrange_action(&mut self, action: &ArrangeAction) {
        let _ = action;
    }

    fn teardown(&mut self) {}
}

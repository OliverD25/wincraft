use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};

pub struct ModuleMetadata {
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

pub struct HostContext<'a> {
    pub hwnd: HWND,
    pub settings: &'a serde_json::Value,
}

pub trait WinCraftModule {
    fn metadata(&self) -> ModuleMetadata;

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

    fn teardown(&mut self) {}
}

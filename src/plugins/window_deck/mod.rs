//! WindowDeck: all windows of an app in a row above the taskbar, to pick,
//! move and close them from the keyboard. The host draws the strip and
//! LayoutKeeper gives it the windows; WindowDeck decides when it opens
//! (Win+Alt+A, a rest on a taskbar button) and how it looks.

pub mod hover_preview;
mod win_digit;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_TIMER;

use crate::core::config::PluginConfig;
use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    StripLook, TrayAction, TrayToggle, WinCraftPlugin,
};
use crate::core::ui_bridge::{ActionKind, ArrangeView};
use crate::core::{host, instance, window_census};

pub const ID: &str = "window_deck";
/// The plugin the strip was part of until WindowDeck was split out.
const SPLIT_FROM: &str = "layout_keeper";

const ACTION_OPEN: u32 = 1;
/// A tray toggle; tray ids share their space with the action above.
const TRAY_HOVER_STRIP: u32 = 2;
const COMMAND_HOVER_STRIP: u32 = 1;

const HOVER_STRIP_KEY: &str = "taskbar_hover_strip";
const HOVER_DELAY_KEY: &str = "hover_delay_seconds";
const PREVIEW_KEY: &str = "preview_on_hover";
const VIEW_KEY: &str = "arrange_view";
const WIN_NUMBER_KEY: &str = "win_number_opens";
const HOTKEY_NAME: &str = "arrange";
const HOVER_STRIP_LABEL: &str = "WindowDeck on taskbar hover";

/// The settings and hotkey that lived in LayoutKeeper's file before the split.
const TAKEN_SETTINGS: [&str; 4] = [HOVER_STRIP_KEY, HOVER_DELAY_KEY, PREVIEW_KEY, VIEW_KEY];

pub struct WindowDeck {
    host_hwnd: Option<HWND>,
    preview_on_hover: bool,
    /// The strip's view, as the user last picked it there.
    view: ArrangeView,
    /// "Show WindowDeck instead of Windows' taskbar preview".
    hover_strip: bool,
    hover_delay_ms: u64,
    hover: hover_preview::HoverPreview,
    /// "Open WindowDeck with Win+1…9".
    win_number: bool,
    /// The switch is on but Windows refused the keyboard hook.
    hook_refused: bool,
    /// A test instance (WINCRAFT_INSTANCE) never hides Windows' preview
    /// unless it was started with `--allow-hide-popup`.
    read_only: bool,
}

impl Default for WindowDeck {
    fn default() -> Self {
        Self {
            host_hwnd: None,
            preview_on_hover: true,
            view: ArrangeView::Auto,
            hover_strip: false,
            hover_delay_ms: hover_preview::DEFAULT_DELAY_MS,
            hover: hover_preview::HoverPreview::default(),
            win_number: false,
            hook_refused: false,
            read_only: instance::is_test(),
        }
    }
}

impl WindowDeck {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_settings(&mut self, settings: &serde_json::Value) {
        self.preview_on_hover = settings
            .get(PREVIEW_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        self.view = ArrangeView::from_setting(
            settings
                .get(VIEW_KEY)
                .and_then(|value| value.as_str())
                .unwrap_or("auto"),
        );
        self.hover_strip = settings
            .get(HOVER_STRIP_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        self.hover_delay_ms =
            hover_preview::delay_ms(settings.get(HOVER_DELAY_KEY).and_then(|v| v.as_f64()));
        self.apply_hover();
        self.win_number = settings
            .get(WIN_NUMBER_KEY)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        self.apply_win_number();
    }

    /// Puts the Win+number hook in or takes it out to match the switch. The
    /// hook goes in only once the plugin runs on the host thread.
    fn apply_win_number(&mut self) {
        let wanted = self.win_number && self.host_hwnd.is_some();
        if wanted == win_digit::is_running() {
            return;
        }
        if !wanted {
            win_digit::stop();
            self.hook_refused = false;
            log::info!("Win+number: back to Windows");
            return;
        }
        self.hook_refused = !win_digit::start();
        if self.hook_refused {
            log::warn!(
                "Win+number: Windows refused the keyboard hook, so Win+number stays with Windows"
            );
        } else {
            log::info!("Win+number: opens WindowDeck for that taskbar button");
        }
        host::plugin_changed();
    }

    /// Starts or stops the hover strip to match the settings, at once.
    fn apply_hover(&mut self) {
        let commands = self
            .hover
            .set(self.host_hwnd, self.hover_strip, self.hover_delay_ms);
        run_hover(commands);
    }

    fn hover_tick(&mut self) {
        let commands = self.hover.tick(&window_census::on_taskbar);
        run_hover(commands);
    }

    /// The tray toggle and the palette command: flips the switch now and
    /// saves it, as the switch on the settings page would.
    fn toggle_hover(&mut self) {
        self.hover_strip = !self.hover_strip;
        self.apply_hover();
        host::save_setting(
            ID,
            HOVER_STRIP_KEY,
            serde_json::Value::Bool(self.hover_strip),
        );
    }
}

fn run_hover(commands: Vec<hover_preview::Command>) {
    for command in commands {
        match command {
            hover_preview::Command::Open { app, button } => host::open_arrange_at(app, button),
            hover_preview::Command::Close => host::close_arrange(),
            hover_preview::Command::Lookup(_) => {}
        }
    }
}

/// The palette entry's second line: what the switch is now.
fn hover_subtitle(on: bool) -> &'static str {
    if on {
        "Now on: apps with 2 or more windows show WindowDeck"
    } else {
        "Now off: Windows' own preview shows"
    }
}

/// Until 0.20.9 the hover delay's default was 0.4 s, so a saved 0.4 is
/// not a choice and takes the new default.
fn migrate_hover_delay(settings: &mut serde_json::Value) -> bool {
    let stored = settings
        .get(HOVER_DELAY_KEY)
        .and_then(|value| value.as_f64());
    if stored != Some(hover_preview::OLD_DEFAULT_DELAY_SECONDS) {
        return false;
    }
    settings[HOVER_DELAY_KEY] =
        serde_json::Value::from(hover_preview::DEFAULT_DELAY_MS as f64 / 1000.0);
    true
}

/// Moves the strip's settings and hotkey out of LayoutKeeper's stored
/// config into WindowDeck's, with the same values. WindowDeck starts on
/// when LayoutKeeper was on and the hover strip was on or the hotkey was
/// set, so nothing the user had goes away (Principle 1). A LayoutKeeper
/// file without any of them (a new install) changes nothing.
fn take_over_from_keeper(old: &mut PluginConfig, new: &mut PluginConfig) -> bool {
    let mut moved = false;
    if let Some(stored) = old.settings.as_object_mut() {
        if !new.settings.is_object() {
            new.settings = serde_json::json!({});
        }
        for key in TAKEN_SETTINGS {
            if let Some(value) = stored.remove(key) {
                new.settings[key] = value;
                moved = true;
            }
        }
    }
    let hotkey = old.hotkeys.remove(HOTKEY_NAME);
    if let Some(keys) = &hotkey {
        new.hotkeys.insert(HOTKEY_NAME.to_string(), keys.clone());
        moved = true;
    }
    if !moved {
        return false;
    }
    let hover_on = new.settings.get(HOVER_STRIP_KEY).and_then(|v| v.as_bool()) == Some(true);
    // A file without the hotkey used its default, so it was set.
    let hotkey_set = hotkey.is_none_or(|keys| !keys.trim().is_empty());
    new.enabled = old.enabled && (hover_on || hotkey_set);
    true
}

impl WinCraftPlugin for WindowDeck {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: ID,
            name: "WindowDeck",
            description: "Shows all windows of an app in a row above the taskbar, to pick, move and close them from the keyboard.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
            category: "Windows",
            thumbnail: None,
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({
            PREVIEW_KEY: true,
            VIEW_KEY: ArrangeView::Auto.setting(),
            HOVER_STRIP_KEY: false,
            HOVER_DELAY_KEY: hover_preview::DEFAULT_DELAY_MS as f64 / 1000.0,
            WIN_NUMBER_KEY: false,
        })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: PREVIEW_KEY,
                label: "Preview window on hover",
                help: "Resting the mouse on a picture in WindowDeck shows that window on its monitor.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: HOVER_STRIP_KEY,
                label: "Show WindowDeck instead of Windows' taskbar preview",
                help: "Resting the mouse on the taskbar button of an app with 2 or more windows opens WindowDeck above it, where you can drag the windows into order. Apps with one window keep Windows' own preview. Win+number and Alt+Tab work as before.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: HOVER_DELAY_KEY,
                label: "Hover delay (seconds)",
                help: "How long the mouse rests on a taskbar button before WindowDeck opens.",
                kind: FieldKind::Slider {
                    min: 0.05,
                    max: 1.5,
                    step: 0.05,
                },
            },
            SettingField {
                key: WIN_NUMBER_KEY,
                label: "Open WindowDeck with Win+1\u{2026}9",
                help: "Win and a number open WindowDeck for that taskbar button, even for an app with one window; press the number again with Win still held to pick the next window. A pinned app that is not running starts as before. Win+Shift, Win+Ctrl and Win+Alt with a number stay with Windows.",
                kind: FieldKind::Toggle,
            },
        ]
    }

    fn migrate_settings(&self, settings: &mut serde_json::Value) {
        if migrate_hover_delay(settings) {
            log::info!(
                "hover_delay_seconds was the old default {}; now {}",
                hover_preview::OLD_DEFAULT_DELAY_SECONDS,
                hover_preview::DEFAULT_DELAY_MS as f64 / 1000.0
            );
        }
    }

    fn split_from(&self) -> Option<&'static str> {
        Some(SPLIT_FROM)
    }

    fn take_over(&self, old: &mut PluginConfig, new: &mut PluginConfig) -> bool {
        take_over_from_keeper(old, new)
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.apply_settings(settings);
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.hover.may_hide = !self.read_only || host::allow_hide_popup();
        self.host_hwnd = Some(ctx.hwnd);
        self.apply_settings(ctx.settings);
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![HotkeyAction {
            id: ACTION_OPEN,
            name: HOTKEY_NAME,
            label: "WindowDeck: Open",
            default: Hotkey {
                modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
                vk: u32::from(b'A'),
            },
        }]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        if action_id == ACTION_OPEN {
            host::arrange_hotkey();
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: ACTION_OPEN,
            label: "WindowDeck: Open",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        match action_id {
            ACTION_OPEN => host::open_arrange(),
            TRAY_HOVER_STRIP => self.toggle_hover(),
            _ => {}
        }
    }

    fn tray_toggles(&self) -> Vec<TrayToggle> {
        vec![TrayToggle {
            id: TRAY_HOVER_STRIP,
            label: HOVER_STRIP_LABEL,
            on: self.hover_strip,
        }]
    }

    fn page_action(&self) -> Option<u32> {
        Some(ACTION_OPEN)
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: COMMAND_HOVER_STRIP,
            label: "WindowDeck: Switch taskbar hover on or off",
            hint: "",
        }]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id == COMMAND_HOVER_STRIP {
            self.toggle_hover();
        }
    }

    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        (kind == ActionKind::Palette && action_id == COMMAND_HOVER_STRIP)
            .then(|| hover_subtitle(self.hover_strip).to_string())
    }

    fn status(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.hook_refused {
            parts.push(
                "Win+1\u{2026}9: Windows refused WinCraft's keyboard hook, so Win+number stays with Windows"
                    .to_string(),
            );
        }
        if self.hover.is_on() {
            parts.push(if self.hover.may_hide {
                "WindowDeck on taskbar hover: on".to_string()
            } else {
                "WindowDeck on taskbar hover: on, but Windows' preview is left alone in this test instance".to_string()
            });
        }
        (!parts.is_empty()).then(|| parts.join(" \u{b7} "))
    }

    fn strip_look(&self) -> Option<StripLook> {
        Some(StripLook {
            preview: self.preview_on_hover,
            view: self.view,
        })
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        let _ = lparam;
        if msg == WM_TIMER && wparam == hover_preview::TIMER_ID {
            self.hover_tick();
        }
    }

    fn teardown(&mut self) {
        run_hover(self.hover.stop());
        win_digit::stop();
        self.hook_refused = false;
        // Off means the strip goes too, however it was opened.
        host::close_arrange();
        self.host_hwnd = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_deck() -> WindowDeck {
        WindowDeck {
            read_only: true,
            ..WindowDeck::default()
        }
    }

    #[test]
    fn the_palette_usage_renames_point_at_windowdecks_real_commands() {
        use crate::core::ui_bridge::{CommandId, PaletteEntry};
        use crate::search::providers::commands::{command_key, WINDOW_DECK_RENAMES};
        let deck = test_deck();
        let key = |kind: ActionKind, action: u32| {
            let entry = PaletteEntry {
                id: CommandId::Plugin {
                    index: 9,
                    kind,
                    action,
                },
                group: "WindowDeck".to_string(),
                label: String::new(),
                hint: String::new(),
                disabled: false,
                plugin: Some(ID.to_string()),
                subtitle: None,
                shortcut_id: String::new(),
                shortcut: String::new(),
                quiet: false,
                badge: None,
            };
            format!("commands:{}", command_key(&entry))
        };
        let open = deck.hotkey_actions()[0].id;
        let tray = deck.tray_actions()[0].id;
        let hover = deck.palette_commands()[0].id;
        let new: Vec<&str> = WINDOW_DECK_RENAMES.iter().map(|(_, new)| *new).collect();
        assert_eq!(
            new,
            [
                key(ActionKind::Hotkey, open),
                key(ActionKind::Tray, tray),
                key(ActionKind::Palette, hover),
            ]
        );
        for (old, _) in WINDOW_DECK_RENAMES {
            assert!(old.starts_with("commands:layout_keeper/"), "{old}");
        }
    }

    #[test]
    fn a_saved_old_default_hover_delay_becomes_the_new_default() {
        let mut old = serde_json::json!({ HOVER_DELAY_KEY: 0.4 });
        assert!(migrate_hover_delay(&mut old));
        assert_eq!(old[HOVER_DELAY_KEY], 0.15);
        let mut chosen = serde_json::json!({ HOVER_DELAY_KEY: 0.8 });
        assert!(!migrate_hover_delay(&mut chosen));
        assert_eq!(chosen[HOVER_DELAY_KEY], 0.8);
    }

    #[test]
    fn the_hover_strip_is_off_by_default_and_switches_at_once() {
        let mut deck = test_deck();
        deck.apply_settings(&WindowDeck::default().default_settings());
        assert!(!deck.hover_strip);
        assert!(!deck.hover.is_on());
        assert_eq!(deck.hover_delay_ms, 150);

        deck.apply_settings(&serde_json::json!({
            HOVER_STRIP_KEY: true,
            HOVER_DELAY_KEY: 0.8,
        }));
        assert!(deck.hover.is_on(), "on without a restart");
        assert_eq!(deck.hover_delay_ms, 800);
        assert!(deck.tray_toggles()[0].on);

        deck.apply_settings(&serde_json::json!({ HOVER_STRIP_KEY: false }));
        assert!(!deck.hover.is_on(), "off at once");
        assert!(!deck.tray_toggles()[0].on);
    }

    #[test]
    fn the_tray_and_the_palette_flip_the_hover_strip() {
        let mut deck = test_deck();
        deck.on_tray_action(TRAY_HOVER_STRIP);
        assert!(deck.hover_strip);
        assert!(deck.hover.is_on());
        assert_eq!(
            deck.palette_subtitle(ActionKind::Palette, COMMAND_HOVER_STRIP),
            Some(hover_subtitle(true).to_string())
        );
        deck.on_palette_command(COMMAND_HOVER_STRIP);
        assert!(!deck.hover_strip);
        assert!(!deck.hover.is_on());
        assert_eq!(
            deck.palette_subtitle(ActionKind::Palette, COMMAND_HOVER_STRIP),
            Some(hover_subtitle(false).to_string())
        );
        let toggles = deck.tray_toggles();
        assert_eq!(toggles.len(), 1);
        assert_eq!(toggles[0].label, HOVER_STRIP_LABEL);
        assert!(toggles[0].id < 100, "tray ids stay below 100");
        deck.teardown();
    }

    #[test]
    fn a_test_instance_says_it_leaves_windows_preview_alone() {
        let mut deck = test_deck();
        deck.apply_settings(&serde_json::json!({ HOVER_STRIP_KEY: true }));
        let status = deck.status().unwrap_or_default();
        assert!(
            status.contains("Windows' preview is left alone in this test instance"),
            "{status}"
        );
        assert!(!deck.hover.may_hide);
        deck.teardown();
        assert!(deck.status().is_none());
    }

    #[test]
    fn the_look_follows_the_settings_and_the_view_is_automatic_by_default() {
        let mut deck = test_deck();
        deck.apply_settings(&deck.default_settings());
        let look = deck.strip_look().expect("WindowDeck decides the look");
        assert!(look.preview);
        assert_eq!(look.view, ArrangeView::Auto);
        deck.apply_settings(&serde_json::json!({ PREVIEW_KEY: false, VIEW_KEY: "flat" }));
        let look = deck.strip_look().expect("a look");
        assert!(!look.preview);
        assert_eq!(look.view, ArrangeView::Flat);
    }

    #[test]
    fn win_number_is_off_by_default_and_needs_no_hook_until_switched_on() {
        let mut deck = test_deck();
        assert_eq!(deck.default_settings()[WIN_NUMBER_KEY], false);
        deck.apply_settings(&deck.default_settings());
        assert!(!deck.win_number);
        assert!(!win_digit::is_running());
        // Outside the host thread (no host window yet) the switch is only
        // remembered; the hook goes in at init.
        deck.apply_settings(&serde_json::json!({ WIN_NUMBER_KEY: true }));
        assert!(deck.win_number);
        assert!(!win_digit::is_running());
        let field = deck
            .settings_fields()
            .into_iter()
            .find(|field| field.key == WIN_NUMBER_KEY)
            .expect("the switch is on the page");
        assert_eq!(field.label, "Open WindowDeck with Win+1\u{2026}9");
        assert!(field
            .help
            .contains("Win+Shift, Win+Ctrl and Win+Alt with a number stay with Windows."));
        deck.hook_refused = true;
        assert!(deck
            .status()
            .unwrap_or_default()
            .contains("stays with Windows"));
        deck.teardown();
        assert!(!deck.hook_refused);
    }

    #[test]
    fn its_names_say_windowdeck_and_win_alt_a_opens_it() {
        let deck = test_deck();
        let meta = deck.metadata();
        assert_eq!(
            (meta.id, meta.name, meta.category),
            (ID, "WindowDeck", "Windows")
        );
        let actions = deck.hotkey_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].name, "arrange", "the name LayoutKeeper used");
        assert!(actions[0].label.starts_with("WindowDeck: "));
        assert_eq!(actions[0].default.vk, u32::from(b'A'));
        assert_eq!(
            actions[0].default.modifiers & (MOD_WIN | MOD_ALT),
            MOD_WIN | MOD_ALT
        );
        assert!(deck.palette_commands()[0].label.starts_with("WindowDeck: "));
        assert_eq!(deck.page_action(), Some(ACTION_OPEN));
        assert_eq!(deck.split_from(), Some("layout_keeper"));
    }

    fn keeper(
        settings: serde_json::Value,
        hotkeys: &[(&str, &str)],
        enabled: bool,
    ) -> PluginConfig {
        PluginConfig {
            enabled,
            hotkeys: hotkeys
                .iter()
                .map(|(name, keys)| (name.to_string(), keys.to_string()))
                .collect(),
            settings,
        }
    }

    #[test]
    fn the_strip_settings_move_from_layoutkeeper_with_their_values() {
        let mut old = keeper(
            serde_json::json!({
                "programs": "*",
                "taskbar_hover_strip": true,
                "hover_delay_seconds": 0.3,
                "preview_on_hover": false,
                "arrange_view": "flat",
            }),
            &[("arrange", "Win+Alt+K, A"), ("restore_now", "Win+Alt+L")],
            true,
        );
        let mut new = PluginConfig::default();
        assert!(test_deck().take_over(&mut old, &mut new));
        assert!(new.enabled);
        assert_eq!(new.settings["taskbar_hover_strip"], true);
        assert_eq!(new.settings["hover_delay_seconds"], 0.3);
        assert_eq!(new.settings["preview_on_hover"], false);
        assert_eq!(new.settings["arrange_view"], "flat");
        assert_eq!(
            new.hotkeys["arrange"], "Win+Alt+K, A",
            "a changed chord stays"
        );
        // LayoutKeeper keeps only its own.
        assert_eq!(old.settings, serde_json::json!({ "programs": "*" }));
        assert_eq!(old.hotkeys.len(), 1);
        assert_eq!(old.hotkeys["restore_now"], "Win+Alt+L");
        assert!(old.enabled);
    }

    #[test]
    fn windowdeck_starts_on_only_if_the_strip_was_in_use() {
        let take = |old: &mut PluginConfig| {
            let mut new = PluginConfig::default();
            take_over_from_keeper(old, &mut new);
            new.enabled
        };
        // The hotkey set, the hover strip off: on.
        let mut hotkey = keeper(
            serde_json::json!({ "taskbar_hover_strip": false }),
            &[("arrange", "Win+Alt+A")],
            true,
        );
        assert!(take(&mut hotkey));
        // The hotkey removed but the hover strip on: on.
        let mut hover = keeper(
            serde_json::json!({ "taskbar_hover_strip": true }),
            &[("arrange", "")],
            true,
        );
        assert!(take(&mut hover));
        // Neither: off.
        let mut neither = keeper(
            serde_json::json!({ "taskbar_hover_strip": false }),
            &[("arrange", " ")],
            true,
        );
        assert!(!take(&mut neither));
        // LayoutKeeper was off, so the strip never opened: off.
        let mut keeper_off = keeper(
            serde_json::json!({ "taskbar_hover_strip": true }),
            &[("arrange", "Win+Alt+A")],
            false,
        );
        assert!(!take(&mut keeper_off));
        // A LayoutKeeper file with nothing of the strip changes nothing.
        let mut fresh = keeper(serde_json::json!({ "programs": "*" }), &[], true);
        let mut new = PluginConfig {
            enabled: true,
            ..PluginConfig::default()
        };
        assert!(!take_over_from_keeper(&mut fresh, &mut new));
        assert!(new.enabled);
        assert!(new.hotkeys.is_empty());
        assert_eq!(fresh.settings, serde_json::json!({ "programs": "*" }));
    }
}

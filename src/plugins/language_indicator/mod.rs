mod detect;
mod names;
mod panel;

use std::collections::BTreeMap;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, WM_TIMER};

use crate::core::traits::{
    FieldKind, HostContext, PaletteCommand, PluginMetadata, SettingField, WinCraftPlugin,
};
use crate::core::{clock, host};
use detect::{Detector, ShellHook, Switch};
use names::Language;
use panel::{Content, Panel, Position};

/// Timer ids on the shared host window, spelling "LI" to stay clear of the
/// other plugins' ids.
const POLL_TIMER: usize = 0x4C49_0001;
const HOLD_TIMER: usize = 0x4C49_0002;
const FADE_TIMER: usize = 0x4C49_0003;
const POLL_MS: u32 = 120;
const FADE_STEP_MS: u32 = 16;
const FADE_MS: u64 = 250;

const COMMAND_SHOW_CURRENT: u32 = 1;

const DEFAULT_DURATION_MS: u64 = 900;
const POSITIONS: &[&str] = &["Centre", "Upper third", "Lower third"];

#[derive(Default)]
pub struct LanguageIndicator {
    host_hwnd: Option<HWND>,
    duration_ms: u64,
    position: Position,
    show_on_focus_change: bool,
    show_layout_name: bool,
    detector: Detector,
    shell: Option<ShellHook>,
    panel: Option<Panel>,
    names: BTreeMap<usize, Language>,
    fading_since: Option<u64>,
    last_switch: Option<String>,
}

impl LanguageIndicator {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_settings(&mut self, settings: &serde_json::Value) {
        self.duration_ms = settings
            .get("duration_ms")
            .and_then(|value| value.as_f64())
            .map(|ms| ms.round().clamp(300.0, 5000.0) as u64)
            .unwrap_or(DEFAULT_DURATION_MS);
        self.position = match settings.get("position").and_then(|value| value.as_str()) {
            Some("Upper third") => Position::UpperThird,
            Some("Lower third") => Position::LowerThird,
            _ => Position::Centre,
        };
        self.show_on_focus_change = settings
            .get("show_on_focus_change")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        self.show_layout_name = settings
            .get("show_layout_name")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
    }

    fn language(&mut self, hkl: usize) -> Language {
        self.names
            .entry(hkl)
            .or_insert_with(|| names::describe(hkl))
            .clone()
    }

    fn poll(&mut self) {
        let from_shell = self.shell.as_ref().and_then(ShellHook::take);
        if let Some(switch) = from_shell.and_then(|hkl| self.detector.shell(hkl)) {
            self.show_switch(switch);
        }
        let Some((window, hkl)) = detect::foreground() else {
            return;
        };
        if let Some(switch) = self.detector.poll(window, hkl, self.show_on_focus_change) {
            self.show_switch(switch);
        }
    }

    fn show_switch(&mut self, switch: Switch) {
        let to = self.language(switch.to);
        let from = switch.from.map(|hkl| self.language(hkl));
        let summary = match &from {
            Some(from) => format!("{} \u{2192} {}", from.code, to.code),
            None => to.code.clone(),
        };
        log::info!(
            "input language: {summary} ({}{})",
            to.name,
            to.layout
                .as_deref()
                .map(|layout| format!(", {layout}"))
                .unwrap_or_default()
        );
        self.last_switch = Some(format!("{summary} at {}", clock::now_hours_minutes()));
        host::plugin_changed();
        self.show(from.map(|from| from.code), &to);
    }

    fn show(&mut self, from: Option<String>, to: &Language) {
        let detail = match (&to.layout, self.show_layout_name) {
            (Some(layout), true) => format!("{} \u{b7} {layout}", to.name),
            _ => to.name.clone(),
        };
        let content = Content {
            from,
            to: to.code.clone(),
            detail,
        };
        let (Some(panel), Some(hwnd)) = (self.panel.as_mut(), self.host_hwnd) else {
            return;
        };
        panel.show(&content, self.position);
        self.fading_since = None;
        unsafe {
            KillTimer(hwnd, FADE_TIMER);
            SetTimer(hwnd, HOLD_TIMER, self.duration_ms as u32, None);
        }
    }

    fn start_fade(&mut self) {
        let Some(hwnd) = self.host_hwnd else {
            return;
        };
        unsafe {
            KillTimer(hwnd, HOLD_TIMER);
            SetTimer(hwnd, FADE_TIMER, FADE_STEP_MS, None);
        }
        self.fading_since = Some(unsafe { GetTickCount64() });
    }

    fn fade_step(&mut self) {
        let (Some(since), Some(hwnd)) = (self.fading_since, self.host_hwnd) else {
            return;
        };
        let elapsed = unsafe { GetTickCount64() }.saturating_sub(since);
        let Some(panel) = self.panel.as_mut() else {
            return;
        };
        if elapsed >= FADE_MS {
            panel.hide();
            self.fading_since = None;
            unsafe { KillTimer(hwnd, FADE_TIMER) };
            return;
        }
        panel.set_alpha((255 * (FADE_MS - elapsed) / FADE_MS) as u8);
    }

    fn show_current(&mut self) {
        let hkl = self
            .detector
            .current()
            .or_else(|| detect::foreground().map(|(_, hkl)| hkl));
        if let Some(hkl) = hkl {
            let language = self.language(hkl);
            self.show(None, &language);
        }
    }
}

impl WinCraftPlugin for LanguageIndicator {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "language_indicator",
            name: "LanguageIndicator",
            description: "Shows which input language you switched to, in the middle of the screen.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "duration_ms": DEFAULT_DURATION_MS,
            "position": POSITIONS[0],
            "show_on_focus_change": false,
            "show_layout_name": true,
        })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: "duration_ms",
                label: "How long it stays",
                help: "Milliseconds before it fades out, 300 to 5000.",
                kind: FieldKind::Number {
                    min: 300.0,
                    max: 5000.0,
                },
            },
            SettingField {
                key: "position",
                label: "Where it appears",
                help: "On the monitor of the window you are typing in.",
                kind: FieldKind::Choice(POSITIONS),
            },
            SettingField {
                key: "show_on_focus_change",
                label: "Show on focus change",
                help: "Also show it when you move to a window that uses another language.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: "show_layout_name",
                label: "Show the layout name",
                help: "Add the keyboard layout after the language, such as \"US\".",
                kind: FieldKind::Toggle,
            },
        ]
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.apply_settings(ctx.settings);
        self.panel = Some(Panel::create()?);
        // The poll works on its own; the shell hook only makes switches in
        // windows that the poll cannot follow visible too.
        self.shell = match ShellHook::create() {
            Ok(shell) => Some(shell),
            Err(err) => {
                log::warn!("{err}; watching the foreground window only");
                None
            }
        };
        if unsafe { SetTimer(ctx.hwnd, POLL_TIMER, POLL_MS, None) } == 0 {
            return Err("could not start its timer".to_string());
        }
        self.host_hwnd = Some(ctx.hwnd);
        self.detector = Detector::default();
        let layouts: Vec<String> = detect::installed_layouts()
            .into_iter()
            .map(|hkl| {
                let language = self.language(hkl);
                format!(
                    "{} {}{}",
                    language.code,
                    language.name,
                    language
                        .layout
                        .map(|layout| format!(" ({layout})"))
                        .unwrap_or_default()
                )
            })
            .collect();
        log::info!("input languages: {}", layouts.join(", "));
        Ok(())
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.apply_settings(settings);
        true
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: COMMAND_SHOW_CURRENT,
            label: "Show current input language",
            hint: "",
        }]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id == COMMAND_SHOW_CURRENT {
            self.show_current();
        }
    }

    fn status(&self) -> Option<String> {
        Some(match &self.last_switch {
            Some(last) => format!("Last switch: {last}"),
            None => "No switch seen yet this session".to_string(),
        })
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, _lparam: LPARAM) {
        if msg != WM_TIMER {
            return;
        }
        match wparam {
            POLL_TIMER => self.poll(),
            HOLD_TIMER => self.start_fade(),
            FADE_TIMER => self.fade_step(),
            _ => {}
        }
    }

    fn teardown(&mut self) {
        if let Some(hwnd) = self.host_hwnd.take() {
            unsafe {
                KillTimer(hwnd, POLL_TIMER);
                KillTimer(hwnd, HOLD_TIMER);
                KillTimer(hwnd, FADE_TIMER);
            }
        }
        if let Some(mut shell) = self.shell.take() {
            shell.destroy();
        }
        if let Some(mut panel) = self.panel.take() {
            panel.destroy();
        }
        self.fading_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_clamped_and_default_sensibly() {
        let mut plugin = LanguageIndicator::new();
        plugin.apply_settings(&serde_json::json!({
            "duration_ms": 99999,
            "position": "Lower third",
        }));
        assert_eq!(plugin.duration_ms, 5000);
        assert_eq!(plugin.position, Position::LowerThird);
        assert!(!plugin.show_on_focus_change);
        assert!(plugin.show_layout_name);

        plugin.apply_settings(&serde_json::json!({ "position": "Somewhere" }));
        assert_eq!(plugin.duration_ms, DEFAULT_DURATION_MS);
        assert_eq!(plugin.position, Position::Centre);
    }
}

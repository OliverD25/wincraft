mod com;
mod desktops;
mod identity;
mod state;
mod windows;

use std::time::SystemTime;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    KillTimer, SetTimer, SW_SHOWNORMAL, WM_ENDSESSION, WM_QUERYENDSESSION, WM_TIMER,
};

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    WinCraftPlugin,
};
use crate::core::{clock, wide};
use identity::WindowIdentity;
use state::{ProgramState, Programs, SavedWindow};

/// The host window is shared, so the timer id spells "LK" to stay clear of
/// any other plugin's.
const TIMER_ID: usize = 0x4C4B;
const TICK_MS: u32 = 1000;

const ACTION_SAVE_NOW: u32 = 2;
const COMMAND_OPEN_STATE: u32 = 1;

const DEFAULT_PROGRAMS: &str = "chrome.exe";
const DEFAULT_SNAPSHOT_SECONDS: u64 = 30;

#[derive(Default)]
pub struct LayoutKeeper {
    host_hwnd: Option<HWND>,
    programs: Vec<String>,
    snapshot_seconds: u64,
    ticks: u64,
    reader: Option<desktops::Reader>,
    writer: state::Writer,
}

impl LayoutKeeper {
    pub fn new() -> Self {
        Self::default()
    }

    fn apply_settings(&mut self, settings: &serde_json::Value) {
        self.programs = parse_programs(
            settings
                .get("programs")
                .and_then(|value| value.as_str())
                .unwrap_or(DEFAULT_PROGRAMS),
        );
        self.snapshot_seconds = number(settings, "snapshot_interval_seconds", 5, 600)
            .unwrap_or(DEFAULT_SNAPSHOT_SECONDS);
    }

    /// The watched programs' windows as they are now, each program's list in
    /// taskbar order.
    fn capture(&self) -> Programs {
        let live = windows::enumerate(&self.programs);
        let desktops = desktops::list();
        let mut programs = Programs::new();
        for exe in &self.programs {
            let mine: Vec<&windows::LiveWindow> = live
                .iter()
                .filter(|window| &window.identity.exe == exe)
                .collect();
            let previous: Vec<WindowIdentity> = self
                .writer
                .last()
                .and_then(|programs| programs.get(exe))
                .map(|program| {
                    program
                        .windows
                        .iter()
                        .map(|window| window.identity(exe))
                        .collect()
                })
                .unwrap_or_default();
            let identities: Vec<WindowIdentity> =
                mine.iter().map(|window| window.identity.clone()).collect();
            let order = identity::carry_order(&previous, &identities);
            let mut saved: Vec<SavedWindow> = mine
                .iter()
                .enumerate()
                .map(|(z_index, window)| {
                    let desktop = self
                        .reader
                        .as_ref()
                        .and_then(|reader| reader.read(window.hwnd));
                    SavedWindow {
                        name: window.identity.name.clone(),
                        title: window.identity.title.clone(),
                        rect: window.identity.rect,
                        maximized: window.identity.maximized,
                        desktop: desktop.map(|id| id.to_string()),
                        desktop_name: desktop.and_then(|id| desktops::name_of(&desktops, id)),
                        taskbar_index: order
                            .iter()
                            .position(|live| *live == z_index)
                            .unwrap_or(z_index),
                        z_index,
                    }
                })
                .collect();
            saved.sort_by_key(|window| window.taskbar_index);
            programs.insert(exe.clone(), ProgramState { windows: saved });
        }
        programs
    }

    fn save(&mut self, reason: &str) {
        let shutdown = reason == "shutdown";
        let merged = state::merge(self.writer.last(), self.capture(), shutdown);
        let count: usize = merged.values().map(|program| program.windows.len()).sum();
        match self
            .writer
            .write(merged, reason, clock::utc_iso(SystemTime::now()))
        {
            Ok(true) => log::info!("saved {count} windows ({reason})"),
            Ok(false) => log::debug!("layout unchanged ({reason})"),
            Err(err) => log::error!("could not save the layout: {err}"),
        }
    }

    fn open_state_file(&mut self) {
        if !state::path().exists() {
            self.save("manual");
        }
        let args = wide(&format!("\"{}\"", state::path().display()));
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide("notepad.exe").as_ptr(),
                args.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
    }
}

fn parse_programs(text: &str) -> Vec<String> {
    let mut programs: Vec<String> = Vec::new();
    for name in text.split(',').map(|name| name.trim().to_lowercase()) {
        if !name.is_empty() && !programs.contains(&name) {
            programs.push(name);
        }
    }
    programs
}

fn number(settings: &serde_json::Value, key: &str, min: u64, max: u64) -> Option<u64> {
    let value = settings.get(key)?.as_f64()?;
    Some((value.round().max(0.0) as u64).clamp(min, max))
}

fn win_alt(vk: u32) -> Hotkey {
    Hotkey {
        modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
        vk,
    }
}

impl WinCraftPlugin for LayoutKeeper {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "layout_keeper",
            name: "LayoutKeeper",
            description: "Remembers which desktop each window is on, the taskbar thumbnail order and which window is in front, and restores them after a reboot.",
            author: "community",
            version: "1.0.0",
            readme: include_str!("README.md"),
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "programs": DEFAULT_PROGRAMS,
            "snapshot_interval_seconds": DEFAULT_SNAPSHOT_SECONDS,
        })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: "programs",
                label: "Programs",
                help: "Exe names to watch, separated by commas.",
                kind: FieldKind::Text,
            },
            SettingField {
                key: "snapshot_interval_seconds",
                label: "Save every (seconds)",
                help: "How often the layout is written to disk while nothing else triggers it.",
                kind: FieldKind::Number {
                    min: 5.0,
                    max: 600.0,
                },
            },
        ]
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.apply_settings(settings);
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.apply_settings(ctx.settings);
        self.ticks = 0;
        self.reader = match desktops::Reader::new() {
            Ok(reader) => Some(reader),
            Err(err) => {
                log::warn!("{err}; desktops will not be recorded");
                None
            }
        };
        self.writer = state::Writer::with_last(state::load().map(|file| file.programs));
        if unsafe { SetTimer(ctx.hwnd, TIMER_ID, TICK_MS, None) } == 0 {
            return Err("could not start its timer".to_string());
        }
        self.host_hwnd = Some(ctx.hwnd);
        log::info!("watching {}", self.programs.join(", "));
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![HotkeyAction {
            id: ACTION_SAVE_NOW,
            name: "save_now",
            label: "Save layout now",
            default: win_alt(u32::from(b'J')),
        }]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        if action_id == ACTION_SAVE_NOW {
            self.save("manual");
        }
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: COMMAND_OPEN_STATE,
            label: "Open state file",
            hint: "",
        }]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id == COMMAND_OPEN_STATE {
            self.open_state_file();
        }
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, _lparam: LPARAM) {
        match msg {
            WM_TIMER if wparam == TIMER_ID => {
                self.ticks += 1;
                if self.ticks % self.snapshot_seconds.max(1) == 0 {
                    self.save("timer");
                }
            }
            WM_QUERYENDSESSION => self.save("shutdown"),
            WM_ENDSESSION if wparam != 0 => self.save("shutdown"),
            _ => {}
        }
    }

    fn teardown(&mut self) {
        if let Some(hwnd) = self.host_hwnd.take() {
            unsafe { KillTimer(hwnd, TIMER_ID) };
            self.save("teardown");
        }
        self.reader = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_list_is_trimmed_lowercased_and_deduplicated() {
        assert_eq!(
            parse_programs(" Chrome.exe, ,msedge.exe,chrome.exe"),
            ["chrome.exe", "msedge.exe"]
        );
    }

    #[test]
    fn numbers_are_rounded_and_clamped() {
        let settings = serde_json::json!({ "a": 29.6, "b": 1.0, "c": "x" });
        assert_eq!(number(&settings, "a", 5, 600), Some(30));
        assert_eq!(number(&settings, "b", 5, 600), Some(5));
        assert_eq!(number(&settings, "c", 5, 600), None);
        assert_eq!(number(&settings, "missing", 5, 600), None);
    }
}

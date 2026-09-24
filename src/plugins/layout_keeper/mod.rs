mod com;
mod desktops;
mod identity;
mod order;
mod state;
mod windows;

use std::collections::BTreeMap;
use std::time::SystemTime;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, KillTimer, SetTimer, SW_SHOWNORMAL,
    WM_ENDSESSION, WM_QUERYENDSESSION, WM_TIMER,
};

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    WinCraftPlugin,
};
use crate::core::{clock, wide};
use order::{Handle, OrderModel};
use state::{ProgramState, Programs, SavedWindow};

/// The host window is shared, so the timer id spells "LK" to stay clear of
/// any other plugin's.
const TIMER_ID: usize = 0x4C4B;
const TICK_MS: u32 = 1000;

const ACTION_SAVE_NOW: u32 = 2;
const ACTION_MOVE_LEFT: u32 = 3;
const ACTION_MOVE_RIGHT: u32 = 4;

const VK_OPEN_BRACKET: u32 = 0xDB;
const VK_CLOSE_BRACKET: u32 = 0xDD;
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
    groups: BTreeMap<String, OrderModel>,
    taskbar: Option<order::Taskbar>,
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

    /// Looks at the windows and brings every group's order model up to date.
    fn refresh(&mut self) -> Vec<windows::LiveWindow> {
        let live = windows::enumerate(&self.programs);
        for exe in &self.programs {
            let mine: Vec<(Handle, identity::WindowIdentity)> = live
                .iter()
                .filter(|window| &window.identity.exe == exe)
                .map(|window| (window.hwnd as Handle, window.identity.clone()))
                .collect();
            self.groups.entry(exe.clone()).or_default().refresh(
                &mine,
                self.ticks,
                self.snapshot_seconds,
            );
        }
        live
    }

    /// The watched programs' windows as they are now, each program's list in
    /// taskbar order.
    fn capture(&self, live: &[windows::LiveWindow]) -> Programs {
        let desktops = desktops::list();
        let mut programs = Programs::new();
        for exe in &self.programs {
            let mine: Vec<&windows::LiveWindow> = live
                .iter()
                .filter(|window| &window.identity.exe == exe)
                .collect();
            let group = self.groups.get(exe);
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
                        taskbar_index: group
                            .and_then(|group| group.position_of(window.hwnd as Handle))
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
        let live = self.refresh();
        let merged = state::merge(self.writer.last(), self.capture(&live), shutdown);
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

    /// Makes one program's taskbar group match its model, if it does not
    /// already.
    fn apply_group(&mut self, exe: &str) {
        let Some(pending) = self.groups.get(exe).and_then(OrderModel::pending) else {
            return;
        };
        if self.taskbar.is_none() {
            match order::Taskbar::new() {
                Ok(taskbar) => self.taskbar = Some(taskbar),
                Err(err) => {
                    log::error!("{err}; the taskbar order cannot be changed");
                    return;
                }
            }
        }
        let Some(taskbar) = &self.taskbar else { return };
        let took = taskbar.apply(&pending);
        log::info!(
            "applied the order of {} {exe} windows in {} ms",
            pending.len(),
            took.as_millis()
        );
        if let Some(group) = self.groups.get_mut(exe) {
            group.mark_applied(pending);
        }
    }

    /// Moves the front window one place along its taskbar group.
    fn shift_front(&mut self, step: isize) {
        let front = unsafe { GetForegroundWindow() };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(front, &mut pid) };
        let exe = windows::exe_name(pid);
        if !self.programs.contains(&exe) {
            log::info!("the front window belongs to \"{exe}\", which is not watched");
            return;
        }
        self.refresh();
        let moved = self
            .groups
            .get_mut(&exe)
            .is_some_and(|group| group.shift(front as Handle, step));
        if !moved {
            log::info!("the front window is already at that end of its group");
            return;
        }
        self.apply_group(&exe);
        self.save("manual");
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
        self.groups = self
            .writer
            .last()
            .map(|programs| {
                programs
                    .iter()
                    .map(|(exe, program)| {
                        let mut windows = program.windows.clone();
                        windows.sort_by_key(|window| window.taskbar_index);
                        let identities = windows.iter().map(|w| w.identity(exe)).collect();
                        (exe.clone(), OrderModel::from_saved(identities))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if unsafe { SetTimer(ctx.hwnd, TIMER_ID, TICK_MS, None) } == 0 {
            return Err("could not start its timer".to_string());
        }
        self.host_hwnd = Some(ctx.hwnd);
        log::info!("watching {}", self.programs.join(", "));
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![
            HotkeyAction {
                id: ACTION_SAVE_NOW,
                name: "save_now",
                label: "Save layout now",
                default: win_alt(u32::from(b'J')),
            },
            HotkeyAction {
                id: ACTION_MOVE_LEFT,
                name: "move_left",
                label: "Move window left in taskbar",
                default: win_alt(VK_OPEN_BRACKET),
            },
            HotkeyAction {
                id: ACTION_MOVE_RIGHT,
                name: "move_right",
                label: "Move window right in taskbar",
                default: win_alt(VK_CLOSE_BRACKET),
            },
        ]
    }

    fn on_hotkey(&mut self, action_id: u32) {
        match action_id {
            ACTION_SAVE_NOW => self.save("manual"),
            ACTION_MOVE_LEFT => self.shift_front(-1),
            ACTION_MOVE_RIGHT => self.shift_front(1),
            _ => {}
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
        self.taskbar = None;
        self.groups.clear();
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

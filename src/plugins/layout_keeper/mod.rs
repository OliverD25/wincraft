mod com;
mod desktops;
mod identity;
mod order;
mod restore;
mod state;
mod windows;

use std::collections::BTreeMap;
use std::time::SystemTime;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_WIN};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, KillTimer, SetTimer, SetWindowPos, HWND_TOP,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNORMAL, WM_ENDSESSION, WM_QUERYENDSESSION,
    WM_TIMER,
};

use crate::core::traits::{
    FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata, SettingField,
    TrayAction, WinCraftPlugin,
};
use crate::core::{clock, host, wide};
use desktops::{Desktop, DesktopId};
use order::{Handle, OrderModel};
use restore::{Restore, Step};
use state::{ProgramState, Programs, SavedWindow};

/// The host window is shared, so the timer id spells "LK" to stay clear of
/// any other plugin's.
const TIMER_ID: usize = 0x4C4B;
const TICK_MS: u32 = 1000;

const ACTION_RESTORE: u32 = 1;
const ACTION_SAVE_NOW: u32 = 2;
const ACTION_MOVE_LEFT: u32 = 3;
const ACTION_MOVE_RIGHT: u32 = 4;

const VK_OPEN_BRACKET: u32 = 0xDB;
const VK_CLOSE_BRACKET: u32 = 0xDD;
const COMMAND_OPEN_STATE: u32 = 1;

const DEFAULT_PROGRAMS: &str = "chrome.exe";
const DEFAULT_SNAPSHOT_SECONDS: u64 = 30;
const DEFAULT_SETTLE_SECONDS: u64 = 5;
const DEFAULT_RESTORE_MINUTES: u64 = 3;

/// The undocumented move chain is built on first use and kept for the
/// session, or given up on for the session after one failure.
#[derive(Default)]
enum Mover {
    #[default]
    Untried,
    Ready(desktops::Mover),
    Disabled(String),
}

pub struct LayoutKeeper {
    host_hwnd: Option<HWND>,
    programs: Vec<String>,
    snapshot_seconds: u64,
    restore_on_start: bool,
    front_order: bool,
    ticks: u64,
    started_once: bool,
    reader: Option<desktops::Reader>,
    writer: state::Writer,
    groups: BTreeMap<String, OrderModel>,
    taskbar: Option<order::Taskbar>,
    mover: Mover,
    restore: Restore,
    last_saved: Option<String>,
    last_restore: Option<String>,
}

impl Default for LayoutKeeper {
    fn default() -> Self {
        Self {
            host_hwnd: None,
            programs: Vec::new(),
            snapshot_seconds: DEFAULT_SNAPSHOT_SECONDS,
            restore_on_start: true,
            front_order: true,
            ticks: 0,
            started_once: false,
            reader: None,
            writer: state::Writer::default(),
            groups: BTreeMap::new(),
            taskbar: None,
            mover: Mover::Untried,
            restore: Restore::new(DEFAULT_SETTLE_SECONDS, DEFAULT_RESTORE_MINUTES * 60),
            last_saved: None,
            last_restore: None,
        }
    }
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
        self.restore_on_start = flag(settings, "restore_on_start");
        self.front_order = flag(settings, "restore_front_order");
        self.restore.settle_seconds =
            number(settings, "settle_seconds", 1, 60).unwrap_or(DEFAULT_SETTLE_SECONDS);
        self.restore.give_up_seconds = number(settings, "restore_window_minutes", 1, 30)
            .unwrap_or(DEFAULT_RESTORE_MINUTES)
            * 60;
    }

    /// Looks at the windows and brings every group's order model up to date.
    fn refresh(&mut self) -> Vec<windows::LiveWindow> {
        let live = windows::enumerate(&self.programs);
        for exe in &self.programs {
            let mine = handles_of(&live, exe);
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
        // Until the restore has run, the windows on screen are the scrambled
        // ones the file is meant to fix; saving them would lose the layout.
        if self.restore.is_running() {
            log::info!("a restore is waiting, so the layout is not saved ({reason})");
            return;
        }
        let shutdown = reason == "shutdown";
        let live = self.refresh();
        let merged = state::merge(self.writer.last(), self.capture(&live), shutdown);
        let count: usize = merged.values().map(|program| program.windows.len()).sum();
        match self
            .writer
            .write(merged, reason, clock::utc_iso(SystemTime::now()))
        {
            Ok(true) => {
                log::info!("saved {count} windows ({reason})");
                self.last_saved = Some(clock::now_hours_minutes());
                host::plugin_changed();
            }
            Ok(false) => log::debug!("layout unchanged ({reason})"),
            Err(err) => log::error!("could not save the layout: {err}"),
        }
    }

    /// Makes one program's taskbar group match its model, if it does not
    /// already. Re-adding a button pulls a window on another desktop over to
    /// this one, so those windows are only included when `pull` is set and
    /// the caller will move them back.
    fn apply_group(&mut self, exe: &str, pull: bool) {
        let Some(pending) = self.groups.get(exe).and_then(OrderModel::pending) else {
            return;
        };
        let send: Vec<Handle> = pending
            .iter()
            .copied()
            .filter(|hwnd| pull || !windows::on_other_desktop(*hwnd as HWND))
            .collect();
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
        let took = taskbar.apply(&send);
        log::info!(
            "applied the order of {} {exe} windows in {} ms ({} on other desktops left alone)",
            send.len(),
            took.as_millis(),
            pending.len() - send.len()
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
        let live = self.refresh();
        let visible: Vec<Handle> = live
            .iter()
            .filter(|window| window.identity.exe == exe && !windows::on_other_desktop(window.hwnd))
            .map(|window| window.hwnd as Handle)
            .collect();
        let moved = self
            .groups
            .get_mut(&exe)
            .is_some_and(|group| group.shift(front as Handle, step, &visible));
        if !moved {
            log::info!("the front window is already at that end of its group");
            return;
        }
        self.apply_group(&exe, false);
        self.save("manual");
    }

    fn start_restore(&mut self) {
        if self.writer.last().is_none() {
            log::info!("no layout has been saved yet, so there is nothing to restore");
            return;
        }
        log::info!(
            "restore starts once the windows have held still for {} s",
            self.restore.settle_seconds
        );
        self.restore.start(self.ticks);
    }

    /// Puts the saved layout back: each group's taskbar order, then every
    /// window's desktop, then the front-to-back order. Re-adding the buttons
    /// pulls every window onto the current desktop, which is why the desktops
    /// come second; without the desktop mover only the windows already on
    /// this desktop are re-added. The current desktop and the foreground
    /// window are left as they are.
    fn restore_layout(&mut self) {
        let Some(saved) = self.writer.last().cloned() else {
            return;
        };
        let live = self.refresh();
        let registry = desktops::list();
        let can_move = self.ensure_mover(&registry);
        let foreground = unsafe { GetForegroundWindow() };
        let mut stack: Vec<(usize, HWND)> = Vec::new();
        let mut parts: Vec<String> = Vec::new();
        let mut missing = 0;

        for exe in self.programs.clone() {
            let Some(program) = saved.get(&exe) else {
                continue;
            };
            let mut windows = program.windows.clone();
            windows.sort_by_key(|window| window.taskbar_index);
            let saved_ids: Vec<identity::WindowIdentity> =
                windows.iter().map(|window| window.identity(&exe)).collect();
            let mine: Vec<&windows::LiveWindow> = live
                .iter()
                .filter(|window| window.identity.exe == exe)
                .collect();
            let live_ids: Vec<identity::WindowIdentity> =
                mine.iter().map(|window| window.identity.clone()).collect();
            let matching = identity::match_windows(&saved_ids, &live_ids);
            let before: Vec<Option<DesktopId>> = mine
                .iter()
                .map(|window| self.reader.as_ref().and_then(|r| r.read(window.hwnd)))
                .collect();

            let mut model = OrderModel::from_saved(saved_ids);
            model.refresh(&handles_of(&live, &exe), self.ticks, self.snapshot_seconds);
            self.groups.insert(exe.clone(), model);
            self.apply_group(&exe, can_move);

            if can_move {
                for (l, window) in mine.iter().enumerate() {
                    let saved_desktop = matching
                        .pairs
                        .iter()
                        .find(|(_, live)| *live == l)
                        .and_then(|(s, _)| windows[*s].desktop.as_deref())
                        .and_then(DesktopId::parse)
                        .filter(|id| {
                            *id == DesktopId::ALL || desktops::name_of(&registry, *id).is_some()
                        });
                    if let Some(target) = saved_desktop.or(before[l]) {
                        self.move_to_desktop(
                            window.hwnd,
                            target,
                            &registry,
                            window.identity.label(),
                        );
                    }
                }
            }
            for (s, l) in &matching.pairs {
                stack.push((windows[*s].z_index, mine[*l].hwnd));
            }

            let lost: Vec<&str> = matching
                .unmatched_saved
                .iter()
                .map(|s| windows[*s].label())
                .collect();
            log::info!(
                "restored {} of {} {exe} windows",
                matching.pairs.len(),
                windows.len()
            );
            if !lost.is_empty() {
                log::info!("not matched: {}", lost.join(" | "));
            }
            missing += lost.len();
            parts.push(format!(
                "{} of {} {} windows",
                matching.pairs.len(),
                windows.len(),
                product_name(&exe)
            ));
        }

        if self.front_order && !stack.is_empty() {
            stack.sort_by(|a, b| b.0.cmp(&a.0));
            for (_, hwnd) in &stack {
                raise(*hwnd);
            }
            if !foreground.is_null() {
                raise(foreground);
            }
        }

        let mut summary = format!("Restored {}", parts.join(", "));
        if missing > 0 {
            summary.push_str(&format!(" ({missing} not found)"));
        }
        log::info!("{summary}");
        host::notify("LayoutKeeper", &summary);
        self.last_restore = Some(format!("{summary} at {}", clock::now_hours_minutes()));
    }

    /// Builds the desktop mover on first use; true when it is usable.
    fn ensure_mover(&mut self, registry: &[Desktop]) -> bool {
        if matches!(self.mover, Mover::Untried) {
            self.mover = match desktops::Mover::new(registry) {
                Ok(mover) => {
                    log::info!("desktop moves are available");
                    Mover::Ready(mover)
                }
                Err(err) => {
                    log::warn!("desktop moves are disabled for this session: {err}");
                    Mover::Disabled(err)
                }
            };
        }
        matches!(self.mover, Mover::Ready(_))
    }

    fn move_to_desktop(
        &mut self,
        hwnd: HWND,
        target: DesktopId,
        registry: &[Desktop],
        label: &str,
    ) {
        if target == DesktopId::ALL {
            return;
        }
        let Some(name) = desktops::name_of(registry, target) else {
            log::info!("\"{label}\" was on desktop {target}, which no longer exists");
            return;
        };
        let now = self.reader.as_ref().and_then(|reader| reader.read(hwnd));
        if now == Some(target) || now == Some(DesktopId::ALL) {
            return;
        }
        let Mover::Ready(mover) = &self.mover else {
            return;
        };
        match mover.move_to(hwnd, target) {
            Ok(()) => log::info!("moved \"{label}\" to {name}"),
            Err(err) => log::warn!("could not move \"{label}\" to {name}: {err}"),
        }
    }

    fn tick(&mut self) {
        self.ticks += 1;
        if self.restore.is_running() {
            let count = windows::enumerate(&self.programs).len();
            match self.restore.step(self.ticks, count) {
                Step::Apply => self.restore_layout(),
                Step::TimedOut => {
                    let names: Vec<String> =
                        self.programs.iter().map(|exe| product_name(exe)).collect();
                    let summary = format!(
                        "No {} windows settled within {} minutes, so nothing was restored",
                        names.join(" or "),
                        self.restore.give_up_seconds / 60
                    );
                    log::info!("{summary}");
                    host::notify("LayoutKeeper", &summary);
                    self.last_restore = Some(summary);
                }
                Step::Wait | Step::Nothing => {}
            }
            return;
        }
        if self.ticks % self.snapshot_seconds.max(1) == 0 {
            self.save("timer");
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

fn handles_of(live: &[windows::LiveWindow], exe: &str) -> Vec<(Handle, identity::WindowIdentity)> {
    live.iter()
        .filter(|window| window.identity.exe == exe)
        .map(|window| (window.hwnd as Handle, window.identity.clone()))
        .collect()
}

/// Puts a window on top of the z-order without activating it.
fn raise(hwnd: HWND) {
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}

fn product_name(exe: &str) -> String {
    match exe {
        "chrome.exe" => "Chrome".to_string(),
        "msedge.exe" => "Edge".to_string(),
        "firefox.exe" => "Firefox".to_string(),
        "brave.exe" => "Brave".to_string(),
        other => other.to_string(),
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

fn flag(settings: &serde_json::Value, key: &str) -> bool {
    settings
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
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
            "restore_on_start": true,
            "settle_seconds": DEFAULT_SETTLE_SECONDS,
            "restore_window_minutes": DEFAULT_RESTORE_MINUTES,
            "snapshot_interval_seconds": DEFAULT_SNAPSHOT_SECONDS,
            "restore_front_order": true,
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
                key: "restore_on_start",
                label: "Restore when WinCraft starts",
                help: "Put the saved layout back once the programs have opened their windows.",
                kind: FieldKind::Toggle,
            },
            SettingField {
                key: "settle_seconds",
                label: "Wait for windows (seconds)",
                help:
                    "The restore starts when the number of windows has not changed for this long.",
                kind: FieldKind::Number {
                    min: 1.0,
                    max: 60.0,
                },
            },
            SettingField {
                key: "restore_window_minutes",
                label: "Give up after (minutes)",
                help: "Stop waiting if the windows never settle.",
                kind: FieldKind::Number {
                    min: 1.0,
                    max: 30.0,
                },
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
            SettingField {
                key: "restore_front_order",
                label: "Restore which window is in front",
                help: "Also put the windows back in their front-to-back order on each desktop.",
                kind: FieldKind::Toggle,
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

        // Switching the plugin off and on again is not a reboot.
        if !self.started_once && self.restore_on_start {
            self.start_restore();
        }
        self.started_once = true;
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        vec![
            HotkeyAction {
                id: ACTION_RESTORE,
                name: "restore_now",
                label: "Restore layout",
                default: win_alt(u32::from(b'L')),
            },
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
            ACTION_RESTORE => self.start_restore(),
            ACTION_SAVE_NOW => self.save("manual"),
            ACTION_MOVE_LEFT => self.shift_front(-1),
            ACTION_MOVE_RIGHT => self.shift_front(1),
            _ => {}
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: ACTION_RESTORE,
            label: "Restore layout",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == ACTION_RESTORE {
            self.start_restore();
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

    fn status(&self) -> Option<String> {
        let saved = match &self.last_saved {
            Some(time) => format!("Layout saved at {time}"),
            None => "Layout not saved yet this session".to_string(),
        };
        let moves = match &self.mover {
            Mover::Untried => "Desktop moves: checked at the first restore".to_string(),
            Mover::Ready(_) => "Desktop moves: available".to_string(),
            Mover::Disabled(reason) => format!("Desktop moves: disabled ({reason})"),
        };
        let mut parts = vec![saved];
        if self.restore.is_running() {
            parts.push("Restore waiting for the windows to settle".to_string());
        } else if let Some(restore) = &self.last_restore {
            parts.push(restore.clone());
        }
        parts.push(moves);
        Some(parts.join(" \u{b7} "))
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, _lparam: LPARAM) {
        match msg {
            WM_TIMER if wparam == TIMER_ID => self.tick(),
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
        self.restore = Restore::new(self.restore.settle_seconds, self.restore.give_up_seconds);
        self.reader = None;
        self.taskbar = None;
        self.mover = Mover::Untried;
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

    #[test]
    fn switches_default_to_on() {
        let settings = serde_json::json!({ "off": false });
        assert!(!flag(&settings, "off"));
        assert!(flag(&settings, "missing"));
    }
}

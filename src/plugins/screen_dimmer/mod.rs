mod ddc;
mod ddc_test;
mod fence;
mod machine;
mod overlay;
mod saved;
mod screens;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    KillTimer, PostMessageW, SetTimer, WM_DISPLAYCHANGE, WM_ENDSESSION, WM_TIMER,
};

use crate::core::traits::{
    DynamicField, FieldKind, HostContext, Hotkey, HotkeyAction, PaletteCommand, PluginMetadata,
    SettingField, TrayAction, WinCraftPlugin,
};
use crate::core::ui_bridge::ActionKind;
use crate::core::{config, host};
use ddc::{Job, Opener, Outcome, Physical, Report, Woke, Worker};
use machine::{Look, Machine, Mode};
use overlay::Overlay;
use saved::StateFile;
use screens::Screen;

/// The plain keys cover, the Shift keys turn off; both count monitors from
/// the left. The cover actions keep the names and ids of the old toggles, so
/// a user's own bindings stay on the same keys.
const ACTION_COVER_BASE: u32 = 1;
const ACTION_OFF_BASE: u32 = 11;
const MONITOR_ACTIONS: u32 = 3;
const ACTION_WAKE_ALL: u32 = 100;
const TRAY_WAKE_ALL: u32 = 1;
const PALETTE_TEST_AGAIN: u32 = 200;

/// While a monitor is off or a DDC/CI job is out: reads the workers'
/// reports.
const TICK_TIMER: usize = 0x5344_0001;
const TICK_MS: u32 = 100;
/// Posted by a worker after each report, so it is read at once.
const NUDGE_TIMER: usize = 0x5344_0002;
/// How long exit waits for the monitors to be asked back on. One request
/// each takes up to 100 ms, and all monitors are asked at once.
const RELEASE_WAIT: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct ScreenDimmer {
    host_hwnd: Option<HWND>,
    /// In the order Windows lists them, which is the order of the hotkeys.
    screens: Vec<Screen>,
    /// One per screen, at the same place in the list.
    overlays: Vec<Option<Overlay>>,
    /// By monitor id, so they outlive a change of the monitor layout; ids
    /// from the state file whose monitor is unplugged keep theirs too.
    machines: BTreeMap<String, Machine>,
    workers: BTreeMap<String, Worker>,
    reports: Option<(Sender<Report>, Receiver<Report>)>,
    modes: BTreeMap<String, Mode>,
    state_path: PathBuf,
    written: StateFile,
    ticking: bool,
    fence: fence::Fence,
    idle_alpha: u8,
    hover_alpha: u8,
}

impl ScreenDimmer {
    pub fn new() -> Self {
        Self::default()
    }

    fn build(&mut self) {
        self.screens = screens::list();
        for screen in &self.screens {
            self.machines
                .entry(screen.id.clone())
                .or_insert_with(|| Machine::new(Default::default()));
        }
        self.overlays = self
            .screens
            .iter()
            .map(|screen| {
                Overlay::create(
                    screen.handle,
                    screen.rect,
                    self.idle_alpha,
                    self.hover_alpha,
                )
            })
            .collect();
        for index in 0..self.screens.len() {
            self.sync_overlay(index);
        }
        let keys: Vec<String> = self
            .screens
            .iter()
            .enumerate()
            .map(|(index, screen)| {
                format!(
                    "F{} = {} ({})",
                    index + 1,
                    screen.place.as_deref().unwrap_or("the only monitor"),
                    screen.id
                )
            })
            .collect();
        log::info!(
            "screen_dimmer covers {} monitor(s); hotkeys from the left: {}",
            self.overlays.iter().flatten().count(),
            keys.join(", ")
        );
    }

    fn destroy_overlays(&mut self) {
        for item in self.overlays.iter_mut().flatten() {
            item.destroy();
        }
        self.overlays.clear();
    }

    /// Monitor handles and rectangles are stale after a layout change, and
    /// so are the DDC/CI handles: everything is built again, while each
    /// monitor keeps its state by id. A worker still busy with a job
    /// finishes it and reports as usual.
    fn rebuild(&mut self) {
        self.destroy_overlays();
        self.workers.clear();
        self.build();
        self.recover();
    }

    /// Whether Windows' monitor list differs from the one the overlays were
    /// built for. Checked before each toggle, so a display change that never
    /// reached the plugin cannot leave it working on the old layout.
    fn layout_changed(&self) -> bool {
        let now = screens::list();
        now.len() != self.screens.len()
            || now.iter().zip(&self.screens).any(|(a, b)| {
                a.handle != b.handle
                    || a.id != b.id
                    || (a.rect.left, a.rect.top, a.rect.right, a.rect.bottom)
                        != (b.rect.left, b.rect.top, b.rect.right, b.rect.bottom)
            })
    }

    fn recover(&mut self) {
        let ids: Vec<String> = self
            .screens
            .iter()
            .filter(|screen| {
                self.machines
                    .get(&screen.id)
                    .is_some_and(Machine::needs_recovery)
            })
            .map(|screen| screen.id.clone())
            .collect();
        for id in ids {
            log::info!("{id} was left in standby or dimmed; putting it back");
            let job = self.machines.get_mut(&id).and_then(Machine::recover);
            self.apply(&id, job);
        }
    }

    fn mode_of(&self, id: &str) -> Mode {
        self.modes.get(id).copied().unwrap_or_default()
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.screens.iter().position(|screen| screen.id == id)
    }

    /// Shows the overlay as the monitor's state asks: solid while it is
    /// off, so nothing shows if it wakes by itself, and see-through under
    /// the pointer while it is only covered or dimmed.
    fn sync_overlay(&mut self, index: usize) {
        let look = self
            .screens
            .get(index)
            .and_then(|screen| self.machines.get(&screen.id))
            .map_or(Look::On, Machine::look);
        let (idle, hover) = (self.idle_alpha, self.hover_alpha);
        let Some(Some(item)) = self.overlays.get_mut(index) else {
            return;
        };
        match look {
            Look::On => item.hide(),
            Look::Off => {
                item.set_alphas(u8::MAX, u8::MAX);
                item.show();
            }
            Look::Covered | Look::Dimmed => {
                item.set_alphas(idle, hover);
                item.show();
            }
        }
    }

    /// Sends the machine's next job, then brings the overlay, the state
    /// file, the timer and the palette up to date. The file is written
    /// before the job goes out too: a crash right after a standby request
    /// must find the record of it at the next start.
    fn apply(&mut self, id: &str, job: Option<Job>) {
        if let Some(job) = job {
            self.write_state();
            self.send(id, job);
        }
        if let Some(index) = self.index_of(id) {
            self.sync_overlay(index);
        }
        self.write_state();
        self.update_timer();
        self.update_fence();
        host::plugin_changed();
    }

    fn update_fence(&mut self) {
        let areas: Vec<fence::Area> = self
            .screens
            .iter()
            .map(|screen| fence::Area {
                rect: [
                    screen.rect.left,
                    screen.rect.top,
                    screen.rect.right,
                    screen.rect.bottom,
                ],
                off: self
                    .machines
                    .get(&screen.id)
                    .is_some_and(|machine| machine.look() == Look::Off),
                primary: screen.primary,
            })
            .collect();
        self.fence.apply(&fence::zones(&areas));
    }

    /// Asks every monitor that is in standby or dimmed back at once, waits
    /// up to `RELEASE_WAIT` for the requests to go out, and stops every
    /// worker. A monitor whose request did not go out keeps its record, so
    /// the next start wakes it.
    fn release_all(&mut self) {
        let mut workers = std::mem::take(&mut self.workers);
        let ids: Vec<String> = self.machines.keys().cloned().collect();
        for id in ids {
            let Some(job) = self.machines.get_mut(&id).and_then(Machine::release) else {
                continue;
            };
            if !workers.contains_key(&id) {
                if let Some(worker) = self.spawn_worker(&id) {
                    workers.insert(id.clone(), worker);
                }
            }
            match workers.get_mut(&id) {
                Some(worker) => {
                    log::info!("{id}: {job:?} before stopping");
                    worker.stop(Some(job));
                }
                None => log::warn!("{id} is not connected; it is woken at the next start"),
            }
        }
        for worker in workers.values_mut() {
            worker.stop(None);
        }
        let all = ddc::wait_all(
            workers.into_values().collect(),
            Instant::now() + RELEASE_WAIT,
        );
        if !all {
            log::warn!("a DDC/CI thread was still busy after {RELEASE_WAIT:?}");
        }
        let reports: Vec<Report> = match &self.reports {
            Some((_, results)) => results.try_iter().collect(),
            None => Vec::new(),
        };
        for report in reports {
            if let (Outcome::Released(ok), Some(machine)) =
                (report.outcome, self.machines.get_mut(&report.monitor))
            {
                machine.released(ok);
                log::info!("{}: put back before stopping: {ok}", report.monitor);
            }
        }
        self.write_state();
        self.fence.release();
    }

    fn send(&mut self, id: &str, job: Job) {
        if !self.workers.contains_key(id) {
            match self.spawn_worker(id) {
                Some(worker) => {
                    self.workers.insert(id.to_string(), worker);
                }
                None => {
                    log::warn!("{id}: no DDC/CI thread for {job:?}");
                    if let Some(machine) = self.machines.get_mut(id) {
                        machine.report(Outcome::NoMonitor);
                    }
                    return;
                }
            }
        }
        log::info!("{id}: {job:?}");
        if let Some(worker) = self.workers.get(id) {
            worker.send(job);
        }
    }

    fn spawn_worker(&self, id: &str) -> Option<Worker> {
        let handle = self
            .screens
            .iter()
            .find(|screen| screen.id == id)
            .map(|screen| screen.handle as isize)?;
        let (reports, _) = self.reports.as_ref()?;
        let host = self.host_hwnd.map_or(0, |hwnd| hwnd as isize);
        let nudge: ddc::Nudge = Arc::new(move || {
            if host != 0 {
                unsafe { PostMessageW(host as HWND, WM_TIMER, NUDGE_TIMER, 0) };
            }
        });
        let open: Opener = Box::new(move |stop| {
            Physical::open(handle, stop).map(|monitor| Box::new(monitor) as Box<dyn ddc::Monitor>)
        });
        Worker::spawn(id, open, reports.clone(), nudge)
    }

    fn drain(&mut self) {
        let reports: Vec<Report> = match &self.reports {
            Some((_, results)) => results.try_iter().collect(),
            None => return,
        };
        for report in reports {
            let Some(machine) = self.machines.get_mut(&report.monitor) else {
                continue;
            };
            let next = machine.report(report.outcome);
            let now = machine.describe();
            match report.outcome {
                Outcome::Woke(Woke::TimedOut) => log::warn!(
                    "{} did not report on within {:?}; overlay removed anyway",
                    report.monitor,
                    ddc::WAKE_FOR
                ),
                Outcome::StandbySent(false) | Outcome::BrightnessSet(false) => {
                    log::warn!("{}: {:?} failed; now {now}", report.monitor, report.job)
                }
                Outcome::NoMonitor => log::warn!(
                    "{}: Windows gave no DDC/CI handle; now {now}",
                    report.monitor
                ),
                outcome => log::info!(
                    "{}: {:?} gave {outcome:?}; now {now}",
                    report.monitor,
                    report.job
                ),
            }
            self.apply(&report.monitor.clone(), next);
        }
    }

    fn write_state(&mut self) {
        let file = StateFile {
            monitors: self
                .machines
                .iter()
                .filter(|(_, machine)| !machine.saved.is_empty())
                .map(|(id, machine)| (id.clone(), machine.saved.clone()))
                .collect(),
        };
        if file == self.written {
            return;
        }
        match saved::save(&self.state_path, &file) {
            Ok(()) => self.written = file,
            Err(err) => log::error!("{err}"),
        }
    }

    /// The timer runs only while there is something to watch.
    fn update_timer(&mut self) {
        let Some(hwnd) = self.host_hwnd else {
            return;
        };
        let wanted = self
            .machines
            .values()
            .any(|machine| machine.busy() || machine.look() == Look::Off);
        if wanted && !self.ticking {
            self.ticking = unsafe { SetTimer(hwnd, TICK_TIMER, TICK_MS, None) } != 0;
        } else if !wanted && self.ticking {
            unsafe { KillTimer(hwnd, TICK_TIMER) };
            self.ticking = false;
        }
    }

    fn press(&mut self, index: usize, shift: bool) {
        if self.layout_changed() {
            log::info!("the monitor layout changed since the overlays were built");
            self.rebuild();
        }
        let Some(id) = self.screens.get(index).map(|screen| screen.id.clone()) else {
            log::info!("no monitor {} from the left", index + 1);
            return;
        };
        let mode = self.mode_of(&id);
        let job = self.machines.get_mut(&id).and_then(|machine| {
            if shift {
                machine.off_key(mode)
            } else {
                machine.cover_key()
            }
        });
        if let Some(machine) = self.machines.get(&id) {
            let key = if shift {
                format!("Shift key, mode {}", mode.label())
            } else {
                "cover key".to_string()
            };
            log::info!(
                "monitor {} from the left ({id}), {key}: {}",
                index + 1,
                machine.describe()
            );
        }
        self.apply(&id, job);
    }

    fn wake_all(&mut self) {
        let ids: Vec<String> = self
            .screens
            .iter()
            .map(|screen| screen.id.clone())
            .collect();
        for id in ids {
            let job = self.machines.get_mut(&id).and_then(Machine::wake);
            self.apply(&id, job);
        }
        log::info!("all monitors asked back on");
    }

    /// Auto tests each monitor again the next time it goes off.
    fn test_again(&mut self) {
        for machine in self.machines.values_mut() {
            machine.forget_tests();
        }
        self.write_state();
        log::info!("screen_dimmer forgot what testing the monitors found");
        host::plugin_changed();
    }
}

fn alpha_from(settings: &serde_json::Value, key: &str, fallback: f64) -> u8 {
    let value = settings
        .get(key)
        .and_then(|v| v.as_f64())
        .unwrap_or(fallback);
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `monitors.<id>.mode` for each monitor that has one. The settings page
/// writes "Auto", "Off", "Dim" or "Cover"; the lower-case words and
/// "overlay" are read too.
fn modes_from(settings: &serde_json::Value) -> BTreeMap<String, Mode> {
    settings
        .get("monitors")
        .and_then(serde_json::Value::as_object)
        .map(|monitors| {
            monitors
                .iter()
                .filter_map(|(id, monitor)| {
                    let mode = monitor.get("mode")?.as_str()?;
                    Some((id.clone(), Mode::parse(mode)))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl WinCraftPlugin for ScreenDimmer {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "screen_dimmer",
            name: "ScreenDimmer",
            description:
                "Cover monitors with click-through overlays, or turn them off over DDC/CI.",
            author: "community",
            version: "1.3.0",
            readme: include_str!("README.md"),
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({ "idle_opacity": 1.0, "hover_opacity": 0.7, "monitors": {} })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![
            SettingField {
                key: "idle_opacity",
                label: "Darkness",
                help: "How solid the overlay is normally. 1.0 is fully black.",
                kind: FieldKind::Slider {
                    min: 0.0,
                    max: 1.0,
                    step: 0.05,
                },
            },
            SettingField {
                key: "hover_opacity",
                label: "Darkness under the pointer",
                help: "How solid it is while your pointer is on that monitor.",
                kind: FieldKind::Slider {
                    min: 0.0,
                    max: 1.0,
                    step: 0.05,
                },
            },
        ]
    }

    /// One mode choice per connected monitor, keyed by its id.
    fn dynamic_fields(&self) -> Vec<DynamicField> {
        self.screens
            .iter()
            .map(|screen| {
                let found = self
                    .machines
                    .get(&screen.id)
                    .map_or("not tested yet", Machine::capability);
                let label = match &screen.place {
                    Some(place) => format!("Shift key turns the {place} monitor"),
                    None => "Shift key turns this monitor".to_string(),
                };
                DynamicField {
                    key: format!("/monitors/{}/mode", config::pointer_token(&screen.id)),
                    label,
                    help: format!(
                        "{}: {found}. The plain key always only covers it. Auto turns it \
                         off when standby works, dims it when only its brightness can be set, \
                         and covers it otherwise.",
                        screen.id
                    ),
                    kind: FieldKind::Choice(Mode::CHOICES),
                    value: serde_json::Value::String(self.mode_of(&screen.id).label().to_string()),
                }
            })
            .collect()
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.idle_alpha = alpha_from(settings, "idle_opacity", 1.0);
        self.hover_alpha = alpha_from(settings, "hover_opacity", 0.7);
        self.modes = modes_from(settings);
        for index in 0..self.screens.len() {
            self.sync_overlay(index);
        }
        true
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        // WM_DISPLAYCHANGE reaches this plugin only through the host window, so
        // its handle is worth having in the log when message routing misbehaves.
        log::debug!("screen_dimmer attached to host window {:p}", ctx.hwnd);
        self.host_hwnd = Some(ctx.hwnd);
        self.idle_alpha = alpha_from(ctx.settings, "idle_opacity", 1.0);
        self.hover_alpha = alpha_from(ctx.settings, "hover_opacity", 0.7);
        self.modes = modes_from(ctx.settings);
        self.reports = Some(mpsc::channel());
        if self.state_path.as_os_str().is_empty() {
            self.state_path = saved::default_path();
        }
        self.written = saved::load(&self.state_path);
        self.machines = self
            .written
            .monitors
            .iter()
            .map(|(id, saved)| (id.clone(), Machine::new(saved.clone())))
            .collect();
        self.build();
        if self.overlays.iter().flatten().next().is_none() {
            return Err("no monitors could be covered".to_string());
        }
        self.recover();
        Ok(())
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        // Adding a fourth monitor later is one more pair in this list.
        const COVER_NAMES: [&str; 3] = ["toggle_monitor_1", "toggle_monitor_2", "toggle_monitor_3"];
        const OFF_NAMES: [&str; 3] = ["off_monitor_1", "off_monitor_2", "off_monitor_3"];
        const F_KEYS: [u32; 3] = [0x70, 0x71, 0x72];
        let (cover, off) = labels(self.screens.len());
        let mut actions = Vec::new();
        for index in 0..MONITOR_ACTIONS as usize {
            actions.push(HotkeyAction {
                id: ACTION_COVER_BASE + index as u32,
                name: COVER_NAMES[index],
                label: cover[index],
                default: win_alt(F_KEYS[index]),
            });
            actions.push(HotkeyAction {
                id: ACTION_OFF_BASE + index as u32,
                name: OFF_NAMES[index],
                label: off[index],
                default: Hotkey {
                    modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT | MOD_SHIFT,
                    vk: F_KEYS[index],
                },
            });
        }
        actions.push(HotkeyAction {
            id: ACTION_WAKE_ALL,
            name: "wake_all",
            label: "Wake all monitors",
            default: win_alt(0x7B),
        });
        actions
    }

    fn on_hotkey(&mut self, action_id: u32) {
        if action_id == ACTION_WAKE_ALL {
            self.wake_all();
        } else if let Some((index, shift)) = monitor_action(action_id) {
            self.press(index, shift);
        }
    }

    fn on_windows_message(&mut self, msg: u32, wparam: WPARAM, _lparam: LPARAM) {
        match msg {
            WM_DISPLAYCHANGE => self.rebuild(),
            WM_TIMER if wparam == TICK_TIMER || wparam == NUDGE_TIMER => {
                self.drain();
                self.update_fence();
            }
            // Windows may end the process without a teardown after this.
            WM_ENDSESSION if wparam != 0 => {
                self.release_all();
                for index in 0..self.screens.len() {
                    self.sync_overlay(index);
                }
            }
            _ => {}
        }
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction {
            id: TRAY_WAKE_ALL,
            label: "Wake all monitors",
        }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == TRAY_WAKE_ALL {
            self.wake_all();
        }
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: PALETTE_TEST_AGAIN,
            label: "Test monitors again",
            hint: "Auto checks standby again the next time each monitor goes off",
        }]
    }

    fn on_palette_command(&mut self, id: u32) {
        if id == PALETTE_TEST_AGAIN {
            self.test_again();
        }
    }

    fn page_action(&self) -> Option<u32> {
        Some(PALETTE_TEST_AGAIN)
    }

    fn status(&self) -> Option<String> {
        let parts: Vec<String> = self
            .screens
            .iter()
            .filter_map(|screen| {
                let machine = self.machines.get(&screen.id)?;
                Some(format!("{}: {}", screen.id, machine.describe()))
            })
            .collect();
        (!parts.is_empty()).then(|| parts.join(" \u{b7} "))
    }

    fn palette_subtitle(&self, kind: ActionKind, action_id: u32) -> Option<String> {
        if kind != ActionKind::Hotkey {
            return None;
        }
        let (index, _) = monitor_action(action_id)?;
        let screen = self.screens.get(index)?;
        let state = self.machines.get(&screen.id)?.describe();
        Some(match &screen.place {
            Some(place) => format!("{state} \u{b7} {place}"),
            None => state.to_string(),
        })
    }

    fn teardown(&mut self) {
        self.release_all();
        if let Some(hwnd) = self.host_hwnd.take() {
            if self.ticking {
                unsafe { KillTimer(hwnd, TICK_TIMER) };
                self.ticking = false;
            }
        }
        self.destroy_overlays();
        self.screens.clear();
    }
}

/// `--ddc-test=<value>` in a test instance; see `ddc_test`.
pub fn run_ddc_test(value: &str) -> i32 {
    ddc_test::run(value)
}

/// Which monitor from the left a hotkey action is for, and whether it is
/// the Shift (off) one.
fn monitor_action(action_id: u32) -> Option<(usize, bool)> {
    let range = |base: u32| base..base + MONITOR_ACTIONS;
    if range(ACTION_COVER_BASE).contains(&action_id) {
        Some(((action_id - ACTION_COVER_BASE) as usize, false))
    } else if range(ACTION_OFF_BASE).contains(&action_id) {
        Some(((action_id - ACTION_OFF_BASE) as usize, true))
    } else {
        None
    }
}

type Labels = [&'static str; 3];

/// The palette names of the cover and off actions for this many monitors,
/// counted from the left as the hotkeys are.
fn labels(monitors: usize) -> (Labels, Labels) {
    match monitors {
        0 | 1 => (
            ["Cover the monitor", "Cover monitor 2", "Cover monitor 3"],
            [
                "Turn the monitor off",
                "Turn monitor 2 off",
                "Turn monitor 3 off",
            ],
        ),
        2 => (
            [
                "Cover left monitor",
                "Cover right monitor",
                "Cover monitor 3",
            ],
            [
                "Turn left monitor off",
                "Turn right monitor off",
                "Turn monitor 3 off",
            ],
        ),
        3 => (
            [
                "Cover left monitor",
                "Cover centre monitor",
                "Cover right monitor",
            ],
            [
                "Turn left monitor off",
                "Turn centre monitor off",
                "Turn right monitor off",
            ],
        ),
        _ => (
            ["Cover monitor 1", "Cover monitor 2", "Cover monitor 3"],
            [
                "Turn monitor 1 off",
                "Turn monitor 2 off",
                "Turn monitor 3 off",
            ],
        ),
    }
}

fn win_alt(vk: u32) -> Hotkey {
    Hotkey {
        modifiers: MOD_NOREPEAT | MOD_WIN | MOD_ALT,
        vk,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{pointer_token, set_pointer};

    #[test]
    fn a_mode_chosen_on_the_page_is_read_back_by_monitor() {
        let mut settings = ScreenDimmer::new().default_settings();
        for (id, mode) in [("PHL 323E7", "Off"), ("LG SDQHD", "Cover")] {
            let key = format!("/monitors/{}/mode", pointer_token(id));
            set_pointer(&mut settings, &key, serde_json::Value::String(mode.into()));
        }
        settings["monitors"]["AORUS FV43U"] = serde_json::json!({ "mode": "dim" });
        let modes = modes_from(&settings);
        assert_eq!(modes.get("PHL 323E7"), Some(&Mode::Off));
        assert_eq!(modes.get("LG SDQHD"), Some(&Mode::Cover));
        assert_eq!(modes.get("AORUS FV43U"), Some(&Mode::Dim));
        assert_eq!(modes.len(), 3);
        assert!(modes_from(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn each_monitor_has_a_cover_key_and_a_shift_off_key() {
        let plugin = ScreenDimmer::new();
        let actions = plugin.hotkey_actions();
        let names: Vec<(&str, String)> = actions
            .iter()
            .map(|action| (action.name, crate::core::hotkeys::format(action.default)))
            .collect();
        let expected = [
            ("toggle_monitor_1", "Win+Alt+F1"),
            ("off_monitor_1", "Win+Alt+Shift+F1"),
            ("toggle_monitor_2", "Win+Alt+F2"),
            ("off_monitor_2", "Win+Alt+Shift+F2"),
            ("toggle_monitor_3", "Win+Alt+F3"),
            ("off_monitor_3", "Win+Alt+Shift+F3"),
            ("wake_all", "Win+Alt+F12"),
        ];
        assert_eq!(names.len(), expected.len());
        for ((name, keys), (want_name, want_keys)) in names.iter().zip(expected) {
            assert_eq!(*name, want_name);
            assert_eq!(keys, want_keys, "{name}");
        }
        for action in &actions {
            if action.id != ACTION_WAKE_ALL {
                assert!(monitor_action(action.id).is_some(), "{}", action.name);
            }
        }
        assert_eq!(monitor_action(1), Some((0, false)));
        assert_eq!(monitor_action(13), Some((2, true)));
        assert_eq!(monitor_action(ACTION_WAKE_ALL), None);
    }

    #[test]
    fn the_palette_names_follow_the_monitors_from_the_left() {
        assert_eq!(labels(2).0[1], "Cover right monitor");
        assert_eq!(
            labels(3).1,
            [
                "Turn left monitor off",
                "Turn centre monitor off",
                "Turn right monitor off"
            ]
        );
        assert_eq!(labels(4).0[0], "Cover monitor 1");
        assert_eq!(labels(1).1[0], "Turn the monitor off");
    }
}

//! The Arrange strip in place of Windows' taskbar hover preview, for apps
//! with two or more windows. Off by default.
//!
//! How the mouse is watched, cheaply:
//! - While the mouse is away from the taskbar, a 100 ms timer reads the
//!   cursor and compares it with the taskbars' rectangles. Nothing else.
//! - On a taskbar, the timer runs every 15 ms. Once the mouse has been still
//!   for 60 ms, UI Automation is asked, on a thread of its own, which button
//!   it is on; moving along the taskbar asks nothing.
//! - Also only on a taskbar, two WinEvent hooks tell the moment Explorer
//!   shows its popup, so it can be hidden before it is drawn. They are
//!   removed when the mouse leaves.
//!
//! Hiding is a plain `ShowWindowAsync(SW_HIDE)` of Explorer's popup, never a
//! move, so the popup comes back on the next hover if WinCraft stops. The
//! key rules in `keys::decide` keep Win+number and Alt+Tab untouched.

pub mod keys;
mod machine;
pub mod popup;

use std::cell::Cell;
use std::sync::mpsc::TryRecvError;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetCursorPos, GetForegroundWindow, GetWindowRect,
    GetWindowThreadProcessId, KillTimer, SetTimer, CHILDID_SELF, EVENT_OBJECT_SHOW,
    EVENT_OBJECT_UNCLOAKED, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

use crate::core::windows_list::class_name;
use crate::core::{taskbar, ui_bridge, wide};
use keys::POPUP_CLASS;
use machine::{contains, Answer, Button, Gate, Look, Machine, Point, Rect};
pub use machine::{delay_ms, Command, DEFAULT_DELAY_MS};
use popup::Method;

/// The host window is shared; "LK" and 2 keep it clear of LayoutKeeper's
/// other timers.
pub const TIMER_ID: usize = 0x4C4B_0002;
const IDLE_MS: u32 = 100;
const FAST_MS: u32 = 15;
/// How often the taskbars' rectangles are read again, for a taskbar that
/// moved, appeared on a new monitor or hides itself.
const TASKBARS_MS: u64 = 2000;

thread_local! {
    /// When Windows' popup may be hidden now. The WinEvent callback runs on
    /// the host thread, possibly while a plugin call is on the stack, so it
    /// reads only this copy and never the plugin.
    static GATE: Cell<Option<Gate>> = const { Cell::new(None) };
    static HIDDEN: Cell<u32> = const { Cell::new(0) };
}

#[derive(Default)]
pub struct HoverPreview {
    machine: Machine,
    hwnd: Option<HWND>,
    timer_ms: Option<u32>,
    hooks: Vec<HWINEVENTHOOK>,
    lookup: Option<(Point, taskbar::Lookup)>,
    answer: Option<Answer>,
    taskbars: Vec<Rect>,
    taskbars_at: Option<u64>,
    /// False in a test instance without `--allow-hide-popup`: Windows'
    /// popup is then never hidden.
    pub may_hide: bool,
    /// The hover the test instance's refusal was last logged for.
    refusal_noted: bool,
}

impl HoverPreview {
    pub fn is_on(&self) -> bool {
        self.machine.on
    }

    /// Applies the switch and the delay. Off stops every timer and hook and
    /// closes an open strip at once.
    pub fn set(&mut self, hwnd: Option<HWND>, on: bool, delay: u64) -> Vec<Command> {
        self.machine.delay_ms = delay;
        if hwnd.is_some() {
            self.hwnd = hwnd;
        }
        let mut out = Vec::new();
        if on != self.machine.on {
            out = self.machine.switch(on);
            log::info!(
                "taskbar hover strip {}",
                if on {
                    "on: apps with 2 or more windows show the Arrange strip"
                } else {
                    "off: Windows' own preview is left alone"
                }
            );
            if on && !self.may_hide {
                log::info!(
                    "read-only test instance: the hover strip opens, but Windows' taskbar preview \
                     is never hidden (start with --allow-hide-popup to allow it)"
                );
            }
        }
        if on {
            if self.timer_ms.is_none() {
                self.set_timer(IDLE_MS);
            }
        } else {
            self.stop_watching();
        }
        out
    }

    /// For teardown: off, and nothing left behind.
    pub fn stop(&mut self) -> Vec<Command> {
        let out = self.machine.switch(false);
        self.stop_watching();
        out
    }

    /// One look at the mouse and keys. `windows_of` counts the windows of an
    /// app that the taskbar button at the given rectangle shows, from its
    /// taskbar app ID.
    pub fn tick(&mut self, windows_of: &dyn Fn(&str, Rect) -> usize) -> Vec<Command> {
        if !self.machine.on {
            return Vec::new();
        }
        let now = unsafe { GetTickCount64() };
        if self
            .taskbars_at
            .is_none_or(|at| now.saturating_sub(at) >= TASKBARS_MS)
        {
            self.taskbars = taskbar_rects();
            self.taskbars_at = Some(now);
        }
        let cursor = cursor();
        let on_taskbar = self.taskbars.iter().any(|rect| contains(*rect, cursor));
        let keys = popup::key_state();
        let strip_hwnd = ui_bridge::arrange_window();
        let strip = strip_hwnd.and_then(|hwnd| window_rect(hwnd as HWND));
        let strip_focused =
            strip_hwnd.is_some_and(|hwnd| unsafe { GetForegroundWindow() } == hwnd as HWND);
        let look = Look {
            now,
            cursor,
            on_taskbar,
            keys,
            strip,
            strip_focused,
            answer: self.take_answer(windows_of),
        };
        let was_open = self
            .machine
            .is_open()
            .then(|| self.machine.app().unwrap_or_default().to_string());
        let mut out = Vec::new();
        for command in self.machine.step(&look) {
            match command {
                Command::Lookup(at) => match taskbar::lookup(at.0, at.1) {
                    Some(receiver) => self.lookup = Some((at, receiver)),
                    None => self.answer = Some(Answer { at, button: None }),
                },
                Command::Open { ref app, .. } => {
                    log::info!("hover strip opens for {app}");
                    out.push(command);
                }
                Command::Close => out.push(command),
            }
        }
        if let (Some(app), false) = (was_open, self.machine.is_open()) {
            log::info!(
                "hover strip for {app} closed; Windows' preview was hidden {} times during the hover",
                HIDDEN.with(|cell| cell.replace(0))
            );
        }

        let gate = self.machine.gate(strip);
        if gate.is_some() && !self.may_hide && !self.refusal_noted {
            log::info!("read-only test instance: hiding Windows' taskbar preview skipped");
        }
        self.refusal_noted = gate.is_some();
        let gate = gate.filter(|_| self.may_hide);
        GATE.with(|cell| cell.set(gate));
        if let Some(gate) = gate {
            // In case a WinEvent came late or not at all.
            for hwnd in popup::find_popups() {
                if popup::is_shown(hwnd) && gate.allows(cursor, keys, now) {
                    hide(hwnd);
                }
            }
        }

        if self.machine.engaged() {
            self.start_hooks();
            self.set_timer(FAST_MS);
        } else {
            self.stop_hooks();
            self.set_timer(IDLE_MS);
        }
        out
    }

    fn take_answer(&mut self, windows_of: &dyn Fn(&str, Rect) -> usize) -> Option<Answer> {
        if let Some(answer) = self.answer.take() {
            return Some(answer);
        }
        let (at, receiver) = self.lookup.as_ref()?;
        let at = *at;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the lookup thread ended".to_string()),
        };
        self.lookup = None;
        let button = match result {
            Ok(button) => button,
            Err(reason) => {
                log::debug!("taskbar button lookup failed: {reason}");
                None
            }
        };
        let button = button.map(|button| Button {
            windows: windows_of(&button.app, button.rect),
            app: button.app,
            rect: button.rect,
        });
        if let Some(button) = button.as_ref().filter(|button| !button.eligible()) {
            log::debug!(
                "not eligible: {}, {} windows counted",
                button.app,
                button.windows
            );
        }
        Some(Answer { at, button })
    }

    fn set_timer(&mut self, ms: u32) {
        let Some(hwnd) = self.hwnd else {
            return;
        };
        if self.timer_ms == Some(ms) {
            return;
        }
        if unsafe { SetTimer(hwnd, TIMER_ID, ms, None) } == 0 {
            log::warn!("the taskbar hover strip could not start its timer");
            self.timer_ms = None;
        } else {
            self.timer_ms = Some(ms);
        }
    }

    fn start_hooks(&mut self) {
        if !self.hooks.is_empty() || !self.may_hide {
            return;
        }
        let tray = unsafe { FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null()) };
        let mut explorer = 0u32;
        unsafe { GetWindowThreadProcessId(tray, &mut explorer) };
        if explorer == 0 {
            return;
        }
        for event in [EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED] {
            let hook = unsafe {
                SetWinEventHook(
                    event,
                    event,
                    std::ptr::null_mut(),
                    Some(on_event),
                    explorer,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            if !hook.is_null() {
                self.hooks.push(hook);
            }
        }
    }

    fn stop_hooks(&mut self) {
        for hook in self.hooks.drain(..) {
            unsafe { UnhookWinEvent(hook) };
        }
    }

    fn stop_watching(&mut self) {
        self.stop_hooks();
        GATE.with(|cell| cell.set(None));
        if let (Some(hwnd), Some(_)) = (self.hwnd, self.timer_ms.take()) {
            unsafe { KillTimer(hwnd, TIMER_ID) };
        }
        self.lookup = None;
        self.answer = None;
        self.taskbars_at = None;
    }
}

fn hide(hwnd: HWND) {
    popup::hide(hwnd, Method::ShowWindowAsync);
    HIDDEN.with(|cell| cell.set(cell.get() + 1));
}

/// Explorer showed or uncloaked a window. Out of context, so it arrives
/// through the host's message loop and Explorer never waits for it.
unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != OBJID_WINDOW || id_child != CHILDID_SELF as i32 || hwnd.is_null() {
        return;
    }
    let Some(gate) = GATE.with(|cell| cell.get()) else {
        return;
    };
    if class_name(hwnd) != POPUP_CLASS {
        return;
    }
    if gate.allows(cursor(), popup::key_state(), unsafe { GetTickCount64() }) {
        hide(hwnd);
    }
}

fn cursor() -> Point {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}

fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    (unsafe { GetWindowRect(hwnd, &mut rect) } != 0).then_some([
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
    ])
}

/// The main taskbar and the one on each other monitor.
fn taskbar_rects() -> Vec<Rect> {
    let mut rects = Vec::new();
    let main = unsafe { FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null()) };
    rects.extend(window_rect(main).filter(|_| !main.is_null()));
    let class = wide("Shell_SecondaryTrayWnd");
    let mut after: HWND = std::ptr::null_mut();
    while rects.len() < 16 {
        after = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                after,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if after.is_null() {
            break;
        }
        rects.extend(window_rect(after));
    }
    rects
}

//! Keys follow the mouse into the hover strip: while the strip that a rest
//! on a taskbar button opened is under the mouse and does not have the
//! keyboard, a low-level keyboard hook keeps the strip's keys (digits, the
//! arrows, Enter, Esc, Ctrl+W, Ctrl+N, F2, D) from the app in front and
//! hands them to the strip. Nothing moves the keyboard focus, and every
//! other key, Win and Alt with anything included, goes on as usual
//! (Principle 1). The strip says so on screen while it lasts.
//!
//! The hook is in only while keys go to the strip, and a moment after, for
//! the release of a routed key (Principle 5). It runs on a thread of its
//! own (`core::ll_hook`) and only decides and posts; a side thread hands
//! the key to the UI, which may make it wait.
//!
//! A read-only test instance never puts the hook in: it only logs that it
//! would have.

mod rule;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard, PoisonError};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

use crate::core::ll_hook::HookThread;
use crate::core::ui_bridge::ArrangeKey;
use crate::core::{host, mask_key};
pub use rule::{deck_key, keep_hook, Router, Step, RELEASE_WAIT_MS};

/// Whether keys go to the strip now; the hook reads it for every key.
static ROUTING: AtomicBool = AtomicBool::new(false);
/// `Router::held` of the hook's router, for the host thread.
static HELD: AtomicUsize = AtomicUsize::new(0);
static WORKER: Mutex<Option<HookThread>> = Mutex::new(None);
static KEYS: Mutex<Option<mpsc::Sender<ArrangeKey>>> = Mutex::new(None);
static SIDE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

thread_local! {
    /// Only the hook's own thread uses it.
    static ROUTER: RefCell<Router> = RefCell::new(Router::default());
}

fn lock<T>(cell: &'static Mutex<T>) -> MutexGuard<'static, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The plugin's side of it: turns routing on and off as the hover strip and
/// the mouse say, and puts the hook in and out.
#[derive(Debug, Default)]
pub struct HoverKeys {
    /// False in a read-only test instance: no hook, only a log line.
    pub may_route: bool,
    routing: bool,
    /// When routing last stopped, for `keep_hook`.
    off_at: u64,
    /// Windows refused the hook during this routing; said once.
    refused: bool,
}

impl HoverKeys {
    /// Routing should be `wanted` now (see `hover_preview::routes_keys`).
    pub fn update(&mut self, wanted: bool, now: u64) {
        if wanted != self.routing {
            self.routing = wanted;
            host::detailed(format_args!(
                "deck: keys follow the mouse {}",
                if wanted { "on" } else { "off" }
            ));
            if !wanted {
                self.off_at = now;
            }
            if self.may_route {
                ROUTING.store(wanted, Ordering::Relaxed);
                if wanted && !self.refused && !start() {
                    self.refused = true;
                    log::warn!(
                        "Windows refused the keyboard hook for the hover strip; its keys need a click into it"
                    );
                }
                host::arrange_keys_routed(wanted && is_running());
            } else if wanted {
                log::info!(
                    "read-only test instance: keys following the mouse into the hover strip skipped"
                );
            }
        }
        if is_running()
            && !keep_hook(
                self.routing,
                HELD.load(Ordering::Relaxed),
                now.saturating_sub(self.off_at),
            )
        {
            stop();
        }
    }

    /// For the hover strip going off: routing ends and the hook goes now.
    pub fn stop(&mut self) {
        let was = std::mem::take(&mut self.routing);
        self.refused = false;
        ROUTING.store(false, Ordering::Relaxed);
        if is_running() {
            stop();
        }
        if was && self.may_route {
            host::arrange_keys_routed(false);
        }
    }

    pub fn is_routing(&self) -> bool {
        self.routing
    }
}

fn start() -> bool {
    let mut worker = lock(&WORKER);
    if worker.is_none() {
        start_side_work();
        *worker = HookThread::start("hover-keys hook", vec![(WH_KEYBOARD_LL, Some(hook_proc))]);
        if worker.is_none() {
            stop_side_work();
        }
    }
    worker.is_some()
}

fn stop() {
    let thread = lock(&WORKER).take();
    if let Some(thread) = thread {
        thread.stop();
        stop_side_work();
    }
    HELD.store(0, Ordering::Relaxed);
}

pub fn is_running() -> bool {
    lock(&WORKER).is_some()
}

fn start_side_work() {
    let (keys, inbox) = mpsc::channel::<ArrangeKey>();
    let thread = std::thread::Builder::new()
        .name("hover-keys side work".to_string())
        .spawn(move || {
            for key in inbox {
                host::route_arrange_key(key);
            }
        });
    match thread {
        Ok(thread) => {
            *lock(&KEYS) = Some(keys);
            *lock(&SIDE) = Some(thread);
        }
        Err(err) => log::warn!("hover-keys hook: no side thread ({err}); it works inline"),
    }
}

fn stop_side_work() {
    lock(&KEYS).take();
    if let Some(thread) = lock(&SIDE).take() {
        let _ = thread.join();
    }
}

fn hand_off(key: ArrangeKey) {
    let sent = match lock(&KEYS).as_ref() {
        Some(keys) => keys.send(key).is_ok(),
        None => false,
    };
    if !sent {
        host::route_arrange_key(key);
    }
}

fn key_down(vk: u32) -> bool {
    (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let began = std::time::Instant::now();
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let event = wparam as u32;
    let is_down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;
    let is_up = event == WM_KEYUP || event == WM_SYSKEYUP;
    if !is_down && !is_up {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let vk = info.vkCode;
    let ours = mask_key::is_ours(info.dwExtraInfo) || vk == mask_key::VK;
    let routing = ROUTING.load(Ordering::Relaxed);
    let step = ROUTER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut router) => {
            let modifiers = super::win_digit::modifiers_now();
            let was_down = is_down && key_down(vk);
            let step = router.step(vk, is_down, modifiers, routing, was_down, ours);
            HELD.store(router.held(), Ordering::Relaxed);
            step
        }
        Err(_) => Step::Pass,
    });
    let result = match step {
        Step::Pass => unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) },
        Step::Hide => 1,
        Step::Route(key) => {
            hand_off(key);
            1
        }
    };
    if host::detailed_on() {
        let lag = (unsafe { GetTickCount64() } as u32).wrapping_sub(info.time);
        host::DetailedKey {
            hook: "hover-keys",
            vk,
            message: event,
            flags: info.flags,
            extra: info.dwExtraInfo,
            lag_ms: lag,
            took_ms: began.elapsed().as_secs_f64() * 1000.0,
        }
        .write(&step);
    }
    result
}

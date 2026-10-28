//! The detailed log's two watchers, on one thread that exists only while
//! the log is on. Both only read; neither changes a key or a window.
//!
//! - The key-state watcher reads the Windows keys about every 5 ms, and
//!   the digit keys too while a Windows key is down, and writes a short
//!   line when they change. It records a key Windows saw that no WinCraft
//!   hook was called for. Windows' timer may make the real step up to
//!   about 16 ms; WinCraft does not change the system timer to make it
//!   finer.
//! - The window-in-front watcher writes every change of the window in
//!   front (a WinEvent hook for `EVENT_SYSTEM_FOREGROUND`), and says
//!   whether WinCraft asked for it (see `note_ask`).

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetWindowThreadProcessId, MsgWaitForMultipleObjects, PeekMessageW,
    PostThreadMessageW, TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG, PM_NOREMOVE, PM_REMOVE,
    QS_ALLINPUT, WINEVENT_OUTOFCONTEXT, WM_QUIT,
};

use crate::core::windows_list;

const POLL_MS: u32 = 5;
const LWIN: u32 = 1;
const RWIN: u32 = 1 << 1;
/// Digit `n` (the top-row key) is bit `DIGIT_SHIFT + n`.
const DIGIT_SHIFT: u32 = 2;

struct Running {
    thread_id: u32,
    thread: std::thread::JoinHandle<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn running() -> std::sync::MutexGuard<'static, Option<Running>> {
    RUNNING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts the watcher thread, unless it runs.
pub fn start() {
    let mut slot = running();
    if slot.is_some() {
        return;
    }
    let (ready, answer) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("detailed watch".to_string())
        .spawn(move || run(ready));
    let Ok(thread) = thread else {
        log::warn!("detailed log: no watcher thread");
        return;
    };
    match answer.recv() {
        Ok(thread_id) => *slot = Some(Running { thread_id, thread }),
        Err(_) => {
            let _ = thread.join();
        }
    }
}

/// Ends the watcher thread and waits for it.
pub fn stop() {
    let Some(running) = running().take() else {
        return;
    };
    unsafe { PostThreadMessageW(running.thread_id, WM_QUIT, 0, 0) };
    let _ = running.thread.join();
}

fn run(ready: std::sync::mpsc::Sender<u32>) {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // Makes the thread's message queue, so `stop` can always reach it.
    unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_null() {
        crate::detailed!("front: the window-in-front watcher could not start");
    }
    let _ = ready.send(unsafe { GetCurrentThreadId() });
    let mut keys = 0u32;
    'outer: loop {
        unsafe { MsgWaitForMultipleObjects(0, std::ptr::null(), 0, POLL_MS, QS_ALLINPUT) };
        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if msg.message == WM_QUIT {
                break 'outer;
            }
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let now = read_keys();
        if let Some(line) = key_change(keys, now) {
            crate::detailed!("{line}");
        }
        keys = now;
    }
    if !hook.is_null() {
        unsafe { UnhookWinEvent(hook) };
    }
}

fn down(vk: u32) -> bool {
    (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
}

/// The Windows keys, and the digits while one of them is down.
fn read_keys() -> u32 {
    let mut state = 0;
    if down(0x5B) {
        state |= LWIN;
    }
    if down(0x5C) {
        state |= RWIN;
    }
    if state != 0 {
        for digit in 0..10u32 {
            if down(0x30 + digit) {
                state |= 1 << (DIGIT_SHIFT + digit);
            }
        }
    }
    state
}

/// The keys of `state` in words: "LWin+3", or "none".
pub(crate) fn describe(state: u32) -> String {
    let mut names = Vec::new();
    if state & LWIN != 0 {
        names.push("LWin".to_string());
    }
    if state & RWIN != 0 {
        names.push("RWin".to_string());
    }
    for digit in 0..10 {
        if state & (1 << (DIGIT_SHIFT + digit)) != 0 {
            names.push(digit.to_string());
        }
    }
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join("+")
    }
}

/// The line for a change from `before` to `now`; None when nothing changed.
pub(crate) fn key_change(before: u32, now: u32) -> Option<String> {
    (before != now).then(|| format!("keystate: {}", describe(now)))
}

/// What WinCraft last did to bring a window to the front, for the line of
/// the change that follows.
struct Ask {
    hwnd: isize,
    at: Instant,
    how: String,
}

static LAST_ASK: Mutex<Option<Ask>> = Mutex::new(None);

/// A change to `hwnd` this soon after WinCraft asked for it counts as
/// asked for.
const ASK_COUNTS: Duration = Duration::from_secs(2);

/// WinCraft asked Windows to bring `hwnd` to the front, and `how` it went.
/// Only while the log is on.
pub fn note_ask(hwnd: HWND, how: &str) {
    if !super::on() {
        return;
    }
    *LAST_ASK.lock().unwrap_or_else(PoisonError::into_inner) = Some(Ask {
        hwnd: hwnd as isize,
        at: Instant::now(),
        how: how.to_string(),
    });
}

fn asked_for(hwnd: isize) -> Option<String> {
    let last = LAST_ASK.lock().unwrap_or_else(PoisonError::into_inner);
    last.as_ref()
        .filter(|ask| ask.hwnd == hwnd && ask.at.elapsed() < ASK_COUNTS)
        .map(|ask| {
            format!(
                "asked by WinCraft {} ms before ({})",
                ask.at.elapsed().as_millis(),
                ask.how
            )
        })
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if !super::on() {
        return;
    }
    let mut pid = 0u32;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let exe = windows_list::exe_path(pid);
    let exe = exe.rsplit('\\').next().unwrap_or("");
    let asked = asked_for(hwnd as isize).unwrap_or_else(|| "not asked by WinCraft".to_string());
    crate::detailed!(
        "front: {:#x} class \"{}\" exe \"{exe}\" pid {pid} thread {thread} title \"{}\"; {asked}",
        hwnd as isize,
        windows_list::class_name(hwnd),
        windows_list::window_text(hwnd),
    );
}

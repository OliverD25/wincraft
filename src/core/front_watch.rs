//! Tells a listener at once when another window comes to the front, so a
//! window that follows the front one (ClaudeDeck's list) repaints then
//! instead of on its next timed frame. A WinEvent hook for
//! `EVENT_SYSTEM_FOREGROUND` runs on a thread of its own, which exists only
//! while someone listens. It only reads; it never changes a window.

use std::sync::{Arc, Mutex, PoisonError};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, EVENT_SYSTEM_FOREGROUND, MSG,
    PM_NOREMOVE, WINEVENT_OUTOFCONTEXT, WM_QUIT,
};

type Wake = Arc<dyn Fn() + Send + Sync>;

struct Running {
    thread_id: u32,
    thread: std::thread::JoinHandle<()>,
}

static LISTENERS: Mutex<Vec<(&'static str, Wake)>> = Mutex::new(Vec::new());
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn listeners() -> std::sync::MutexGuard<'static, Vec<(&'static str, Wake)>> {
    LISTENERS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `name` listens now.
pub fn listens(name: &str) -> bool {
    listeners().iter().any(|(at, _)| *at == name)
}

/// Calls `wake` on the watcher's thread each time the window in front
/// changes, until `stop(name)`. A second call with the same name does
/// nothing. `wake` must be quick and must never wait for a lock that a
/// window call can hold (asking for a repaint is right).
pub fn listen(name: &'static str, wake: Wake) {
    {
        let mut all = listeners();
        if all.iter().any(|(at, _)| *at == name) {
            return;
        }
        all.push((name, wake));
    }
    start();
}

/// Ends `name`'s listening; the thread ends with the last listener.
pub fn stop(name: &str) {
    let empty = {
        let mut all = listeners();
        all.retain(|(at, _)| *at != name);
        all.is_empty()
    };
    if empty {
        end();
    }
}

fn start() {
    let mut slot = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_some() || cfg!(test) {
        return;
    }
    let (ready, answer) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("front watch".to_string())
        .spawn(move || run(ready));
    let Ok(thread) = thread else {
        log::warn!("front watch: no thread");
        return;
    };
    match answer.recv() {
        Ok(thread_id) => *slot = Some(Running { thread_id, thread }),
        Err(_) => {
            let _ = thread.join();
        }
    }
}

fn end() {
    let running = RUNNING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(running) = running {
        unsafe { PostThreadMessageW(running.thread_id, WM_QUIT, 0, 0) };
        let _ = running.thread.join();
    }
}

fn run(ready: std::sync::mpsc::Sender<u32>) {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // Makes the thread's message queue, so `end` can always reach it.
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
        log::warn!("front watch: the hook could not start");
    }
    let _ = ready.send(unsafe { GetCurrentThreadId() });
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe { DispatchMessageW(&msg) };
    }
    if !hook.is_null() {
        unsafe { UnhookWinEvent(hook) };
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    wake_all();
}

/// Calls every listener, with the list's lock let go first.
fn wake_all() {
    let wakes: Vec<Wake> = listeners()
        .iter()
        .map(|(_, wake)| Arc::clone(wake))
        .collect();
    for wake in wakes {
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_listener_is_woken_until_it_stops() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        listen(
            "front_watch_test",
            Arc::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        );
        listen(
            "front_watch_test",
            Arc::new(|| panic!("a second listen is ignored")),
        );
        assert!(listens("front_watch_test"));
        wake_all();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        stop("front_watch_test");
        assert!(!listens("front_watch_test"));
        wake_all();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

//! A low-level keyboard or mouse hook on a thread of its own.
//!
//! Windows waits for a low-level hook before any key or click goes on. When
//! the hook is late it hands the input to Windows without asking it, and
//! after too many late answers it removes the hook without a word. The host
//! thread can be busy for a good part of a second (reading the taskbar's
//! buttons, building the strip), so a hook there can miss keys it should
//! keep. On this thread the hook only decides and posts; the work happens
//! where the post arrives.
//!
//! The hooks are put in and taken out on their own thread, which runs the
//! message loop Windows calls them from.

use std::sync::mpsc;

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, HOOKPROC, MSG, PM_NOREMOVE, WINDOWS_HOOK_ID, WM_QUIT,
};

/// A running hook thread. `stop` ends it; dropping it without `stop` ends
/// it too.
pub struct HookThread {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HookThread {
    /// Starts a thread named `name` with `hooks` in, each a hook kind
    /// (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`) and its procedure. The first must go
    /// in; the others are put in when Windows allows. None when the thread
    /// could not start or Windows refused the first hook.
    pub fn start(name: &str, hooks: Vec<(WINDOWS_HOOK_ID, HOOKPROC)>) -> Option<HookThread> {
        let (ready, answer) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || run(hooks, ready))
            .ok()?;
        match answer.recv() {
            Ok(Some(thread_id)) => Some(HookThread {
                thread_id,
                thread: Some(thread),
            }),
            _ => {
                let _ = thread.join();
                None
            }
        }
    }

    /// Takes the hooks out and waits for the thread to end. The hook
    /// procedures must not wait for anything the caller holds meanwhile.
    pub fn stop(mut self) {
        self.end();
    }

    fn end(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        let _ = thread.join();
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        self.end();
    }
}

fn run(hooks: Vec<(WINDOWS_HOOK_ID, HOOKPROC)>, ready: mpsc::Sender<Option<u32>>) {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // Makes the thread's message queue, so `stop` can always reach it.
    unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    let mut handles = Vec::new();
    for (index, (kind, procedure)) in hooks.into_iter().enumerate() {
        let handle = unsafe { SetWindowsHookExW(kind, procedure, module, 0) };
        if handle.is_null() {
            if index == 0 {
                let _ = ready.send(None);
                return;
            }
            continue;
        }
        handles.push(handle);
    }
    let _ = ready.send(Some(unsafe { GetCurrentThreadId() }));
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe { DispatchMessageW(&msg) };
    }
    for handle in handles {
        unsafe { UnhookWindowsHookEx(handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, WH_KEYBOARD_LL};

    /// Lets every key through, so the test never keeps one from the user.
    unsafe extern "system" fn pass(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    #[test]
    fn a_hook_thread_starts_and_ends_cleanly() {
        let thread = HookThread::start("test hook", vec![(WH_KEYBOARD_LL, Some(pass))])
            .expect("Windows refused a pass-through keyboard hook");
        assert_ne!(thread.thread_id, 0);
        // Returns only once the thread has taken the hook out and ended.
        thread.stop();
    }

    #[test]
    fn a_refused_first_hook_starts_nothing() {
        // No procedure: Windows refuses the hook.
        assert!(HookThread::start("test hook", vec![(WH_KEYBOARD_LL, None)]).is_none());
    }
}

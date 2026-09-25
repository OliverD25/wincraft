//! `wincraft.exe --quit`: asks the running WinCraft to shut down the way its
//! tray menu's Quit does, and waits for it to end. Installs use it instead of
//! killing the process, so plugins get to save and the tray icon goes away.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, IsWindow, PostMessageW, WM_CLOSE,
};

use crate::core::{host, instance, wide};

pub const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Closed,
    NotRunning,
    StillRunning,
}

impl Outcome {
    pub fn exit_code(self) -> i32 {
        match self {
            Outcome::Closed => 0,
            Outcome::NotRunning => 1,
            Outcome::StillRunning => 2,
        }
    }

    pub fn message(self) -> String {
        match self {
            Outcome::Closed => "WinCraft closed".to_string(),
            Outcome::NotRunning => "WinCraft is not running".to_string(),
            Outcome::StillRunning => format!(
                "WinCraft is still running after {} seconds",
                TIMEOUT.as_secs()
            ),
        }
    }
}

/// WM_CLOSE reaches the host's DefWindowProc, which destroys the window, and
/// the host shuts down on WM_DESTROY: the same shutdown as the tray's Quit,
/// in every version that has the host window, 0.8.3 included.
pub fn run(timeout: Duration) -> Outcome {
    let class = wide(&instance::name(host::CLASS_NAME));
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return Outcome::NotRunning;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let process = if pid == 0 {
        std::ptr::null_mut()
    } else {
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) }
    };
    if unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } == 0 {
        if !process.is_null() {
            unsafe { CloseHandle(process) };
        }
        // The window went away between finding it and posting to it.
        return if unsafe { IsWindow(hwnd) } == 0 {
            Outcome::NotRunning
        } else {
            Outcome::StillRunning
        };
    }
    if process.is_null() {
        // Without a handle to wait on, the host window going away is the
        // best sign left that the process is shutting down.
        let start = Instant::now();
        while start.elapsed() < timeout {
            if unsafe { IsWindow(hwnd) } == 0 {
                return Outcome::Closed;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        return Outcome::StillRunning;
    }
    let waited = unsafe { WaitForSingleObject(process, timeout.as_millis() as u32) };
    unsafe { CloseHandle(process) };
    if waited == WAIT_OBJECT_0 {
        Outcome::Closed
    } else {
        Outcome::StillRunning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_outcome_has_its_own_exit_code() {
        assert_eq!(Outcome::Closed.exit_code(), 0);
        assert_eq!(Outcome::NotRunning.exit_code(), 1);
        assert_eq!(Outcome::StillRunning.exit_code(), 2);
    }

    #[test]
    fn the_timeout_message_names_the_wait() {
        assert_eq!(
            Outcome::StillRunning.message(),
            "WinCraft is still running after 10 seconds"
        );
        assert_eq!(Outcome::Closed.message(), "WinCraft closed");
        assert_eq!(Outcome::NotRunning.message(), "WinCraft is not running");
    }
}

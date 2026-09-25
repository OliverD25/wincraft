//! Seeing the input language change. Windows sends ordinary programs no
//! "language changed" event, so two sources are combined:
//!
//! 1. A poll of the foreground window's thread layout (`GetKeyboardLayout`).
//! 2. The shell hook's `HSHELL_LANGUAGE` message to a hidden window.
//!
//! Either may see a switch first; whichever does reports it, and the other
//! finds the language already known and stays quiet.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyWindow, GetForegroundWindow,
    GetWindowThreadProcessId, IsWindow, RegisterClassW, RegisterShellHookWindow,
    RegisterWindowMessageW, HSHELL_LANGUAGE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::core::wide;

const CLASS_NAME: &str = "WinCraftLanguageShellHook";

/// A change of input language worth showing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Switch {
    /// None when the language before is not known, such as the first look.
    pub from: Option<usize>,
    pub to: usize,
}

/// The pure part: what each source saw, and whether that is news.
#[derive(Default)]
pub struct Detector {
    window: usize,
    /// What the poll read for the foreground window last time.
    polled: Option<usize>,
    /// The language last reported, or first seen, for that window.
    known: Option<usize>,
}

impl Detector {
    /// One poll: the foreground window and the layout its thread reports.
    ///
    /// Only a change in the poll's own reading for the same window counts.
    /// A window whose thread does not follow the switch (reportedly some
    /// console windows) then never reports anything wrong: its reading stays
    /// put, and the shell hook reports the switch instead.
    pub fn poll(&mut self, window: usize, hkl: usize, show_on_focus: bool) -> Option<Switch> {
        if window != self.window {
            let before = self.known;
            self.window = window;
            self.polled = Some(hkl);
            self.known = Some(hkl);
            let changed = before.is_some() && before != Some(hkl);
            return (show_on_focus && changed).then_some(Switch {
                from: before,
                to: hkl,
            });
        }
        if self.polled.replace(hkl) == Some(hkl) {
            return None;
        }
        self.report(hkl)
    }

    /// The shell hook named a new language for the foreground window.
    pub fn shell(&mut self, hkl: usize) -> Option<Switch> {
        self.report(hkl)
    }

    pub fn current(&self) -> Option<usize> {
        self.known
    }

    fn report(&mut self, to: usize) -> Option<Switch> {
        if self.known == Some(to) {
            return None;
        }
        let from = self.known.replace(to);
        Some(Switch { from, to })
    }
}

/// The foreground window and its thread's keyboard layout, as numbers.
pub fn foreground() -> Option<(usize, usize)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    Some((hwnd as usize, layout_of(hwnd)?))
}

fn layout_of(hwnd: HWND) -> Option<usize> {
    let thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    if thread == 0 {
        return None;
    }
    let hkl = unsafe { GetKeyboardLayout(thread) } as usize;
    (hkl != 0).then_some(hkl)
}

pub fn installed_layouts() -> Vec<usize> {
    let count = unsafe { GetKeyboardLayoutList(0, std::ptr::null_mut()) };
    let mut list = vec![std::ptr::null_mut(); count.max(0) as usize];
    let filled = unsafe { GetKeyboardLayoutList(count, list.as_mut_ptr()) };
    list.truncate(filled.max(0) as usize);
    list.into_iter().map(|hkl| hkl as usize).collect()
}

static SHELL_MESSAGE: AtomicU32 = AtomicU32::new(0);
static SHELL_PENDING: AtomicBool = AtomicBool::new(false);
static SHELL_LPARAM: AtomicIsize = AtomicIsize::new(0);

/// A hidden window that receives the shell hook's messages. The window
/// procedure only records the latest `HSHELL_LANGUAGE`; the plugin collects
/// it on its next poll, at most 120 ms later.
pub struct ShellHook {
    hwnd: HWND,
}

impl ShellHook {
    pub fn create() -> Result<Self, String> {
        let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let name = wide(CLASS_NAME);
        let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
        class.lpfnWndProc = Some(shell_proc);
        class.hInstance = hinstance;
        class.lpszClassName = name.as_ptr();
        // Registering again after the plugin was switched off and on fails
        // with "class already exists", which is fine.
        unsafe { RegisterClassW(&class) };
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                name.as_ptr(),
                wide("").as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("could not create its shell hook window".to_string());
        }
        let message = unsafe { RegisterWindowMessageW(wide("SHELLHOOK").as_ptr()) };
        SHELL_MESSAGE.store(message, Ordering::Relaxed);
        if unsafe { RegisterShellHookWindow(hwnd) } == 0 {
            unsafe { DestroyWindow(hwnd) };
            return Err("Windows refused its shell hook".to_string());
        }
        Ok(Self { hwnd })
    }

    /// The layout the last `HSHELL_LANGUAGE` points at, if one arrived since
    /// the last call. Its lParam is read both ways: as the window whose
    /// language changed, and as the new layout itself.
    pub fn take(&self) -> Option<usize> {
        if !SHELL_PENDING.swap(false, Ordering::Relaxed) {
            return None;
        }
        let lparam = SHELL_LPARAM.load(Ordering::Relaxed);
        let window = lparam as HWND;
        if !window.is_null() && unsafe { IsWindow(window) } != 0 {
            log::debug!("HSHELL_LANGUAGE named window {lparam:#x}");
            return layout_of(window);
        }
        let hkl = lparam as usize;
        if installed_layouts().contains(&hkl) {
            log::debug!("HSHELL_LANGUAGE named layout {hkl:#x}");
            return Some(hkl);
        }
        log::debug!("HSHELL_LANGUAGE with an lParam that is neither: {lparam:#x}");
        None
    }

    pub fn destroy(&mut self) {
        if !self.hwnd.is_null() {
            unsafe {
                DeregisterShellHookWindow(self.hwnd);
                DestroyWindow(self.hwnd);
            }
            self.hwnd = std::ptr::null_mut();
        }
    }
}

unsafe extern "system" fn shell_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let shell = SHELL_MESSAGE.load(Ordering::Relaxed);
    if shell != 0 && msg == shell {
        if (wparam as u32 & 0x7FFF) == HSHELL_LANGUAGE {
            SHELL_LPARAM.store(lparam, Ordering::Relaxed);
            SHELL_PENDING.store(true, Ordering::Relaxed);
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: usize = 0x0409_0409;
    const UK: usize = 0xF0A8_0422;

    #[test]
    fn a_switch_in_the_same_window_is_reported_once() {
        let mut detector = Detector::default();
        assert_eq!(detector.poll(1, EN, false), None);
        assert_eq!(detector.poll(1, EN, false), None);
        assert_eq!(
            detector.poll(1, UK, false),
            Some(Switch {
                from: Some(EN),
                to: UK
            })
        );
        assert_eq!(detector.poll(1, UK, false), None);
        // The shell hook arrives a moment later with the same news.
        assert_eq!(detector.shell(UK), None);
    }

    #[test]
    fn the_shell_hook_first_then_the_poll_stays_quiet() {
        let mut detector = Detector::default();
        detector.poll(1, EN, false);
        assert_eq!(
            detector.shell(UK),
            Some(Switch {
                from: Some(EN),
                to: UK
            })
        );
        assert_eq!(detector.poll(1, UK, false), None);
        assert_eq!(detector.current(), Some(UK));
    }

    #[test]
    fn moving_to_a_window_in_another_language_is_not_a_switch_by_default() {
        let mut detector = Detector::default();
        detector.poll(1, EN, false);
        assert_eq!(detector.poll(2, UK, false), None);
        assert_eq!(
            detector.poll(1, EN, true),
            Some(Switch {
                from: Some(UK),
                to: EN
            })
        );
        // The very first look knows no language before it.
        assert_eq!(Detector::default().poll(3, EN, true), None);
    }

    #[test]
    fn a_window_whose_reading_never_follows_relies_on_the_shell_hook() {
        let mut detector = Detector::default();
        detector.poll(7, EN, false);
        assert_eq!(
            detector.shell(UK),
            Some(Switch {
                from: Some(EN),
                to: UK
            })
        );
        // Its thread still reports English; that is not a switch back.
        assert_eq!(detector.poll(7, EN, false), None);
        assert_eq!(detector.current(), Some(UK));
    }

    #[test]
    fn switching_back_and_forth_reports_each_step() {
        let mut detector = Detector::default();
        detector.poll(1, EN, false);
        assert!(detector.poll(1, UK, false).is_some());
        assert_eq!(
            detector.poll(1, EN, false),
            Some(Switch {
                from: Some(UK),
                to: EN
            })
        );
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, GetStockObject, MonitorFromPoint, BLACK_BRUSH, HBRUSH,
    HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONULL,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowLongPtrW, KillTimer,
    RegisterClassW, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, GWLP_USERDATA, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::core::wide;

const CLASS_NAME: &str = "WinCraftDimOverlay";
const HOVER_TIMER_ID: usize = 1;
const HOVER_TIMER_MS: u32 = 100;
/// Window extra slot 0 holds the HMONITOR this overlay covers.
const EXTRA_MONITOR: i32 = 0;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

pub struct Overlay {
    hwnd: HWND,
    pub visible: bool,
}

pub fn monitors() -> Vec<(HMONITOR, RECT)> {
    let mut found: Vec<(HMONITOR, RECT)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_monitor),
            &mut found as *mut Vec<(HMONITOR, RECT)> as LPARAM,
        )
    };
    found
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> i32 {
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        let found = unsafe { &mut *(data as *mut Vec<(HMONITOR, RECT)>) };
        found.push((monitor, info.rcMonitor));
    }
    1
}

impl Overlay {
    pub fn create(monitor: HMONITOR, rect: RECT, idle: u8, hover: u8) -> Option<Self> {
        ensure_class()?;
        let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let hwnd = unsafe {
            CreateWindowExW(
                // Layered gives the alpha, Transparent lets clicks reach the
                // window underneath, NoActivate and ToolWindow keep the overlay
                // out of the focus chain and off the taskbar.
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                wide(CLASS_NAME).as_ptr(),
                wide("").as_ptr(),
                WS_POPUP,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            log::warn!("could not create a dim overlay window");
            return None;
        }
        unsafe {
            SetWindowLongPtrW(hwnd, EXTRA_MONITOR, monitor as isize);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, pack(idle, hover, idle));
            SetLayeredWindowAttributes(hwnd, 0, idle, LWA_ALPHA);
        }
        Some(Self {
            hwnd,
            visible: false,
        })
    }

    pub fn show(&mut self) {
        if self.visible {
            return;
        }
        let (idle, hover, _) = unpack(unsafe { GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) });
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, pack(idle, hover, idle));
            SetLayeredWindowAttributes(self.hwnd, 0, idle, LWA_ALPHA);
            ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            SetTimer(self.hwnd, HOVER_TIMER_ID, HOVER_TIMER_MS, None);
        }
        self.visible = true;
    }

    /// The last-applied byte is kept, so the hover timer notices the difference
    /// on its next tick and repaints once instead of on every frame.
    pub fn set_alphas(&mut self, idle: u8, hover: u8) {
        let (_, _, last) = unpack(unsafe { GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) });
        unsafe { SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, pack(idle, hover, last)) };
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        unsafe {
            KillTimer(self.hwnd, HOVER_TIMER_ID);
            ShowWindow(self.hwnd, SW_HIDE);
        }
        self.visible = false;
    }

    pub fn destroy(&mut self) {
        if self.visible {
            unsafe { KillTimer(self.hwnd, HOVER_TIMER_ID) };
            self.visible = false;
        }
        if !self.hwnd.is_null() {
            unsafe { DestroyWindow(self.hwnd) };
            self.hwnd = std::ptr::null_mut();
        }
    }
}

fn ensure_class() -> Option<()> {
    if CLASS_REGISTERED.load(Ordering::Relaxed) {
        return Some(());
    }
    let name = wide(CLASS_NAME);
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(overlay_proc);
    class.hInstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    class.lpszClassName = name.as_ptr();
    class.hbrBackground = unsafe { GetStockObject(BLACK_BRUSH) } as HBRUSH;
    class.cbWndExtra = std::mem::size_of::<isize>() as i32;
    if unsafe { RegisterClassW(&class) } == 0 {
        log::error!("could not register the overlay window class");
        return None;
    }
    CLASS_REGISTERED.store(true, Ordering::Relaxed);
    Some(())
}

/// The three alpha bytes live in GWLP_USERDATA so an overlay needs no heap
/// state and the WndProc never has to follow a pointer it does not own.
fn pack(idle: u8, hover: u8, last: u8) -> isize {
    ((idle as isize) << 16) | ((hover as isize) << 8) | last as isize
}

fn unpack(packed: isize) -> (u8, u8, u8) {
    (
        ((packed >> 16) & 0xFF) as u8,
        ((packed >> 8) & 0xFF) as u8,
        (packed & 0xFF) as u8,
    )
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == windows_sys::Win32::UI::WindowsAndMessaging::WM_TIMER && wparam == HOVER_TIMER_ID {
        let packed = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
        let (idle, hover, last) = unpack(packed);
        let mut point = POINT { x: 0, y: 0 };
        let wanted = if unsafe { GetCursorPos(&mut point) } != 0 {
            let under_cursor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONULL) };
            let mine = unsafe { GetWindowLongPtrW(hwnd, EXTRA_MONITOR) } as HMONITOR;
            if !under_cursor.is_null() && under_cursor == mine {
                hover
            } else {
                idle
            }
        } else {
            idle
        };
        if wanted != last {
            unsafe {
                SetLayeredWindowAttributes(hwnd, 0, wanted, LWA_ALPHA);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, pack(idle, hover, wanted));
            }
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

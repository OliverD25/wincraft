use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Controls::{LoadIconMetric, LIM_SMALL};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow, TrackPopupMenu,
    HICON, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_APP,
};

use crate::core::{theme, wide};

pub const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
/// The tiled W, for light taskbars, and the white W alone, for dark ones.
const COLOUR_ICON: u32 = 1;
const MONOCHROME_ICON: u32 = 2;
const TRAY_ICON_ID: u32 = 1;

/// LoadIconMetric picks the frame that matches the tray size at the current
/// scaling, so the 16, 20 and 24 px drawings are used instead of a shrunken 32.
fn small_icon(hinstance: *mut core::ffi::c_void, resource: u32) -> HICON {
    let mut icon: HICON = std::ptr::null_mut();
    unsafe {
        LoadIconMetric(
            hinstance,
            resource as usize as *const u16,
            LIM_SMALL,
            &mut icon,
        )
    };
    icon
}

/// Windows does not tint notification icons for the taskbar, so the white W
/// would vanish on a light taskbar; there the tiled colour icon is used.
fn icon_for_taskbar(hinstance: *mut core::ffi::c_void) -> HICON {
    let resource = if theme::taskbar_is_light() {
        COLOUR_ICON
    } else {
        MONOCHROME_ICON
    };
    small_icon(hinstance, resource)
}

pub enum MenuItem {
    Separator,
    Entry {
        id: u32,
        label: String,
        checked: bool,
    },
}

pub struct Tray {
    data: NOTIFYICONDATAW,
    visible: bool,
    hinstance: *mut core::ffi::c_void,
}

impl Tray {
    pub fn new(hwnd: HWND, hinstance: *mut core::ffi::c_void) -> Self {
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = TRAY_ICON_ID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY_CALLBACK;
        data.hIcon = icon_for_taskbar(hinstance);
        copy_into(&mut data.szTip, "WinCraft");
        Self {
            data,
            visible: false,
            hinstance,
        }
    }

    pub fn refresh_icon(&mut self) {
        self.data.hIcon = icon_for_taskbar(self.hinstance);
        if self.visible {
            let mut data = self.data;
            data.uFlags = NIF_ICON;
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
        }
    }

    pub fn add(&mut self) {
        self.data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &self.data) };
        self.visible = ok != 0;
        if !self.visible {
            log::warn!("could not add the tray icon");
        }
    }

    pub fn remove(&mut self) {
        if self.visible {
            unsafe { Shell_NotifyIconW(NIM_DELETE, &self.data) };
            self.visible = false;
        }
    }

    pub fn balloon(&mut self, title: &str, text: &str) {
        if !self.visible {
            return;
        }
        let mut data = self.data;
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_INFO;
        copy_into(&mut data.szInfoTitle, title);
        copy_into(&mut data.szInfo, text);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }
}

/// Runs a nested message loop, so the caller must not be holding a borrow of
/// the host state while this is on the stack.
pub fn show_menu(hwnd: HWND, items: &[MenuItem]) -> Option<u32> {
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return None;
    }
    for item in items {
        match item {
            MenuItem::Separator => unsafe {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            },
            MenuItem::Entry { id, label, checked } => {
                let flags = MF_STRING | if *checked { MF_CHECKED } else { MF_UNCHECKED };
                let text = wide(label);
                unsafe { AppendMenuW(menu, flags, *id as usize, text.as_ptr()) };
            }
        }
    }

    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    // Without this the menu stays open after a click somewhere else; the
    // WM_NULL afterwards is the other half of the same documented workaround.
    unsafe { SetForegroundWindow(hwnd) };
    let chosen = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        )
    };
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd, 0, 0, 0);
        DestroyMenu(menu);
    }

    if chosen > 0 {
        Some(chosen as u32)
    } else {
        None
    }
}

fn copy_into(buffer: &mut [u16], text: &str) {
    let encoded = wide(text);
    let take = encoded.len().min(buffer.len());
    buffer[..take].copy_from_slice(&encoded[..take]);
    if let Some(last) = buffer.get_mut(take.saturating_sub(1)) {
        *last = 0;
    }
}

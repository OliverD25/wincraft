//! Putting text on the clipboard, for the palette's answers and the
//! ShortcutDetector's table, and reading the text on it, for QuickActions.

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

use crate::core::wide;

/// Replaces the clipboard with `text`. `owner` is the window the clipboard
/// names as its owner. False when the clipboard could not be opened or the
/// memory for the text could not be had.
pub fn put_text(owner: HWND, text: &str) -> bool {
    let encoded = wide(text);
    let bytes = encoded.len() * 2;
    if unsafe { OpenClipboard(owner) } == 0 {
        return false;
    }
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if handle.is_null() {
        unsafe { CloseClipboard() };
        return false;
    }
    let target = unsafe { GlobalLock(handle) } as *mut u16;
    if target.is_null() {
        unsafe {
            GlobalFree(handle);
            CloseClipboard();
        }
        return false;
    }
    let placed = unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), target, encoded.len());
        GlobalUnlock(handle);
        EmptyClipboard();
        !SetClipboardData(CF_UNICODETEXT as u32, handle).is_null()
    };
    // Once SetClipboardData succeeds the clipboard owns the block, and freeing
    // it here would be a double free; if it failed, the block is still ours.
    if !placed {
        unsafe { GlobalFree(handle) };
    }
    unsafe { CloseClipboard() };
    placed
}

/// The text on the clipboard, or None when it holds no text or could not
/// be opened. Only reads; the clipboard keeps everything it had.
pub fn text(owner: HWND) -> Option<String> {
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT as u32) } == 0 {
        return None;
    }
    if unsafe { OpenClipboard(owner) } == 0 {
        return None;
    }
    let handle = unsafe { GetClipboardData(CF_UNICODETEXT as u32) };
    let mut found = None;
    if !handle.is_null() {
        let source = unsafe { GlobalLock(handle) } as *const u16;
        if !source.is_null() {
            found = Some(unsafe { crate::core::from_wide_ptr(source) });
            unsafe { GlobalUnlock(handle) };
        }
    }
    unsafe { CloseClipboard() };
    found
}

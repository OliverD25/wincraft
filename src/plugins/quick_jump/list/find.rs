//! Finds the Quick access window that belongs to the viewport just opened.
//! Closing and opening again fast can leave the old window, still being
//! destroyed, under the same title; it must never be placed or shown as the
//! new one.

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW, IsWindow};

/// The first window in `found` that is not one of the `retired` ones.
pub(super) fn first_fresh(found: &[isize], retired: &[isize]) -> Option<isize> {
    found.iter().copied().find(|hwnd| !retired.contains(hwnd))
}

/// Forgets retired windows that are gone, so a handle Windows hands out
/// again is not mistaken for the old window.
pub(super) fn forget_gone(retired: &mut Vec<isize>) {
    retired.retain(|hwnd| unsafe { IsWindow(*hwnd as HWND) } != 0);
}

/// The window of this thread with `title` that is neither the palette nor
/// one of `retired`; 0 when there is none yet.
pub(super) fn find_fresh(title: &str, palette: isize, retired: &mut Vec<isize>) -> isize {
    struct Search<'a> {
        title: &'a str,
        skip: HWND,
        found: Vec<isize>,
    }

    unsafe extern "system" fn collect(hwnd: HWND, search: LPARAM) -> BOOL {
        let search = unsafe { &mut *(search as *mut Search) };
        let mut title = [0u16; 64];
        let len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
        let titled = String::from_utf16_lossy(&title[..len.max(0) as usize]) == search.title;
        if hwnd != search.skip && titled {
            search.found.push(hwnd as isize);
        }
        1
    }

    forget_gone(retired);
    let mut search = Search {
        title,
        skip: palette as HWND,
        found: Vec::new(),
    };
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(collect),
            &mut search as *mut Search as LPARAM,
        );
    }
    first_fresh(&search.found, retired).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_window_is_skipped_for_the_new_one() {
        assert_eq!(first_fresh(&[10, 20], &[10]), Some(20));
        assert_eq!(first_fresh(&[20, 10], &[10]), Some(20));
        assert_eq!(first_fresh(&[10], &[10]), None, "only the dying one so far");
        assert_eq!(first_fresh(&[], &[10]), None);
        assert_eq!(first_fresh(&[10], &[]), Some(10));
    }
}

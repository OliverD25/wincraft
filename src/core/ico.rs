//! Window icons from an .ico file built into the exe, at the size Windows
//! wants, so a plugin window can show its own icon in the taskbar. The
//! icon of the right size is picked from the file, never scaled from
//! another one, so small sizes stay sharp.

use std::sync::Mutex;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, SendMessageW, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SM_CXICON,
    SM_CXSMICON, WM_SETICON,
};

/// The icons made so far: the file's address, the size, the icon. They
/// live as long as WinCraft, so a window may keep using them.
static MADE: Mutex<Vec<(usize, i32, isize)>> = Mutex::new(Vec::new());

/// Where in an .ico file the picture for one entry starts and how long it
/// is: the directory entry `id` (from 1) says both. None when the file is
/// too short for it.
pub fn entry(file: &[u8], id: usize) -> Option<&[u8]> {
    let at = 6 + (id.checked_sub(1)?) * 16;
    let field = |from: usize| -> Option<usize> {
        let bytes: [u8; 4] = file.get(at + from..at + from + 4)?.try_into().ok()?;
        Some(u32::from_le_bytes(bytes) as usize)
    };
    let (length, offset) = (field(8)?, field(12)?);
    file.get(offset..offset.checked_add(length)?)
}

/// The width of each picture in the file, in its order (0 in the file
/// means 256).
pub fn widths(file: &[u8]) -> Vec<i32> {
    let count = file
        .get(4..6)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as usize);
    (0..count)
        .map_while(|at| file.get(6 + at * 16).copied())
        .map(|width| if width == 0 { 256 } else { i32::from(width) })
        .collect()
}

/// Which entry (from 1) to use for `size`: the exact size, else the
/// smallest bigger one, else the biggest.
pub fn best(widths: &[i32], size: i32) -> Option<usize> {
    let bigger = widths
        .iter()
        .enumerate()
        .filter(|(_, width)| **width >= size)
        .min_by_key(|(_, width)| **width);
    let any = widths.iter().enumerate().max_by_key(|(_, width)| **width);
    bigger.or(any).map(|(at, _)| at + 1)
}

/// An icon of `size` pixels from the .ico `file`, made once.
pub fn icon(file: &'static [u8], size: i32) -> isize {
    let key = file.as_ptr() as usize;
    let Ok(mut made) = MADE.lock() else {
        return 0;
    };
    if let Some((_, _, icon)) = made.iter().find(|(f, s, _)| *f == key && *s == size) {
        return *icon;
    }
    let Some(picture) = best(&widths(file), size).and_then(|id| entry(file, id)) else {
        return 0;
    };
    let icon = unsafe {
        CreateIconFromResourceEx(
            picture.as_ptr(),
            picture.len() as u32,
            1,
            0x0003_0000,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    } as isize;
    if icon != 0 {
        made.push((key, size, icon));
    }
    icon
}

/// Gives `hwnd` the big (taskbar, Alt+Tab) and small (title) icons from
/// the .ico `file`, at the sizes its DPI asks for.
pub fn set_window_icons(hwnd: isize, file: &'static [u8]) {
    let handle = hwnd as HWND;
    let dpi = unsafe { GetDpiForWindow(handle) };
    for (which, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
        let size = unsafe { GetSystemMetricsForDpi(metric, dpi) };
        let icon = icon(file, size);
        if icon != 0 {
            unsafe { SendMessageW(handle, WM_SETICON, which as WPARAM, icon as LPARAM) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_is_read_from_the_directory() {
        // One entry: 16 x 16, 4 bytes of picture at offset 22.
        let mut file = vec![0, 0, 1, 0, 1, 0];
        file.extend([16, 16, 0, 0, 1, 0, 32, 0]);
        file.extend(4u32.to_le_bytes());
        file.extend(22u32.to_le_bytes());
        file.extend([9, 8, 7, 6]);
        assert_eq!(entry(&file, 1), Some(&[9u8, 8, 7, 6][..]));
        assert_eq!(entry(&file, 0), None, "ids start at 1");
        assert_eq!(entry(&file, 2), None, "no second entry");
        assert_eq!(entry(&file[..24], 1), None, "a cut file");
    }

    #[test]
    fn the_exact_size_wins_then_the_next_bigger_then_the_biggest() {
        let widths = [16, 24, 32, 48, 256];
        assert_eq!(best(&widths, 24), Some(2));
        assert_eq!(best(&widths, 36), Some(4), "150% of 24");
        assert_eq!(best(&widths, 300), Some(5));
        assert_eq!(best(&[], 16), None);
    }

    #[test]
    fn the_wincraft_icon_file_has_pictures_for_the_taskbar() {
        let file: &[u8] = include_bytes!("../../assets/wincraft.ico");
        let count = widths(file).len();
        assert!(count >= 1);
        for id in 1..=count {
            assert!(entry(file, id).is_some_and(|picture| !picture.is_empty()));
        }
    }
}

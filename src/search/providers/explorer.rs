//! The folder an open File Explorer window shows, read through the shell's
//! automation objects. Every call crosses into explorer.exe, so this runs
//! only for the row the user is on, and the answer is cached.

use std::ffi::c_void;

use windows_sys::core::{BSTR, GUID, HRESULT};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Variant::{VARIANT, VT_I4};

use crate::core::com::{self, ComPtr};
use crate::search;

#[link(name = "shell32")]
extern "system" {
    // Declared here with a plain pointer: windows-sys puts it behind the
    // Win32_UI_Shell_Common feature for the ITEMIDLIST type alone.
    fn SHGetPathFromIDListW(pidl: *const c_void, path: *mut u16) -> i32;
}

const CLSID_SHELL_WINDOWS: GUID = com::guid(
    0x9ba05972,
    0xf6a8,
    0x11cf,
    [0xa4, 0x42, 0x00, 0xa0, 0xc9, 0x0a, 0x8f, 0x39],
);
const IID_ISHELL_WINDOWS: GUID = com::guid(
    0x85cb6900,
    0x4d95,
    0x11cf,
    [0x96, 0x0c, 0x00, 0x80, 0xc7, 0xf4, 0xee, 0x85],
);
const IID_IWEB_BROWSER2: GUID = com::guid(
    0xd30c1661,
    0xcdaf,
    0x11d0,
    [0x8a, 0x3e, 0x00, 0xc0, 0x4f, 0xc9, 0xe2, 0x6e],
);
const IID_ISERVICE_PROVIDER: GUID = com::guid(
    0x6d5140c1,
    0x7436,
    0x11ce,
    [0x80, 0x34, 0x00, 0xaa, 0x00, 0x60, 0x09, 0xfa],
);
const SID_STOP_LEVEL_BROWSER: GUID = com::guid(
    0x4c96be40,
    0x915c,
    0x11cf,
    [0x99, 0xd3, 0x00, 0xaa, 0x00, 0x4a, 0xe8, 0x37],
);
const IID_ISHELL_BROWSER: GUID = com::guid(
    0x000214e2,
    0x0000,
    0x0000,
    [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
);
const IID_IFOLDER_VIEW: GUID = com::guid(
    0xcde725b0,
    0xccc9,
    0x4519,
    [0x91, 0x7e, 0x32, 0x5d, 0x72, 0xfa, 0xb4, 0xce],
);
const IID_IPERSIST_FOLDER2: GUID = com::guid(
    0x1ac3d9f0,
    0x175c,
    0x11d1,
    [0x95, 0xbe, 0x00, 0x60, 0x97, 0x97, 0xea, 0x4f],
);

// Positions in the vtables, counted from IUnknown's three methods. The
// interfaces are large and only these few methods are called, so they are
// reached by index instead of by declaring every method.
const SHELL_WINDOWS_COUNT: usize = 7;
const SHELL_WINDOWS_ITEM: usize = 8;
const WEB_BROWSER_LOCATION_NAME: usize = 29;
const WEB_BROWSER_LOCATION_URL: usize = 30;
const WEB_BROWSER_HWND: usize = 37;
const SERVICE_PROVIDER_QUERY_SERVICE: usize = 3;
const SHELL_BROWSER_ACTIVE_VIEW: usize = 15;
const FOLDER_VIEW_GET_FOLDER: usize = 5;
const PERSIST_FOLDER2_CURRENT: usize = 5;

type GetLong = unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT;
type GetPointer = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
type GetString = unsafe extern "system" fn(*mut c_void, *mut BSTR) -> HRESULT;
type GetHandle = unsafe extern "system" fn(*mut c_void, *mut isize) -> HRESULT;
type Item = unsafe extern "system" fn(*mut c_void, VARIANT, *mut *mut c_void) -> HRESULT;
type QueryService =
    unsafe extern "system" fn(*mut c_void, *const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
type GetFolder = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;

/// # Safety
/// `index` must be the position of a method of type `F` in the vtable of the
/// interface `object` was obtained as.
unsafe fn method<F: Copy>(object: &ComPtr, index: usize) -> F {
    unsafe {
        let table = *(object.as_raw() as *const *const usize);
        std::mem::transmute_copy(&*table.add(index))
    }
}

/// The file system folder the Explorer window `hwnd` shows, or None for a
/// folder without one, such as This PC or the Recycle Bin. Windows 11
/// Explorer keeps each tab as its own shell window with the same handle;
/// the tab whose name the window title starts with is the one on screen.
pub fn folder_of(hwnd: isize, title: &str) -> Option<String> {
    let windows = search::create_com(&CLSID_SHELL_WINDOWS, &IID_ISHELL_WINDOWS)?;
    let mut count = 0i32;
    let hr =
        unsafe { method::<GetLong>(&windows, SHELL_WINDOWS_COUNT)(windows.as_raw(), &mut count) };
    if !com::ok(hr) {
        return None;
    }
    let tabs: Vec<ComPtr> = (0..count)
        .filter_map(|index| {
            let browser = item(&windows, index)?.query(&IID_IWEB_BROWSER2)?;
            let mut handle = 0isize;
            let hr = unsafe {
                method::<GetHandle>(&browser, WEB_BROWSER_HWND)(browser.as_raw(), &mut handle)
            };
            (com::ok(hr) && handle == hwnd).then_some(browser)
        })
        .collect();
    let shown = tabs
        .iter()
        .position(|tab| {
            string(tab, WEB_BROWSER_LOCATION_NAME)
                .is_some_and(|name| !name.is_empty() && title_shows(title, &name))
        })
        .unwrap_or(0);
    let tab = tabs.get(shown)?;
    path_of(tab)
        .or_else(|| string(tab, WEB_BROWSER_LOCATION_URL).and_then(|url| file_url_path(&url)))
}

/// "pswds_OneDrive - File Explorer" shows the tab named "pswds_OneDrive".
pub fn title_shows(title: &str, name: &str) -> bool {
    title == name || title.starts_with(&format!("{name} - "))
}

fn item(windows: &ComPtr, index: i32) -> Option<ComPtr> {
    let mut which: VARIANT = unsafe { std::mem::zeroed() };
    which.Anonymous.Anonymous.vt = VT_I4;
    which.Anonymous.Anonymous.Anonymous.lVal = index;
    let mut raw = std::ptr::null_mut();
    let hr =
        unsafe { method::<Item>(windows, SHELL_WINDOWS_ITEM)(windows.as_raw(), which, &mut raw) };
    com::ok(hr).then(|| ComPtr::from_raw(raw)).flatten()
}

fn string(object: &ComPtr, index: usize) -> Option<String> {
    let mut text: BSTR = std::ptr::null_mut();
    let hr = unsafe { method::<GetString>(object, index)(object.as_raw(), &mut text) };
    let value = unsafe { com::take_bstr(text) };
    com::ok(hr).then_some(value).flatten()
}

/// Asks the window's view which folder it holds, and the shell for that
/// folder's path; a virtual folder has none.
fn path_of(browser: &ComPtr) -> Option<String> {
    let services = browser.query(&IID_ISERVICE_PROVIDER)?;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        method::<QueryService>(&services, SERVICE_PROVIDER_QUERY_SERVICE)(
            services.as_raw(),
            &SID_STOP_LEVEL_BROWSER,
            &IID_ISHELL_BROWSER,
            &mut raw,
        )
    };
    let shell_browser = com::ok(hr).then(|| ComPtr::from_raw(raw)).flatten()?;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        method::<GetPointer>(&shell_browser, SHELL_BROWSER_ACTIVE_VIEW)(
            shell_browser.as_raw(),
            &mut raw,
        )
    };
    let view = com::ok(hr).then(|| ComPtr::from_raw(raw)).flatten()?;
    let folder_view = view.query(&IID_IFOLDER_VIEW)?;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        method::<GetFolder>(&folder_view, FOLDER_VIEW_GET_FOLDER)(
            folder_view.as_raw(),
            &IID_IPERSIST_FOLDER2,
            &mut raw,
        )
    };
    let folder = com::ok(hr).then(|| ComPtr::from_raw(raw)).flatten()?;
    let mut pidl = std::ptr::null_mut();
    let hr = unsafe {
        method::<GetPointer>(&folder, PERSIST_FOLDER2_CURRENT)(folder.as_raw(), &mut pidl)
    };
    if !com::ok(hr) || pidl.is_null() {
        return None;
    }
    let mut buffer = [0u16; 32768];
    let found = unsafe { SHGetPathFromIDListW(pidl, buffer.as_mut_ptr()) } != 0;
    unsafe { CoTaskMemFree(pidl) };
    if !found {
        return None;
    }
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(0);
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len]))
}

/// file:///C:/Users/My%20Files → C:\Users\My Files. Anything else, such as
/// the "::{GUID}" of a virtual folder, has no path.
pub fn file_url_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file:///")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let mut chars = rest.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let high = chars.next()?;
            let low = chars.next()?;
            let hex = [high, low];
            let text = std::str::from_utf8(&hex).ok()?;
            bytes.push(u8::from_str_radix(text, 16).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    let path = String::from_utf8(bytes).ok()?.replace('/', "\\");
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_names_the_tab_on_screen() {
        assert!(title_shows(
            "pswds_OneDrive - File Explorer",
            "pswds_OneDrive"
        ));
        assert!(title_shows("Downloads", "Downloads"));
        assert!(!title_shows("Downloads - File Explorer", "Down"));
    }

    #[test]
    fn a_file_url_becomes_a_windows_path() {
        assert_eq!(
            file_url_path("file:///C:/Users/My%20Files").as_deref(),
            Some(r"C:\Users\My Files")
        );
        assert_eq!(
            file_url_path("file:///D:/%D0%9F%D1%80%D0%BE%D1%94%D0%BA%D1%82").as_deref(),
            Some(r"D:\Проєкт")
        );
        assert_eq!(
            file_url_path("::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"),
            None
        );
        assert_eq!(file_url_path("file:///C:/bad%zz"), None);
    }
}

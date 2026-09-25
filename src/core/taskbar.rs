//! Which app's taskbar button is at a point on the screen, read through UI
//! Automation: Windows 11 draws the taskbar in XAML, so its buttons are not
//! windows of their own.

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::core::{IUnknown_Vtbl, BSTR, GUID, HRESULT, PWSTR};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::System::Com::{
    CLSIDFromString, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};
use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetCursorPos, WindowFromPoint, GA_ROOT,
};

use crate::core::com::{self, ComPtr};
use crate::core::windows_list::class_name;
use crate::core::{from_wide_ptr, wide};

const CLSID_CUI_AUTOMATION: GUID = GUID::from_u128(0xff48dba4_60ef_4201_aa87_54103eef594e);
const IID_IUI_AUTOMATION: GUID = GUID::from_u128(0x30cbe57d_d9d0_452a_ab13_7ac5ac4825ee);

const BUTTON_CLASS: &str = "Taskbar.TaskListButtonAutomationPeer";
const ID_PREFIX: &str = "Appid: ";
/// The point UI Automation reports is usually the button's icon or label,
/// a level or two below the button itself.
const MAX_DEPTH: usize = 5;
const TIMEOUT: Duration = Duration::from_millis(300);

// Slot orders follow UIAutomationClient.h (Windows SDK 10.0.26100.0).

#[repr(C)]
struct IUIAutomationVtbl {
    _base: IUnknown_Vtbl,
    _compare_elements: unsafe extern "system" fn(),
    _compare_runtime_ids: unsafe extern "system" fn(),
    _get_root_element: unsafe extern "system" fn(),
    _element_from_handle: unsafe extern "system" fn(),
    element_from_point: unsafe extern "system" fn(*mut c_void, POINT, *mut *mut c_void) -> HRESULT,
    _get_focused_element: unsafe extern "system" fn(),
    _get_root_element_build_cache: unsafe extern "system" fn(),
    _element_from_handle_build_cache: unsafe extern "system" fn(),
    _element_from_point_build_cache: unsafe extern "system" fn(),
    _get_focused_element_build_cache: unsafe extern "system" fn(),
    _create_tree_walker: unsafe extern "system" fn(),
    _get_control_view_walker: unsafe extern "system" fn(),
    _get_content_view_walker: unsafe extern "system" fn(),
    get_raw_view_walker: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationTreeWalkerVtbl {
    _base: IUnknown_Vtbl,
    get_parent_element:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IUIAutomationElementVtbl {
    _base: IUnknown_Vtbl,
    _set_focus: unsafe extern "system" fn(),
    _get_runtime_id: unsafe extern "system" fn(),
    _find_first: unsafe extern "system" fn(),
    _find_all: unsafe extern "system" fn(),
    _find_first_build_cache: unsafe extern "system" fn(),
    _find_all_build_cache: unsafe extern "system" fn(),
    _build_updated_cache: unsafe extern "system" fn(),
    _get_current_property_value: unsafe extern "system" fn(),
    _get_current_property_value_ex: unsafe extern "system" fn(),
    _get_cached_property_value: unsafe extern "system" fn(),
    _get_cached_property_value_ex: unsafe extern "system" fn(),
    _get_current_pattern_as: unsafe extern "system" fn(),
    _get_cached_pattern_as: unsafe extern "system" fn(),
    _get_current_pattern: unsafe extern "system" fn(),
    _get_cached_pattern: unsafe extern "system" fn(),
    _get_cached_parent: unsafe extern "system" fn(),
    _get_cached_children: unsafe extern "system" fn(),
    _get_current_process_id: unsafe extern "system" fn(),
    _get_current_control_type: unsafe extern "system" fn(),
    _get_current_localized_control_type: unsafe extern "system" fn(),
    _get_current_name: unsafe extern "system" fn(),
    _get_current_accelerator_key: unsafe extern "system" fn(),
    _get_current_access_key: unsafe extern "system" fn(),
    _get_current_has_keyboard_focus: unsafe extern "system" fn(),
    _get_current_is_keyboard_focusable: unsafe extern "system" fn(),
    _get_current_is_enabled: unsafe extern "system" fn(),
    get_current_automation_id: unsafe extern "system" fn(*mut c_void, *mut BSTR) -> HRESULT,
    get_current_class_name: unsafe extern "system" fn(*mut c_void, *mut BSTR) -> HRESULT,
}

/// The app ID of the taskbar button under the mouse, without the
/// `Appid: ` prefix, or `None` when the mouse is not on an app button.
pub fn app_under_cursor() -> Option<String> {
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    app_at(point.x, point.y)
}

/// The app ID of the taskbar button at a screen point, in physical pixels.
pub fn app_at(x: i32, y: i32) -> Option<String> {
    let point = POINT { x, y };
    if !on_taskbar(point) {
        return None;
    }
    // The caller owns windows and a message loop; asking explorer across
    // processes on a thread of its own means a slow explorer only costs the
    // timeout, never a hung host.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(on_worker(point));
    });
    match receiver.recv_timeout(TIMEOUT) {
        Ok(Ok(id)) => id,
        Ok(Err(reason)) => {
            log::debug!("taskbar button lookup failed: {reason}");
            None
        }
        Err(_) => {
            log::debug!("taskbar button lookup gave no answer within {TIMEOUT:?}");
            None
        }
    }
}

fn on_taskbar(point: POINT) -> bool {
    let hwnd = unsafe { WindowFromPoint(point) };
    if hwnd.is_null() {
        return false;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    matches!(
        class_name(root).as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

fn on_worker(point: POINT) -> Result<Option<String>, String> {
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if hr < 0 {
        return Err(format!("COM did not start ({})", com::hex(hr)));
    }
    let result = button_at(point);
    unsafe { CoUninitialize() };
    result
}

fn button_at(point: POINT) -> Result<Option<String>, String> {
    let automation = ComPtr::create(
        &CLSID_CUI_AUTOMATION,
        &IID_IUI_AUTOMATION,
        CLSCTX_INPROC_SERVER,
    )
    .map_err(|hr| format!("UI Automation is unavailable ({})", com::hex(hr)))?;
    let vtable = unsafe { automation.vtable::<IUIAutomationVtbl>() };

    let mut raw = std::ptr::null_mut();
    let hr = unsafe { (vtable.get_raw_view_walker)(automation.as_raw(), &mut raw) };
    let walker = (hr >= 0)
        .then(|| ComPtr::from_raw(raw))
        .flatten()
        .ok_or_else(|| format!("RawViewWalker failed ({})", com::hex(hr)))?;

    let mut raw = std::ptr::null_mut();
    let hr = unsafe { (vtable.element_from_point)(automation.as_raw(), point, &mut raw) };
    let mut element = (hr >= 0)
        .then(|| ComPtr::from_raw(raw))
        .flatten()
        .ok_or_else(|| format!("ElementFromPoint failed ({})", com::hex(hr)))?;

    for _ in 0..MAX_DEPTH {
        let element_vtable = unsafe { element.vtable::<IUIAutomationElementVtbl>() };
        let class = bstr_property(&element, element_vtable.get_current_class_name);
        if class.as_deref() == Some(BUTTON_CLASS) {
            let id = bstr_property(&element, element_vtable.get_current_automation_id);
            return Ok(id.map(|id| strip_prefix(&id).to_string()));
        }
        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            (walker
                .vtable::<IUIAutomationTreeWalkerVtbl>()
                .get_parent_element)(walker.as_raw(), element.as_raw(), &mut raw)
        };
        match (hr >= 0).then(|| ComPtr::from_raw(raw)).flatten() {
            Some(parent) => element = parent,
            None => break,
        }
    }
    Ok(None)
}

fn bstr_property(
    element: &ComPtr,
    getter: unsafe extern "system" fn(*mut c_void, *mut BSTR) -> HRESULT,
) -> Option<String> {
    let mut value: BSTR = std::ptr::null();
    let hr = unsafe { getter(element.as_raw(), &mut value) };
    let text = unsafe { com::take_bstr(value) };
    com::ok(hr).then_some(text).flatten()
}

fn strip_prefix(id: &str) -> &str {
    id.strip_prefix(ID_PREFIX).unwrap_or(id)
}

/// Whether a taskbar button's app ID names the same app as a LayoutKeeper
/// group key: the explicit AppUserModelID, or the lowercased program path
/// of an app without one.
pub fn same_app(taskbar_id: &str, group_key: &str) -> bool {
    same_app_with(taskbar_id, group_key, &known_folder)
}

/// Apps without an explicit ID show their path on the taskbar, sometimes
/// with a known folder written as its GUID: `{6D809377-...}\Surfshark\...`.
fn same_app_with(
    taskbar_id: &str,
    group_key: &str,
    folder: &dyn Fn(&str) -> Option<String>,
) -> bool {
    let id = strip_prefix(taskbar_id);
    let expanded = match id.find('}') {
        Some(end) if id.starts_with('{') => match folder(&id[..=end]) {
            Some(path) => format!("{path}{}", &id[end + 1..]),
            None => id.to_string(),
        },
        _ => id.to_string(),
    };
    expanded.to_lowercase() == group_key.to_lowercase()
}

fn known_folder(guid: &str) -> Option<String> {
    let mut id = GUID::from_u128(0);
    let hr = unsafe { CLSIDFromString(wide(guid).as_ptr(), &mut id) };
    if hr < 0 {
        return None;
    }
    let mut path: PWSTR = std::ptr::null_mut();
    let hr = unsafe { SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut path) };
    let text = (hr >= 0 && !path.is_null()).then(|| unsafe { from_wide_ptr(path) });
    // The shell allocates the buffer even when the call fails.
    unsafe { CoTaskMemFree(path as *const c_void) };
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFSHARK_ID: &str = r"{6D809377-6AF0-444B-8957-A3773F02200E}\Surfshark\Surfshark.exe";

    fn program_files(guid: &str) -> Option<String> {
        (guid == "{6D809377-6AF0-444B-8957-A3773F02200E}").then(|| r"C:\Program Files".to_string())
    }

    fn no_folders(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn an_explicit_id_matches_its_group() {
        assert!(same_app_with(
            "Telegram.TelegramDesktop",
            "Telegram.TelegramDesktop",
            &no_folders
        ));
        assert!(same_app_with(
            "Claude_pzs8sxrjxfjjc!Claude",
            "Claude_pzs8sxrjxfjjc!Claude",
            &no_folders
        ));
    }

    #[test]
    fn case_does_not_matter() {
        assert!(same_app_with(
            "Chrome.UserData.Profile3",
            "chrome.userdata.profile3",
            &no_folders
        ));
    }

    #[test]
    fn a_full_path_matches_the_lowercased_program_path() {
        assert!(same_app_with(
            r"C:\Users\Admin\AppData\Local\Viber\Viber.exe",
            r"c:\users\admin\appdata\local\viber\viber.exe",
            &no_folders
        ));
    }

    #[test]
    fn a_known_folder_guid_is_expanded() {
        assert!(same_app_with(
            SURFSHARK_ID,
            r"c:\program files\surfshark\surfshark.exe",
            &program_files
        ));
        assert!(!same_app_with(
            SURFSHARK_ID,
            r"c:\program files\surfshark\surfshark.exe",
            &no_folders
        ));
    }

    #[test]
    fn a_different_app_does_not_match() {
        assert!(!same_app_with(
            "Chrome.UserData.Profile3",
            "Chrome._crx_gdfaibffkckdkhn.UserData.Profile3",
            &no_folders
        ));
        assert!(!same_app_with(
            "Telegram.TelegramDesktop",
            r"c:\users\admin\appdata\local\viber\viber.exe",
            &no_folders
        ));
    }

    #[test]
    fn the_automation_prefix_is_ignored() {
        assert!(same_app_with(
            "Appid: Telegram.TelegramDesktop",
            "Telegram.TelegramDesktop",
            &no_folders
        ));
        assert!(same_app_with(
            &format!("Appid: {SURFSHARK_ID}"),
            r"c:\program files\surfshark\surfshark.exe",
            &program_files
        ));
        assert_eq!(strip_prefix("Appid: md.obsidian"), "md.obsidian");
        assert_eq!(strip_prefix("md.obsidian"), "md.obsidian");
    }
}

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
use windows_sys::Win32::Foundation::{HWND, PROPERTYKEY};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows_sys::Win32::System::Variant::VT_LPWSTR;
use windows_sys::Win32::UI::Shell::PropertiesSystem::SHGetPropertyStoreForWindow;
use windows_sys::Win32::UI::Shell::{
    SHCreateItemFromParsingName, SHLoadIndirectString, SIGDN_NORMALDISPLAY,
};

use crate::core::com::ComPtr;
use crate::core::wide;

const IID_IPROPERTY_STORE: GUID = GUID::from_u128(0x886d8eeb_8cf2_4446_8d02_cdba1dbdcf99);
const IID_ISHELL_ITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);
const APP_USER_MODEL: GUID = GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3);
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_USER_MODEL,
    pid: 5,
};
const PKEY_RELAUNCH_DISPLAY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_USER_MODEL,
    pid: 4,
};

#[repr(C)]
struct IPropertyStoreVtbl {
    _base: IUnknown_Vtbl,
    _get_count: unsafe extern "system" fn(),
    _get_at: unsafe extern "system" fn(),
    get_value:
        unsafe extern "system" fn(*mut c_void, *const PROPERTYKEY, *mut PROPVARIANT) -> HRESULT,
    _set_value: unsafe extern "system" fn(),
    _commit: unsafe extern "system" fn(),
}

#[repr(C)]
struct IShellItemVtbl {
    _base: IUnknown_Vtbl,
    _bind_to_handler: unsafe extern "system" fn(),
    _get_parent: unsafe extern "system" fn(),
    get_display_name: unsafe extern "system" fn(*mut c_void, i32, *mut *mut u16) -> HRESULT,
}

/// What a window tells the taskbar about the app it belongs to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppId {
    /// The explicit AppUserModelID, which Windows groups taskbar buttons
    /// by; Chrome gives each installed web app its own.
    pub id: Option<String>,
    /// The app's name as the taskbar shows it on a pinned button.
    pub name: Option<String>,
}

pub fn read(hwnd: HWND) -> AppId {
    let mut raw = std::ptr::null_mut();
    let hr = unsafe { SHGetPropertyStoreForWindow(hwnd, &IID_IPROPERTY_STORE, &mut raw) };
    let Some(store) = (hr >= 0).then(|| ComPtr::from_raw(raw)).flatten() else {
        return AppId::default();
    };
    AppId {
        id: string_value(&store, &PKEY_APP_USER_MODEL_ID).filter(|id| !id.is_empty()),
        name: string_value(&store, &PKEY_RELAUNCH_DISPLAY_NAME)
            .map(|name| resolve(&name))
            .filter(|name| !name.is_empty()),
    }
}

fn string_value(store: &ComPtr, key: &PROPERTYKEY) -> Option<String> {
    let mut value = PROPVARIANT::default();
    let hr = unsafe {
        (store.vtable::<IPropertyStoreVtbl>().get_value)(store.as_raw(), key, &mut value)
    };
    if hr < 0 {
        return None;
    }
    let text = unsafe {
        let inner = &value.Anonymous.Anonymous;
        let pointer = inner.Anonymous.pwszVal;
        (inner.vt == VT_LPWSTR && !pointer.is_null()).then(|| {
            let mut len = 0;
            while *pointer.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(pointer, len))
        })
    };
    unsafe { PropVariantClear(&mut value) };
    text
}

/// The name Windows shows for an app ID, from the Start menu entry the app
/// registered: Chrome gives installed web apps no relaunch name on their
/// windows, but their shortcut is called "Gemini".
pub fn registered_name(id: &str) -> Option<String> {
    let path = wide(&format!("shell:AppsFolder\\{id}"));
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        SHCreateItemFromParsingName(
            path.as_ptr(),
            std::ptr::null_mut(),
            &IID_ISHELL_ITEM,
            &mut raw,
        )
    };
    let item = (hr >= 0).then(|| ComPtr::from_raw(raw)).flatten()?;
    let mut name: *mut u16 = std::ptr::null_mut();
    let hr = unsafe {
        (item.vtable::<IShellItemVtbl>().get_display_name)(
            item.as_raw(),
            SIGDN_NORMALDISPLAY,
            &mut name,
        )
    };
    if hr < 0 || name.is_null() {
        return None;
    }
    let text = unsafe {
        let mut len = 0;
        while *name.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(name, len))
    };
    unsafe { CoTaskMemFree(name as *const c_void) };
    (!text.is_empty() && text != id).then_some(text)
}

/// Names like "@C:\app.dll,-101" point into a resource and must be looked up.
fn resolve(name: &str) -> String {
    if !name.starts_with('@') {
        return name.to_string();
    }
    let mut buffer = [0u16; 256];
    let hr = unsafe {
        SHLoadIndirectString(
            wide(name).as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            std::ptr::null(),
        )
    };
    if hr < 0 {
        return String::new();
    }
    let len = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

/// The key a window's taskbar group has: its explicit AppUserModelID, or,
/// like Windows itself, its program's path when it has none.
pub fn group_key(explicit: Option<&str>, exe_path: &str) -> String {
    match explicit {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => exe_path.to_lowercase(),
    }
}

/// What the strip calls a group: the program, plus the app's name when
/// there is one, else what the ID says beyond the program. Chrome names its
/// profile shortcuts "<profile> - Chrome", so a trailing program name is cut.
pub fn group_label(program: &str, name: Option<&str>, key: &str, exe_path: &str) -> String {
    let suffix = format!(" - {program}");
    let name = name.map(|name| name.strip_suffix(&suffix).unwrap_or(name));
    if let Some(name) = name.filter(|name| !name.is_empty() && *name != program) {
        return format!("{program} \u{b7} {name}");
    }
    if key != exe_path.to_lowercase() {
        let last = key.rsplit(['.', '\\', '!']).next().unwrap_or(key);
        if !last.is_empty() && !last.eq_ignore_ascii_case(program) {
            return format!("{program} \u{b7} {last}");
        }
    }
    program.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: &str = r"C:\Program Files\Google\Chrome\Application\chrome.exe";

    #[test]
    fn a_window_without_an_id_is_grouped_by_its_program_path() {
        assert_eq!(group_key(None, CHROME), CHROME.to_lowercase());
        assert_eq!(group_key(Some(""), CHROME), CHROME.to_lowercase());
        assert_eq!(
            group_key(Some("Chrome._crx_abc"), CHROME),
            "Chrome._crx_abc"
        );
    }

    #[test]
    fn the_label_names_the_app_when_the_window_does() {
        assert_eq!(
            group_label("Chrome", Some("Gemini"), "Chrome._crx_abc", CHROME),
            "Chrome \u{b7} Gemini"
        );
        assert_eq!(group_label("Chrome", None, "Chrome", CHROME), "Chrome");
        assert_eq!(
            group_label("Chrome", None, &CHROME.to_lowercase(), CHROME),
            "Chrome"
        );
        assert_eq!(
            group_label("Chrome", None, "Chrome.UserData.Profile2", CHROME),
            "Chrome \u{b7} Profile2"
        );
        assert_eq!(
            group_label("Chrome", Some("Chrome"), "Chrome", CHROME),
            "Chrome"
        );
        assert_eq!(
            group_label(
                "Chrome",
                Some("Work - Chrome"),
                "Chrome.UserData.Profile3",
                CHROME
            ),
            "Chrome \u{b7} Work"
        );
    }
}

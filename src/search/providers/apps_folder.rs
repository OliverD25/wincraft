//! The apps the shell lists in `shell:AppsFolder`: Store and other packaged
//! apps (which have no shortcut anywhere), and the desktop apps Windows
//! knows by an app ID or a program path. Read through the shell's own
//! enumeration, so it matches what the Start menu's "All apps" shows.

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT, PWSTR};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::SHGetKnownFolderItem;

use crate::core::com::{self, ComPtr};
use crate::core::{from_wide_ptr, taskbar};

const FOLDERID_APPS_FOLDER: GUID = GUID::from_u128(0x1e87508d_89c2_42f0_8a7e_645a0f50ca58);
const BHID_ENUM_ITEMS: GUID = GUID::from_u128(0x94f60519_2850_4924_aa5a_d15e84868039);
const IID_ISHELL_ITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);
const IID_IENUM_SHELL_ITEMS: GUID = GUID::from_u128(0x70629033_e363_4a28_a567_0db78006e6d7);

const SIGDN_NORMALDISPLAY: i32 = 0;
const SIGDN_PARENTRELATIVEPARSING: i32 = 0x8001_8001_u32 as i32;
/// `Next` returns S_OK for an item and S_FALSE at the end.
const S_OK: HRESULT = 0;

#[repr(C)]
struct IShellItemVtbl {
    _base: IUnknown_Vtbl,
    bind_to_handler: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const GUID,
        *const GUID,
        *mut *mut c_void,
    ) -> HRESULT,
    _get_parent: unsafe extern "system" fn(),
    get_display_name: unsafe extern "system" fn(*mut c_void, i32, *mut PWSTR) -> HRESULT,
}

#[repr(C)]
struct IEnumShellItemsVtbl {
    _base: IUnknown_Vtbl,
    next: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void, *mut u32) -> HRESULT,
}

/// One entry of the apps folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    /// What follows `shell:AppsFolder\` to start it: an app ID for a
    /// packaged app, a program path (maybe led by a known-folder GUID) for
    /// a desktop app.
    pub id: String,
}

impl Item {
    /// A path, rather than an app ID.
    fn path_like(&self) -> bool {
        let id = self.id.as_str();
        id.starts_with(r"\\")
            || id.as_bytes().get(1) == Some(&b':')
            || (id.starts_with('{') && id.contains("}\\"))
    }

    /// A packaged (Store) app: its ID is `<package family>!<app>`.
    pub fn is_package(&self) -> bool {
        !self.path_like() && self.id.contains('!')
    }

    /// The app ID, lower case, for an item that is not a path.
    pub fn aumid(&self) -> Option<String> {
        (!self.path_like() && !self.id.is_empty()).then(|| self.id.to_lowercase())
    }

    /// The program, lower case, for an item that names one.
    pub fn exe(&self) -> Option<String> {
        self.exe_with(&taskbar::expand_known_folder)
    }

    fn exe_with(&self, expand: &dyn Fn(&str) -> String) -> Option<String> {
        if !self.path_like() {
            return None;
        }
        let path = expand(&self.id).to_lowercase();
        path.ends_with(".exe").then_some(path)
    }

    /// What `Action::Open` gets: the shell opens `shell:AppsFolder\<id>`.
    pub fn launch(&self) -> String {
        format!(r"shell:AppsFolder\{}", self.id)
    }
}

/// Every named entry of `shell:AppsFolder`. Needs COM on this thread.
pub fn enumerate() -> Vec<Item> {
    let mut folder = std::ptr::null_mut();
    let hr = unsafe {
        SHGetKnownFolderItem(
            &FOLDERID_APPS_FOLDER,
            0,
            std::ptr::null_mut(),
            &IID_ISHELL_ITEM,
            &mut folder,
        )
    };
    let Some(folder) = (com::ok(hr)).then(|| ComPtr::from_raw(folder)).flatten() else {
        log::warn!("the apps folder cannot be opened ({})", com::hex(hr));
        return Vec::new();
    };
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (folder.vtable::<IShellItemVtbl>().bind_to_handler)(
            folder.as_raw(),
            std::ptr::null_mut(),
            &BHID_ENUM_ITEMS,
            &IID_IENUM_SHELL_ITEMS,
            &mut raw,
        )
    };
    let Some(items) = (com::ok(hr)).then(|| ComPtr::from_raw(raw)).flatten() else {
        log::warn!("the apps folder cannot be listed ({})", com::hex(hr));
        return Vec::new();
    };

    let mut found = Vec::new();
    loop {
        let mut raw = std::ptr::null_mut();
        let mut fetched = 0u32;
        let hr = unsafe {
            (items.vtable::<IEnumShellItemsVtbl>().next)(items.as_raw(), 1, &mut raw, &mut fetched)
        };
        if hr != S_OK || fetched != 1 {
            break;
        }
        let Some(item) = ComPtr::from_raw(raw) else {
            break;
        };
        let name = display_name(&item, SIGDN_NORMALDISPLAY);
        let id = display_name(&item, SIGDN_PARENTRELATIVEPARSING);
        if let (Some(name), Some(id)) = (name, id) {
            let name = name.trim().to_string();
            if !name.is_empty() && !id.is_empty() {
                found.push(Item { name, id });
            }
        }
    }
    found
}

fn display_name(item: &ComPtr, form: i32) -> Option<String> {
    let mut text: PWSTR = std::ptr::null_mut();
    let hr = unsafe {
        (item.vtable::<IShellItemVtbl>().get_display_name)(item.as_raw(), form, &mut text)
    };
    if !com::ok(hr) || text.is_null() {
        return None;
    }
    let value = unsafe { from_wide_ptr(text) };
    unsafe { CoTaskMemFree(text as *const c_void) };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> Item {
        Item {
            name: "App".to_string(),
            id: id.to_string(),
        }
    }

    fn program_files(id: &str) -> String {
        id.replace(
            "{6D809377-6AF0-444B-8957-A3773F02200E}",
            r"C:\Program Files",
        )
    }

    #[test]
    fn a_package_has_an_id_and_no_program() {
        let terminal = item("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App");
        assert!(terminal.is_package());
        assert_eq!(
            terminal.aumid().as_deref(),
            Some("microsoft.windowsterminal_8wekyb3d8bbwe!app")
        );
        assert_eq!(terminal.exe_with(&program_files), None);
        assert_eq!(
            terminal.launch(),
            r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"
        );
    }

    #[test]
    fn a_program_path_names_its_program_even_with_a_known_folder_guid() {
        let notepad = item(r"C:\Windows\System32\Notepad.exe");
        assert!(!notepad.is_package());
        assert_eq!(notepad.aumid(), None);
        assert_eq!(
            notepad.exe_with(&program_files).as_deref(),
            Some(r"c:\windows\system32\notepad.exe")
        );
        let surfshark = item(r"{6D809377-6AF0-444B-8957-A3773F02200E}\Surfshark\Surfshark.exe");
        assert_eq!(
            surfshark.exe_with(&program_files).as_deref(),
            Some(r"c:\program files\surfshark\surfshark.exe")
        );
    }

    #[test]
    fn an_id_that_is_not_a_program_has_only_its_id() {
        let chrome = item("Chrome");
        assert!(!chrome.is_package());
        assert_eq!(chrome.aumid().as_deref(), Some("chrome"));
        assert_eq!(chrome.exe_with(&program_files), None);
        // A shortcut or a document path names no program to compare with.
        let shortcut = item(r"C:\ProgramData\Start Menu\Thing.lnk");
        assert_eq!(shortcut.exe_with(&program_files), None);
        assert_eq!(shortcut.aumid(), None);
    }
}

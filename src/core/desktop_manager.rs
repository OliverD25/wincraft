//! The documented IVirtualDesktopManager: which virtual desktop a window
//! is on. It reads any window's desktop, but moving another process's
//! window through it fails with E_ACCESSDENIED, so LayoutKeeper's moves use
//! the undocumented interfaces it keeps with the plugin.

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, BOOL, GUID, HRESULT};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::CLSCTX_ALL;

use crate::core::com::{self, ComPtr};

const CLSID_VIRTUAL_DESKTOP_MANAGER: GUID = GUID::from_u128(0xaa509086_5ca9_4c25_8f95_589d3c07b48a);
const IID_IVIRTUAL_DESKTOP_MANAGER: GUID = GUID::from_u128(0xa5cd92ff_29be_454c_8d04_d82879fb3f1b);

#[repr(C)]
struct IVirtualDesktopManagerVtbl {
    _base: IUnknown_Vtbl,
    _is_window_on_current_virtual_desktop:
        unsafe extern "system" fn(*mut c_void, HWND, *mut BOOL) -> HRESULT,
    get_window_desktop_id: unsafe extern "system" fn(*mut c_void, HWND, *mut GUID) -> HRESULT,
    _move_window_to_desktop: unsafe extern "system" fn(*mut c_void, HWND, *const GUID) -> HRESULT,
}

pub struct DesktopManager(ComPtr);

impl DesktopManager {
    /// COM must already be running on the calling thread.
    pub fn new() -> Result<Self, HRESULT> {
        ComPtr::create(
            &CLSID_VIRTUAL_DESKTOP_MANAGER,
            &IID_IVIRTUAL_DESKTOP_MANAGER,
            CLSCTX_ALL,
        )
        .map(Self)
    }

    /// The id of the desktop the window is on. A window pinned to every
    /// desktop reads as the null GUID, and so does one the shell keeps on
    /// no desktop at all; None when the manager does not answer.
    pub fn desktop_of(&self, hwnd: HWND) -> Option<GUID> {
        let mut desktop = com::guid(0, 0, 0, [0; 8]);
        let hr = unsafe {
            (self
                .0
                .vtable::<IVirtualDesktopManagerVtbl>()
                .get_window_desktop_id)(self.0.as_raw(), hwnd, &mut desktop)
        };
        com::ok(hr).then_some(desktop)
    }
}

pub fn is_null(guid: &GUID) -> bool {
    (guid.data1, guid.data2, guid.data3, guid.data4) == (0, 0, 0, [0; 8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_all_zero_guid_is_null() {
        assert!(is_null(&com::guid(0, 0, 0, [0; 8])));
        assert!(!is_null(&com::guid(1, 0, 0, [0; 8])));
        assert!(!is_null(&com::guid(0, 0, 0, [0, 0, 0, 0, 0, 0, 0, 1])));
        assert!(!is_null(&CLSID_VIRTUAL_DESKTOP_MANAGER));
    }
}

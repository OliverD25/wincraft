use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
use windows_sys::Win32::System::Com::CoCreateInstance;

/// An owned COM interface pointer. windows-sys has no COM wrappers, so each
/// interface is a hand-declared vtable reached through [`ComPtr::vtable`].
pub struct ComPtr(*mut c_void);

impl ComPtr {
    pub fn create(clsid: &GUID, iid: &GUID, context: u32) -> Result<Self, HRESULT> {
        let mut raw = std::ptr::null_mut();
        let hr = unsafe { CoCreateInstance(clsid, std::ptr::null_mut(), context, iid, &mut raw) };
        check(hr)?;
        Self::from_raw(raw).ok_or(hr)
    }

    /// Takes ownership of a pointer a COM method handed out.
    pub fn from_raw(raw: *mut c_void) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw))
    }

    pub fn as_raw(&self) -> *mut c_void {
        self.0
    }

    /// # Safety
    /// `T` must be the vtable of the interface this pointer was obtained as.
    pub unsafe fn vtable<T>(&self) -> &T {
        unsafe { &**(self.0 as *const *const T) }
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        unsafe { (self.vtable::<IUnknown_Vtbl>().Release)(self.0) };
    }
}

pub fn check(hr: HRESULT) -> Result<(), HRESULT> {
    if hr < 0 {
        Err(hr)
    } else {
        Ok(())
    }
}

pub fn hex(hr: HRESULT) -> String {
    format!("0x{:08X}", hr as u32)
}

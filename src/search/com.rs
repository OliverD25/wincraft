use std::cell::Cell;
use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, COINIT_APARTMENTTHREADED};

/// An owned COM interface pointer. windows-sys has no COM wrappers, so each
/// interface is a hand-declared vtable reached through [`ComPtr::vtable`].
/// LayoutKeeper has the same helper; the search keeps its own so that the
/// palette does not depend on a plugin.
pub struct ComPtr(*mut c_void);

impl ComPtr {
    pub fn create(clsid: &GUID, iid: &GUID) -> Option<Self> {
        ensure_initialized();
        let mut raw = std::ptr::null_mut();
        let hr =
            unsafe { CoCreateInstance(clsid, std::ptr::null_mut(), CLSCTX_ALL, iid, &mut raw) };
        ok(hr).then(|| Self::from_raw(raw)).flatten()
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

pub fn ok(hr: HRESULT) -> bool {
    hr >= 0
}

const CLSCTX_ALL: u32 = 0x17;

thread_local! {
    static INITIALIZED: Cell<bool> = const { Cell::new(false) };
}

/// The UI thread calls COM for window desktops and Explorer folders. winit
/// usually has COM running there already; if not, this starts it. It is
/// never shut down: the thread lives as long as WinCraft.
fn ensure_initialized() {
    INITIALIZED.with(|done| {
        if !done.replace(true) {
            unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        }
    });
}

pub const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> GUID {
    GUID {
        data1,
        data2,
        data3,
        data4,
    }
}

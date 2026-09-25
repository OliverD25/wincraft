//! The one owned COM pointer. windows-sys has no COM wrappers, so each
//! interface is a hand-declared vtable reached through [`ComPtr::vtable`].
//! The host, the palette search and LayoutKeeper all use this one.

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
use windows_sys::Win32::System::Com::CoCreateInstance;

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
        // Not then_some: that builds the value first, and dropping a null
        // one would call Release through it.
        (!raw.is_null()).then(|| Self(raw))
    }

    pub fn as_raw(&self) -> *mut c_void {
        self.0
    }

    /// # Safety
    /// `T` must be the vtable of the interface this pointer was obtained as.
    pub unsafe fn vtable<T>(&self) -> &T {
        unsafe { &**(self.0 as *const *const T) }
    }

    pub fn query(&self, iid: &GUID) -> Option<ComPtr> {
        let mut raw = std::ptr::null_mut();
        let hr = unsafe { (self.vtable::<IUnknown_Vtbl>().QueryInterface)(self.0, iid, &mut raw) };
        ok(hr).then(|| Self::from_raw(raw)).flatten()
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

pub fn check(hr: HRESULT) -> Result<(), HRESULT> {
    if ok(hr) {
        Ok(())
    } else {
        Err(hr)
    }
}

pub fn hex(hr: HRESULT) -> String {
    format!("0x{:08X}", hr as u32)
}

pub const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> GUID {
    GUID {
        data1,
        data2,
        data3,
        data4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A COM object in miniature: the vtable pointer first, as COM lays it
    /// out, then counters the fake methods write to.
    #[repr(C)]
    struct Fake {
        vtable: *const IUnknown_Vtbl,
        add_refs: Cell<u32>,
        releases: Cell<u32>,
        answer: HRESULT,
    }

    const E_NOINTERFACE: HRESULT = 0x8000_4002_u32 as i32;

    unsafe extern "system" fn query_interface(
        this: *mut c_void,
        _iid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        let fake = unsafe { &*(this as *const Fake) };
        if fake.answer < 0 {
            unsafe { *out = std::ptr::null_mut() };
            return fake.answer;
        }
        fake.add_refs.set(fake.add_refs.get() + 1);
        unsafe { *out = this };
        0
    }

    unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
        let fake = unsafe { &*(this as *const Fake) };
        fake.add_refs.set(fake.add_refs.get() + 1);
        fake.add_refs.get()
    }

    unsafe extern "system" fn release(this: *mut c_void) -> u32 {
        let fake = unsafe { &*(this as *const Fake) };
        fake.releases.set(fake.releases.get() + 1);
        0
    }

    static VTABLE: IUnknown_Vtbl = IUnknown_Vtbl {
        QueryInterface: query_interface,
        AddRef: add_ref,
        Release: release,
    };

    fn fake(answer: HRESULT) -> Fake {
        Fake {
            vtable: &VTABLE,
            add_refs: Cell::new(0),
            releases: Cell::new(0),
            answer,
        }
    }

    fn raw(fake: &Fake) -> *mut c_void {
        fake as *const Fake as *mut c_void
    }

    #[test]
    fn a_null_pointer_is_not_taken() {
        assert!(ComPtr::from_raw(std::ptr::null_mut()).is_none());
    }

    #[test]
    fn dropping_releases_exactly_once() {
        let object = fake(0);
        let pointer = ComPtr::from_raw(raw(&object)).expect("not null");
        assert_eq!(pointer.as_raw(), raw(&object));
        assert_eq!(object.releases.get(), 0);
        drop(pointer);
        assert_eq!(object.releases.get(), 1);
        assert_eq!(object.add_refs.get(), 0);
    }

    #[test]
    fn a_queried_interface_is_a_second_reference_released_on_its_own() {
        let object = fake(0);
        let first = ComPtr::from_raw(raw(&object)).unwrap();
        let second = first
            .query(&guid(1, 2, 3, [4; 8]))
            .expect("the fake answers");
        assert_eq!(object.add_refs.get(), 1);
        drop(second);
        assert_eq!(object.releases.get(), 1);
        drop(first);
        assert_eq!(object.releases.get(), 2);
    }

    #[test]
    fn a_failed_query_gives_nothing_and_releases_nothing() {
        let object = fake(E_NOINTERFACE);
        let pointer = ComPtr::from_raw(raw(&object)).unwrap();
        assert!(pointer.query(&guid(1, 2, 3, [4; 8])).is_none());
        assert_eq!(object.releases.get(), 0);
        drop(pointer);
        assert_eq!(object.releases.get(), 1);
    }

    #[test]
    fn results_read_as_success_or_failure() {
        assert!(ok(0));
        assert!(ok(1));
        assert!(!ok(E_NOINTERFACE));
        assert_eq!(check(0), Ok(()));
        assert_eq!(check(E_NOINTERFACE), Err(E_NOINTERFACE));
        assert_eq!(hex(E_NOINTERFACE), "0x80004002");
    }

    #[test]
    fn a_guid_is_built_from_its_parts() {
        let built = guid(
            0xaa5b6a80,
            0xb834,
            0x11d0,
            [0x93, 0x2f, 0, 0xa0, 0xc9, 0x0d, 0xca, 0xa9],
        );
        let same = GUID::from_u128(0xaa5b6a80_b834_11d0_932f_00a0c90dcaa9);
        assert_eq!(
            (built.data1, built.data2, built.data3, built.data4),
            (same.data1, same.data2, same.data3, same.data4)
        );
    }
}

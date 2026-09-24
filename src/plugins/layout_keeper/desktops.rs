//! Reading and moving virtual desktops.
//!
//! The move chain (`Mover`) is ported from MScholtes' VirtualDesktop,
//! https://github.com/MScholtes/VirtualDesktop, file VirtualDesktop11-24H2.cs
//! (version 1.21). Every GUID and vtable order in `mod undocumented` comes
//! from that file. Its licence:
//!
//! MIT License
//!
//! Copyright (c) 2017 Markus Scholtes
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use std::ffi::c_void;
use std::fmt;

use windows_sys::core::{IUnknown_Vtbl, BOOL, GUID, HRESULT};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows_sys::Win32::System::Com::{CLSCTX_ALL, CLSCTX_LOCAL_SERVER};
use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
};

use super::com::{self, ComPtr};
use crate::core::wide;

const VIRTUAL_DESKTOPS: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops";

/// A virtual desktop's id. Kept as a number so it can be a map key and
/// compared, and printed the way the registry spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DesktopId(pub u128);

impl DesktopId {
    /// What Windows reports for a window pinned to every desktop.
    pub const ALL: DesktopId = DesktopId(0);

    pub fn from_guid(guid: &GUID) -> Self {
        Self(
            (u128::from(guid.data1) << 96)
                | (u128::from(guid.data2) << 80)
                | (u128::from(guid.data3) << 64)
                | u128::from(u64::from_be_bytes(guid.data4)),
        )
    }

    pub fn to_guid(self) -> GUID {
        GUID::from_u128(self.0)
    }

    /// The registry stores ids as raw GUID structs: three little-endian
    /// fields and eight plain bytes.
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; 16] = bytes.try_into().ok()?;
        let mut data4 = [0u8; 8];
        data4.copy_from_slice(&bytes[8..]);
        Some(Self::from_guid(&GUID {
            data1: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            data2: u16::from_le_bytes([bytes[4], bytes[5]]),
            data3: u16::from_le_bytes([bytes[6], bytes[7]]),
            data4,
        }))
    }

    pub fn parse(text: &str) -> Option<Self> {
        let hex: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() != 32 {
            return None;
        }
        u128::from_str_radix(&hex, 16).ok().map(Self)
    }
}

impl fmt::Display for DesktopId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.0;
        write!(
            f,
            "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
            (v >> 96) as u32,
            (v >> 80) as u16,
            (v >> 64) as u16,
            (v >> 48) as u16,
            v & 0xFFFF_FFFF_FFFF
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desktop {
    pub id: DesktopId,
    pub name: String,
}

/// Every desktop in the order Task View shows them. A desktop the user never
/// renamed has no Name value; Windows calls it "Desktop N" by position.
pub fn list() -> Vec<Desktop> {
    let Some(bytes) = reg_binary(VIRTUAL_DESKTOPS, "VirtualDesktopIDs") else {
        return Vec::new();
    };
    bytes
        .chunks(16)
        .filter_map(DesktopId::from_bytes)
        .enumerate()
        .map(|(index, id)| {
            let key = format!(r"{VIRTUAL_DESKTOPS}\Desktops\{id}");
            let name = reg_string(&key, "Name")
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("Desktop {}", index + 1));
            Desktop { id, name }
        })
        .collect()
}

pub fn current() -> Option<DesktopId> {
    reg_binary(VIRTUAL_DESKTOPS, "CurrentVirtualDesktop")
        .as_deref()
        .and_then(DesktopId::from_bytes)
}

pub fn name_of(desktops: &[Desktop], id: DesktopId) -> Option<String> {
    if id == DesktopId::ALL {
        return Some("All desktops".to_string());
    }
    desktops
        .iter()
        .find(|desktop| desktop.id == id)
        .map(|desktop| desktop.name.clone())
}

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

/// The documented interface. It reads any window's desktop, but moving
/// another process's window through it fails with E_ACCESSDENIED.
pub struct Reader(ComPtr);

impl Reader {
    pub fn new() -> Result<Self, String> {
        ComPtr::create(
            &CLSID_VIRTUAL_DESKTOP_MANAGER,
            &IID_IVIRTUAL_DESKTOP_MANAGER,
            CLSCTX_ALL,
        )
        .map(Self)
        .map_err(|hr| format!("IVirtualDesktopManager unavailable ({})", com::hex(hr)))
    }

    pub fn read(&self, hwnd: HWND) -> Option<DesktopId> {
        let mut guid = DesktopId::ALL.to_guid();
        let hr = unsafe {
            (self
                .0
                .vtable::<IVirtualDesktopManagerVtbl>()
                .get_window_desktop_id)(self.0.as_raw(), hwnd, &mut guid)
        };
        com::check(hr).ok().map(|()| DesktopId::from_guid(&guid))
    }
}

/// Undocumented shell interfaces, laid out exactly as in MScholtes'
/// VirtualDesktop11-24H2.cs. Microsoft changes them between Windows builds,
/// so this table is the one place to update when a build breaks moving.
///
/// C# interop turns a declared return value into a trailing out pointer and
/// returns an HRESULT, so `Guid GetId()` there is `GetId(*mut GUID)` here.
/// Methods WinCraft never calls are kept as placeholders so the offsets stay
/// right.
mod undocumented {
    use std::ffi::c_void;

    use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
    use windows_sys::Win32::Foundation::HWND;

    pub const MIN_BUILD: u32 = 26100;

    pub const CLSID_IMMERSIVE_SHELL: GUID = GUID::from_u128(0xC2F03A33_21F5_47FA_B4BB_156362A2F239);
    pub const CLSID_VIRTUAL_DESKTOP_MANAGER_INTERNAL: GUID =
        GUID::from_u128(0xC5E0CDCA_7B6E_41B2_9FC4_D93975CC467B);
    pub const IID_ISERVICE_PROVIDER: GUID = GUID::from_u128(0x6D5140C1_7436_11CE_8034_00AA006009FA);
    pub const IID_IVIRTUAL_DESKTOP_MANAGER_INTERNAL: GUID =
        GUID::from_u128(0x53F5CA0B_158F_4124_900C_057158060B27);
    pub const IID_IAPPLICATION_VIEW_COLLECTION: GUID =
        GUID::from_u128(0x1841C6D7_4F9D_42C0_AF41_8747538F10E5);
    pub const IID_IVIRTUAL_DESKTOP: GUID = GUID::from_u128(0x3F07F4BE_B107_441A_AF0F_39D82529072C);

    type Placeholder = unsafe extern "system" fn();

    #[repr(C)]
    pub struct IServiceProviderVtbl {
        pub _base: IUnknown_Vtbl,
        pub query_service: unsafe extern "system" fn(
            *mut c_void,
            *const GUID,
            *const GUID,
            *mut *mut c_void,
        ) -> HRESULT,
    }

    #[repr(C)]
    pub struct IApplicationViewCollectionVtbl {
        pub _base: IUnknown_Vtbl,
        pub _get_views: Placeholder,
        pub _get_views_by_z_order: Placeholder,
        pub _get_views_by_app_user_model_id: Placeholder,
        pub get_view_for_hwnd:
            unsafe extern "system" fn(*mut c_void, HWND, *mut *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    pub struct IVirtualDesktopVtbl {
        pub _base: IUnknown_Vtbl,
        pub _is_view_visible: Placeholder,
        pub get_id: unsafe extern "system" fn(*mut c_void, *mut GUID) -> HRESULT,
    }

    #[repr(C)]
    pub struct IVirtualDesktopManagerInternalVtbl {
        pub _base: IUnknown_Vtbl,
        pub _get_count: Placeholder,
        pub move_view_to_desktop:
            unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
        pub _can_view_move_desktops: Placeholder,
        pub _get_current_desktop: Placeholder,
        pub get_desktops: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    }

    /// The documented IObjectArray (shobjidl_core.h).
    #[repr(C)]
    pub struct IObjectArrayVtbl {
        pub _base: IUnknown_Vtbl,
        pub get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
        pub get_at:
            unsafe extern "system" fn(*mut c_void, u32, *const GUID, *mut *mut c_void) -> HRESULT,
    }
}

/// Moves other processes' windows between desktops through the shell's
/// internal interfaces, which the documented interface refuses to do.
pub struct Mover {
    internal: ComPtr,
    views: ComPtr,
}

impl Mover {
    /// Builds the chain and checks it against the registry before trusting
    /// it: a vtable that moved in a new Windows build would return nonsense
    /// here instead of crashing later in a move.
    pub fn new(registry: &[Desktop]) -> Result<Self, String> {
        use undocumented::*;

        let build = os_build().ok_or("the Windows build number cannot be read")?;
        if build < MIN_BUILD {
            return Err(format!(
                "Windows build {build} predates 24H2 ({MIN_BUILD}), whose interfaces this uses"
            ));
        }
        let shell = ComPtr::create(
            &CLSID_IMMERSIVE_SHELL,
            &IID_ISERVICE_PROVIDER,
            CLSCTX_LOCAL_SERVER,
        )
        .map_err(|hr| format!("the immersive shell is unavailable ({})", com::hex(hr)))?;
        let service = |sid: &GUID, iid: &GUID, what: &str| -> Result<ComPtr, String> {
            let mut raw = std::ptr::null_mut();
            let hr = unsafe {
                (shell.vtable::<IServiceProviderVtbl>().query_service)(
                    shell.as_raw(),
                    sid,
                    iid,
                    &mut raw,
                )
            };
            com::check(hr).map_err(|hr| format!("{what} is unavailable ({})", com::hex(hr)))?;
            ComPtr::from_raw(raw).ok_or_else(|| format!("{what} came back empty"))
        };
        let mover = Self {
            internal: service(
                &CLSID_VIRTUAL_DESKTOP_MANAGER_INTERNAL,
                &IID_IVIRTUAL_DESKTOP_MANAGER_INTERNAL,
                "IVirtualDesktopManagerInternal",
            )?,
            views: service(
                &IID_IAPPLICATION_VIEW_COLLECTION,
                &IID_IAPPLICATION_VIEW_COLLECTION,
                "IApplicationViewCollection",
            )?,
        };

        let found: Vec<DesktopId> = mover.desktops()?.into_iter().map(|(id, _)| id).collect();
        let known = found
            .iter()
            .all(|id| registry.iter().any(|desktop| desktop.id == *id));
        if found.len() != registry.len() || !known {
            return Err(format!(
                "the shell lists {} desktops that do not match the {} in the registry",
                found.len(),
                registry.len()
            ));
        }
        Ok(mover)
    }

    fn desktops(&self) -> Result<Vec<(DesktopId, ComPtr)>, String> {
        use undocumented::*;

        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            (self
                .internal
                .vtable::<IVirtualDesktopManagerInternalVtbl>()
                .get_desktops)(self.internal.as_raw(), &mut raw)
        };
        com::check(hr).map_err(|hr| format!("GetDesktops failed ({})", com::hex(hr)))?;
        let array = ComPtr::from_raw(raw).ok_or("GetDesktops came back empty")?;
        let vtable = unsafe { array.vtable::<IObjectArrayVtbl>() };
        let mut count = 0;
        let hr = unsafe { (vtable.get_count)(array.as_raw(), &mut count) };
        com::check(hr).map_err(|hr| format!("IObjectArray::GetCount failed ({})", com::hex(hr)))?;

        let mut desktops = Vec::new();
        for index in 0..count {
            let mut raw = std::ptr::null_mut();
            let hr =
                unsafe { (vtable.get_at)(array.as_raw(), index, &IID_IVIRTUAL_DESKTOP, &mut raw) };
            com::check(hr)
                .map_err(|hr| format!("IObjectArray::GetAt failed ({})", com::hex(hr)))?;
            let desktop = ComPtr::from_raw(raw).ok_or("a desktop came back empty")?;
            let mut guid = DesktopId::ALL.to_guid();
            let hr = unsafe {
                (desktop.vtable::<IVirtualDesktopVtbl>().get_id)(desktop.as_raw(), &mut guid)
            };
            com::check(hr)
                .map_err(|hr| format!("IVirtualDesktop::GetId failed ({})", com::hex(hr)))?;
            desktops.push((DesktopId::from_guid(&guid), desktop));
        }
        Ok(desktops)
    }

    pub fn move_to(&self, hwnd: HWND, target: DesktopId) -> Result<(), String> {
        use undocumented::*;

        let desktop = self
            .desktops()?
            .into_iter()
            .find(|(id, _)| *id == target)
            .map(|(_, desktop)| desktop)
            .ok_or_else(|| format!("no desktop {target}"))?;
        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            (self
                .views
                .vtable::<IApplicationViewCollectionVtbl>()
                .get_view_for_hwnd)(self.views.as_raw(), hwnd, &mut raw)
        };
        com::check(hr).map_err(|hr| format!("GetViewForHwnd failed ({})", com::hex(hr)))?;
        let view = ComPtr::from_raw(raw).ok_or("the window has no application view")?;
        let hr = unsafe {
            (self
                .internal
                .vtable::<IVirtualDesktopManagerInternalVtbl>()
                .move_view_to_desktop)(
                self.internal.as_raw(), view.as_raw(), desktop.as_raw()
            )
        };
        com::check(hr).map_err(|hr| format!("MoveViewToDesktop failed ({})", com::hex(hr)))
    }
}

fn os_build() -> Option<u32> {
    reg_value_in(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        "CurrentBuildNumber",
        RRF_RT_REG_SZ,
    )
    .map(|bytes| utf16_string(&bytes))
    .and_then(|text| text.trim().parse().ok())
}

fn reg_value(subkey: &str, value: &str, flags: u32) -> Option<Vec<u8>> {
    reg_value_in(HKEY_CURRENT_USER, subkey, value, flags)
}

fn reg_value_in(root: HKEY, subkey: &str, value: &str, flags: u32) -> Option<Vec<u8>> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut size: u32 = 0;
    let first = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if first != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    let second = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value.as_ptr(),
            flags,
            std::ptr::null_mut(),
            data.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if second != ERROR_SUCCESS {
        return None;
    }
    data.truncate(size as usize);
    Some(data)
}

fn reg_binary(subkey: &str, value: &str) -> Option<Vec<u8>> {
    reg_value(subkey, value, RRF_RT_REG_BINARY)
}

fn reg_string(subkey: &str, value: &str) -> Option<String> {
    reg_value(subkey, value, RRF_RT_REG_SZ).map(|bytes| utf16_string(&bytes))
}

fn utf16_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_prints_the_way_the_registry_spells_it_and_parses_back() {
        let id = DesktopId(0x1150cf3a_755e_4731_b223_b07262115ad0);
        let text = id.to_string();
        assert_eq!(text, "{1150CF3A-755E-4731-B223-B07262115AD0}");
        assert_eq!(DesktopId::parse(&text), Some(id));
        assert_eq!(DesktopId::parse("nonsense"), None);
        assert_eq!(DesktopId::from_guid(&id.to_guid()), id);
    }

    #[test]
    fn this_build_is_readable() {
        assert!(os_build().is_some_and(|build| build > 10_000));
    }

    #[test]
    fn registry_bytes_are_a_raw_guid_struct() {
        let bytes = [
            0x3a, 0xcf, 0x50, 0x11, 0x5e, 0x75, 0x31, 0x47, 0xb2, 0x23, 0xb0, 0x72, 0x62, 0x11,
            0x5a, 0xd0,
        ];
        assert_eq!(
            DesktopId::from_bytes(&bytes),
            Some(DesktopId(0x1150cf3a_755e_4731_b223_b07262115ad0))
        );
        assert_eq!(DesktopId::from_bytes(&bytes[..15]), None);
    }
}

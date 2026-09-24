use std::ffi::c_void;
use std::fmt;

use windows_sys::core::{IUnknown_Vtbl, BOOL, GUID, HRESULT};
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows_sys::Win32::System::Com::CLSCTX_ALL;
use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
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
    base: IUnknown_Vtbl,
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

fn reg_value(subkey: &str, value: &str, flags: u32) -> Option<Vec<u8>> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut size: u32 = 0;
    let first = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
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
            HKEY_CURRENT_USER,
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
    let bytes = reg_value(subkey, value, RRF_RT_REG_SZ)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_prints_the_way_the_registry_spells_it() {
        let id = DesktopId(0x1150cf3a_755e_4731_b223_b07262115ad0);
        let text = id.to_string();
        assert_eq!(text, "{1150CF3A-755E-4731-B223-B07262115AD0}");
        assert_eq!(DesktopId::from_guid(&id.to_guid()), id);
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

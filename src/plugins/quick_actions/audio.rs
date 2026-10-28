//! The default microphone's mute switch, through Windows' Core Audio COM
//! interfaces. windows-sys has no COM wrappers, so the three vtables are
//! declared here by hand, in the order the SDK headers list them.

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
use windows_sys::Win32::Foundation::PROPERTYKEY;
use windows_sys::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows_sys::Win32::System::Com::{CoTaskMemFree, CLSCTX_ALL};
use windows_sys::Win32::System::Variant::VT_LPWSTR;

use crate::core::com::{self, ComPtr};
use crate::core::from_wide_ptr;

const CLSID_MM_DEVICE_ENUMERATOR: GUID = GUID::from_u128(0xbcde0395_e52f_467c_8e3d_c4579291692e);
const IID_IMM_DEVICE_ENUMERATOR: GUID = GUID::from_u128(0xa95664d2_9614_4f35_a746_de8db63617e6);
const IID_IAUDIO_ENDPOINT_VOLUME: GUID = GUID::from_u128(0x5cdf2c82_841e_4546_9722_0cf74078229a);
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};
/// `eCapture` and `eConsole`: the microphone Sound settings calls default.
const CAPTURE: i32 = 1;
const CONSOLE: i32 = 0;
const STGM_READ: u32 = 0;

#[repr(C)]
struct IMMDeviceEnumeratorVtbl {
    _base: IUnknown_Vtbl,
    enum_audio_endpoints:
        unsafe extern "system" fn(*mut c_void, i32, u32, *mut *mut c_void) -> HRESULT,
    get_default_audio_endpoint:
        unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IMMDeviceVtbl {
    _base: IUnknown_Vtbl,
    activate: unsafe extern "system" fn(
        *mut c_void,
        *const GUID,
        u32,
        *const PROPVARIANT,
        *mut *mut c_void,
    ) -> HRESULT,
    open_property_store: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT,
    get_id: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
}

#[repr(C)]
struct IMMDeviceCollectionVtbl {
    _base: IUnknown_Vtbl,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    item: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT,
}

/// Windows' own interface for setting the default sound device, which
/// Sound settings uses. It is not documented; its layout is the one every
/// sound switcher has used since Windows 7. Only `set_default_endpoint` is
/// called.
#[repr(C)]
struct IPolicyConfigVtbl {
    _base: IUnknown_Vtbl,
    _get_mix_format: unsafe extern "system" fn(),
    _get_device_format: unsafe extern "system" fn(),
    _reset_device_format: unsafe extern "system" fn(),
    _set_device_format: unsafe extern "system" fn(),
    _get_processing_period: unsafe extern "system" fn(),
    _set_processing_period: unsafe extern "system" fn(),
    _get_share_mode: unsafe extern "system" fn(),
    _set_share_mode: unsafe extern "system" fn(),
    _get_property_value: unsafe extern "system" fn(),
    _set_property_value: unsafe extern "system" fn(),
    set_default_endpoint: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT,
}

const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const IID_IPOLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
/// `eRender`, and the three roles a default device has.
const RENDER: i32 = 0;
const ROLES: [i32; 3] = [0, 1, 2];
const DEVICE_STATE_ACTIVE: u32 = 1;

/// A sound output device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Windows' id for it, which Undo and the pick use.
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[repr(C)]
struct IPropertyStoreVtbl {
    _base: IUnknown_Vtbl,
    _get_count: unsafe extern "system" fn(),
    _get_at: unsafe extern "system" fn(),
    get_value:
        unsafe extern "system" fn(*mut c_void, *const PROPERTYKEY, *mut PROPVARIANT) -> HRESULT,
}

#[repr(C)]
struct IAudioEndpointVolumeVtbl {
    _base: IUnknown_Vtbl,
    _register_control_change_notify: unsafe extern "system" fn(),
    _unregister_control_change_notify: unsafe extern "system" fn(),
    _get_channel_count: unsafe extern "system" fn(),
    _set_master_volume_level: unsafe extern "system" fn(),
    _set_master_volume_level_scalar: unsafe extern "system" fn(),
    _get_master_volume_level: unsafe extern "system" fn(),
    _get_master_volume_level_scalar: unsafe extern "system" fn(),
    _set_channel_volume_level: unsafe extern "system" fn(),
    _set_channel_volume_level_scalar: unsafe extern "system" fn(),
    _get_channel_volume_level: unsafe extern "system" fn(),
    _get_channel_volume_level_scalar: unsafe extern "system" fn(),
    set_mute: unsafe extern "system" fn(*mut c_void, i32, *const GUID) -> HRESULT,
    get_mute: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

const NO_MIC: &str = "No microphone is connected";

fn enumerator() -> Result<ComPtr, String> {
    ComPtr::create(
        &CLSID_MM_DEVICE_ENUMERATOR,
        &IID_IMM_DEVICE_ENUMERATOR,
        CLSCTX_ALL,
    )
    .map_err(|hr| format!("Windows sound is not available ({})", com::hex(hr)))
}

fn default_device(flow: i32) -> Option<ComPtr> {
    let enumerator = enumerator().ok()?;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (enumerator
            .vtable::<IMMDeviceEnumeratorVtbl>()
            .get_default_audio_endpoint)(enumerator.as_raw(), flow, CONSOLE, &mut raw)
    };
    com::ok(hr).then(|| ComPtr::from_raw(raw)).flatten()
}

/// The default microphone.
fn device() -> Result<ComPtr, String> {
    enumerator()?;
    default_device(CAPTURE).ok_or_else(|| NO_MIC.to_string())
}

fn id_of(device: &ComPtr) -> Option<String> {
    let mut raw: *mut u16 = std::ptr::null_mut();
    let hr = unsafe { (device.vtable::<IMMDeviceVtbl>().get_id)(device.as_raw(), &mut raw) };
    if !com::ok(hr) || raw.is_null() {
        return None;
    }
    let id = unsafe { from_wide_ptr(raw) };
    unsafe { CoTaskMemFree(raw as *const c_void) };
    Some(id)
}

/// The sound output devices that are connected, the default one marked.
pub fn render_devices() -> Result<Vec<Device>, String> {
    let enumerator = enumerator()?;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (enumerator
            .vtable::<IMMDeviceEnumeratorVtbl>()
            .enum_audio_endpoints)(
            enumerator.as_raw(), RENDER, DEVICE_STATE_ACTIVE, &mut raw
        )
    };
    com::check(hr).map_err(|hr| format!("Windows did not list the devices ({})", com::hex(hr)))?;
    let collection = ComPtr::from_raw(raw).ok_or("Windows did not list the devices")?;
    let vtable = unsafe { collection.vtable::<IMMDeviceCollectionVtbl>() };
    let mut count = 0u32;
    unsafe { (vtable.get_count)(collection.as_raw(), &mut count) };
    let default = default_device(RENDER).and_then(|device| id_of(&device));
    let mut devices = Vec::new();
    for index in 0..count {
        let mut raw = std::ptr::null_mut();
        if !com::ok(unsafe { (vtable.item)(collection.as_raw(), index, &mut raw) }) {
            continue;
        }
        let Some(device) = ComPtr::from_raw(raw) else {
            continue;
        };
        let Some(id) = id_of(&device) else {
            continue;
        };
        devices.push(Device {
            default: default.as_deref() == Some(id.as_str()),
            name: name_of(&device).unwrap_or_else(|| "Sound device".to_string()),
            id,
        });
    }
    Ok(devices)
}

/// Makes the device the default for every role: games and music, films,
/// and calls.
pub fn set_default_render(id: &str) -> Result<(), String> {
    let policy = ComPtr::create(&CLSID_POLICY_CONFIG_CLIENT, &IID_IPOLICY_CONFIG, CLSCTX_ALL)
        .map_err(|hr| {
            format!(
                "this Windows does not let apps switch the sound device ({})",
                com::hex(hr)
            )
        })?;
    let wide_id = crate::core::wide(id);
    for role in ROLES {
        let hr = unsafe {
            (policy.vtable::<IPolicyConfigVtbl>().set_default_endpoint)(
                policy.as_raw(),
                wide_id.as_ptr(),
                role,
            )
        };
        com::check(hr).map_err(|hr| format!("Windows refused the switch ({})", com::hex(hr)))?;
    }
    Ok(())
}

fn volume() -> Result<ComPtr, String> {
    let device = device()?;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (device.vtable::<IMMDeviceVtbl>().activate)(
            device.as_raw(),
            &IID_IAUDIO_ENDPOINT_VOLUME,
            CLSCTX_ALL,
            std::ptr::null(),
            &mut raw,
        )
    };
    com::check(hr).map_err(|hr| format!("the microphone did not answer ({})", com::hex(hr)))?;
    ComPtr::from_raw(raw).ok_or_else(|| NO_MIC.to_string())
}

/// Whether the default microphone is muted, and its name.
pub fn state() -> Result<(bool, String), String> {
    let volume = volume()?;
    let mut muted = 0;
    let hr = unsafe {
        (volume.vtable::<IAudioEndpointVolumeVtbl>().get_mute)(volume.as_raw(), &mut muted)
    };
    com::check(hr).map_err(|hr| format!("the microphone did not answer ({})", com::hex(hr)))?;
    let name = device().ok().and_then(|device| name_of(&device));
    Ok((muted != 0, name.unwrap_or_default()))
}

pub fn set_muted(muted: bool) -> Result<(), String> {
    let volume = volume()?;
    let hr = unsafe {
        (volume.vtable::<IAudioEndpointVolumeVtbl>().set_mute)(
            volume.as_raw(),
            i32::from(muted),
            std::ptr::null(),
        )
    };
    com::check(hr).map_err(|hr| format!("Windows refused the change ({})", com::hex(hr)))
}

/// A device's name as Sound settings shows it.
fn name_of(device: &ComPtr) -> Option<String> {
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (device.vtable::<IMMDeviceVtbl>().open_property_store)(device.as_raw(), STGM_READ, &mut raw)
    };
    if !com::ok(hr) {
        return None;
    }
    let store = ComPtr::from_raw(raw)?;
    let mut value: PROPVARIANT = unsafe { std::mem::zeroed() };
    let hr = unsafe {
        (store.vtable::<IPropertyStoreVtbl>().get_value)(
            store.as_raw(),
            &PKEY_DEVICE_FRIENDLY_NAME,
            &mut value,
        )
    };
    let mut name = None;
    unsafe {
        if com::ok(hr) && value.Anonymous.Anonymous.vt == VT_LPWSTR {
            let text = value.Anonymous.Anonymous.Anonymous.pwszVal;
            if !text.is_null() {
                name = Some(from_wide_ptr(text));
            }
        }
        PropVariantClear(&mut value);
    }
    name
}

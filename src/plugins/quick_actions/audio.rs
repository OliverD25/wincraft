//! The default microphone's mute switch, through Windows' Core Audio COM
//! interfaces. windows-sys has no COM wrappers, so the three vtables are
//! declared here by hand, in the order the SDK headers list them.

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT};
use windows_sys::Win32::Foundation::PROPERTYKEY;
use windows_sys::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows_sys::Win32::System::Com::CLSCTX_ALL;
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
    _enum_audio_endpoints: unsafe extern "system" fn(),
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

/// The default microphone.
fn device() -> Result<ComPtr, String> {
    let enumerator = ComPtr::create(
        &CLSID_MM_DEVICE_ENUMERATOR,
        &IID_IMM_DEVICE_ENUMERATOR,
        CLSCTX_ALL,
    )
    .map_err(|hr| format!("Windows sound is not available ({})", com::hex(hr)))?;
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (enumerator
            .vtable::<IMMDeviceEnumeratorVtbl>()
            .get_default_audio_endpoint)(enumerator.as_raw(), CAPTURE, CONSOLE, &mut raw)
    };
    com::check(hr).map_err(|_| NO_MIC.to_string())?;
    ComPtr::from_raw(raw).ok_or_else(|| NO_MIC.to_string())
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
    Ok((muted != 0, name().unwrap_or_default()))
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

/// The microphone's name as Sound settings shows it.
fn name() -> Option<String> {
    let device = device().ok()?;
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

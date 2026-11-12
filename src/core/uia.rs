//! The UI Automation client calls WinCraft makes. windows-sys has no COM
//! wrappers, so the interfaces are hand-declared vtables, with small readers
//! over them. The taskbar button lookup and the thumbnail probe use them.
//!
//! Every call here crosses into the process that draws the element, so a
//! caller asks from a thread of its own that started COM itself (see
//! [`com::joined_apartment`]).

use std::ffi::c_void;

use windows_sys::core::{IUnknown_Vtbl, BOOL, BSTR, GUID, HRESULT};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::System::Com::CLSCTX_INPROC_SERVER;

use crate::core::com::{self, ComPtr};

const CLSID_CUI_AUTOMATION: GUID = GUID::from_u128(0xff48dba4_60ef_4201_aa87_54103eef594e);
const IID_IUI_AUTOMATION: GUID = GUID::from_u128(0x30cbe57d_d9d0_452a_ab13_7ac5ac4825ee);

// Slot orders follow UIAutomationClient.h (Windows SDK 10.0.26100.0).

#[repr(C)]
struct IUIAutomationVtbl {
    _base: IUnknown_Vtbl,
    _compare_elements: unsafe extern "system" fn(),
    _compare_runtime_ids: unsafe extern "system" fn(),
    _get_root_element: unsafe extern "system" fn(),
    element_from_handle:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
    element_from_point: unsafe extern "system" fn(*mut c_void, POINT, *mut *mut c_void) -> HRESULT,
    _get_focused_element: unsafe extern "system" fn(),
    _get_root_element_build_cache: unsafe extern "system" fn(),
    _element_from_handle_build_cache: unsafe extern "system" fn(),
    _element_from_point_build_cache: unsafe extern "system" fn(),
    _get_focused_element_build_cache: unsafe extern "system" fn(),
    _create_tree_walker: unsafe extern "system" fn(),
    _get_control_view_walker: unsafe extern "system" fn(),
    _get_content_view_walker: unsafe extern "system" fn(),
    get_raw_view_walker: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

type WalkStep = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

#[repr(C)]
struct IUIAutomationTreeWalkerVtbl {
    _base: IUnknown_Vtbl,
    get_parent_element: WalkStep,
    get_first_child_element: WalkStep,
    _get_last_child_element: WalkStep,
    get_next_sibling_element: WalkStep,
}

const IID_IUI_AUTOMATION_INVOKE_PATTERN: GUID =
    GUID::from_u128(0xfb377fbe_8ea6_46d5_9c73_6499642d3059);
const UIA_INVOKE_PATTERN_ID: i32 = 10000;

/// Control type ids of UIAutomationClient.h.
pub const CONTROL_BUTTON: i32 = 50000;
pub const CONTROL_GROUP: i32 = 50026;

#[repr(C)]
struct IUIAutomationInvokePatternVtbl {
    _base: IUnknown_Vtbl,
    invoke: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

type BstrGetter = unsafe extern "system" fn(*mut c_void, *mut BSTR) -> HRESULT;

#[repr(C)]
struct IUIAutomationElementVtbl {
    _base: IUnknown_Vtbl,
    _set_focus: unsafe extern "system" fn(),
    _get_runtime_id: unsafe extern "system" fn(),
    _find_first: unsafe extern "system" fn(),
    _find_all: unsafe extern "system" fn(),
    _find_first_build_cache: unsafe extern "system" fn(),
    _find_all_build_cache: unsafe extern "system" fn(),
    _build_updated_cache: unsafe extern "system" fn(),
    _get_current_property_value: unsafe extern "system" fn(),
    _get_current_property_value_ex: unsafe extern "system" fn(),
    _get_cached_property_value: unsafe extern "system" fn(),
    _get_cached_property_value_ex: unsafe extern "system" fn(),
    _get_current_pattern_as: unsafe extern "system" fn(),
    _get_cached_pattern_as: unsafe extern "system" fn(),
    get_current_pattern: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> HRESULT,
    _get_cached_pattern: unsafe extern "system" fn(),
    _get_cached_parent: unsafe extern "system" fn(),
    _get_cached_children: unsafe extern "system" fn(),
    _get_current_process_id: unsafe extern "system" fn(),
    get_current_control_type: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
    _get_current_localized_control_type: unsafe extern "system" fn(),
    get_current_name: BstrGetter,
    _get_current_accelerator_key: unsafe extern "system" fn(),
    _get_current_access_key: unsafe extern "system" fn(),
    _get_current_has_keyboard_focus: unsafe extern "system" fn(),
    _get_current_is_keyboard_focusable: unsafe extern "system" fn(),
    _get_current_is_enabled: unsafe extern "system" fn(),
    get_current_automation_id: BstrGetter,
    get_current_class_name: BstrGetter,
    _get_current_help_text: unsafe extern "system" fn(),
    _get_current_culture: unsafe extern "system" fn(),
    _get_current_is_control_element: unsafe extern "system" fn(),
    _get_current_is_content_element: unsafe extern "system" fn(),
    _get_current_is_password: unsafe extern "system" fn(),
    get_current_native_window_handle:
        unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    _get_current_item_type: unsafe extern "system" fn(),
    get_current_is_offscreen: unsafe extern "system" fn(*mut c_void, *mut BOOL) -> HRESULT,
    _get_current_orientation: unsafe extern "system" fn(),
    _get_current_framework_id: unsafe extern "system" fn(),
    _get_current_is_required_for_form: unsafe extern "system" fn(),
    _get_current_item_status: unsafe extern "system" fn(),
    get_current_bounding_rectangle: unsafe extern "system" fn(*mut c_void, *mut RECT) -> HRESULT,
}

/// The UI Automation client object, with the raw view walker, which shows
/// every element, not only the ones a screen reader would read.
pub struct Automation {
    automation: ComPtr,
    walker: ComPtr,
}

impl Automation {
    pub fn new() -> Result<Self, String> {
        let automation = ComPtr::create(
            &CLSID_CUI_AUTOMATION,
            &IID_IUI_AUTOMATION,
            CLSCTX_INPROC_SERVER,
        )
        .map_err(|hr| format!("UI Automation is unavailable ({})", com::hex(hr)))?;
        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            (automation.vtable::<IUIAutomationVtbl>().get_raw_view_walker)(
                automation.as_raw(),
                &mut raw,
            )
        };
        let walker =
            taken(hr, raw).ok_or_else(|| format!("RawViewWalker failed ({})", com::hex(hr)))?;
        Ok(Self { automation, walker })
    }

    pub fn element_from_point(&self, x: i32, y: i32) -> Result<ComPtr, String> {
        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            (self.vtable().element_from_point)(self.automation.as_raw(), POINT { x, y }, &mut raw)
        };
        taken(hr, raw).ok_or_else(|| format!("ElementFromPoint failed ({})", com::hex(hr)))
    }

    pub fn element_from_handle(&self, hwnd: HWND) -> Result<ComPtr, String> {
        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            (self.vtable().element_from_handle)(self.automation.as_raw(), hwnd, &mut raw)
        };
        taken(hr, raw).ok_or_else(|| format!("ElementFromHandle failed ({})", com::hex(hr)))
    }

    pub fn parent(&self, element: &ComPtr) -> Option<ComPtr> {
        self.step(element, self.walker_vtable().get_parent_element)
    }

    pub fn first_child(&self, element: &ComPtr) -> Option<ComPtr> {
        self.step(element, self.walker_vtable().get_first_child_element)
    }

    pub fn next_sibling(&self, element: &ComPtr) -> Option<ComPtr> {
        self.step(element, self.walker_vtable().get_next_sibling_element)
    }

    fn step(&self, element: &ComPtr, how: WalkStep) -> Option<ComPtr> {
        let mut raw = std::ptr::null_mut();
        let hr = unsafe { how(self.walker.as_raw(), element.as_raw(), &mut raw) };
        taken(hr, raw)
    }

    fn vtable(&self) -> &IUIAutomationVtbl {
        unsafe { self.automation.vtable::<IUIAutomationVtbl>() }
    }

    fn walker_vtable(&self) -> &IUIAutomationTreeWalkerVtbl {
        unsafe { self.walker.vtable::<IUIAutomationTreeWalkerVtbl>() }
    }
}

fn taken(hr: HRESULT, raw: *mut c_void) -> Option<ComPtr> {
    com::ok(hr).then(|| ComPtr::from_raw(raw)).flatten()
}

fn element_vtable(element: &ComPtr) -> &IUIAutomationElementVtbl {
    unsafe { element.vtable::<IUIAutomationElementVtbl>() }
}

fn bstr_property(element: &ComPtr, getter: BstrGetter) -> Option<String> {
    let mut value: BSTR = std::ptr::null();
    let hr = unsafe { getter(element.as_raw(), &mut value) };
    let text = unsafe { com::take_bstr(value) };
    com::ok(hr).then_some(text).flatten()
}

/// The name a screen reader reads for the element: for a button, its text.
pub fn name(element: &ComPtr) -> Option<String> {
    bstr_property(element, element_vtable(element).get_current_name)
}

/// The control type id (`CONTROL_BUTTON`, `CONTROL_GROUP`, ...).
pub fn control_type(element: &ComPtr) -> Option<i32> {
    let mut kind = 0;
    let hr =
        unsafe { (element_vtable(element).get_current_control_type)(element.as_raw(), &mut kind) };
    com::ok(hr).then_some(kind)
}

/// Presses the element through its Invoke pattern, as a screen reader
/// would. Fails when the element cannot be invoked.
pub fn invoke(element: &ComPtr) -> Result<(), String> {
    let mut raw = std::ptr::null_mut();
    let hr = unsafe {
        (element_vtable(element).get_current_pattern)(
            element.as_raw(),
            UIA_INVOKE_PATTERN_ID,
            &mut raw,
        )
    };
    let unknown = taken(hr, raw)
        .ok_or_else(|| format!("the element has no Invoke pattern ({})", com::hex(hr)))?;
    let pattern = unknown
        .query(&IID_IUI_AUTOMATION_INVOKE_PATTERN)
        .ok_or_else(|| "the Invoke pattern could not be used".to_string())?;
    let hr =
        unsafe { (pattern.vtable::<IUIAutomationInvokePatternVtbl>().invoke)(pattern.as_raw()) };
    com::check(hr).map_err(|hr| format!("Invoke failed ({})", com::hex(hr)))
}

pub fn class_name(element: &ComPtr) -> Option<String> {
    bstr_property(element, element_vtable(element).get_current_class_name)
}

pub fn automation_id(element: &ComPtr) -> Option<String> {
    bstr_property(element, element_vtable(element).get_current_automation_id)
}

/// Left, top, right, bottom in physical screen pixels.
pub fn bounding_rect(element: &ComPtr) -> Option<[i32; 4]> {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let hr = unsafe {
        (element_vtable(element).get_current_bounding_rectangle)(element.as_raw(), &mut rect)
    };
    com::ok(hr).then_some([rect.left, rect.top, rect.right, rect.bottom])
}

pub fn is_offscreen(element: &ComPtr) -> Option<bool> {
    let mut value: BOOL = 0;
    let hr =
        unsafe { (element_vtable(element).get_current_is_offscreen)(element.as_raw(), &mut value) };
    com::ok(hr).then_some(value != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_created_on_a_thread_without_com() {
        let result = std::thread::spawn(|| Automation::new().err())
            .join()
            .unwrap();
        assert!(result.is_some_and(|reason| reason.contains("unavailable")));
    }
}

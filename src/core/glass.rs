//! Windows 11 glass behind the palette in the Fluent theme: the acrylic
//! backdrop Windows draws behind a window, seen through the palette's
//! translucent fill. Only the palette gets it: the settings window and the
//! Arrange strip are second eframe windows, whose OpenGL setup cannot be
//! transparent, so they stay solid.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{
    DwmEnableBlurBehindWindow, DwmSetWindowAttribute, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND,
    DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND,
};
use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

use crate::core::config::ThemeName;

/// The first Windows 11 build with the system backdrop (22H2).
pub const MIN_BUILD: u32 = 22621;

/// What decides whether the glass can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conditions {
    /// The Windows build, when it could be read.
    pub build: Option<u32>,
    /// Settings → Personalization → Colors → Transparency effects.
    pub transparency: bool,
    pub battery_saver: bool,
    pub high_contrast: bool,
}

/// Glass only in Fluent, on a Windows that has it, and never when the
/// person turned transparency off or the PC saves battery.
pub fn allowed(theme: ThemeName, conditions: Conditions) -> bool {
    theme == ThemeName::Fluent
        && conditions.build.is_some_and(|build| build >= MIN_BUILD)
        && conditions.transparency
        && !conditions.battery_saver
        && !conditions.high_contrast
}

/// The conditions right now. Cheap: two registry reads and a power status.
pub fn conditions() -> Conditions {
    let mut power = SYSTEM_POWER_STATUS::default();
    let battery_saver =
        unsafe { GetSystemPowerStatus(&mut power) } != 0 && power.SystemStatusFlag & 1 != 0;
    Conditions {
        build: crate::core::virtual_desktops::os_build(),
        transparency: crate::core::theme::transparency_effects(),
        battery_saver,
        high_contrast: crate::core::theme::windows_high_contrast(),
    }
}

const FRAMED: isize =
    (WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX) as isize;

/// The palette window's style without a caption or frame: winit keeps
/// WS_CAPTION on undecorated windows, and Windows would draw the caption
/// buttons of a captioned window over the glass.
pub fn popup_style(style: isize) -> isize {
    (style & !FRAMED) | WS_POPUP as isize
}

/// Asks Windows to round the palette window's corners (with the glass, where
/// the window is the panel) or not (without it, where the window is larger
/// than the panel). Called again each time the palette shows: the host turns
/// rounding off once when the window is made, and that can land after the
/// glass turned it on.
pub fn set_corners(window: isize, round: bool) -> i32 {
    if window == 0 {
        return 0;
    }
    let corners = if round {
        DWMWCP_ROUND
    } else {
        DWMWCP_DONOTROUND
    };
    unsafe {
        DwmSetWindowAttribute(
            window as HWND,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corners as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&corners) as u32,
        )
    }
}

/// Puts the glass on the palette window, or takes it off. `saved` keeps
/// the window's own style while the glass is on, to put it back.
pub fn set_palette(window: isize, on: bool, saved: &mut Option<isize>) {
    if window == 0 {
        return;
    }
    let hwnd = window as HWND;
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let (new_style, corners, backdrop) = if on {
        saved.get_or_insert(style);
        (popup_style(style), DWMWCP_ROUND, DWMSBT_TRANSIENTWINDOW)
    } else {
        (
            saved.take().unwrap_or(style),
            DWMWCP_DONOTROUND,
            DWMSBT_NONE,
        )
    };
    let corner_hr;
    let backdrop_hr;
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        corner_hr = set_corners(window, corners == DWMWCP_ROUND);
        backdrop_hr = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            &backdrop as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&backdrop) as u32,
        );
    }
    // DWM uses the alpha of the OpenGL picture only with blur-behind on an
    // empty region. winit sets it for a transparent window already; it is
    // set again so the glass does not depend on that.
    let blur_hr = if on {
        let region = unsafe { CreateRectRgn(0, 0, -1, -1) };
        let behind = DWM_BLURBEHIND {
            dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
            fEnable: 1,
            hRgnBlur: region,
            fTransitionOnMaximized: 0,
        };
        let hr = unsafe { DwmEnableBlurBehindWindow(hwnd, &behind) };
        unsafe { DeleteObject(region) };
        hr
    } else {
        0
    };
    if on {
        log::info!(
            "palette glass on (corners {corner_hr:#x}, backdrop {backdrop_hr:#x}, blur-behind {blur_hr:#x})"
        );
    } else {
        log::info!("palette glass off (corners {corner_hr:#x}, backdrop {backdrop_hr:#x})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fine() -> Conditions {
        Conditions {
            build: Some(26200),
            transparency: true,
            battery_saver: false,
            high_contrast: false,
        }
    }

    #[test]
    fn glass_needs_fluent_a_new_windows_and_nothing_that_turns_it_off() {
        assert!(allowed(ThemeName::Fluent, fine()));
        for theme in ThemeName::ALL {
            if theme != ThemeName::Fluent {
                assert!(!allowed(theme, fine()), "{theme:?}");
            }
        }
        let with = |change: fn(&mut Conditions)| {
            let mut conditions = fine();
            change(&mut conditions);
            allowed(ThemeName::Fluent, conditions)
        };
        assert!(!with(|c| c.build = Some(22000)));
        assert!(with(|c| c.build = Some(MIN_BUILD)));
        assert!(!with(|c| c.build = None));
        assert!(!with(|c| c.transparency = false));
        assert!(!with(|c| c.battery_saver = true));
        assert!(!with(|c| c.high_contrast = true));
    }

    #[test]
    fn the_popup_style_drops_the_caption_and_frame_and_keeps_the_rest() {
        let winit = 0x06cb_0000_isize;
        let popup = popup_style(winit);
        assert_eq!(popup, 0x8600_0000_u32 as isize);
        assert_eq!(popup & WS_CAPTION as isize, 0);
        assert_ne!(popup & WS_POPUP as isize, 0);
    }

    #[test]
    fn the_conditions_can_be_read_and_no_window_is_left_alone() {
        let _ = conditions();
        let mut saved = None;
        set_palette(0, true, &mut saved);
        assert_eq!(saved, None);
    }
}

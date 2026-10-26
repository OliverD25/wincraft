use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_SUCCESS, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromRect, MonitorFromWindow, HDC, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, GetWindowRect, IsIconic, IsZoomed, PostMessageW, SetWindowPlacement,
    SetWindowPos, MONITORINFOF_PRIMARY, SC_MAXIMIZE, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
    SWP_NOZORDER, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, WINDOWPLACEMENT, WM_SYSCOMMAND,
};

/// Left, top, right, bottom in physical pixels.
pub type Rect = [i32; 4];

/// Each monitor's place word by where its left edge is: "Left", "Centre",
/// "Right" for three, "Left" and "Right" for two, "Monitor N" counted from
/// the left past three, and nothing for a single monitor.
pub fn position_names(lefts: &[i32]) -> Vec<Option<String>> {
    const NAMES: [&[&str]; 4] = [&[], &[], &["Left", "Right"], &["Left", "Centre", "Right"]];
    let mut order: Vec<usize> = (0..lefts.len()).collect();
    order.sort_by_key(|index| lefts[*index]);
    let mut result = vec![None; lefts.len()];
    for (rank, index) in order.into_iter().enumerate() {
        result[index] = match NAMES.get(lefts.len()) {
            Some(names) => names.get(rank).map(|name| name.to_string()),
            None => Some(format!("Monitor {}", rank + 1)),
        };
    }
    result
}

/// GDI device name to friendly name, for every active display path. The
/// friendly name comes from the monitor's EDID and can be empty, as on
/// many laptop panels.
pub fn friendly_names() -> Vec<(String, String)> {
    let (mut path_count, mut mode_count) = (0u32, 0u32);
    if unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
    } != ERROR_SUCCESS
    {
        return Vec::new();
    }
    let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> =
        vec![unsafe { std::mem::zeroed() }; path_count as usize];
    let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> =
        vec![unsafe { std::mem::zeroed() }; mode_count as usize];
    let status = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Vec::new();
    }
    paths.truncate(path_count as usize);
    paths
        .iter()
        .filter_map(|path| {
            let mut source: DISPLAYCONFIG_SOURCE_DEVICE_NAME = unsafe { std::mem::zeroed() };
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0 {
                return None;
            }
            let mut target: DISPLAYCONFIG_TARGET_DEVICE_NAME = unsafe { std::mem::zeroed() };
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0 {
                return None;
            }
            Some((
                from_wide(&source.viewGdiDeviceName),
                from_wide(&target.monitorFriendlyDeviceName),
            ))
        })
        .collect()
}

/// A window rectangle on one monitor's work area put at the same place on
/// another's, in proportion: a window half as wide as the left monitor is
/// half as wide as the right one, as far in from its edges. Physical pixels,
/// so a different scale on the other monitor changes nothing here.
pub fn map_rect(rect: Rect, from: Rect, to: Rect) -> Rect {
    let along = |value: i32, from_start: i32, from_end: i32, to_start: i32, to_end: i32| {
        let from_length = i64::from(from_end - from_start);
        if from_length <= 0 {
            return to_start + (value - from_start);
        }
        let offset = i64::from(value - from_start) * i64::from(to_end - to_start);
        to_start + (offset as f64 / from_length as f64).round() as i32
    };
    [
        along(rect[0], from[0], from[2], to[0], to[2]),
        along(rect[1], from[1], from[3], to[1], to[3]),
        along(rect[2], from[0], from[2], to[0], to[2]),
        along(rect[3], from[1], from[3], to[1], to[3]),
    ]
}

/// `map_rect` for a window's normal position, which is in workspace
/// coordinates: they start at the corner of the primary monitor's work area
/// (`primary`) rather than of the screen.
pub fn normal_on(normal: Rect, from: Rect, to: Rect, primary: Rect) -> Rect {
    let [dx, dy] = [primary[0], primary[1]];
    let screen = [
        normal[0] + dx,
        normal[1] + dy,
        normal[2] + dx,
        normal[3] + dy,
    ];
    let [left, top, right, bottom] = map_rect(screen, from, to);
    [left - dx, top - dy, right - dx, bottom - dy]
}

/// Moves a window from one monitor to another with `map_rect`, without
/// activating it: activating a window on another virtual desktop would
/// switch to that desktop. A minimized window only gets the place it comes
/// back to. A maximized one is put back to its normal size on the new
/// monitor and asked to maximize there, since Windows has no way to
/// maximize another program's window without activating it.
/// `from` and `to` are the two monitors' work areas, `primary` the primary
/// monitor's.
pub fn move_window(hwnd: HWND, from: Rect, to: Rect, primary: Rect) -> Result<(), String> {
    let failed = |what: &str| format!("{what} failed (error {})", unsafe { GetLastError() });
    let mut place: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    if unsafe { GetWindowPlacement(hwnd, &mut place) } == 0 {
        return Err(failed("GetWindowPlacement"));
    }
    let [left, top, right, bottom] = normal_on(corners(place.rcNormalPosition), from, to, primary);
    place.rcNormalPosition = RECT {
        left,
        top,
        right,
        bottom,
    };
    if unsafe { IsIconic(hwnd) } != 0 {
        place.showCmd = SW_SHOWMINNOACTIVE as u32;
        if unsafe { SetWindowPlacement(hwnd, &place) } == 0 {
            return Err(failed("SetWindowPlacement"));
        }
        return Ok(());
    }
    if unsafe { IsZoomed(hwnd) } != 0 {
        place.showCmd = SW_SHOWNOACTIVATE as u32;
        if unsafe { SetWindowPlacement(hwnd, &place) } == 0 {
            return Err(failed("SetWindowPlacement"));
        }
        unsafe { PostMessageW(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE as usize, 0) };
        return Ok(());
    }
    let mut now: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut now) } == 0 {
        return Err(failed("GetWindowRect"));
    }
    let [left, top, right, bottom] = map_rect(corners(now), from, to);
    let flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    // Twice: on a monitor with another scale the window resizes itself to
    // the new DPI as it arrives, and the second call gives it its size.
    for _ in 0..2 {
        if unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                left,
                top,
                right - left,
                bottom - top,
                flags,
            )
        } == 0
        {
            return Err(failed("SetWindowPos"));
        }
    }
    Ok(())
}

fn corners(rect: RECT) -> Rect {
    [rect.left, rect.top, rect.right, rect.bottom]
}

/// One active monitor: its whole area and the part the taskbar leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    handle: HMONITOR,
    pub rect: Rect,
    pub work: Rect,
    pub primary: bool,
}

/// Every active monitor, from the left.
pub fn screens() -> Vec<Screen> {
    let mut found: Vec<Screen> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut found as *mut Vec<Screen> as LPARAM,
        )
    };
    found.sort_by_key(|screen| (screen.rect[0], screen.rect[1]));
    found
}

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> i32 {
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        let found = unsafe { &mut *(data as *mut Vec<Screen>) };
        found.push(Screen {
            handle: monitor,
            rect: corners(info.rcMonitor),
            work: corners(info.rcWork),
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    1
}

/// The place in `screens` of the monitor a window is mostly on.
pub fn screen_of(hwnd: HWND, screens: &[Screen]) -> Option<usize> {
    let handle = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    screens.iter().position(|screen| screen.handle == handle)
}

/// The place in `screens` of the monitor a rectangle is mostly on.
pub fn screen_at(rect: Rect, screens: &[Screen]) -> Option<usize> {
    let rect = RECT {
        left: rect[0],
        top: rect[1],
        right: rect[2],
        bottom: rect[3],
    };
    let handle = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    screens.iter().position(|screen| screen.handle == handle)
}

/// The monitor after `from`, going right and round to the first.
pub fn next_screen(from: usize, count: usize) -> usize {
    if count == 0 {
        0
    } else {
        (from + 1) % count
    }
}

/// Moves a window to the next monitor to the right, or round to the
/// leftmost, keeping its place relative to the screen. Nothing with one
/// monitor.
pub fn move_to_next(hwnd: HWND) -> Result<(), String> {
    let screens = screens();
    if screens.len() < 2 {
        return Ok(());
    }
    let from = screen_of(hwnd, &screens).ok_or("the window is on no monitor")?;
    let to = next_screen(from, screens.len());
    let primary = screens
        .iter()
        .find(|screen| screen.primary)
        .map_or([0, 0, 0, 0], |screen| screen.work);
    move_window(hwnd, screens[from].work, screens[to].work, primary)
}

/// A fixed-size UTF-16 field of a Win32 struct, up to its first NUL.
pub fn from_wide(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(names: &[&str]) -> Vec<Option<String>> {
        names.iter().map(|name| Some(name.to_string())).collect()
    }

    #[test]
    fn one_monitor_has_no_name() {
        assert_eq!(position_names(&[0]), [None]);
        assert!(position_names(&[]).is_empty());
    }

    #[test]
    fn two_monitors_are_left_and_right() {
        assert_eq!(position_names(&[0, 2560]), words(&["Left", "Right"]));
        assert_eq!(position_names(&[2560, 0]), words(&["Right", "Left"]));
    }

    #[test]
    fn three_monitors_with_the_primary_in_the_centre() {
        // The primary monitor is at 0, whatever order Windows lists them in.
        assert_eq!(
            position_names(&[0, -1920, 3840]),
            words(&["Centre", "Left", "Right"])
        );
        assert_eq!(
            position_names(&[1920, -1920, 0]),
            words(&["Right", "Left", "Centre"])
        );
    }

    #[test]
    fn four_or_more_monitors_are_numbered_from_the_left() {
        assert_eq!(
            position_names(&[3840, 0, -1920, 1920]),
            words(&["Monitor 4", "Monitor 2", "Monitor 1", "Monitor 3"])
        );
    }

    #[test]
    fn a_device_name_ends_at_its_nul() {
        let mut buffer = [0u16; 32];
        for (slot, unit) in buffer.iter_mut().zip(r"\\.\DISPLAY1".encode_utf16()) {
            *slot = unit;
        }
        assert_eq!(from_wide(&buffer), r"\\.\DISPLAY1");
    }

    /// The monitors of the PC this was built on: left 1920 x 1080 at 100 %,
    /// primary 3840 x 2160 at 150 %, right 2560 x 2880 at 175 %, each with
    /// its taskbar at the bottom (48, 72 and 84 physical pixels high).
    const LEFT: Rect = [-1920, 0, 0, 1032];
    const PRIMARY: Rect = [0, 0, 3840, 2088];
    const RIGHT: Rect = [3840, 0, 6400, 2796];

    #[test]
    fn a_window_keeps_its_place_and_size_relative_to_the_screen() {
        // Left half of the primary monitor, invisible borders and all.
        let half = [-7, 0, 1927, 2095];
        assert_eq!(map_rect(half, PRIMARY, LEFT), [-1924, 0, -956, 1035]);
        // A quarter of the left monitor in its middle, to the right one.
        let quarter = [-1440, 258, -480, 774];
        assert_eq!(map_rect(quarter, LEFT, RIGHT), [4480, 699, 5760, 2097]);
        // And back again lands where it started, give or take rounding.
        let back = map_rect(map_rect(quarter, LEFT, RIGHT), RIGHT, LEFT);
        for (a, b) in back.iter().zip(quarter) {
            assert!((a - b).abs() <= 1, "{back:?}");
        }
    }

    #[test]
    fn a_maximized_or_minimized_window_maps_its_normal_rectangle() {
        // The normal rectangle of a window maximized on the primary
        // monitor, the part of it that matters for the move.
        let normal = [400, 300, 2400, 1500];
        assert_eq!(map_rect(normal, PRIMARY, RIGHT), [4107, 402, 5440, 2009]);
        // A window off the edge stays off it by the same share.
        assert_eq!(
            map_rect([-2020, 0, -960, 520], LEFT, PRIMARY),
            [-200, 0, 1920, 1052]
        );
        // A monitor with no size is treated as a plain shift.
        assert_eq!(
            map_rect([10, 10, 20, 20], [0, 0, 0, 0], [100, 100, 900, 900]),
            [110, 110, 120, 120]
        );
    }

    #[test]
    fn a_normal_position_is_mapped_in_screen_terms_and_given_back_in_workspace_terms() {
        // Taskbar at the bottom: workspace and screen coordinates agree.
        let normal = [400, 300, 2400, 1500];
        assert_eq!(
            normal_on(normal, PRIMARY, RIGHT, PRIMARY),
            map_rect(normal, PRIMARY, RIGHT)
        );
        // Taskbar on the left of the primary monitor, 100 px wide: its work
        // area starts at x 100, and so does workspace x 0.
        let primary = [100, 0, 3840, 2160];
        let mapped = normal_on([0, 0, 1870, 1080], primary, LEFT, primary);
        let on_screen = map_rect([100, 0, 1970, 1080], primary, LEFT);
        assert_eq!(
            mapped,
            [
                on_screen[0] - 100,
                on_screen[1],
                on_screen[2] - 100,
                on_screen[3]
            ]
        );
        assert_eq!(on_screen, [-1920, 0, -960, 516]);
    }

    #[test]
    fn the_next_monitor_goes_right_and_round() {
        assert_eq!(next_screen(0, 3), 1);
        assert_eq!(next_screen(2, 3), 0);
        assert_eq!(next_screen(0, 1), 0);
        assert_eq!(next_screen(0, 0), 0);
    }
}

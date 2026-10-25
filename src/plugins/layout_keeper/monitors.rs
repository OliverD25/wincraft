//! Which monitor each window is on, recorded with every snapshot so the
//! strip can group windows by monitor. Three names for one monitor: the GDI
//! device name Windows uses internally ("\\.\DISPLAY1"), the name a person
//! knows it by ("DELL U2720Q"), and its place from the left. Neither the
//! device name nor the place is sure to survive a reboot or a monitor being
//! unplugged, so a saved monitor is found again by all three (`find`).

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromRect, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, GetWindowRect, IsIconic, IsZoomed, PostMessageW, SetWindowPlacement,
    SetWindowPos, MONITORINFOF_PRIMARY, SC_MAXIMIZE, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
    SWP_NOZORDER, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, WINDOWPLACEMENT, WM_SYSCOMMAND,
};

use super::identity::{self, Rect};
use super::state::{SavedMonitor, SavedWindow};
use crate::core::monitors::{friendly_names, from_wide, position_names};
use crate::core::ui_bridge::ArrangeMonitor;

pub struct Monitor {
    handle: HMONITOR,
    saved: SavedMonitor,
    rect: Rect,
    work: Rect,
    primary: bool,
}

struct Found {
    handle: HMONITOR,
    device: String,
    rect: RECT,
    work: RECT,
    primary: bool,
}

/// Every active monitor, numbered from the left.
pub fn list() -> Vec<Monitor> {
    let mut found: Vec<Found> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut found as *mut Vec<Found> as LPARAM,
        )
    };
    let friendly = friendly_names();
    let order = left_to_right(
        &found
            .iter()
            .map(|monitor| (monitor.rect.left, monitor.rect.top))
            .collect::<Vec<_>>(),
    );
    found
        .into_iter()
        .zip(order)
        .map(|(monitor, position)| {
            let name = friendly
                .iter()
                .find(|(gdi, _)| *gdi == monitor.device)
                .map(|(_, name)| name.clone())
                .unwrap_or_default();
            Monitor {
                handle: monitor.handle,
                saved: SavedMonitor {
                    device: monitor.device,
                    name,
                    position,
                },
                rect: corners(monitor.rect),
                work: corners(monitor.work),
                primary: monitor.primary,
            }
        })
        .collect()
}

fn corners(rect: RECT) -> Rect {
    [rect.left, rect.top, rect.right, rect.bottom]
}

impl Monitor {
    pub fn saved(&self) -> &SavedMonitor {
        &self.saved
    }

    pub fn is_primary(&self) -> bool {
        self.primary
    }
}

/// Every monitor as the saved layout names it, in the order of `list`.
pub fn saved_names(monitors: &[Monitor]) -> Vec<SavedMonitor> {
    monitors
        .iter()
        .map(|monitor| monitor.saved.clone())
        .collect()
}

/// The connected monitor that a saved one is, or None when it is not
/// connected. The friendly name decides; a monitor without one (many laptop
/// panels, or when Windows cannot say) is matched by its device name. Two
/// monitors of the same model are told apart by device name, then by place,
/// and left unmatched when neither settles it.
pub fn find(saved: &SavedMonitor, live: &[SavedMonitor]) -> Option<usize> {
    let candidates: Vec<usize> = (0..live.len())
        .filter(|index| {
            let monitor = &live[*index];
            monitor.name == saved.name
                || (monitor.name.is_empty() && monitor.device == saved.device)
        })
        .collect();
    match candidates.as_slice() {
        [] => None,
        [only] => Some(*only),
        several => several
            .iter()
            .find(|index| live[**index].device == saved.device)
            .or_else(|| {
                several
                    .iter()
                    .find(|index| live[**index].position == saved.position)
            })
            .copied(),
    }
}

/// The monitors for the strip: the connected ones in the order of `list`,
/// then `disconnected`, which are saved monitors not connected now.
pub fn for_strip(monitors: &[Monitor], disconnected: &[SavedMonitor]) -> Vec<ArrangeMonitor> {
    let lefts: Vec<i32> = monitors.iter().map(|monitor| monitor.rect[0]).collect();
    let places = position_names(&lefts);
    let mut strip: Vec<ArrangeMonitor> = monitors
        .iter()
        .zip(places)
        .map(|(monitor, place)| ArrangeMonitor {
            device: monitor.saved.device.clone(),
            label: place.unwrap_or_else(|| fallback_label(&monitor.saved)),
            name: monitor.saved.name.clone(),
            rect: monitor.rect,
            work: monitor.work,
            primary: monitor.primary,
            connected: true,
        })
        .collect();
    strip.extend(disconnected.iter().map(|saved| ArrangeMonitor {
        device: saved.device.clone(),
        label: fallback_label(saved),
        name: saved.name.clone(),
        rect: [0; 4],
        work: [0; 4],
        primary: false,
        connected: false,
    }));
    strip
}

/// A monitor's name when its place gives none: alone, or no longer there.
fn fallback_label(saved: &SavedMonitor) -> String {
    if saved.name.is_empty() {
        format!("Monitor {}", saved.position + 1)
    } else {
        saved.name.clone()
    }
}

unsafe extern "system" fn collect(
    monitor: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> i32 {
    let mut info: MONITORINFOEXW = unsafe { std::mem::zeroed() };
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if unsafe {
        GetMonitorInfoW(
            monitor,
            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
        )
    } != 0
    {
        let found = unsafe { &mut *(data as *mut Vec<Found>) };
        found.push(Found {
            handle: monitor,
            device: from_wide(&info.szDevice),
            rect: info.monitorInfo.rcMonitor,
            work: info.monitorInfo.rcWork,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    1
}

/// What the Arrange strip says about a window's monitor: its place word,
/// or nothing when the window is on the primary monitor.
pub fn strip_label(rect: Rect, monitors: &[Monitor]) -> Option<String> {
    let index = index_of(rect, monitors)?;
    if monitors[index].primary {
        return None;
    }
    let lefts: Vec<i32> = monitors.iter().map(|monitor| monitor.rect[0]).collect();
    position_names(&lefts).swap_remove(index)
}

/// The place in `monitors` of the monitor a rectangle is mostly on.
pub fn index_of(rect: Rect, monitors: &[Monitor]) -> Option<usize> {
    let rect = RECT {
        left: rect[0],
        top: rect[1],
        right: rect[2],
        bottom: rect[3],
    };
    let handle = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    monitors.iter().position(|monitor| monitor.handle == handle)
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
fn normal_on(normal: Rect, from: Rect, to: Rect, primary: Rect) -> Rect {
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
pub fn move_window(
    hwnd: HWND,
    from: &Monitor,
    to: &Monitor,
    monitors: &[Monitor],
) -> Result<(), String> {
    let failed = |what: &str| format!("{what} failed (error {})", unsafe { GetLastError() });
    let mut place: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    if unsafe { GetWindowPlacement(hwnd, &mut place) } == 0 {
        return Err(failed("GetWindowPlacement"));
    }
    let primary = monitors
        .iter()
        .find(|monitor| monitor.primary)
        .map_or([0, 0, 0, 0], |primary| primary.work);
    let [left, top, right, bottom] =
        normal_on(corners(place.rcNormalPosition), from.work, to.work, primary);
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
    let [left, top, right, bottom] = map_rect(corners(now), from.work, to.work);
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

/// What a restore does about a window's monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Return {
    /// It is on its saved monitor, or none was saved.
    Stay,
    /// Its saved monitor is not connected: it stays where it is and keeps
    /// the saved one, rather than being piled onto the primary monitor.
    Unplugged,
    /// Back to the connected monitor at this place in the list.
    To(usize),
}

/// Where a restore puts a window saved on `saved` that is on the monitor at
/// place `now` in the list, from `live`, the list's saved names.
pub fn restore_target(
    saved: Option<&SavedMonitor>,
    now: Option<usize>,
    live: &[SavedMonitor],
) -> Return {
    let Some(saved) = saved else {
        return Return::Stay;
    };
    match find(saved, live) {
        None => Return::Unplugged,
        Some(target) if now.is_some_and(|now| now != target) => Return::To(target),
        Some(_) => Return::Stay,
    }
}

/// A fresh look at one program's windows keeps the monitor of a window
/// saved on one that is unplugged now, so a restore once it is plugged in
/// again still knows where the window goes. Windows are paired with the
/// previous save the way a restore pairs them. `released[i]` is true for a
/// fresh window the user has since put on a monitor in the strip.
pub fn keep_unplugged(
    exe: &str,
    previous: &[SavedWindow],
    fresh: &mut [SavedWindow],
    released: &[bool],
    live: &[SavedMonitor],
) {
    let before: Vec<identity::WindowIdentity> =
        previous.iter().map(|window| window.identity(exe)).collect();
    let now: Vec<identity::WindowIdentity> =
        fresh.iter().map(|window| window.identity(exe)).collect();
    for (s, l) in identity::match_windows(&before, &now).pairs {
        let Some(monitor) = &previous[s].monitor else {
            continue;
        };
        if find(monitor, live).is_none() && !released.get(l).copied().unwrap_or(false) {
            fresh[l].monitor = Some(monitor.clone());
        }
    }
}

/// Each monitor's place from the left, 0 first; monitors stacked above one
/// another are counted from the top.
fn left_to_right(corners: &[(i32, i32)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..corners.len()).collect();
    order.sort_by_key(|index| corners[*index]);
    let mut position = vec![0; corners.len()];
    for (place, index) in order.into_iter().enumerate() {
        position[index] = place;
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitors_are_numbered_from_the_left() {
        // Primary in the middle at 0, one to the left, one to the right.
        assert_eq!(left_to_right(&[(0, 0), (-1920, 0), (2560, 200)]), [1, 0, 2]);
        // Two stacked at the same left edge: the upper one first.
        assert_eq!(left_to_right(&[(0, 1080), (0, 0)]), [1, 0]);
        assert!(left_to_right(&[]).is_empty());
    }

    fn saved(device: &str, name: &str, position: usize) -> SavedMonitor {
        SavedMonitor {
            device: format!(r"\\.\{device}"),
            name: name.to_string(),
            position,
        }
    }

    #[test]
    fn a_saved_monitor_is_found_by_its_name_whatever_its_device_is_now() {
        let live = [
            saved("DISPLAY2", "LG HDR 4K", 1),
            saved("DISPLAY1", "DELL P2419H", 0),
            saved("DISPLAY3", "LG SDQHD", 2),
        ];
        // After a reboot the right monitor came up as DISPLAY3, not 2.
        assert_eq!(find(&saved("DISPLAY2", "LG SDQHD", 2), &live), Some(2));
        assert_eq!(find(&saved("DISPLAY9", "DELL P2419H", 0), &live), Some(1));
        // Unplugged: nothing else has its name, so it is not connected.
        assert_eq!(find(&saved("DISPLAY4", "Samsung", 3), &live), None);
    }

    #[test]
    fn two_monitors_of_one_model_are_told_apart_by_device_then_place() {
        let live = [saved("DISPLAY1", "DELL", 0), saved("DISPLAY2", "DELL", 1)];
        assert_eq!(find(&saved("DISPLAY2", "DELL", 0), &live), Some(1));
        assert_eq!(find(&saved("DISPLAY7", "DELL", 1), &live), Some(1));
        assert_eq!(find(&saved("DISPLAY7", "DELL", 5), &live), None);
    }

    #[test]
    fn a_monitor_without_a_name_is_found_by_its_device() {
        // A laptop panel, or Windows could not read the names this time.
        let live = [saved("DISPLAY1", "", 0), saved("DISPLAY2", "", 1)];
        assert_eq!(find(&saved("DISPLAY2", "LG SDQHD", 1), &live), Some(1));
        assert_eq!(find(&saved("DISPLAY1", "", 0), &live), Some(0));
        assert_eq!(find(&saved("DISPLAY3", "LG SDQHD", 2), &live), None);
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
    fn a_restore_moves_a_window_only_to_a_connected_saved_monitor() {
        let live = [
            saved("DISPLAY1", "LG HDR 4K", 1),
            saved("DISPLAY2", "DELL", 0),
        ];
        let dell = saved("DISPLAY2", "DELL", 0);
        assert_eq!(restore_target(Some(&dell), Some(0), &live), Return::To(1));
        assert_eq!(restore_target(Some(&dell), Some(1), &live), Return::Stay);
        assert_eq!(restore_target(None, Some(0), &live), Return::Stay);
        // Unplugged: left where it is, not piled onto the primary monitor.
        let gone = saved("DISPLAY3", "LG SDQHD", 2);
        assert_eq!(
            restore_target(Some(&gone), Some(0), &live),
            Return::Unplugged
        );
    }

    fn window(title: &str, monitor: Option<SavedMonitor>) -> SavedWindow {
        SavedWindow {
            name: Some(title.to_string()),
            title: title.to_string(),
            rect: [0, 0, 1200, 800],
            maximized: false,
            desktop: None,
            desktop_name: None,
            group: "Chrome".to_string(),
            taskbar_index: 0,
            z_index: 0,
            monitor,
            seen: None,
        }
    }

    #[test]
    fn a_save_keeps_the_monitor_of_a_window_whose_monitor_is_unplugged() {
        let live = [saved("DISPLAY1", "LG HDR 4K", 0)];
        let primary = Some(live[0].clone());
        let gone = Some(saved("DISPLAY3", "LG SDQHD", 1));
        let previous = [
            window("Mail", gone.clone()),
            window("Docs", primary.clone()),
        ];
        // Both are on the primary monitor now; Docs was there already.
        let mut fresh = [
            window("Docs", primary.clone()),
            window("Mail", primary.clone()),
        ];
        keep_unplugged("chrome.exe", &previous, &mut fresh, &[false, false], &live);
        assert_eq!(fresh[0].monitor, primary);
        assert_eq!(fresh[1].monitor, gone);

        // Put on the primary monitor in the strip: that is its place now.
        let mut fresh = [
            window("Docs", primary.clone()),
            window("Mail", primary.clone()),
        ];
        keep_unplugged("chrome.exe", &previous, &mut fresh, &[false, true], &live);
        assert_eq!(fresh[1].monitor, primary);

        // Plugged in again, nothing is kept: the window is wherever it is.
        let both = [live[0].clone(), saved("DISPLAY3", "LG SDQHD", 1)];
        let mut fresh = [window("Mail", primary.clone())];
        keep_unplugged("chrome.exe", &previous, &mut fresh, &[false], &both);
        assert_eq!(fresh[0].monitor, primary);
    }
}

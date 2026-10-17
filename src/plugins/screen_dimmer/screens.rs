//! The connected monitors, each with a name that stays the same from one
//! start to the next, for the settings and the saved state. The HMONITOR
//! value changes whenever Windows rebuilds its monitor list.

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

use crate::core::monitors::{friendly_names, from_wide, position_names};

#[derive(Clone)]
pub struct Screen {
    pub handle: HMONITOR,
    pub rect: RECT,
    pub primary: bool,
    /// The monitor's own name from its EDID, such as "PHL 323E7"; empty
    /// when Windows cannot say.
    pub name: String,
    /// "left", "centre", "right"; None for a single monitor.
    pub place: Option<String>,
    pub id: String,
}

/// Every monitor in the order Windows lists them, which is the order of the
/// toggle hotkeys.
pub fn list() -> Vec<Screen> {
    let mut found: Vec<(HMONITOR, RECT, bool, String)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut found as *mut Vec<(HMONITOR, RECT, bool, String)> as LPARAM,
        )
    };
    let friendly = friendly_names();
    let names: Vec<String> = found
        .iter()
        .map(|(_, _, _, device)| {
            friendly
                .iter()
                .find(|(gdi, _)| gdi == device)
                .map(|(_, name)| name.clone())
                .unwrap_or_default()
        })
        .collect();
    let corners: Vec<(i32, i32)> = found
        .iter()
        .map(|(_, rect, _, _)| (rect.left, rect.top))
        .collect();
    let lefts: Vec<i32> = corners.iter().map(|(left, _)| *left).collect();
    let places = position_names(&lefts);
    let ids = identities(&names, &corners);
    found
        .into_iter()
        .zip(names)
        .zip(places)
        .zip(ids)
        .map(|((((handle, rect, primary, _), name), place), id)| Screen {
            handle,
            rect,
            primary,
            name,
            place: place.map(|place| place.to_lowercase()),
            id,
        })
        .collect()
}

/// Each monitor's id: its EDID name when no other monitor has the same one,
/// otherwise the name and its number counted from the left ("Monitor 2"
/// when it has no name).
pub fn identities(names: &[String], corners: &[(i32, i32)]) -> Vec<String> {
    let mut order: Vec<usize> = (0..corners.len()).collect();
    order.sort_by_key(|index| corners[*index]);
    let mut number = vec![0; corners.len()];
    for (rank, index) in order.into_iter().enumerate() {
        number[index] = rank + 1;
    }
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let unique = names.iter().filter(|other| *other == name).count() == 1;
            match (name.trim().is_empty(), unique) {
                (true, _) => format!("Monitor {}", number[index]),
                (false, true) => name.trim().to_string(),
                (false, false) => format!("{} {}", name.trim(), number[index]),
            }
        })
        .collect()
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
        let found = unsafe { &mut *(data as *mut Vec<(HMONITOR, RECT, bool, String)>) };
        found.push((
            monitor,
            info.monitorInfo.rcMonitor,
            info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            from_wide(&info.szDevice),
        ));
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn the_users_three_monitors_are_named_by_their_edid() {
        let ids = identities(
            &names(&["AORUS FV43U", "PHL 323E7", "MQ780"]),
            &[(0, 0), (-1920, 0), (3840, 0)],
        );
        assert_eq!(ids, names(&["AORUS FV43U", "PHL 323E7", "MQ780"]));
    }

    #[test]
    fn two_of_one_model_and_a_nameless_one_are_numbered_from_the_left() {
        let ids = identities(
            &names(&["DELL P2419H", "", "DELL P2419H"]),
            &[(1920, 0), (-1920, 0), (0, 0)],
        );
        assert_eq!(ids, names(&["DELL P2419H 3", "Monitor 1", "DELL P2419H 2"]));
    }
}

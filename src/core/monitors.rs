use windows_sys::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;

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
}

//! Whether WinCraft runs inside another app's Windows package. A program
//! started from a packaged app, such as Claude Desktop, has no package
//! identity of its own but inherits the package's file and registry
//! redirection: its settings, log and autostart value land in the package's
//! private copy, where Explorer and the next sign-in never look.

use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

/// The name of the package this process runs in or under, if any. The file
/// at `probe` must be one this process wrote through `%LOCALAPPDATA%`; where
/// Windows really put it says whether the write was redirected.
pub fn current(probe: &Path) -> Option<String> {
    own_package().or_else(|| {
        let file = File::open(probe).ok()?;
        let mut buffer = [0u16; 1024];
        let len = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle() as HANDLE,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        } as usize;
        (len > 0 && len < buffer.len())
            .then(|| package_in_path(&String::from_utf16_lossy(&buffer[..len])))
            .flatten()
    })
}

fn own_package() -> Option<String> {
    let mut len = 0u32;
    unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
    if len == 0 {
        return None;
    }
    let mut buffer = vec![0u16; len as usize];
    let status = unsafe { GetCurrentPackageFullName(&mut len, buffer.as_mut_ptr()) };
    if status != ERROR_SUCCESS {
        return None;
    }
    let full = String::from_utf16_lossy(&buffer[..len.saturating_sub(1) as usize]);
    Some(package_name(&full))
}

/// "Claude_2.9939.4.0_x64__pzs8sxrjxfjjc" is called Claude.
fn package_name(full_or_family: &str) -> String {
    full_or_family
        .split('_')
        .next()
        .unwrap_or(full_or_family)
        .to_string()
}

/// The package a redirected file belongs to, from its real path:
/// `...\Packages\<family>\LocalCache\...`.
fn package_in_path(path: &str) -> Option<String> {
    let mut parts = path.split(std::path::MAIN_SEPARATOR);
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("Packages") {
            let family = parts.next()?;
            let cache = parts.next()?;
            if cache.eq_ignore_ascii_case("LocalCache") && !family.is_empty() {
                return Some(package_name(family));
            }
        }
    }
    None
}

/// The balloon text for the user.
pub fn warning(package: &str) -> String {
    format!(
        "WinCraft runs inside {package}. Its settings and autostart are hidden from Windows. \
         Start it from the Start menu or Task Scheduler instead."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_in_a_packages_local_cache_belongs_to_that_package() {
        let path = r"\?\D:\WpSystem\S-1-5-21-1\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\WinCraft\wincraft.log";
        assert_eq!(package_in_path(path).as_deref(), Some("Claude"));
        assert_eq!(
            package_in_path(r"c:\x\packages\Foo.Bar_abc\localcache\Local\a.log").as_deref(),
            Some("Foo.Bar")
        );
    }

    #[test]
    fn a_normal_profile_path_or_another_packages_folder_is_not_redirection() {
        assert_eq!(
            package_in_path(r"\?\C:\Users\Ann\AppData\Local\WinCraft\wincraft.log"),
            None
        );
        assert_eq!(
            package_in_path(r"C:\Users\Ann\AppData\Local\Packages\Claude_x\Settings\a.dat"),
            None
        );
        assert_eq!(
            package_in_path(r"C:\Users\Ann\AppData\Local\Packages"),
            None
        );
        assert_eq!(package_in_path(""), None);
    }

    #[test]
    fn a_package_full_name_is_shortened_to_its_name() {
        assert_eq!(
            package_name("Claude_2.9939.4.0_x64__pzs8sxrjxfjjc"),
            "Claude"
        );
        assert_eq!(package_name("Plain"), "Plain");
    }

    #[test]
    fn the_warning_names_the_package_and_the_way_out() {
        let text = warning("Claude");
        assert!(text.starts_with(
            "WinCraft runs inside Claude. Its settings and autostart are hidden from Windows."
        ));
        assert!(text.ends_with("Start it from the Start menu or Task Scheduler instead."));
    }
}

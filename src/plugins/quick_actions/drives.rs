//! Removable drives (USB sticks and memory cards: GetDriveTypeW says
//! DRIVE_REMOVABLE) and ejecting one safely: lock the volume, so no open
//! file can be cut off, dismount it, allow removal, then eject. When the
//! lock fails, a program still uses the drive and nothing is done.

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA,
    IOCTL_STORAGE_MEDIA_REMOVAL, PREVENT_MEDIA_REMOVAL,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::core::wide;

const DRIVE_REMOVABLE: u32 = 2;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// "E".
    pub letter: char,
    /// The volume's name, such as "SANDISK"; may be empty.
    pub label: String,
    pub bytes: u64,
}

impl Drive {
    pub fn title(&self) -> String {
        if self.label.is_empty() {
            format!("{}: USB drive", self.letter)
        } else {
            format!("{}: {}", self.letter, self.label)
        }
    }
}

/// Why an eject did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EjectError {
    /// A program has a file open on it.
    InUse,
    Other(String),
}

/// The letters of a GetLogicalDrives mask, from A.
pub fn letters(mask: u32) -> Vec<char> {
    (0..26u8)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| char::from(b'A' + bit))
        .collect()
}

pub fn read() -> Vec<Drive> {
    let mut found = Vec::new();
    for letter in letters(unsafe { GetLogicalDrives() }) {
        let root = wide(&format!("{letter}:\\"));
        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_REMOVABLE {
            continue;
        }
        let mut name = [0u16; 261];
        // A card reader with no card in it has no volume: leave it out.
        let has_volume = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                name.as_mut_ptr(),
                name.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        } != 0;
        if !has_volume {
            continue;
        }
        let mut total = 0u64;
        unsafe {
            GetDiskFreeSpaceExW(
                root.as_ptr(),
                std::ptr::null_mut(),
                &mut total,
                std::ptr::null_mut(),
            )
        };
        let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        found.push(Drive {
            letter,
            label: String::from_utf16_lossy(&name[..end]),
            bytes: total,
        });
    }
    found
}

fn control(volume: windows_sys::Win32::Foundation::HANDLE, code: u32, input: &[u8]) -> bool {
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            volume,
            code,
            if input.is_empty() {
                std::ptr::null()
            } else {
                input.as_ptr().cast()
            },
            input.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    ok != 0
}

pub fn eject(letter: char) -> Result<(), EjectError> {
    let path = wide(&format!("\\\\.\\{letter}:"));
    let volume = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if volume == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        return Err(EjectError::Other(format!(
            "Windows could not open the drive (error {error})"
        )));
    }
    let result = (|| {
        // The lock fails while any program has a file open on the drive.
        let mut locked = false;
        for _ in 0..5 {
            if control(volume, FSCTL_LOCK_VOLUME, &[]) {
                locked = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !locked {
            return Err(EjectError::InUse);
        }
        if !control(volume, FSCTL_DISMOUNT_VOLUME, &[]) {
            return Err(EjectError::InUse);
        }
        let allow = PREVENT_MEDIA_REMOVAL {
            PreventMediaRemoval: false,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&allow as *const PREVENT_MEDIA_REMOVAL).cast::<u8>(),
                std::mem::size_of::<PREVENT_MEDIA_REMOVAL>(),
            )
        };
        control(volume, IOCTL_STORAGE_MEDIA_REMOVAL, bytes);
        if control(volume, IOCTL_STORAGE_EJECT_MEDIA, &[]) {
            Ok(())
        } else {
            let error = unsafe { GetLastError() };
            Err(EjectError::Other(format!(
                "Windows could not eject it (error {error})"
            )))
        }
    })();
    unsafe { CloseHandle(volume) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mask_gives_the_drive_letters() {
        assert_eq!(letters(0b1_0101), ['A', 'C', 'E']);
        assert_eq!(letters(1 << 25), ['Z']);
        assert!(letters(0).is_empty());
    }

    #[test]
    fn a_drive_is_named_by_letter_and_label() {
        let mut drive = Drive {
            letter: 'E',
            label: "SANDISK".to_string(),
            bytes: 0,
        };
        assert_eq!(drive.title(), "E: SANDISK");
        drive.label.clear();
        assert_eq!(drive.title(), "E: USB drive");
    }
}

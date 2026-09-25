use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{ERROR_CONNECTION_UNAVAIL, NO_ERROR};
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use super::paths::size_text;
use crate::core::wide;

/// GetDriveTypeW's answers. They live in the windows-sys feature
/// Win32_System_WindowsProgramming, which nothing else here needs.
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;
const DRIVE_RAMDISK: u32 = 6;

#[link(name = "mpr")]
extern "system" {
    fn WNetGetConnectionW(local: *const u16, remote: *mut u16, length: *mut u32) -> u32;
}

/// Reading again at most this often, unless a drive comes or goes.
const FRESH_FOR: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Fixed,
    Removable,
    Cd,
    Network,
    RamDisk,
    Other,
}

/// What Windows said about one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub kind: Kind,
    pub label: String,
    /// Free and total bytes, when the drive answered.
    pub space: Option<(u64, u64)>,
    /// For a network drive: the share it maps, and whether it is reachable.
    pub share: Option<String>,
    pub connected: bool,
}

/// The drive row's second line: "Windows · 631 GB free of 1.8 TB". A drive
/// without a label gets the name Explorer gives it.
pub fn subtitle(volume: &Volume) -> String {
    let space = volume
        .space
        .filter(|(_, total)| *total > 0)
        .map(|(free, total)| format!("{} free of {}", size_text(free), size_text(total)));
    let mut parts: Vec<String> = Vec::new();
    if volume.kind == Kind::Network {
        parts.push("Network".to_string());
        if !volume.connected {
            parts.push("disconnected".to_string());
            return parts.join(" \u{b7} ");
        }
        parts.extend(volume.share.clone());
    } else if volume.label.trim().is_empty() {
        parts.push(
            match volume.kind {
                Kind::Fixed => "Local Disk",
                Kind::Removable => "Removable Disk",
                Kind::Cd => "CD Drive",
                Kind::RamDisk => "RAM Disk",
                Kind::Network | Kind::Other => "Drive",
            }
            .to_string(),
        );
    } else {
        parts.push(volume.label.trim().to_string());
    }
    parts.extend(space);
    parts.join(" \u{b7} ")
}

#[derive(Default)]
struct State {
    /// Drive letter → finished subtitle.
    subtitles: HashMap<char, String>,
    /// Letters whose read has not come back yet. A read that hangs on a dead
    /// share stays here, so it is never started twice.
    reading: HashSet<char>,
    read_at: Option<Instant>,
    mask: u32,
    news: bool,
}

/// Labels and free space, read off the UI thread. Asking a sleeping disk or
/// a dead network share can take seconds, so the list shows at once and each
/// drive's second line appears when its answer arrives.
#[derive(Default)]
pub struct Drives {
    state: Arc<Mutex<State>>,
}

impl Drives {
    /// The drive letters present now, with whatever subtitle is known. Starts
    /// a background read when the list is new, changed or 30 s old.
    pub fn list(&self) -> Vec<(char, String)> {
        let mask = unsafe { GetLogicalDrives() };
        let letters: Vec<char> = (0..26u8)
            .filter(|bit| mask & (1 << bit) != 0)
            .map(|bit| (b'A' + bit) as char)
            .collect();
        let Ok(mut state) = self.state.lock() else {
            return letters
                .into_iter()
                .map(|letter| (letter, String::new()))
                .collect();
        };
        let stale = state
            .read_at
            .is_none_or(|at| at.elapsed() >= FRESH_FOR || state.mask != mask);
        if stale {
            state.read_at = Some(Instant::now());
            state.mask = mask;
            for &letter in &letters {
                if state.reading.insert(letter) {
                    self.read_in_background(letter);
                }
            }
        }
        letters
            .into_iter()
            .map(|letter| {
                let known = state.subtitles.get(&letter).cloned().unwrap_or_default();
                (letter, known)
            })
            .collect()
    }

    /// Some drive's answer is still out.
    pub fn waiting(&self) -> bool {
        self.state
            .lock()
            .map(|state| !state.reading.is_empty())
            .unwrap_or(false)
    }

    /// An answer arrived since the last call.
    pub fn take_news(&self) -> bool {
        self.state
            .lock()
            .map(|mut state| std::mem::take(&mut state.news))
            .unwrap_or(false)
    }

    fn read_in_background(&self, letter: char) {
        let state = Arc::clone(&self.state);
        let started = std::thread::Builder::new()
            .name(format!("wincraft-drive-{letter}"))
            .spawn(move || {
                let text = subtitle(&read_volume(letter));
                if let Ok(mut state) = state.lock() {
                    state.subtitles.insert(letter, text);
                    state.reading.remove(&letter);
                    state.news = true;
                }
            });
        if started.is_err() {
            if let Ok(mut state) = self.state.lock() {
                state.reading.remove(&letter);
            }
        }
    }
}

fn read_volume(letter: char) -> Volume {
    let root = wide(&format!("{letter}:\\"));
    let kind = match unsafe { GetDriveTypeW(root.as_ptr()) } {
        DRIVE_FIXED => Kind::Fixed,
        DRIVE_REMOVABLE => Kind::Removable,
        DRIVE_CDROM => Kind::Cd,
        DRIVE_REMOTE => Kind::Network,
        DRIVE_RAMDISK => Kind::RamDisk,
        _ => Kind::Other,
    };
    let (share, connected) = if kind == Kind::Network {
        network_share(letter)
    } else {
        (None, true)
    };
    if !connected {
        return Volume {
            kind,
            label: String::new(),
            space: None,
            share,
            connected,
        };
    }

    let mut name = [0u16; 261];
    let named = unsafe {
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
    let label = if named {
        let len = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        String::from_utf16_lossy(&name[..len])
    } else {
        String::new()
    };

    let (mut free, mut total) = (0u64, 0u64);
    let measured =
        unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) }
            != 0;
    Volume {
        kind,
        label,
        space: measured.then_some((free, total)),
        share,
        connected,
    }
}

/// The share a network letter maps, like \\server\share. Windows still names
/// the share of a mapping that cannot be reached, and says so in the result.
fn network_share(letter: char) -> (Option<String>, bool) {
    let local = wide(&format!("{letter}:"));
    let mut remote = [0u16; 1024];
    let mut length = remote.len() as u32;
    let result = unsafe { WNetGetConnectionW(local.as_ptr(), remote.as_mut_ptr(), &mut length) };
    let len = remote.iter().position(|c| *c == 0).unwrap_or(0);
    let share = (len > 0).then(|| String::from_utf16_lossy(&remote[..len]));
    match result {
        NO_ERROR => (share, true),
        ERROR_CONNECTION_UNAVAIL => (share, false),
        // Not a mapped letter at all, for example a SUBST of a network path:
        // its space can still be read.
        _ => (None, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn volume(kind: Kind, label: &str, space: Option<(u64, u64)>) -> Volume {
        Volume {
            kind,
            label: label.to_string(),
            space,
            share: None,
            connected: true,
        }
    }

    #[test]
    fn a_labelled_drive_shows_its_label_and_space() {
        let windows = volume(Kind::Fixed, "Windows", Some((631 * GB, 1843 * GB)));
        assert_eq!(subtitle(&windows), "Windows \u{b7} 631 GB free of 1.8 TB");
    }

    #[test]
    fn a_drive_without_a_label_gets_explorers_name_for_its_kind() {
        let usb = volume(Kind::Removable, "  ", Some((7 * GB + GB / 2, 15 * GB)));
        assert_eq!(subtitle(&usb), "Removable Disk \u{b7} 7.5 GB free of 15 GB");
        assert_eq!(subtitle(&volume(Kind::Fixed, "", None)), "Local Disk");
        assert_eq!(subtitle(&volume(Kind::Cd, "", None)), "CD Drive");
        assert_eq!(subtitle(&volume(Kind::RamDisk, "", None)), "RAM Disk");
    }

    #[test]
    fn a_network_drive_names_its_share_or_says_it_is_disconnected() {
        let mut share = volume(Kind::Network, "Data", Some((GB, 4 * GB)));
        share.share = Some(r"\\nas\media".to_string());
        assert_eq!(
            subtitle(&share),
            "Network \u{b7} \\\\nas\\media \u{b7} 1.0 GB free of 4.0 GB"
        );
        share.connected = false;
        assert_eq!(subtitle(&share), "Network \u{b7} disconnected");
    }

    #[test]
    fn a_drive_reporting_no_size_shows_no_space() {
        let empty = volume(Kind::Removable, "", Some((0, 0)));
        assert_eq!(subtitle(&empty), "Removable Disk");
    }
}

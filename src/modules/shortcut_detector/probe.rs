use std::time::Instant;

use windows_sys::Win32::Foundation::{GetLastError, ERROR_HOTKEY_ALREADY_REGISTERED, HWND};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

use crate::core::hotkeys;
use crate::core::traits::Hotkey;
use crate::modules::shortcut_detector::known;

/// Probe ids live far away from the host's sequential hotkey ids so a scan can
/// never unregister a hotkey a module is relying on.
const PROBE_ID_BASE: i32 = 0x7000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Free,
    TakenByApp,
    Windows,
    WinCraft,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Free => "Free",
            Status::TakenByApp => "Taken by an app",
            Status::Windows => "Windows",
            Status::WinCraft => "WinCraft",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub hotkey: Hotkey,
    pub status: Status,
    pub owner: String,
    pub source: &'static str,
}

pub struct ScanResult {
    pub entries: Vec<Entry>,
    pub probed: usize,
    pub elapsed_ms: u32,
}

impl ScanResult {
    pub fn count(&self, status: Status) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.status == status)
            .count()
    }

    pub fn find(&self, hotkey: Hotkey) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.hotkey == hotkey)
    }

    pub fn status_line(&self) -> String {
        format!(
            "{} combinations probed in {} ms \u{2014} {} taken by apps, {} Windows shortcuts, {} WinCraft. \
             Apps that use keyboard hooks (AutoHotkey, PowerToys) cannot be detected.",
            thousands(self.probed),
            self.elapsed_ms,
            self.count(Status::TakenByApp),
            self.count(Status::Windows),
            self.count(Status::WinCraft),
        )
    }
}

/// The 15 non-empty modifier sets, ordered the way the filter dropdown reads
/// rather than by bit value.
pub fn modifier_sets() -> Vec<u32> {
    vec![
        MOD_WIN,
        MOD_WIN | MOD_ALT,
        MOD_WIN | MOD_CONTROL,
        MOD_WIN | MOD_SHIFT,
        MOD_WIN | MOD_CONTROL | MOD_ALT,
        MOD_WIN | MOD_CONTROL | MOD_SHIFT,
        MOD_WIN | MOD_ALT | MOD_SHIFT,
        MOD_WIN | MOD_CONTROL | MOD_ALT | MOD_SHIFT,
        MOD_CONTROL,
        MOD_CONTROL | MOD_ALT,
        MOD_CONTROL | MOD_SHIFT,
        MOD_CONTROL | MOD_ALT | MOD_SHIFT,
        MOD_ALT,
        MOD_ALT | MOD_SHIFT,
        MOD_SHIFT,
    ]
}

pub fn scan(probe_hwnd: HWND, wincraft: &[(String, String, Hotkey)]) -> ScanResult {
    let start = Instant::now();
    let keys = hotkeys::all_keys();
    let sets = modifier_sets();
    let mut entries = Vec::with_capacity(sets.len() * keys.len());
    let mut next_id = PROBE_ID_BASE;

    for modifiers in sets {
        for vk in &keys {
            let taken = probe_taken(probe_hwnd, next_id, modifiers, *vk);
            next_id += 1;
            let hotkey = Hotkey {
                modifiers: modifiers | MOD_NOREPEAT,
                vk: *vk,
            };
            entries.push(classify(hotkey, taken, owner_in(wincraft, hotkey)));
        }
    }

    let probed = entries.len();
    ScanResult {
        entries,
        probed,
        elapsed_ms: start.elapsed().as_millis() as u32,
    }
}

pub fn verdict(probe_hwnd: HWND, hotkey: Hotkey, wincraft: &[(String, String, Hotkey)]) -> Entry {
    if known::is_reserved(hotkey) {
        let meaning = known::meaning(hotkey).unwrap_or("Windows keeps this one");
        return Entry {
            hotkey,
            status: Status::Windows,
            owner: format!("Reserved by Windows: {meaning}"),
            source: "windows list",
        };
    }
    let taken = probe_taken(
        probe_hwnd,
        PROBE_ID_BASE - 1,
        hotkey.modifiers & !MOD_NOREPEAT,
        hotkey.vk,
    );
    classify(hotkey, taken, owner_in(wincraft, hotkey))
}

pub fn verdict_text(entry: &Entry) -> String {
    match entry.status {
        Status::Free => "Free".to_string(),
        Status::TakenByApp => {
            if entry.owner.is_empty() {
                "Taken by another app".to_string()
            } else {
                format!("Taken by another app {}", entry.owner)
            }
        }
        Status::Windows => {
            if entry.owner.starts_with("Reserved") {
                entry.owner.clone()
            } else {
                format!("Windows shortcut: {}", entry.owner)
            }
        }
        Status::WinCraft => format!(
            "Used by WinCraft: {}",
            entry.owner.replacen(": ", " \u{2014} ", 1)
        ),
    }
}

/// Pure decision table, kept apart from the Win32 probe so it can be tested.
pub fn classify(hotkey: Hotkey, probe_failed: bool, wincraft: Option<(&str, &str)>) -> Entry {
    if let Some((module, label)) = wincraft {
        return Entry {
            hotkey,
            status: Status::WinCraft,
            owner: format!("{module}: {label}"),
            source: "wincraft",
        };
    }
    let meaning = known::meaning(hotkey);
    if probe_failed {
        return Entry {
            hotkey,
            status: Status::TakenByApp,
            owner: meaning
                .map(|m| format!("(Windows: {m})"))
                .unwrap_or_default(),
            source: "probe",
        };
    }
    match meaning {
        Some(meaning) => Entry {
            hotkey,
            status: Status::Windows,
            owner: meaning.to_string(),
            source: "windows list",
        },
        None => Entry {
            hotkey,
            status: Status::Free,
            owner: String::new(),
            source: "probe",
        },
    }
}

fn owner_in<'a>(
    wincraft: &'a [(String, String, Hotkey)],
    hotkey: Hotkey,
) -> Option<(&'a str, &'a str)> {
    wincraft
        .iter()
        .find(|(_, _, mine)| *mine == hotkey)
        .map(|(module, label, _)| (module.as_str(), label.as_str()))
}

fn probe_taken(hwnd: HWND, id: i32, modifiers: u32, vk: u32) -> bool {
    if unsafe { RegisterHotKey(hwnd, id, modifiers, vk) } != 0 {
        unsafe { UnregisterHotKey(hwnd, id) };
        return false;
    }
    let error = unsafe { GetLastError() };
    if error == ERROR_HOTKEY_ALREADY_REGISTERED {
        return true;
    }
    log::debug!(
        "probe of {} failed with error {error}, treating it as free",
        hotkeys::format(Hotkey {
            modifiers: modifiers | MOD_NOREPEAT,
            vk
        })
    );
    false
}

fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Hotkey {
        hotkeys::parse(text).expect(text)
    }

    #[test]
    fn there_are_fifteen_modifier_sets() {
        let sets = modifier_sets();
        assert_eq!(sets.len(), 15);
        for (index, set) in sets.iter().enumerate() {
            assert_ne!(*set, 0);
            assert!(!sets[index + 1..].contains(set), "{set} appears twice");
        }
    }

    #[test]
    fn wincraft_wins_over_everything_else() {
        let entry = classify(
            key("Win+E"),
            true,
            Some(("ScreenDimmer", "Toggle monitor 1")),
        );
        assert_eq!(entry.status, Status::WinCraft);
        assert_eq!(entry.owner, "ScreenDimmer: Toggle monitor 1");
        assert_eq!(
            verdict_text(&entry),
            "Used by WinCraft: ScreenDimmer \u{2014} Toggle monitor 1"
        );
    }

    #[test]
    fn a_failed_probe_beats_the_windows_list_but_keeps_its_meaning() {
        let entry = classify(key("Win+E"), true, None);
        assert_eq!(entry.status, Status::TakenByApp);
        assert_eq!(entry.owner, "(Windows: Open File Explorer)");
        assert_eq!(entry.source, "probe");
    }

    #[test]
    fn the_windows_list_explains_a_combination_the_probe_won() {
        let entry = classify(key("Win+E"), false, None);
        assert_eq!(entry.status, Status::Windows);
        assert_eq!(entry.owner, "Open File Explorer");
        assert_eq!(verdict_text(&entry), "Windows shortcut: Open File Explorer");
    }

    #[test]
    fn an_unknown_combination_that_registers_is_free() {
        let entry = classify(key("Win+Alt+F7"), false, None);
        assert_eq!(entry.status, Status::Free);
        assert!(entry.owner.is_empty());
        assert_eq!(verdict_text(&entry), "Free");
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_575), "1,575");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}

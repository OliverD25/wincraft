//! The "Shortcut key" of .lnk files. Explorer registers the key of every
//! shortcut in the Start menu and on the desktop, so a match here names the
//! owner of a taken combination for certain.
//!
//! The key is two bytes at a fixed place in the file's header (MS-SHLLINK,
//! `HotKey` at offset 0x40): the virtual key, then HOTKEYF_* flags. Reading
//! and writing them directly needs no COM and no new crate.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_UPDATEITEM, SHCNF_PATHW};

use crate::core::traits::Hotkey;
use crate::core::{taskbar, wide};

const HEADER_SIZE: usize = 0x4C;
const HOTKEY_AT: u64 = 0x40;
/// The shell link CLSID 00021401-0000-0000-C000-000000000046 as stored.
const LINK_CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];
const HOTKEYF_SHIFT: u8 = 0x01;
const HOTKEYF_CONTROL: u8 = 0x02;
const HOTKEYF_ALT: u8 = 0x04;
/// Start menu folders nest a few levels; deeper is a program's own data.
const START_MENU_DEPTH: usize = 6;

/// The Start menu Programs folders (user and common), with their sub-folders,
/// and the desktops (user and public), without: Explorer gives keys only to
/// the shortcuts lying on the desktop itself, and a desktop's folders can
/// hold thousands of files.
const FOLDERS: [(&str, usize); 4] = [
    ("{A77F5D77-2E2B-44C3-A6A2-ABA601054A51}", START_MENU_DEPTH),
    ("{0139D44E-6AFE-49F2-8690-3DAFCAE6FFB8}", START_MENU_DEPTH),
    ("{B4BFCC3A-DB2C-424C-B029-7FE99A87C641}", 0),
    ("{C4AA340D-F20F-4863-AFEF-F87EF2E6BA25}", 0),
];

/// One shortcut file with a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkKey {
    pub hotkey: Hotkey,
    /// The file name without ".lnk", as Explorer shows it.
    pub name: String,
    pub path: PathBuf,
}

/// The two header bytes as a hotkey. None when there is no key, or one with
/// no modifier, which the scan never lists.
pub fn decode(raw: u16) -> Option<Hotkey> {
    let [vk, flags] = raw.to_le_bytes();
    if vk == 0 {
        return None;
    }
    let mut modifiers = 0;
    if flags & HOTKEYF_SHIFT != 0 {
        modifiers |= MOD_SHIFT;
    }
    if flags & HOTKEYF_CONTROL != 0 {
        modifiers |= MOD_CONTROL;
    }
    if flags & HOTKEYF_ALT != 0 {
        modifiers |= MOD_ALT;
    }
    (modifiers != 0).then_some(Hotkey {
        modifiers: modifiers | MOD_NOREPEAT,
        vk: u32::from(vk),
    })
}

/// The header bytes for `hotkey`, or why a shortcut file cannot hold it.
pub fn encode(hotkey: Hotkey) -> Result<u16, &'static str> {
    if hotkey.modifiers & MOD_WIN != 0 {
        return Err("A shortcut file cannot use the Win key.");
    }
    let mut flags = 0u8;
    if hotkey.modifiers & MOD_SHIFT != 0 {
        flags |= HOTKEYF_SHIFT;
    }
    if hotkey.modifiers & MOD_CONTROL != 0 {
        flags |= HOTKEYF_CONTROL;
    }
    if hotkey.modifiers & MOD_ALT != 0 {
        flags |= HOTKEYF_ALT;
    }
    if flags == 0 {
        return Err("A shortcut file key needs Ctrl or Alt.");
    }
    let vk = u8::try_from(hotkey.vk).map_err(|_| "That key cannot go into a shortcut file.")?;
    Ok(u16::from_le_bytes([vk, flags]))
}

/// The key bytes of a .lnk header, or None when it is not a shell link.
fn header_key(header: &[u8]) -> Option<u16> {
    if header.len() < HEADER_SIZE
        || header[..4] != (HEADER_SIZE as u32).to_le_bytes()
        || header[4..20] != LINK_CLSID
    {
        return None;
    }
    let at = HOTKEY_AT as usize;
    Some(u16::from_le_bytes([header[at], header[at + 1]]))
}

fn read_key(path: &Path) -> Option<u16> {
    let mut header = [0u8; HEADER_SIZE];
    File::open(path).ok()?.read_exact(&mut header).ok()?;
    header_key(&header)
}

/// Every shortcut with a key under `folders`, each file once. Each folder
/// comes with how many levels of sub-folders to read.
pub fn scan(folders: &[(PathBuf, usize)]) -> Vec<LinkKey> {
    let mut found = Vec::new();
    for (folder, levels) in folders {
        walk(folder, *levels, &mut found);
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);
    found
}

fn walk(folder: &Path, levels: usize, found: &mut Vec<LinkKey>) {
    let Ok(items) = fs::read_dir(folder) else {
        return;
    };
    for item in items.flatten() {
        let path = item.path();
        let Ok(kind) = item.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if levels > 0 {
                walk(&path, levels - 1, found);
            }
            continue;
        }
        let is_link = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
        if !is_link {
            continue;
        }
        if let Some(hotkey) = read_key(&path).and_then(decode) {
            let name = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            found.push(LinkKey { hotkey, name, path });
        }
    }
}

/// The folders whose shortcuts Explorer gives keys to, as they are on this PC.
pub fn folders() -> Vec<(PathBuf, usize)> {
    FOLDERS
        .iter()
        .filter_map(|(guid, levels)| {
            taskbar::known_folder(guid).map(|path| (PathBuf::from(path), *levels))
        })
        .collect()
}

/// Writes a new key into the shortcut file, or takes its key away with
/// `None`, and tells the shell the file changed. Only the two key bytes
/// change.
pub fn write_key(path: &Path, hotkey: Option<Hotkey>) -> Result<(), String> {
    let raw = match hotkey {
        Some(hotkey) => encode(hotkey)?,
        None => 0,
    };
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::PermissionDenied => {
                "Windows did not let WinCraft change this file. It may need administrator rights."
                    .to_string()
            }
            _ => format!("Could not open the file: {err}"),
        })?;
    let mut header = [0u8; HEADER_SIZE];
    file.read_exact(&mut header)
        .map_err(|_| "This is not a shortcut file.".to_string())?;
    if header_key(&header).is_none() {
        return Err("This is not a shortcut file.".to_string());
    }
    file.seek(SeekFrom::Start(HOTKEY_AT))
        .and_then(|_| file.write_all(&raw.to_le_bytes()))
        .and_then(|_| file.flush())
        .map_err(|err| format!("Could not save the file: {err}"))?;
    drop(file);
    let text = wide(&path.to_string_lossy());
    unsafe {
        SHChangeNotify(
            SHCNE_UPDATEITEM as i32,
            SHCNF_PATHW,
            text.as_ptr().cast(),
            std::ptr::null(),
        )
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hotkeys;

    fn key(text: &str) -> Hotkey {
        hotkeys::parse(text).expect(text)
    }

    /// A made-up shortcut file: a valid header with `raw` as its key and a
    /// few bytes of body.
    fn link_bytes(raw: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_SIZE + 8];
        bytes[..4].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
        bytes[4..20].copy_from_slice(&LINK_CLSID);
        bytes[64..66].copy_from_slice(&raw.to_le_bytes());
        bytes[HEADER_SIZE..].copy_from_slice(b"rest-ok!");
        bytes
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-lnk-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn the_hotkeyf_flags_become_modifiers() {
        // Ctrl+Alt+N: 'N' = 0x4E, HOTKEYF_CONTROL | HOTKEYF_ALT = 0x06.
        assert_eq!(decode(0x064E), Some(key("Ctrl+Alt+N")));
        assert_eq!(decode(0x0170), Some(key("Shift+F1")));
        assert_eq!(decode(0x0731), Some(key("Ctrl+Alt+Shift+1")));
        assert_eq!(decode(0), None, "no key");
        assert_eq!(decode(0x0070), None, "a key with no modifier is not listed");
        assert_eq!(decode(0x0600), None, "modifiers without a key");
    }

    #[test]
    fn encoding_turns_round_and_refuses_the_win_key() {
        for text in ["Ctrl+Alt+N", "Shift+F1", "Ctrl+Alt+Shift+1", "Alt+F9"] {
            let raw = encode(key(text)).expect(text);
            assert_eq!(decode(raw), Some(key(text)), "{text}");
        }
        assert!(encode(key("Win+Alt+N")).is_err());
    }

    #[test]
    fn only_a_shell_link_header_has_a_key() {
        assert_eq!(header_key(&link_bytes(0x064E)), Some(0x064E));
        let mut other = link_bytes(0x064E);
        other[4] = 0x02;
        assert_eq!(header_key(&other), None, "another CLSID");
        assert_eq!(header_key(&[0u8; 10]), None, "too short");
    }

    #[test]
    fn the_scan_finds_keys_in_sub_folders_and_skips_the_rest() {
        let dir = temp_dir("scan");
        fs::create_dir_all(dir.join("Tools")).unwrap();
        fs::write(dir.join("Notes.lnk"), link_bytes(0x064E)).unwrap();
        fs::write(dir.join("Tools").join("Calc.LNK"), link_bytes(0x0170)).unwrap();
        fs::write(dir.join("Plain.lnk"), link_bytes(0)).unwrap();
        fs::write(dir.join("Readme.txt"), link_bytes(0x064E)).unwrap();
        fs::write(dir.join("Broken.lnk"), b"nope").unwrap();
        // The same folder twice still gives each file once.
        let found = scan(&[(dir.clone(), 1), (dir.clone(), 1), (dir.join("missing"), 1)]);
        let names: Vec<(&str, String)> = found
            .iter()
            .map(|link| (link.name.as_str(), hotkeys::format(link.hotkey)))
            .collect();
        assert_eq!(
            names,
            [
                ("Notes", "Ctrl+Alt+N".to_string()),
                ("Calc", "Shift+F1".to_string())
            ]
        );
        let top_only = scan(&[(dir.clone(), 0)]);
        assert_eq!(top_only.len(), 1, "a desktop is read without its folders");
        assert_eq!(top_only[0].name, "Notes");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_changes_only_the_key_bytes() {
        let dir = temp_dir("write");
        let path = dir.join("Notes.lnk");
        fs::write(&path, link_bytes(0x064E)).unwrap();
        write_key(&path, Some(key("Ctrl+Shift+F5"))).expect("change");
        let changed = fs::read(&path).unwrap();
        let mut wanted = link_bytes(encode(key("Ctrl+Shift+F5")).unwrap());
        assert_eq!(changed, wanted);
        write_key(&path, None).expect("remove");
        wanted = link_bytes(0);
        assert_eq!(fs::read(&path).unwrap(), wanted);
        assert!(write_key(&path, Some(key("Win+Alt+N"))).is_err());
        assert_eq!(
            fs::read(&path).unwrap(),
            wanted,
            "a refused key writes nothing"
        );

        let text = dir.join("Readme.lnk");
        fs::write(
            &text,
            b"not a link at all, but long enough to read a header from it, yes indeed it is",
        )
        .unwrap();
        let before = fs::read(&text).unwrap();
        assert!(write_key(&text, None).is_err());
        assert_eq!(fs::read(&text).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "reads the real PC; run by hand"]
    fn the_real_scan_is_quick() {
        let start = std::time::Instant::now();
        let folders = folders();
        let found = scan(&folders);
        let ms = start.elapsed().as_millis();
        println!("{} folders, {} keys, {ms} ms", folders.len(), found.len());
        for link in &found {
            println!("  {} = {}", hotkeys::format(link.hotkey), link.name);
        }
        assert!(ms < 1_000);
    }
}

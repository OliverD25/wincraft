//! Programs listed as installed in the registry's Uninstall keys, for the
//! ones that have no Start menu shortcut and no entry in the apps folder:
//! a program unpacked or installed by an installer that skips both. The
//! registry names the program, not the file to start, so the file is
//! worked out from the entry.

use std::cmp::Reverse;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, MAX_PATH};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD, REG_EXPAND_SZ, REG_SZ,
};

use crate::core::{expand_env, version_info, wide};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
/// How deep under the install folder a program is looked for.
const SCAN_DEPTH: usize = 3;
/// A folder tree is never walked past this many entries.
const SCAN_LIMIT: usize = 4000;
/// Only this many of the biggest programs of a folder have their version
/// resource read.
const VERSION_READS: usize = 60;

/// What an Uninstall key says about a program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub install_location: String,
    pub display_icon: String,
    /// `SystemComponent=1`: hidden from "Installed apps" on purpose.
    pub system_component: bool,
    /// `ParentKeyName` is set: a patch or a part of another entry.
    pub has_parent: bool,
    pub release_type: String,
}

/// An installed program with the file that starts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub name: String,
    pub exe: String,
    /// The install folder the entry names, lower case, without a closing
    /// backslash; empty when it names none.
    pub install_dir: String,
}

/// Whether an entry is a program a person would start: not a hidden system
/// component, a patch, an update or a nameless leftover.
pub fn wanted(entry: &Entry) -> bool {
    let name = entry.name.trim();
    if name.is_empty() || entry.system_component || entry.has_parent {
        return false;
    }
    let lower = name.to_lowercase();
    let release = entry.release_type.to_lowercase();
    // Parts a program needs, which are not programs to start.
    let is_part = lower.contains("driver package")
        || lower.contains("redistributable")
        || lower.contains("webview")
        || lower.contains("toolkit")
        || lower.contains("maintenance service")
        || lower.ends_with(" service")
        || lower.ends_with(" runtime")
        || lower.ends_with(" sdk")
        || lower.contains(" sdk ");
    let is_update = is_part
        || release.contains("update")
        || release.contains("hotfix")
        || lower.contains("(kb")
        || lower.starts_with("update for")
        || lower.starts_with("security update")
        || lower.starts_with("hotfix")
        || lower
            .strip_prefix("kb")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    !is_update
}

/// The path in a DisplayIcon value: `"C:\App\app.exe",0` is `C:\App\app.exe`.
fn icon_path(display_icon: &str) -> String {
    let text = expand_env(display_icon.trim());
    let without_index = match text.rfind(',') {
        Some(at)
            if text[at + 1..]
                .trim()
                .trim_start_matches('-')
                .chars()
                .all(|c| c.is_ascii_digit()) =>
        {
            &text[..at]
        }
        _ => text.as_str(),
    };
    without_index.trim().trim_matches('"').to_string()
}

fn file_stem_lower(path: &str) -> String {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

/// Files that are part of a program but not the program: uninstallers,
/// installers, updaters, patchers, crash reporters, helper processes, the
/// browser engine's helper, feedback and guard stubs, the path setter some
/// SDKs ship, Java's web launcher. Applies to the DisplayIcon's program as
/// much as to files found in the folder.
///
/// No rule was found for "launcher" stubs: a launcher is the program itself
/// for some (TikTok LIVE Studio Launcher) and a stub for others.
pub fn excluded_name(path: &str) -> bool {
    let stem = file_stem_lower(path);
    [
        "uninst", "setup", "install", "update", "patch", "crash", "helper", "feedback", "appguard",
        "setpaths", "javaws",
    ]
    .iter()
    .any(|bad| stem.contains(bad))
        || stem.starts_with("unins")
        || stem.contains("cef")
        || stem.contains("redist")
        || stem.contains("clicktorun")
}

/// Whether a file found under `root` is excluded by its name or by a folder
/// between `root` and it: `Feedback\dump64a.exe` belongs to the feedback tool.
fn excluded_path(root: &str, path: &str) -> bool {
    if excluded_name(path) {
        return true;
    }
    let relative = path
        .get(root.trim_end_matches('\\').len()..)
        .unwrap_or(path);
    let mut parts: Vec<&str> = relative
        .split('\\')
        .filter(|part| !part.is_empty())
        .collect();
    parts.pop();
    parts.into_iter().any(excluded_name)
}

/// A file inside a folder Windows keeps installer leftovers in.
fn in_installer_cache(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains(r"\package cache\") || lower.contains(r"\windows\installer\")
}

/// A program found while looking through an install folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub path: String,
    pub size: u64,
}

/// What `pick_exe` needs from the disk, so the choice can be tested on
/// made-up files.
pub trait Files {
    fn exists(&self, path: &str) -> bool;
    /// The `.exe` files under `root`, down to `depth` folders.
    fn exes(&self, root: &str, depth: usize) -> Vec<Candidate>;
    /// (ProductName, FileDescription) from the version resource.
    fn version(&self, path: &str) -> (Option<String>, Option<String>);
    /// The program runs in a console window, like a command-line tool, and
    /// is not something to start from a search box.
    fn console(&self, _path: &str) -> bool {
        false
    }
}

/// Whether a program's PE header says it is a console program: the
/// Subsystem field of the optional header is 3.
pub fn is_console_header(bytes: &[u8]) -> bool {
    const SUBSYSTEM_OFFSET: usize = 24 + 68;
    let word = |at: usize| {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let dword = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if word(0) != Some(0x5A4D) {
        return false;
    }
    let Some(pe) = dword(0x3C).map(|at| at as usize) else {
        return false;
    };
    dword(pe) == Some(0x0000_4550) && word(pe + SUBSYSTEM_OFFSET) == Some(3)
}

/// The words of a display name that can be told apart: letters and digits,
/// at least two characters and one letter, in lower case.
fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2 && word.chars().any(char::is_alphabetic))
    {
        if !out.iter().any(|seen| seen == word) {
            out.push(word.to_string());
        }
    }
    out
}

/// Whether the file's own name starts with a word of the display name or has
/// one as a word: `Lightroom.exe` for "Adobe Lightroom Classic".
fn name_hit(display: &[String], path: &str) -> bool {
    let stem = file_stem_lower(path);
    stem.split(|c: char| !c.is_alphanumeric())
        .any(|word| display.iter().any(|shown| shown == word))
        || display.iter().any(|shown| stem.starts_with(shown.as_str()))
}

/// How well a program's own names fit the display name: (words of the
/// display name found, extra words the program's names have).
fn fit(display: &[String], product: Option<&str>, description: Option<&str>) -> (usize, usize) {
    let text = format!(
        "{} {}",
        product.unwrap_or_default(),
        description.unwrap_or_default()
    );
    let theirs = words(&text);
    let found = display.iter().filter(|word| theirs.contains(word)).count();
    (found, theirs.len().saturating_sub(found))
}

/// How well a program fits an entry: words of the name found, named like it,
/// fewer extra words, nearer the top of the folder. Bigger is better.
type Rank = (usize, bool, Reverse<usize>, Reverse<usize>);

/// The file that starts the program an entry describes, or None when
/// nothing fits.
pub fn pick_exe(entry: &Entry, files: &dyn Files) -> Option<String> {
    let icon = icon_path(&entry.display_icon);
    let icon_is_exe = icon.to_lowercase().ends_with(".exe");
    if icon_is_exe
        && !excluded_name(&icon)
        && !in_installer_cache(&icon)
        && files.exists(&icon)
        && !files.console(&icon)
    {
        return Some(icon);
    }

    let location = expand_env(entry.install_location.trim())
        .trim()
        .trim_matches('"')
        .to_string();
    let root = if !location.is_empty() {
        location
    } else if !icon.is_empty() && !in_installer_cache(&icon) {
        // An icon file beside the program says where the program is.
        Path::new(&icon)
            .parent()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let depth = if entry.install_location.trim().is_empty() {
        1
    } else {
        SCAN_DEPTH
    };
    if root.is_empty() || root.len() < 4 {
        return None;
    }
    let mut candidates: Vec<Candidate> = files
        .exes(&root, depth)
        .into_iter()
        .filter(|candidate| !excluded_path(&root, &candidate.path))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let display = words(&entry.name);
    let base = root.trim_end_matches('\\').len() + 1;
    let depth_of = |path: &str| {
        path.get(base..)
            .map_or(0, |rest| rest.matches('\\').count())
    };
    // Only the first few have their version resource read, so the ones most
    // likely to be the program come first: named like the entry, near the
    // top of the folder, big.
    candidates.sort_by(|a, b| {
        name_hit(&display, &b.path)
            .cmp(&name_hit(&display, &a.path))
            .then_with(|| depth_of(&a.path).cmp(&depth_of(&b.path)))
            .then_with(|| b.size.cmp(&a.size))
            .then_with(|| a.path.cmp(&b.path))
    });

    // The program's own idea of its name beats its size. Among programs
    // that fit, one named like the entry wins, then the closest description,
    // then the one nearest the top of the folder.
    let needed = display.len().div_ceil(2).max(1);
    let mut best: Option<(Rank, &Candidate)> = None;
    for candidate in candidates.iter().take(VERSION_READS) {
        if files.console(&candidate.path) {
            continue;
        }
        let (product, description) = files.version(&candidate.path);
        let (found, extra) = fit(&display, product.as_deref(), description.as_deref());
        if found < needed {
            continue;
        }
        let rank = (
            found,
            name_hit(&display, &candidate.path),
            Reverse(extra),
            Reverse(depth_of(&candidate.path)),
        );
        // An equal rank keeps the earlier, which is the more likely one.
        if best.as_ref().is_none_or(|(top, _)| rank > *top) {
            best = Some((rank, candidate));
        }
    }
    if let Some((_, candidate)) = best {
        return Some(candidate.path.clone());
    }
    // Nothing is described like the entry: a file named like it, else the
    // biggest.
    let named = candidates
        .iter()
        .find(|candidate| name_hit(&display, &candidate.path) && !files.console(&candidate.path));
    if let Some(candidate) = named {
        return Some(candidate.path.clone());
    }
    // Only the biggest few are looked at, so a folder of command-line tools
    // costs a handful of reads.
    let mut by_size: Vec<&Candidate> = candidates.iter().collect();
    by_size.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    by_size
        .into_iter()
        .take(10)
        .find(|candidate| !files.console(&candidate.path))
        .map(|candidate| candidate.path.clone())
}

/// The real disk.
pub struct Disk;

impl Files for Disk {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn exes(&self, root: &str, depth: usize) -> Vec<Candidate> {
        let mut found = Vec::new();
        let mut budget = SCAN_LIMIT;
        walk(Path::new(root), depth, &mut budget, &mut found);
        found
    }

    fn version(&self, path: &str) -> (Option<String>, Option<String>) {
        (
            version_info::string(path, "ProductName"),
            version_info::string(path, "FileDescription"),
        )
    }

    fn console(&self, path: &str) -> bool {
        use std::io::Read;
        let mut header = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(4096).read_to_end(&mut header))
            .is_ok()
            && is_console_header(&header)
    }
}

fn walk(dir: &Path, depth: usize, budget: &mut usize, found: &mut Vec<Candidate>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 1 {
                walk(&path, depth - 1, budget, found);
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            found.push(Candidate {
                path: path.to_string_lossy().to_string(),
                size,
            });
        }
    }
}

/// Every Uninstall entry of this machine and this user that
/// `wanted` accepts, with the program that starts it. `skip` says which
/// names need no lookup because something else already lists them.
pub fn programs(skip: &dyn Fn(&str) -> bool) -> Vec<Program> {
    let mut found = Vec::new();
    for (root, path) in [
        (HKEY_LOCAL_MACHINE, UNINSTALL),
        (HKEY_LOCAL_MACHINE, UNINSTALL_32),
        (HKEY_CURRENT_USER, UNINSTALL),
    ] {
        for entry in read_entries(root, path) {
            if !wanted(&entry) || skip(&entry.name) {
                continue;
            }
            if let Some(exe) = pick_exe(&entry, &Disk) {
                let install_dir = expand_env(entry.install_location.trim())
                    .trim()
                    .trim_matches('"')
                    .trim_end_matches('\\')
                    .to_lowercase();
                found.push(Program {
                    name: entry.name.trim().to_string(),
                    exe,
                    install_dir,
                });
            }
        }
    }
    found
}

/// The entries under one Uninstall key; an unreadable key has none.
pub fn read_entries(root: HKEY, path: &str) -> Vec<Entry> {
    let mut parent: HKEY = std::ptr::null_mut();
    if unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, KEY_READ, &mut parent) }
        != ERROR_SUCCESS
    {
        return Vec::new();
    }
    let mut entries = Vec::new();
    let mut index = 0;
    loop {
        let mut name = [0u16; MAX_PATH as usize];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                parent,
                index,
                name.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        index += 1;
        if status != ERROR_SUCCESS {
            break;
        }
        let mut key: HKEY = std::ptr::null_mut();
        let opened = unsafe { RegOpenKeyExW(parent, name.as_ptr(), 0, KEY_READ, &mut key) };
        if opened != ERROR_SUCCESS {
            continue;
        }
        entries.push(Entry {
            name: string_value(key, "DisplayName").unwrap_or_default(),
            install_location: string_value(key, "InstallLocation").unwrap_or_default(),
            display_icon: string_value(key, "DisplayIcon").unwrap_or_default(),
            system_component: dword_value(key, "SystemComponent") == Some(1),
            has_parent: string_value(key, "ParentKeyName").is_some_and(|text| !text.is_empty()),
            release_type: string_value(key, "ReleaseType").unwrap_or_default(),
        });
        unsafe { RegCloseKey(key) };
    }
    unsafe { RegCloseKey(parent) };
    entries
}

fn string_value(key: HKEY, name: &str) -> Option<String> {
    let name = wide(name);
    let (mut kind, mut size) = (0u32, 0u32);
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS || !(kind == REG_SZ || kind == REG_EXPAND_SZ) || size == 0 {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let units = (size as usize / 2).min(buffer.len());
    let end = buffer[..units]
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units);
    let text = String::from_utf16_lossy(&buffer[..end]);
    Some(text.trim().to_string())
}

fn dword_value(key: HKEY, name: &str) -> Option<u32> {
    let name = wide(name);
    let (mut kind, mut size) = (0u32, 4u32);
    let mut value = 0u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            (&raw mut value).cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS && kind == REG_DWORD).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Made-up files: path to (size, ProductName, FileDescription).
    struct Fake {
        files: HashMap<String, (u64, Option<&'static str>, Option<&'static str>)>,
    }

    impl Fake {
        fn new(files: &[(&str, u64, Option<&'static str>, Option<&'static str>)]) -> Self {
            Self {
                files: files
                    .iter()
                    .map(|(path, size, product, description)| {
                        (path.to_string(), (*size, *product, *description))
                    })
                    .collect(),
            }
        }
    }

    impl Files for Fake {
        fn exists(&self, path: &str) -> bool {
            self.files.contains_key(path)
        }

        fn exes(&self, root: &str, depth: usize) -> Vec<Candidate> {
            let base = root.trim_end_matches('\\').len() + 1;
            self.files
                .iter()
                .filter(|(path, _)| path.starts_with(root))
                .filter(|(path, _)| path[base..].matches('\\').count() < depth)
                .map(|(path, (size, _, _))| Candidate {
                    path: path.clone(),
                    size: *size,
                })
                .collect()
        }

        fn version(&self, path: &str) -> (Option<String>, Option<String>) {
            self.files
                .get(path)
                .map(|(_, product, description)| {
                    (product.map(str::to_string), description.map(str::to_string))
                })
                .unwrap_or((None, None))
        }
    }

    fn entry(name: &str, location: &str, icon: &str) -> Entry {
        Entry {
            name: name.to_string(),
            install_location: location.to_string(),
            display_icon: icon.to_string(),
            ..Entry::default()
        }
    }

    const ROOT: &str = r"D:\Programs\Lightroom_Classic_v12.2\Adobe";
    const LIGHTROOM: &str =
        r"D:\Programs\Lightroom_Classic_v12.2\Adobe\Adobe Lightroom Classic\Lightroom.exe";

    fn lightroom_disk() -> Fake {
        Fake::new(&[
            (
                LIGHTROOM,
                9_000_000,
                Some("Adobe Lightroom Classic"),
                Some("Adobe Lightroom Classic"),
            ),
            (
                r"D:\Programs\Lightroom_Classic_v12.2\Adobe\Adobe Lightroom Classic\Adobe Lightroom CEF Helper.exe",
                40_000_000,
                Some("Adobe Lightroom"),
                Some("Adobe Lightroom CEF Helper"),
            ),
            (
                r"D:\Programs\Lightroom_Classic_v12.2\Adobe\Adobe Lightroom Classic\CrashReporter.exe",
                3_000_000,
                None,
                None,
            ),
            (
                r"D:\Programs\Lightroom_Classic_v12.2\Adobe\Adobe Lightroom Classic\Uninstall.exe",
                50_000_000,
                None,
                None,
            ),
        ])
    }

    #[test]
    fn lightroom_resolves_to_its_own_program_not_a_helper() {
        let entry = entry(
            "Adobe Lightroom Classic",
            ROOT,
            r"D:\Programs\Lightroom_Classic_v12.2\Adobe\Adobe Lightroom Classic\app.ico",
        );
        assert_eq!(
            pick_exe(&entry, &lightroom_disk()).as_deref(),
            Some(LIGHTROOM)
        );
    }

    fn tool_disk() -> Fake {
        Fake::new(&[
            (r"C:\Apps\Tool\tool.exe", 5_000, None, None),
            (r"C:\Apps\Tool\engine.exe", 90_000, None, None),
            (r"C:\Apps\Tool\unins000.exe", 900_000, None, None),
            (r"C:\Apps\Tool\update_check.exe", 800_000, None, None),
            (r"C:\Apps\Tool\setup.exe", 700_000, None, None),
            (r"C:\Apps\Tool\UnityCrashHandler64.exe", 600_000, None, None),
            (r"C:\Apps\Tool\Tool helper.exe", 500_000, None, None),
            (r"C:\Apps\Tool\vendor-installer.exe", 400_000, None, None),
        ])
    }

    #[test]
    fn without_a_description_a_file_named_like_the_entry_comes_first() {
        let entry = entry("Tool", r"C:\Apps\Tool", "");
        assert_eq!(
            pick_exe(&entry, &tool_disk()).as_deref(),
            Some(r"C:\Apps\Tool\tool.exe")
        );
    }

    #[test]
    fn the_largest_ordinary_program_is_the_last_resort() {
        // Nothing is named like the entry, and the bigger files are
        // uninstallers, updaters, installers, crash handlers and helpers.
        let entry = entry("Gadget", r"C:\Apps\Tool", "");
        assert_eq!(
            pick_exe(&entry, &tool_disk()).as_deref(),
            Some(r"C:\Apps\Tool\engine.exe")
        );
    }

    #[test]
    fn a_shallower_program_wins_between_equal_fits() {
        let disk = Fake::new(&[
            (
                r"C:\Apps\Calibre\bin\cjpeg-calibre.exe",
                900,
                Some("calibre"),
                Some("calibre"),
            ),
            (
                r"C:\Apps\Calibre\calibre.exe",
                100,
                Some("calibre"),
                Some("calibre"),
            ),
        ]);
        let entry = entry("calibre 64bit", r"C:\Apps\Calibre", "");
        assert_eq!(
            pick_exe(&entry, &disk).as_deref(),
            Some(r"C:\Apps\Calibre\calibre.exe")
        );
    }

    #[test]
    fn the_version_resource_beats_size() {
        let disk = Fake::new(&[
            (
                r"C:\Apps\Suite\launcher.exe",
                100,
                Some("Suite Studio"),
                Some("Suite Studio"),
            ),
            (
                r"C:\Apps\Suite\bin\python.exe",
                9_000_000,
                Some("Python"),
                Some("Python"),
            ),
        ]);
        let entry = entry("Suite Studio 2024", r"C:\Apps\Suite", "");
        assert_eq!(
            pick_exe(&entry, &disk).as_deref(),
            Some(r"C:\Apps\Suite\launcher.exe")
        );
    }

    #[test]
    fn a_closer_description_wins_between_two_matches() {
        let disk = Fake::new(&[
            (
                r"C:\Apps\Photo\photo.exe",
                100,
                Some("Photo Editor"),
                Some("Photo Editor"),
            ),
            (
                r"C:\Apps\Photo\photo_batch.exe",
                900,
                Some("Photo Editor"),
                Some("Photo Editor batch converter command line tool"),
            ),
        ]);
        let entry = entry("Photo Editor", r"C:\Apps\Photo", "");
        assert_eq!(
            pick_exe(&entry, &disk).as_deref(),
            Some(r"C:\Apps\Photo\photo.exe")
        );
    }

    #[test]
    fn a_display_icon_that_is_a_program_is_used_as_it_is() {
        let disk = Fake::new(&[(r"C:\Apps\Neo\neo.exe", 10, None, None)]);
        let with_index = entry("Neo", "", r#""C:\Apps\Neo\neo.exe",0"#);
        assert_eq!(
            pick_exe(&with_index, &disk).as_deref(),
            Some(r"C:\Apps\Neo\neo.exe")
        );
        let negative = entry("Neo", "", r"C:\Apps\Neo\neo.exe,-103");
        assert_eq!(
            pick_exe(&negative, &disk).as_deref(),
            Some(r"C:\Apps\Neo\neo.exe")
        );
        // A missing file is not trusted.
        let gone = entry("Neo", "", r"C:\Apps\Gone\gone.exe");
        assert_eq!(pick_exe(&gone, &disk), None);
    }

    #[test]
    fn an_uninstaller_or_an_installer_is_never_the_program() {
        let disk = Fake::new(&[
            (r"C:\Apps\Old\unins000.exe", 10, None, None),
            (
                r"C:\ProgramData\Package Cache\{1}\vc_redist.x64.exe",
                10,
                None,
                None,
            ),
        ]);
        assert_eq!(
            pick_exe(&entry("Old", "", r"C:\Apps\Old\unins000.exe"), &disk),
            None
        );
        assert_eq!(
            pick_exe(
                &entry(
                    "Visual C++ Redistributable",
                    "",
                    r"C:\ProgramData\Package Cache\{1}\vc_redist.x64.exe"
                ),
                &disk
            ),
            None
        );
    }

    #[test]
    fn a_folder_with_only_helpers_gives_nothing() {
        let disk = Fake::new(&[
            (r"C:\Apps\Odd\Odd Helper.exe", 10, None, None),
            (r"C:\Apps\Odd\uninstall.exe", 10, None, None),
        ]);
        assert_eq!(pick_exe(&entry("Odd", r"C:\Apps\Odd", ""), &disk), None);
        assert_eq!(pick_exe(&entry("Odd", "", ""), &disk), None);
    }

    #[test]
    fn programs_are_looked_for_three_folders_down_and_no_further() {
        let disk = Fake::new(&[
            (r"C:\Apps\Deep\a\b\ok.exe", 10, None, None),
            (r"C:\Apps\Deep\a\b\c\too_deep.exe", 99, None, None),
        ]);
        assert_eq!(
            pick_exe(&entry("Deep", r"C:\Apps\Deep", ""), &disk).as_deref(),
            Some(r"C:\Apps\Deep\a\b\ok.exe")
        );
    }

    #[test]
    fn system_components_patches_and_updates_are_not_programs() {
        let program = entry("Notepad++", r"C:\Notepad++", "");
        assert!(wanted(&program));
        let hidden = Entry {
            system_component: true,
            ..program.clone()
        };
        assert!(!wanted(&hidden));
        let child = Entry {
            has_parent: true,
            ..program.clone()
        };
        assert!(!wanted(&child));
        assert!(!wanted(&entry("", "x", "")));
        assert!(!wanted(&entry("   ", "x", "")));
        for update in [
            "KB5031356",
            "Security Update for Microsoft Windows (KB5031)",
            "Update for Windows Defender",
            "Hotfix for Something",
            "Some Product (KB123456)",
        ] {
            assert!(!wanted(&entry(update, "x", "")), "{update}");
        }
        let typed = Entry {
            release_type: "Security Update".to_string(),
            ..program
        };
        assert!(!wanted(&typed));
        for part in [
            "Windows Driver Package - libusb-win32 (libusb0)",
            "Microsoft Visual C++ 2015-2022 Redistributable (x64)",
            "UXP WebView Support",
            "Mozilla Maintenance Service",
            "ASUS Framework Service",
            "NVIDIA Video Effects SDK",
            "Some Runtime",
        ] {
            assert!(!wanted(&entry(part, "x", "")), "{part}");
        }
        assert!(wanted(&entry("Servicemaster Pro", "x", "")));
        // A name that only starts with the letters is a program.
        assert!(wanted(&entry("Kbox Player", "x", "")));
    }

    #[test]
    fn words_leave_out_numbers_and_short_bits() {
        assert_eq!(
            words("Adobe Lightroom Classic 12.2 (x64)"),
            ["adobe", "lightroom", "classic", "x64"]
        );
        assert_eq!(words("7-Zip"), ["zip"]);
    }

    #[test]
    fn a_file_in_a_folder_named_like_a_tool_is_that_tools_file() {
        let disk = Fake::new(&[
            (r"C:\Apps\Studio\Feedback\dump64a.exe", 900, None, None),
            (r"C:\Apps\Studio\Updater\run.exe", 800, None, None),
            (r"C:\Apps\Studio\studio.exe", 10, None, None),
        ]);
        let entry = entry("Studio", r"C:\Apps\Studio", "");
        assert_eq!(
            pick_exe(&entry, &disk).as_deref(),
            Some(r"C:\Apps\Studio\studio.exe")
        );
        // The folder the search starts in is not judged by its own name.
        let inside = Fake::new(&[(r"C:\Apps\Updates\tool.exe", 10, None, None)]);
        let entry = entry_named("Tool", r"C:\Apps\Updates");
        assert_eq!(
            pick_exe(&entry, &inside).as_deref(),
            Some(r"C:\Apps\Updates\tool.exe")
        );
    }

    fn entry_named(name: &str, location: &str) -> Entry {
        entry(name, location, "")
    }

    #[test]
    fn stubs_and_setters_are_never_the_program() {
        for name in [
            "OneDrivePatcher.exe",
            "feedback.exe",
            "officeappguardwin32.exe",
            "setpaths.exe",
            "javaws.exe",
            "vs_installer_setup.exe",
        ] {
            assert!(excluded_name(&format!(r"C:\Apps\{name}")), "{name}");
        }
        // The program itself is not caught by the new words.
        for name in [
            "OneDrive.exe",
            "java.exe",
            "Lightroom.exe",
            "TikTok LIVE Studio Launcher.exe",
        ] {
            assert!(!excluded_name(&format!(r"C:\Apps\{name}")), "{name}");
        }
    }

    #[test]
    fn a_display_icon_that_is_a_stub_falls_back_to_the_folder() {
        let disk = Fake::new(&[
            (r"C:\Apps\Java\bin\javaws.exe", 10, None, None),
            (r"C:\Apps\Java\bin\java.exe", 5, None, None),
        ]);
        let entry = entry("Java", r"C:\Apps\Java", r"C:\Apps\Java\bin\javaws.exe");
        assert_eq!(
            pick_exe(&entry, &disk).as_deref(),
            Some(r"C:\Apps\Java\bin\java.exe")
        );
        let feedback = Fake::new(&[(r"C:\Apps\VS\feedback.exe", 10, None, None)]);
        let entry = entry_with_icon_only(r"C:\Apps\VS\feedback.exe");
        assert_eq!(pick_exe(&entry, &feedback), None);
    }

    fn entry_with_icon_only(icon: &str) -> Entry {
        entry("Visual Studio Installer", "", icon)
    }

    /// A tiny PE header with the given subsystem.
    fn pe_header(subsystem: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x100];
        bytes[0] = b'M';
        bytes[1] = b'Z';
        bytes[0x3C] = 0x80;
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        let at = 0x80 + 24 + 68;
        bytes[at..at + 2].copy_from_slice(&subsystem.to_le_bytes());
        bytes
    }

    #[test]
    fn the_pe_header_tells_a_console_program_from_a_window_program() {
        assert!(is_console_header(&pe_header(3)));
        assert!(!is_console_header(&pe_header(2)));
        assert!(!is_console_header(b"not a program"));
        assert!(!is_console_header(&[]));
        // A header that stops short is not guessed at.
        assert!(!is_console_header(&pe_header(3)[..0x90]));
    }

    struct WithConsoles {
        inner: Fake,
        consoles: Vec<&'static str>,
    }

    impl Files for WithConsoles {
        fn exists(&self, path: &str) -> bool {
            self.inner.exists(path)
        }
        fn exes(&self, root: &str, depth: usize) -> Vec<Candidate> {
            self.inner.exes(root, depth)
        }
        fn version(&self, path: &str) -> (Option<String>, Option<String>) {
            self.inner.version(path)
        }
        fn console(&self, path: &str) -> bool {
            self.consoles.contains(&path)
        }
    }

    #[test]
    fn a_command_line_tool_is_not_the_program_to_start() {
        let disk = WithConsoles {
            inner: Fake::new(&[
                (
                    r"C:\Apps\Shell\shell.exe",
                    100,
                    Some("Shell"),
                    Some("Shell"),
                ),
                (
                    r"C:\Apps\Shell\shell-gui.exe",
                    50,
                    Some("Shell"),
                    Some("Shell"),
                ),
                (r"C:\Apps\Shell\tool.exe", 900, None, None),
            ]),
            consoles: vec![r"C:\Apps\Shell\shell.exe", r"C:\Apps\Shell\tool.exe"],
        };
        // By description, the window program wins over the console one.
        assert_eq!(
            pick_exe(&entry("Shell", r"C:\Apps\Shell", ""), &disk).as_deref(),
            Some(r"C:\Apps\Shell\shell-gui.exe")
        );
        // With no description, the biggest is not a console program either.
        let plain = WithConsoles {
            inner: Fake::new(&[
                (r"C:\Apps\Gadget\a.exe", 10, None, None),
                (r"C:\Apps\Gadget\b.exe", 900, None, None),
            ]),
            consoles: vec![r"C:\Apps\Gadget\b.exe"],
        };
        assert_eq!(
            pick_exe(&entry("Gadget", r"C:\Apps\Gadget", ""), &plain).as_deref(),
            Some(r"C:\Apps\Gadget\a.exe")
        );
        // Only console programs: nothing fits.
        let only = WithConsoles {
            inner: Fake::new(&[(r"C:\Apps\Cli\cli.exe", 10, None, None)]),
            consoles: vec![r"C:\Apps\Cli\cli.exe"],
        };
        assert_eq!(pick_exe(&entry("Cli", r"C:\Apps\Cli", ""), &only), None);
        // A DisplayIcon that is a console program is passed over as well.
        let icon = entry("Cli", "", r"C:\Apps\Cli\cli.exe");
        assert_eq!(pick_exe(&icon, &only), None);
    }
}

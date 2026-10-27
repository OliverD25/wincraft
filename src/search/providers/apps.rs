use std::collections::HashSet;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::core::{IUnknown_Vtbl, GUID, HRESULT, PCWSTR, PWSTR};
use windows_sys::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows_sys::Win32::System::Com::{
    CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};

use super::{apps_folder, installed};
use crate::core::appid;
use crate::core::com::{self, ComPtr};
use crate::search::{
    Action, Choice, Completion, Context, IconRef, Query, ResultItem, SearchProvider,
};
use crate::ui::fuzzy;

const REINDEX_EVERY: Duration = Duration::from_secs(10 * 60);
/// Start menu folders are shallow; this only stops a folder loop.
const MAX_DEPTH: usize = 8;

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);
const IID_IPROPERTY_STORE: GUID = GUID::from_u128(0x886d8eeb_8cf2_4446_8d02_cdba1dbdcf99);

#[repr(C)]
struct IShellLinkWVtbl {
    _base: IUnknown_Vtbl,
    get_path:
        unsafe extern "system" fn(*mut c_void, PWSTR, i32, *mut WIN32_FIND_DATAW, u32) -> HRESULT,
}

#[repr(C)]
struct IPersistFileVtbl {
    _base: IUnknown_Vtbl,
    _get_class_id: unsafe extern "system" fn(),
    _is_dirty: unsafe extern "system" fn(),
    load: unsafe extern "system" fn(*mut c_void, PCWSTR, u32) -> HRESULT,
}

/// Where a row came from. A program found by more than one is listed once,
/// from the first of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A shortcut in a Start menu folder.
    StartMenu,
    /// The shell's `shell:AppsFolder`: Store apps, and desktop apps known
    /// by an app ID.
    AppsFolder,
    /// A registry Uninstall entry with no shortcut of its own.
    Installed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    /// What Enter opens: a shortcut, `shell:AppsFolder\<id>` or a program.
    pub target: PathBuf,
    /// Whose icon the row shows.
    pub icon: PathBuf,
    /// The row's second line: where it sits in the Start menu, or what kind
    /// of program it is.
    pub subtitle: String,
    /// Stays the same for the same thing, so picks keep their rank.
    pub usage_key: String,
    pub source: Source,
    /// The program it starts, lower case, when known: how the same program
    /// from two sources is told.
    pub exe: Option<String>,
    /// The app ID, lower case, when it has one.
    pub aumid: Option<String>,
    /// For an installed program, the install folder its entry names, lower
    /// case; empty when it names none.
    pub install_dir: String,
}

/// The newest index, for the programs that name an app by its app ID or its
/// program file but not the way the Start menu does. None until the first
/// index is built.
static PUBLISHED: Mutex<Option<Arc<Vec<App>>>> = Mutex::new(None);

/// The newest index, or None while the first one is being built.
pub fn published() -> Option<Arc<Vec<App>>> {
    PUBLISHED.lock().ok()?.clone()
}

/// What the Start menu or the app list calls the app with this group key: an
/// app ID, or the lower-case path of a program. None when no entry has that
/// ID or program. The first entry wins (the index is in priority order:
/// Start menu shortcuts, then the app list, then installed programs).
pub fn name_in(apps: &[App], key: &str) -> Option<String> {
    let key = key.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let by_path = key.contains('\\');
    apps.iter()
        .find(|app| {
            if by_path {
                app.exe.as_deref() == Some(key.as_str())
            } else {
                app.aumid.as_deref() == Some(key.as_str())
            }
        })
        .map(|app| app.name.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// Programs from the Start menu shortcuts of this user and of all users,
/// the shell's app list (Store apps among them) and the installed programs
/// of the registry.
pub struct Apps {
    index: Arc<Mutex<Arc<Vec<App>>>>,
    /// Set once the first index is in `index`.
    ready: Arc<AtomicBool>,
    /// Set with `ready`, cleared when the palette has been told.
    fresh: Arc<AtomicBool>,
}

impl Apps {
    /// Starts the indexing thread. Reading the folders and the shell takes
    /// tens to hundreds of milliseconds, too long for the UI thread, and
    /// runs again every ten minutes so newly installed programs show up.
    pub fn new() -> Self {
        let index = Arc::new(Mutex::new(Arc::new(Vec::new())));
        let shared = Arc::clone(&index);
        let ready = Arc::new(AtomicBool::new(false));
        let fresh = Arc::new(AtomicBool::new(false));
        let (thread_ready, thread_fresh) = (Arc::clone(&ready), Arc::clone(&fresh));
        let started = std::thread::Builder::new()
            .name("wincraft-apps".to_string())
            .spawn(move || {
                // The shell's namespace and shortcuts are apartment COM.
                unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
                loop {
                    let began = Instant::now();
                    let (apps, counts) = build_index(&start_menu_roots());
                    log::info!(
                        "indexed {} apps in {} ms: {} Start menu shortcuts, {} from the app list, {} installed programs ({} duplicates left out)",
                        apps.len(),
                        began.elapsed().as_millis(),
                        counts.start_menu,
                        counts.apps_folder,
                        counts.installed,
                        counts.duplicates
                    );
                    let apps = Arc::new(apps);
                    if let Ok(mut guard) = shared.lock() {
                        *guard = Arc::clone(&apps);
                    }
                    if let Ok(mut published) = PUBLISHED.lock() {
                        *published = Some(apps);
                    }
                    if !thread_ready.swap(true, Ordering::SeqCst) {
                        thread_fresh.store(true, Ordering::SeqCst);
                    }
                    std::thread::sleep(REINDEX_EVERY);
                }
            });
        if let Err(err) = started {
            log::warn!("could not start the app index thread: {err}");
        }
        Self {
            index,
            ready,
            fresh,
        }
    }
}

/// How many entries each source gave, before and after the duplicates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub start_menu: usize,
    pub apps_folder: usize,
    pub installed: usize,
    pub duplicates: usize,
}

/// Every source, merged. Needs COM on this thread.
pub fn build_index(roots: &[PathBuf]) -> (Vec<App>, Counts) {
    let mut start = scan(roots);
    for app in &mut start {
        if app
            .target
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
        {
            let (exe, aumid) = resolve_shortcut(&app.target);
            app.exe = exe;
            app.aumid = aumid;
        }
    }
    let folder = apps_folder_apps();
    let known: HashSet<String> = start
        .iter()
        .chain(&folder)
        .map(|app| app.name.trim().to_lowercase())
        .collect();
    // An entry something else already lists by name needs no file search.
    let installed = installed_apps(&|name| known.contains(&name.trim().to_lowercase()));
    let mut counts = Counts {
        start_menu: start.len(),
        apps_folder: folder.len(),
        installed: installed.len(),
        duplicates: 0,
    };
    let merged = merge(start, folder, installed);
    counts.duplicates = counts.start_menu + counts.apps_folder + counts.installed - merged.len();
    (merged, counts)
}

fn apps_folder_apps() -> Vec<App> {
    apps_folder::enumerate()
        .into_iter()
        .filter(|item| !item.name.to_lowercase().starts_with("uninstall"))
        .map(|item| {
            let launch = PathBuf::from(item.launch());
            App {
                subtitle: if item.is_package() {
                    "Store app".to_string()
                } else {
                    "Start menu".to_string()
                },
                usage_key: launch.to_string_lossy().to_lowercase(),
                exe: item.exe(),
                aumid: item.aumid(),
                install_dir: String::new(),
                name: item.name,
                icon: launch.clone(),
                target: launch,
                source: Source::AppsFolder,
            }
        })
        .collect()
}

fn installed_apps(skip: &dyn Fn(&str) -> bool) -> Vec<App> {
    installed::programs(skip)
        .into_iter()
        .map(|program| {
            let exe = PathBuf::from(&program.exe);
            App {
                name: program.name,
                usage_key: program.exe.to_lowercase(),
                exe: Some(program.exe.to_lowercase()),
                aumid: None,
                install_dir: program.install_dir,
                subtitle: "Installed program".to_string(),
                icon: exe.clone(),
                target: exe,
                source: Source::Installed,
            }
        })
        .collect()
}

/// Puts the sources together in priority order: Start menu shortcuts, then
/// the app list, then installed programs. A later entry is left out when an
/// earlier one has the same program, the same app ID or the same name. An
/// installed program is also left out when its program, or its install
/// folder, is the folder of a program the Start menu or the app list already
/// lists: the same product under another name (OneDrive, Office).
pub fn merge(start: Vec<App>, folder: Vec<App>, installed: Vec<App>) -> Vec<App> {
    let mut seen = Seen::default();
    let mut all = start;
    for app in &all {
        seen.add(app);
    }
    for app in folder.into_iter().chain(installed) {
        if !seen.has(&app) {
            seen.add(&app);
            all.push(app);
        }
    }
    all.sort_by_key(|app| app.name.to_lowercase());
    all
}

/// What the entries kept so far stand for.
#[derive(Default)]
struct Seen {
    exes: HashSet<String>,
    ids: HashSet<String>,
    names: HashSet<String>,
    /// The folders of the programs the Start menu and the app list list.
    folders: HashSet<String>,
}

/// The folder of a lower-case program path.
fn parent_dir(path: &str) -> Option<String> {
    path.rsplit_once('\\')
        .map(|(dir, _)| dir.to_string())
        .filter(|dir| !dir.is_empty())
}

/// Whether the folder of a listed program holds `dir`: the same folder, or
/// one inside it (a version folder). A folder near the top of a drive, such
/// as `c:\program files`, holds many products and covers none of them.
fn covers(listed: &str, dir: &str) -> bool {
    dir == listed || (listed.matches('\\').count() >= 2 && dir.starts_with(&format!("{listed}\\")))
}

impl Seen {
    fn add(&mut self, app: &App) {
        // Only the first two sources say which folders are covered.
        if app.source != Source::Installed {
            self.folders.extend(app.exe.as_deref().and_then(parent_dir));
        }
        self.exes.extend(app.exe.clone());
        self.ids.extend(app.aumid.clone());
        self.names.insert(app.name.trim().to_lowercase());
    }

    fn has(&self, app: &App) -> bool {
        let covered_folder = app.source == Source::Installed
            && [
                app.exe.as_deref().and_then(parent_dir),
                Some(app.install_dir.clone()).filter(|dir| !dir.is_empty()),
            ]
            .into_iter()
            .flatten()
            .any(|dir| self.folders.iter().any(|listed| covers(listed, &dir)));
        covered_folder
            || app.exe.as_ref().is_some_and(|exe| self.exes.contains(exe))
            || app.aumid.as_ref().is_some_and(|id| self.ids.contains(id))
            || self.names.contains(&app.name.trim().to_lowercase())
    }
}

/// The program and app ID a shortcut points at. A shortcut to a Store app
/// has an app ID and no path.
fn resolve_shortcut(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(link) = ComPtr::create(&CLSID_SHELL_LINK, &IID_ISHELL_LINK_W, CLSCTX_INPROC_SERVER)
    else {
        return (None, None);
    };
    let Some(file) = link.query(&IID_IPERSIST_FILE) else {
        return (None, None);
    };
    let loaded = unsafe {
        (file.vtable::<IPersistFileVtbl>().load)(
            file.as_raw(),
            crate::core::wide(&path.to_string_lossy()).as_ptr(),
            0,
        )
    };
    if !com::ok(loaded) {
        return (None, None);
    }
    let mut buffer = [0u16; 1024];
    let got = unsafe {
        (link.vtable::<IShellLinkWVtbl>().get_path)(
            link.as_raw(),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    let exe = if com::ok(got) {
        let len = buffer.iter().position(|unit| *unit == 0).unwrap_or(0);
        let target =
            crate::core::expand_env(&String::from_utf16_lossy(&buffer[..len])).to_lowercase();
        target.ends_with(".exe").then_some(target)
    } else {
        None
    };
    let aumid = link
        .query(&IID_IPROPERTY_STORE)
        .and_then(|store| appid::string_value(&store, &appid::PKEY_APP_USER_MODEL_ID))
        .filter(|id| !id.is_empty())
        .map(|id| id.to_lowercase());
    (exe, aumid)
}

impl SearchProvider for Apps {
    fn id(&self) -> &'static str {
        "apps"
    }

    fn name(&self) -> &'static str {
        "Apps"
    }

    fn description(&self) -> &'static str {
        "Programs, Store apps and installed programs"
    }

    fn blended(&self) -> bool {
        true
    }

    fn scope(&self) -> Option<&'static str> {
        Some("a")
    }

    fn waiting(&self) -> bool {
        !self.ready.load(Ordering::SeqCst)
    }

    fn index_news(&mut self) -> bool {
        self.fresh.swap(false, Ordering::SeqCst)
    }

    /// Every app; the router keeps those picked most.
    fn suggestions(&mut self, _context: &Context) -> Vec<ResultItem> {
        match self.index.lock() {
            Ok(apps) => search(&apps, "", usize::MAX),
            Err(_) => Vec::new(),
        }
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        // Under the "a" scope an empty box lists every app, A to Z.
        if query.text.is_empty() && query.prefix.is_none() {
            return Vec::new();
        }
        let apps = match self.index.lock() {
            Ok(guard) => Arc::clone(&guard),
            Err(_) => return Vec::new(),
        };
        search(&apps, &query.text, query.limit)
    }
}

pub fn search(apps: &[App], text: &str, limit: usize) -> Vec<ResultItem> {
    let mut scored: Vec<(i32, &App)> = apps
        .iter()
        .filter_map(|app| fuzzy::score(text, &app.name).map(|score| (score, app)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored
        .into_iter()
        .take(limit)
        .map(|(score, app)| ResultItem {
            group: "Apps".to_string(),
            title: app.name.clone(),
            subtitle: app.subtitle.clone(),
            icon: IconRef::Path(app.icon.clone()),
            score,
            enter: Some(Choice {
                label: "Open".to_string(),
                action: Action::Open(app.target.clone()),
            }),
            tab: Some(Completion::quiet(&app.name)),
            usage_key: Some(app.usage_key.clone()),
            ..Default::default()
        })
        .collect()
}

/// This user's folder first, so a shortcut in both places is listed once,
/// from the folder the user can edit.
fn start_menu_roots() -> Vec<PathBuf> {
    let programs = Path::new(r"Microsoft\Windows\Start Menu\Programs");
    ["APPDATA", "ProgramData"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join(programs))
        .collect()
}

pub fn scan(roots: &[PathBuf]) -> Vec<App> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        walk(root, root, 0, &mut apps, &mut seen);
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

fn walk(root: &Path, dir: &Path, depth: usize, apps: &mut Vec<App>, seen: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                walk(root, &path, depth + 1, apps, seen);
            }
            continue;
        }
        let Some(app) = app_from(root, &path) else {
            continue;
        };
        if seen.insert(app.name.to_lowercase()) {
            apps.push(app);
        }
    }
}

/// Uninstallers live next to the programs in many Start menu folders and
/// would crowd the results, so they are left out, as Windows search does.
fn app_from(root: &Path, path: &Path) -> Option<App> {
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    if !matches!(extension.as_str(), "lnk" | "url" | "appref-ms") {
        return None;
    }
    let name = path.file_stem()?.to_string_lossy().to_string();
    if name.to_lowercase().starts_with("uninstall") {
        return None;
    }
    let relative = path
        .parent()
        .and_then(|parent| parent.strip_prefix(root).ok())
        .map(|folder| folder.to_string_lossy().to_string())
        .unwrap_or_default();
    let folder = if relative.is_empty() {
        "Start menu".to_string()
    } else {
        format!(r"Start menu\{relative}")
    };
    Some(App {
        name,
        target: path.to_path_buf(),
        icon: path.to_path_buf(),
        subtitle: folder,
        usage_key: path.to_string_lossy().to_lowercase(),
        source: Source::StartMenu,
        exe: None,
        aumid: None,
        install_dir: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, source: Source, exe: Option<&str>, aumid: Option<&str>) -> App {
        let target = PathBuf::from(format!(r"C:\Start\{name}.lnk"));
        App {
            name: name.to_string(),
            icon: target.clone(),
            usage_key: target.to_string_lossy().to_lowercase(),
            target,
            subtitle: "Start menu".to_string(),
            source,
            exe: exe.map(str::to_string),
            aumid: aumid.map(str::to_string),
            install_dir: String::new(),
        }
    }

    fn fake_apps(count: usize) -> Vec<App> {
        let words = [
            "Visual",
            "Studio",
            "Code",
            "Notepad",
            "Paint",
            "Terminal",
            "Office",
            "Word",
            "Excel",
            "Chrome",
            "Firefox",
            "Telegram",
            "Calculator",
            "Photos",
            "Settings",
            "Steam",
        ];
        (0..count)
            .map(|index| {
                let name = format!(
                    "{} {} {index}",
                    words[index % words.len()],
                    words[(index / words.len()) % words.len()]
                );
                app(&name, Source::StartMenu, None, None)
            })
            .collect()
    }

    #[test]
    fn the_best_match_comes_first_and_the_limit_holds() {
        let apps = fake_apps(300);
        let found = search(&apps, "note", 5);
        assert_eq!(found.len(), 5);
        assert!(found.iter().all(|item| item.title.contains("Notepad")));
        assert!(found.windows(2).all(|pair| pair[0].score >= pair[1].score));
    }

    #[test]
    fn searching_two_thousand_apps_takes_under_five_milliseconds() {
        // Debug builds are several times slower and say nothing about what
        // the user will feel.
        if cfg!(debug_assertions) {
            return;
        }
        let apps = fake_apps(2000);
        let started = Instant::now();
        for query in ["v", "vs", "code", "tele", "calc", "zzz"] {
            search(&apps, query, 8);
        }
        let each = started.elapsed() / 6;
        assert!(each < Duration::from_millis(5), "took {each:?} per query");
    }

    #[test]
    fn shortcuts_are_indexed_once_and_uninstallers_are_skipped() {
        let base = std::env::temp_dir().join(format!("wincraft-apps-{}", std::process::id()));
        let user = base.join("user");
        let common = base.join("common");
        std::fs::create_dir_all(user.join("Tools")).unwrap();
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(user.join("Tools").join("Notepad.lnk"), b"").unwrap();
        std::fs::write(common.join("notepad.lnk"), b"").unwrap();
        std::fs::write(common.join("Uninstall Thing.lnk"), b"").unwrap();
        std::fs::write(common.join("readme.txt"), b"").unwrap();

        let apps = scan(&[user.clone(), common]);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Notepad");
        assert_eq!(apps[0].subtitle, r"Start menu\Tools");
        assert_eq!(apps[0].target, user.join("Tools").join("Notepad.lnk"));
        // The key picks were recorded under before other sources existed.
        assert_eq!(
            apps[0].usage_key,
            user.join("Tools")
                .join("Notepad.lnk")
                .to_string_lossy()
                .to_lowercase()
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_program_from_two_sources_is_listed_once_from_the_stronger() {
        let start = vec![app(
            "Notepad",
            Source::StartMenu,
            Some(r"c:\win\notepad.exe"),
            None,
        )];
        let folder = vec![
            // Same program under another name.
            app(
                "Editor",
                Source::AppsFolder,
                Some(r"c:\win\notepad.exe"),
                None,
            ),
            app("Terminal", Source::AppsFolder, None, Some("terminal!app")),
        ];
        let installed = vec![
            // The same Store app, found again by its ID.
            app(
                "Windows Terminal",
                Source::Installed,
                None,
                Some("terminal!app"),
            ),
            app(
                "Lightroom",
                Source::Installed,
                Some(r"d:\lr\lightroom.exe"),
                None,
            ),
        ];
        let merged = merge(start, folder, installed);
        let names: Vec<(&str, Source)> = merged
            .iter()
            .map(|app| (app.name.as_str(), app.source))
            .collect();
        assert_eq!(
            names,
            [
                ("Lightroom", Source::Installed),
                ("Notepad", Source::StartMenu),
                ("Terminal", Source::AppsFolder),
            ]
        );
    }

    #[test]
    fn the_same_program_path_is_one_app() {
        let start = vec![app(
            "Tool",
            Source::StartMenu,
            Some(r"c:\apps\tool.exe"),
            None,
        )];
        let installed = vec![app(
            "Tool 2",
            Source::Installed,
            Some(r"c:\apps\tool.exe"),
            None,
        )];
        assert_eq!(merge(start, Vec::new(), installed).len(), 1);
    }

    #[test]
    fn the_same_name_is_one_app_and_the_stronger_source_keeps_it() {
        let start = vec![app("Telegram", Source::StartMenu, None, None)];
        let folder = vec![app("telegram ", Source::AppsFolder, None, Some("tg!app"))];
        let merged = merge(start, folder, Vec::new());
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].source, Source::StartMenu);
        // Two shortcuts to one program are both kept: the user made both.
        let twins = vec![
            app("A", Source::StartMenu, Some("x.exe"), None),
            app("B", Source::StartMenu, Some("x.exe"), None),
        ];
        assert_eq!(merge(twins, Vec::new(), Vec::new()).len(), 2);
    }

    #[test]
    fn rows_say_where_they_come_from_and_what_enter_opens() {
        let mut store = app("Claude", Source::AppsFolder, None, Some("claude!app"));
        store.target = PathBuf::from(r"shell:AppsFolder\Claude!App");
        store.subtitle = "Store app".to_string();
        let found = search(&[store], "claude", 5);
        assert_eq!(found[0].subtitle, "Store app");
        let enter = found[0].enter.as_ref().unwrap();
        assert_eq!(
            enter.action,
            Action::Open(PathBuf::from(r"shell:AppsFolder\Claude!App"))
        );
    }

    #[test]
    fn an_installed_program_in_the_folder_of_a_listed_one_is_the_same_product() {
        let start = vec![
            app(
                "Word",
                Source::StartMenu,
                Some(r"c:\office\root\winword.exe"),
                None,
            ),
            app(
                "OneDrive",
                Source::StartMenu,
                Some(r"c:\onedrive\onedrive.exe"),
                None,
            ),
        ];
        let folder = vec![app(
            "Installer",
            Source::AppsFolder,
            Some(r"c:\vs\installer\vs_installer.exe"),
            None,
        )];
        // Another file of the same folder, under another name.
        let office = app(
            "Microsoft Office Home and Student 2021",
            Source::Installed,
            Some(r"c:\office\root\officeclicktorun.exe"),
            None,
        );
        // Its own exe is elsewhere, but its install folder is covered.
        let mut onedrive = app(
            "Microsoft OneDrive",
            Source::Installed,
            Some(r"c:\onedrive\26.1\other.exe"),
            None,
        );
        onedrive.install_dir = r"c:\onedrive".to_string();
        let mut vs = app(
            "Microsoft Visual Studio Installer",
            Source::Installed,
            Some(r"c:\vs\installer\feedback_like.exe"),
            None,
        );
        vs.install_dir = r"c:\vs\installer".to_string();
        // Nothing else covers this folder, so it stays.
        let mut lightroom = app(
            "Adobe Lightroom Classic",
            Source::Installed,
            Some(r"d:\adobe\lightroom classic\lightroom.exe"),
            None,
        );
        lightroom.install_dir = r"d:\adobe".to_string();

        let merged = merge(start, folder, vec![office, onedrive, vs, lightroom]);
        let names: Vec<&str> = merged.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(
            names,
            ["Adobe Lightroom Classic", "Installer", "OneDrive", "Word"]
        );
    }

    #[test]
    fn a_version_folder_inside_a_listed_programs_folder_is_covered() {
        let start = vec![app(
            "OneDrive",
            Source::StartMenu,
            Some(r"c:\program files\microsoft onedrive\onedrive.exe"),
            None,
        )];
        let launcher = app(
            "Microsoft OneDrive",
            Source::Installed,
            Some(r"c:\program files\microsoft onedrive\26.1\launcher.exe"),
            None,
        );
        assert_eq!(merge(start, Vec::new(), vec![launcher]).len(), 1);
        // A folder at the top of a drive holds many products and covers none.
        let top = vec![app(
            "Tool",
            Source::StartMenu,
            Some(r"c:\program files	ool.exe"),
            None,
        )];
        let other = app(
            "Other",
            Source::Installed,
            Some(r"c:\program files\other\other.exe"),
            None,
        );
        assert_eq!(merge(top, Vec::new(), vec![other]).len(), 2);
    }

    #[test]
    fn two_installed_programs_do_not_cover_each_other() {
        let a = app("Suite A", Source::Installed, Some(r"c:\suite\a.exe"), None);
        let b = app("Suite B", Source::Installed, Some(r"c:\suite\b.exe"), None);
        assert_eq!(merge(Vec::new(), Vec::new(), vec![a, b]).len(), 2);
    }

    #[test]
    fn a_folder_helps_only_installed_programs() {
        // Two Start menu shortcuts into one folder are both kept, and so is
        // an app-list entry beside a Start menu program.
        let start = vec![app("A", Source::StartMenu, Some(r"c:\x\a.exe"), None)];
        let folder = vec![app("B", Source::AppsFolder, Some(r"c:\x\b.exe"), None)];
        assert_eq!(merge(start, folder, Vec::new()).len(), 2);
    }

    #[test]
    fn a_group_key_is_named_by_the_entry_with_its_app_id() {
        let apps = vec![
            app(
                "Terminal",
                Source::AppsFolder,
                None,
                Some("microsoft.windowsterminal_8wekyb3d8bbwe!app"),
            ),
            app(
                "WulfScreen",
                Source::StartMenu,
                None,
                Some("com.codewulf.wulfscreen"),
            ),
        ];
        // The key is the app ID as the window gave it, in any case.
        assert_eq!(
            name_in(&apps, "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App").as_deref(),
            Some("Terminal")
        );
        assert_eq!(
            name_in(&apps, "Com.Codewulf.WulfScreen").as_deref(),
            Some("WulfScreen")
        );
        assert_eq!(name_in(&apps, "Some.Other.App"), None);
    }

    #[test]
    fn a_path_key_is_named_by_the_entry_with_that_program() {
        let apps = vec![
            app(
                "Lightroom",
                Source::StartMenu,
                Some(r"c:\adobe\lightroom.exe"),
                None,
            ),
            app(
                "Lightroom (installed)",
                Source::Installed,
                Some(r"c:\adobe\lightroom.exe"),
                None,
            ),
            app(
                "Notepad",
                Source::StartMenu,
                Some(r"c:\windows\notepad.exe"),
                None,
            ),
        ];
        assert_eq!(
            name_in(&apps, r"C:\Adobe\Lightroom.exe").as_deref(),
            Some("Lightroom"),
            "the first entry, the stronger source, names it"
        );
        assert_eq!(name_in(&apps, r"c:\other\tool.exe"), None);
        // An app ID never matches a program path or the other way round.
        assert_eq!(name_in(&apps, "lightroom"), None);
    }

    #[test]
    fn nothing_names_an_empty_key_or_an_entry_without_a_name() {
        let apps = vec![app("  ", Source::StartMenu, Some(r"c:\x.exe"), None)];
        assert_eq!(name_in(&apps, ""), None);
        assert_eq!(name_in(&apps, "   "), None);
        assert_eq!(name_in(&apps, r"c:\x.exe"), None);
    }
}

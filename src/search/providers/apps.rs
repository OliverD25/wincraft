use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::search::{
    Action, Choice, Completion, Context, IconRef, Query, ResultItem, SearchProvider,
};
use crate::ui::fuzzy;

const REINDEX_EVERY: Duration = Duration::from_secs(10 * 60);
/// Start menu folders are shallow; this only stops a folder loop.
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    pub shortcut: PathBuf,
    /// Where it sits in the Start menu, for the row's second line.
    pub folder: String,
}

/// Programs from the Start menu shortcuts of this user and of all users.
pub struct Apps {
    index: Arc<Mutex<Arc<Vec<App>>>>,
}

impl Apps {
    /// Starts the indexing thread. Reading the folders takes tens of
    /// milliseconds, too long for the UI thread, and runs again every ten
    /// minutes so newly installed programs show up.
    pub fn new() -> Self {
        let index = Arc::new(Mutex::new(Arc::new(Vec::new())));
        let shared = Arc::clone(&index);
        let started = std::thread::Builder::new()
            .name("wincraft-apps".to_string())
            .spawn(move || loop {
                let began = Instant::now();
                let apps = scan(&start_menu_roots());
                log::info!(
                    "indexed {} Start menu shortcuts in {} ms",
                    apps.len(),
                    began.elapsed().as_millis()
                );
                if let Ok(mut guard) = shared.lock() {
                    *guard = Arc::new(apps);
                }
                std::thread::sleep(REINDEX_EVERY);
            });
        if let Err(err) = started {
            log::warn!("could not start the app index thread: {err}");
        }
        Self { index }
    }
}

impl SearchProvider for Apps {
    fn id(&self) -> &'static str {
        "apps"
    }

    fn name(&self) -> &'static str {
        "Apps"
    }

    fn description(&self) -> &'static str {
        "Programs in the Start menu"
    }

    fn blended(&self) -> bool {
        true
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        if query.text.is_empty() {
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
            subtitle: app.folder.clone(),
            icon: IconRef::Path(app.shortcut.clone()),
            score,
            enter: Some(Choice {
                label: "Open".to_string(),
                action: Action::Open(app.shortcut.clone()),
            }),
            tab: Some(Completion::quiet(&app.name)),
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
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
        shortcut: path.to_path_buf(),
        folder,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
                App {
                    shortcut: PathBuf::from(format!(r"C:\Start\{name}.lnk")),
                    folder: "Start menu".to_string(),
                    name,
                }
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
        assert_eq!(apps[0].folder, r"Start menu\Tools");
        assert_eq!(apps[0].shortcut, user.join("Tools").join("Notepad.lnk"));
        let _ = std::fs::remove_dir_all(&base);
    }
}

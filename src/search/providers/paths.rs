use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;

use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM};

use super::deep::Deep;
use super::drives::Drives;

use crate::search::{
    usage, Action, Choice, Completion, Context, IconRef, Query, ResultItem, SearchProvider,
};
use crate::ui::fuzzy;

/// What the text after the prefix asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Target<'a> {
    /// No folder yet: pick a drive; the text filters the drive list.
    Drives(&'a str),
    /// Everything up to the last backslash is the folder, the rest filters
    /// what is inside it.
    Folder { dir: String, filter: &'a str },
}

/// Forward slashes are accepted and read as backslashes, since typing a path
/// after the `/` prefix makes them the natural thing to reach for.
pub fn target(text: &str) -> Target<'_> {
    match text.rfind(['\\', '/']) {
        None => Target::Drives(text),
        Some(at) => Target::Folder {
            dir: text[..=at].replace('/', "\\"),
            filter: &text[at + 1..],
        },
    }
}

/// What Tab puts in the search box: a folder's path with a closing backslash,
/// so its contents are listed at once, or a file's full path.
pub fn completion(dir: &str, name: &str, is_dir: bool) -> String {
    if is_dir {
        format!("{dir}{name}\\")
    } else {
        format!("{dir}{name}")
    }
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    is_dir: bool,
    size: u64,
}

/// One match in the list: in the folder itself (`depth` 1) or below it.
struct Row {
    score: i32,
    depth: usize,
    /// The folders between the browsed one and the name, with a closing
    /// backslash, or empty.
    parent: String,
    name: String,
    is_dir: bool,
    size: u64,
}

/// Browses drives and folders. The folder's own items are listed at once;
/// typing also finds matches in the folders below it, as they are read.
#[derive(Default)]
pub struct Paths {
    /// The last folder read, so typing a filter does not read it again.
    listed: Option<(String, Vec<Entry>)>,
    drives: Drives,
    deep: Deep,
}

impl SearchProvider for Paths {
    fn id(&self) -> &'static str {
        "paths"
    }

    fn name(&self) -> &'static str {
        "Folders and files"
    }

    fn description(&self) -> &'static str {
        "Browse drives and folders; Tab opens the selected folder"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some("/")
    }

    fn placeholder(&self) -> &'static str {
        "Type a drive, like C:\\ \u{2014} Tab goes into a folder"
    }

    fn needle<'a>(&self, text: &'a str) -> &'a str {
        match target(text) {
            Target::Drives(filter) => filter,
            Target::Folder { filter, .. } => filter,
        }
    }

    fn opened(&mut self) {
        self.listed = None;
        self.deep.reset();
    }

    fn waiting(&self) -> bool {
        self.drives.waiting() || self.deep.waiting()
    }

    /// The folder last listed, so `>` can run a command in it. A typed path
    /// that does not exist is not one.
    fn folder(&self) -> Option<String> {
        self.listed
            .as_ref()
            .map(|(dir, _)| dir.clone())
            .filter(|dir| std::path::Path::new(dir).is_dir())
    }

    fn has_news(&mut self) -> bool {
        let drives = self.drives.take_news();
        let deep = self.deep.take_news();
        drives || deep
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        match target(&query.text) {
            Target::Drives(filter) => self.drives(filter),
            Target::Folder { dir, filter } => self.list_folder(dir, filter, query.limit),
        }
    }
}

impl Paths {
    fn list_folder(&mut self, dir: String, filter: &str, limit: usize) -> Vec<ResultItem> {
        if self.listed.as_ref().map(|(listed, _)| listed) != Some(&dir) {
            self.listed = Some((dir.clone(), read_folder(&dir)));
        }
        let Some((_, entries)) = &self.listed else {
            return Vec::new();
        };
        let group = header(&dir);
        let mut items = Vec::new();
        if filter.is_empty() {
            items.push(ResultItem {
                group: group.clone(),
                title: folder_name(&dir),
                subtitle: "This folder".to_string(),
                icon: IconRef::Path(PathBuf::from(&dir)),
                usage_key: Some(usage::path_key(&dir)),
                enter: open(&dir),
                shift_enter: reveal(&dir),
                tab: Some(Completion {
                    label: "Complete".to_string(),
                    text: dir.clone(),
                }),
                bookmark: bookmark(&dir),
                ..Default::default()
            });
        }
        let mut rows: Vec<Row> = entries
            .iter()
            .filter_map(|entry| {
                let score = if filter.is_empty() {
                    0
                } else {
                    fuzzy::score(filter, &entry.name)?
                };
                Some(Row {
                    score,
                    depth: 1,
                    parent: String::new(),
                    name: entry.name.clone(),
                    is_dir: entry.is_dir,
                    size: entry.size,
                })
            })
            .collect();
        if !filter.is_empty() {
            for found in self.deep.find(&dir, filter) {
                let Some(score) = fuzzy::score(filter, found.name()) else {
                    continue;
                };
                rows.push(Row {
                    // Nearer matches rank a little higher than deeper ones.
                    score: (score - 2 * (found.depth as i32 - 1)).max(1),
                    depth: found.depth,
                    parent: found.parent().to_string(),
                    name: found.name().to_string(),
                    is_dir: found.is_dir,
                    size: found.size,
                });
            }
        }
        rows.sort_by_key(|row| (std::cmp::Reverse(row.score), row.depth));
        items.extend(rows.into_iter().take(limit).map(|row| {
            let place = format!("{dir}{}", row.parent);
            let path = format!("{place}{}", row.name);
            let kind = if row.is_dir {
                "Folder".to_string()
            } else {
                size_text(row.size)
            };
            ResultItem {
                group: group.clone(),
                title: row.name.clone(),
                subtitle: if row.parent.is_empty() {
                    kind
                } else {
                    format!("{kind} \u{b7} in {}", row.parent.trim_end_matches('\\'))
                },
                icon: IconRef::Path(PathBuf::from(&path)),
                score: row.score,
                usage_key: Some(usage::path_key(&path)),
                enter: open(&path),
                shift_enter: reveal(&path),
                tab: Some(Completion {
                    label: if row.is_dir {
                        "Open folder".to_string()
                    } else {
                        "Complete".to_string()
                    },
                    text: completion(&place, &row.name, row.is_dir),
                }),
                bookmark: bookmark(&path),
                ..Default::default()
            }
        }));
        items
    }
}

/// Long folder paths keep their end, the part that tells folders apart.
fn header(dir: &str) -> String {
    const MOST: usize = 56;
    let count = dir.chars().count();
    if count <= MOST {
        return dir.to_string();
    }
    let tail: String = dir.chars().skip(count - (MOST - 1)).collect();
    format!("\u{2026}{tail}")
}

/// "Users" for C:\Users\, and the drive itself for C:\.
fn folder_name(dir: &str) -> String {
    dir.trim_end_matches('\\')
        .rsplit('\\')
        .next()
        .filter(|name| !name.is_empty() && !name.ends_with(':'))
        .map(str::to_string)
        .unwrap_or_else(|| dir.to_string())
}

fn open(path: &str) -> Option<Choice> {
    Some(Choice {
        label: "Open".to_string(),
        action: Action::Open(PathBuf::from(path)),
    })
}

/// Ctrl+B on a folder or file row.
fn bookmark(path: &str) -> Option<Choice> {
    super::bookmarks::choice(path, &folder_name(path))
}

fn reveal(path: &str) -> Option<Choice> {
    Some(Choice {
        label: "Show in folder".to_string(),
        action: Action::Reveal(PathBuf::from(path)),
    })
}

impl Paths {
    /// The title stays the bare root so typing "c:" still matches it; the
    /// label and free space fill the second line once they have been read.
    fn drives(&self, filter: &str) -> Vec<ResultItem> {
        self.drives
            .list()
            .into_iter()
            .filter_map(|(letter, subtitle)| {
                let root = format!("{letter}:\\");
                let score = fuzzy::score(filter, &root)?;
                Some(ResultItem {
                    group: "Drives".to_string(),
                    title: root.clone(),
                    subtitle,
                    icon: IconRef::Path(PathBuf::from(&root)),
                    score,
                    usage_key: Some(usage::path_key(&root)),
                    enter: open(&root),
                    tab: Some(Completion {
                        label: "Open folder".to_string(),
                        text: root.clone(),
                    }),
                    bookmark: bookmark(&root),
                    ..Default::default()
                })
            })
            .collect()
    }
}

/// Folders first, then files, each by name. Hidden and system items are left
/// out, as Explorer does by default.
fn read_folder(dir: &str) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if metadata.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0 {
                return None;
            }
            Some(Entry {
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_a_backslash_filters_the_drives() {
        assert_eq!(target(""), Target::Drives(""));
        assert_eq!(target("c:"), Target::Drives("c:"));
    }

    #[test]
    fn the_folder_ends_at_the_last_backslash() {
        assert_eq!(
            target(r"C:\Users\ad"),
            Target::Folder {
                dir: r"C:\Users\".to_string(),
                filter: "ad"
            }
        );
        assert_eq!(
            target(r"C:\Users\"),
            Target::Folder {
                dir: r"C:\Users\".to_string(),
                filter: ""
            }
        );
    }

    #[test]
    fn forward_slashes_read_as_backslashes() {
        assert_eq!(
            target("c:/users/ad"),
            Target::Folder {
                dir: r"c:\users\".to_string(),
                filter: "ad"
            }
        );
    }

    #[test]
    fn tab_on_a_folder_goes_inside_it_and_on_a_file_completes_it() {
        assert_eq!(completion(r"C:\Users\", "Admin", true), r"C:\Users\Admin\");
        assert_eq!(
            completion(r"C:\Users\Admin\", "notes.txt", false),
            r"C:\Users\Admin\notes.txt"
        );
        let drilled = completion(r"C:\", "Users", true);
        assert_eq!(
            target(&drilled),
            Target::Folder {
                dir: r"C:\Users\".to_string(),
                filter: ""
            }
        );
    }

    #[test]
    fn a_real_folder_lists_folders_before_files() {
        let base = std::env::temp_dir().join(format!("wincraft-paths-{}", std::process::id()));
        std::fs::create_dir_all(base.join("zeta")).unwrap();
        std::fs::write(base.join("alpha.txt"), b"hello").unwrap();
        let dir = format!("{}\\", base.display());

        let mut paths = Paths::default();
        let listed = paths.list_folder(dir.clone(), "", 50);
        let titles: Vec<&str> = listed.iter().map(|item| item.title.as_str()).collect();
        let own_name = base.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(titles, [own_name.as_str(), "zeta", "alpha.txt"]);
        assert_eq!(listed[2].subtitle, "5 bytes");
        assert_eq!(
            listed[1].tab.as_ref().map(|tab| tab.text.clone()),
            Some(format!("{dir}zeta\\"))
        );

        let filtered = paths.list_folder(dir.clone(), "alp", 50);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "alpha.txt");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn typing_after_tab_finds_matches_in_the_subfolders_too() {
        let base = std::env::temp_dir().join(format!("wincraft-paths-deep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("one").join("two")).unwrap();
        std::fs::write(base.join("one").join("two").join("budget.txt"), b"x").unwrap();
        std::fs::write(base.join("budget-old.txt"), b"x").unwrap();
        let dir = format!("{}\\", base.display());

        let mut paths = Paths::default();
        let mut titles = Vec::new();
        for _ in 0..200 {
            let listed = paths.list_folder(dir.clone(), "budget", 50);
            titles = listed.iter().map(|item| item.title.clone()).collect();
            if !paths.waiting() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(titles, ["budget-old.txt", "budget.txt"]);
        let listed = paths.list_folder(dir.clone(), "budget", 50);
        assert_eq!(listed[1].subtitle, "1 bytes \u{b7} in one\\two");
        assert_eq!(
            listed[1].tab.as_ref().map(|tab| tab.text.clone()),
            Some(format!(r"{dir}one\two\budget.txt"))
        );
        assert!(paths.has_news());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_folder_row_is_named_by_its_last_part() {
        assert_eq!(folder_name(r"C:\Users\"), "Users");
        assert_eq!(folder_name(r"C:\"), r"C:\");
    }

    #[test]
    fn a_long_folder_header_keeps_its_end() {
        let long = format!(r"C:\{}\browse\", "x".repeat(80));
        let shown = header(&long);
        assert_eq!(shown.chars().count(), 56);
        assert!(shown.starts_with('\u{2026}') && shown.ends_with(r"\browse\"));
        assert_eq!(header(r"C:\Users\"), r"C:\Users\");
    }

    #[test]
    fn sizes_read_like_explorer() {
        assert_eq!(size_text(512), "512 bytes");
        assert_eq!(size_text(2048), "2.0 KB");
        assert_eq!(size_text(50 * 1024 * 1024), "50 MB");
    }

    #[test]
    fn folders_files_and_drives_can_be_bookmarked_with_ctrl_b() {
        let command = |choice: Option<Choice>| match choice.map(|c| c.action) {
            Some(Action::Provider { provider, command }) => (provider, command),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            command(bookmark(r"D:\Docs\pswds_OneDrive")),
            (
                "bookmarks".to_string(),
                format!("new\t{}\tpswds_OneDrive", r"D:\Docs\pswds_OneDrive")
            )
        );
        assert_eq!(
            command(bookmark(r"D:\Docs\")),
            (
                "bookmarks".to_string(),
                format!("new\t{}\tDocs", r"D:\Docs\")
            )
        );
    }
}

//! The names below a folder, at any depth, for the `/` folder browser.
//!
//! Tab on a folder puts its path in the box, and typing after it should find
//! matches in the subfolders too. Reading every level on the UI thread would
//! freeze the palette, so a thread walks the tree one level at a time (the
//! nearest names first) and the palette matches the typed text against what
//! has been read so far. The walk is capped in time and in names, and never
//! enters hidden or system folders, or junctions and links, so it stays light
//! and cannot loop.

use std::collections::VecDeque;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_SYSTEM,
};

/// Names read before the walk stops. About 5 MB of memory at most.
const MOST_NAMES: usize = 50_000;
/// Time the walk may run before it stops.
const MOST_TIME: Duration = Duration::from_secs(2);
/// The walk hands its names over this often, so matches appear while it runs.
const BATCH: usize = 500;

/// One file or folder below the browsed folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Path from the browsed folder, like `docs\2024\plan.txt`.
    pub rel: String,
    /// Where the name starts in `rel`.
    pub name_at: usize,
    pub is_dir: bool,
    pub size: u64,
    /// 1 for something directly in the folder.
    pub depth: usize,
    lower: String,
}

impl Found {
    pub fn name(&self) -> &str {
        &self.rel[self.name_at..]
    }

    /// The folders between the browsed one and this name, with a closing
    /// backslash, or empty.
    pub fn parent(&self) -> &str {
        &self.rel[..self.name_at]
    }

    /// Whether the typed text is in the name, in any letter case.
    pub fn contains(&self, lower_text: &str) -> bool {
        self.lower.contains(lower_text)
    }
}

pub struct Limits {
    pub names: usize,
    pub time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            names: MOST_NAMES,
            time: MOST_TIME,
        }
    }
}

/// Walks `root` one level at a time, hands the names over in batches, and
/// stops when `keep_going` says no or a limit is reached. Hidden and system
/// items are left out, as Explorer does by default. A junction or link is
/// listed but not entered, so a loop cannot trap the walk.
pub fn walk(
    root: &Path,
    limits: &Limits,
    mut keep_going: impl FnMut() -> bool,
    mut hand_over: impl FnMut(Vec<Found>),
) {
    let started = Instant::now();
    let mut count = 0usize;
    let mut batch: Vec<Found> = Vec::new();
    let mut queue: VecDeque<(PathBuf, String, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), String::new(), 1));
    'levels: while let Some((dir, prefix, depth)) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            if count >= limits.names || started.elapsed() >= limits.time || !keep_going() {
                break 'levels;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let attributes = metadata.file_attributes();
            if attributes & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0 {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
            let rel = format!("{prefix}{name}");
            if is_dir && attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
                queue.push_back((entry.path(), format!("{rel}\\"), depth + 1));
            }
            batch.push(Found {
                name_at: prefix.len(),
                lower: name.to_lowercase(),
                rel,
                is_dir,
                size: metadata.len(),
                depth,
            });
            count += 1;
            if batch.len() >= BATCH {
                hand_over(std::mem::take(&mut batch));
            }
        }
    }
    if !batch.is_empty() {
        hand_over(batch);
    }
}

#[derive(Default)]
struct State {
    /// The folder being walked or already walked, with a closing backslash.
    dir: String,
    /// Bumped to tell an older walk to stop.
    generation: u64,
    found: Vec<Found>,
    running: bool,
    news: bool,
}

/// The names below one folder, read off the UI thread.
#[derive(Default)]
pub struct Deep {
    state: Arc<Mutex<State>>,
}

impl Deep {
    /// Forgets the folder, and stops a walk that is still running.
    pub fn reset(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.generation += 1;
            state.dir.clear();
            state.found.clear();
            state.running = false;
            state.news = false;
        }
    }

    /// The names below `dir` that contain `text`, those below the first level
    /// only (the first level is listed by the caller), nearest first. Starts
    /// the walk when `dir` is not the folder read last.
    pub fn find(&self, dir: &str, text: &str) -> Vec<Found> {
        let lower = text.trim().to_lowercase();
        let Ok(mut state) = self.state.lock() else {
            return Vec::new();
        };
        if state.dir != dir {
            state.generation += 1;
            state.dir = dir.to_string();
            state.found.clear();
            state.running = true;
            state.news = false;
            self.start(dir, state.generation);
        }
        if lower.is_empty() {
            return Vec::new();
        }
        state
            .found
            .iter()
            .filter(|found| found.depth > 1 && found.contains(&lower))
            .cloned()
            .collect()
    }

    /// The walk is still reading.
    pub fn waiting(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.running)
            .unwrap_or(false)
    }

    /// Names arrived since the last call.
    pub fn take_news(&self) -> bool {
        self.state
            .lock()
            .map(|mut state| std::mem::take(&mut state.news))
            .unwrap_or(false)
    }

    fn start(&self, dir: &str, generation: u64) {
        let shared = Arc::clone(&self.state);
        let root = PathBuf::from(dir);
        let started = std::thread::Builder::new()
            .name("wincraft-deep-folder".to_string())
            .spawn(move || {
                let current = |shared: &Mutex<State>| {
                    shared
                        .lock()
                        .map(|state| state.generation == generation)
                        .unwrap_or(false)
                };
                walk(
                    &root,
                    &Limits::default(),
                    || current(&shared),
                    |names| {
                        if let Ok(mut state) = shared.lock() {
                            if state.generation == generation {
                                state.found.extend(names);
                                state.news = true;
                            }
                        }
                    },
                );
                if let Ok(mut state) = shared.lock() {
                    if state.generation == generation {
                        state.running = false;
                        state.news = true;
                    }
                }
            });
        if started.is_err() {
            if let Ok(mut state) = self.state.lock() {
                state.running = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("wincraft-deep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn all(root: &Path, limits: &Limits) -> Vec<Found> {
        let mut names = Vec::new();
        walk(root, limits, || true, |batch| names.extend(batch));
        names
    }

    fn rels(found: &[Found]) -> Vec<&str> {
        found.iter().map(|found| found.rel.as_str()).collect()
    }

    #[test]
    fn a_file_two_levels_down_is_found_and_the_nearest_come_first() {
        let base = scratch("nested");
        std::fs::create_dir_all(base.join("a").join("b")).unwrap();
        std::fs::write(base.join("top.txt"), b"1").unwrap();
        std::fs::write(base.join("a").join("mid.txt"), b"22").unwrap();
        std::fs::write(base.join("a").join("b").join("deep.txt"), b"333").unwrap();

        let found = all(&base, &Limits::default());
        let depths: Vec<usize> = found.iter().map(|found| found.depth).collect();
        assert!(depths.windows(2).all(|pair| pair[0] <= pair[1]));
        let deep = found
            .iter()
            .find(|found| found.name() == "deep.txt")
            .unwrap();
        assert_eq!(deep.rel, r"a\b\deep.txt");
        assert_eq!(deep.parent(), r"a\b\");
        assert_eq!((deep.depth, deep.is_dir, deep.size), (3, false, 3));
        assert!(deep.contains("deep"));
        assert!(rels(&found).contains(&"a"));
        assert!(
            !rels(&found).contains(&""),
            "the folder is not its own result"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hidden_and_system_items_are_left_out() {
        let base = scratch("hidden");
        std::fs::create_dir_all(base.join("secret").join("inner")).unwrap();
        std::fs::write(base.join("secret").join("inner").join("x.txt"), b"x").unwrap();
        std::fs::create_dir_all(base.join("shown")).unwrap();
        std::fs::write(base.join("shown").join("y.txt"), b"y").unwrap();
        let hide = std::process::Command::new("attrib")
            .arg("+h")
            .arg(base.join("secret"))
            .status()
            .unwrap();
        assert!(hide.success());

        let found = all(&base, &Limits::default());
        assert_eq!(rels(&found), ["shown", r"shown\y.txt"]);
        let _ = std::process::Command::new("attrib")
            .arg("-h")
            .arg(base.join("secret"))
            .status();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_junction_loop_is_listed_but_not_followed() {
        let base = scratch("loop");
        std::fs::create_dir_all(base.join("real")).unwrap();
        std::fs::write(base.join("real").join("file.txt"), b"x").unwrap();
        let made = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(base.join("real").join("back"))
            .arg(&base)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");

        let found = all(&base, &Limits::default());
        let mut names = rels(&found);
        names.sort_unstable();
        assert_eq!(names, ["real", r"real\back", r"real\file.txt"]);
        let back = found
            .iter()
            .find(|found| found.rel == r"real\back")
            .unwrap();
        assert!(back.is_dir);
        // Removing the junction first leaves the target alone.
        let _ = std::fs::remove_dir(base.join("real").join("back"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_cap_on_names_stops_the_walk() {
        let base = scratch("cap");
        for index in 0..20 {
            std::fs::write(base.join(format!("file{index}.txt")), b"x").unwrap();
        }
        let limits = Limits {
            names: 5,
            time: Duration::from_secs(60),
        };
        assert_eq!(all(&base, &limits).len(), 5);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_cap_on_time_stops_the_walk() {
        let base = scratch("time");
        std::fs::write(base.join("a.txt"), b"x").unwrap();
        let limits = Limits {
            names: 1000,
            time: Duration::ZERO,
        };
        assert!(all(&base, &limits).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_walk_that_is_told_to_stop_stops() {
        let base = scratch("stop");
        for index in 0..10 {
            std::fs::write(base.join(format!("file{index}.txt")), b"x").unwrap();
        }
        let mut asked = 0;
        let mut names = Vec::new();
        walk(
            &base,
            &Limits::default(),
            || {
                asked += 1;
                asked <= 3
            },
            |batch| names.extend(batch),
        );
        assert_eq!(names.len(), 3);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_missing_folder_gives_nothing() {
        assert!(all(Path::new(r"Z:\no\such\folder"), &Limits::default()).is_empty());
    }

    #[test]
    fn deep_finds_what_is_below_the_first_level_once_the_walk_is_done() {
        let base = scratch("find");
        std::fs::create_dir_all(base.join("one").join("two")).unwrap();
        std::fs::write(base.join("report.txt"), b"x").unwrap();
        std::fs::write(base.join("one").join("two").join("Report-final.txt"), b"x").unwrap();
        let dir = format!("{}\\", base.display());

        let deep = Deep::default();
        let mut hits = deep.find(&dir, "report");
        for _ in 0..200 {
            if !deep.waiting() {
                hits = deep.find(&dir, "report");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(deep.take_news());
        assert!(!deep.take_news());
        assert_eq!(rels(&hits), [r"one\two\Report-final.txt"]);
        assert!(deep.find(&dir, "").is_empty());

        deep.reset();
        assert!(!deep.waiting());
        let _ = std::fs::remove_dir_all(&base);
    }
}

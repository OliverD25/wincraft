//! Saved names and how they find their window again after WinCraft or the
//! app restarts. Pure, so every rule can be tested on made-up windows.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::window_identity::page_title;
use crate::core::{config, taskbar};

pub type Rect = [i32; 4];

pub const VERSION: u32 = 1;
/// A name that has not been on any window for this long is forgotten.
pub const KEEP_SECONDS: u64 = 30 * 24 * 60 * 60;

/// One saved name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    /// The window's taskbar group: its AppUserModelID, or its program's
    /// path in lower case (`appid::group_key`).
    pub group: String,
    /// The app's own title: at first the one it had when it was renamed,
    /// later the latest one the app set while the name was on, since that
    /// is the title the app shows when it opens the window again (Chrome
    /// restores the tab that was in front).
    pub app_title: String,
    pub rect: Rect,
    /// Unix seconds.
    pub created: u64,
    pub last_used: u64,
}

#[derive(Serialize, Deserialize)]
struct NamesFile {
    version: u32,
    rules: Vec<Rule>,
}

pub fn path() -> PathBuf {
    config::plugins_dir().join("window_namer.names.json")
}

pub fn parse(text: &str) -> Result<Vec<Rule>, String> {
    serde_json::from_str::<NamesFile>(config::strip_bom(text))
        .map(|file| file.rules)
        .map_err(|err| err.to_string())
}

pub fn to_json(rules: &[Rule]) -> String {
    let file = NamesFile {
        version: VERSION,
        rules: rules.to_vec(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// No file means no names yet. A file that cannot be read is logged and
/// treated as empty; the next save replaces it.
pub fn load(path: &Path) -> Vec<Rule> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    match parse(&text) {
        Ok(rules) => rules,
        Err(err) => {
            log::warn!(
                "{} cannot be read ({err}); starting with no saved names, the next save replaces it",
                path.display()
            );
            Vec::new()
        }
    }
}

/// Drops the names unused for `KEEP_SECONDS`; returns how many went.
pub fn expire(rules: &mut Vec<Rule>, now: u64) -> usize {
    let before = rules.len();
    rules.retain(|rule| now.saturating_sub(rule.last_used) <= KEEP_SECONDS);
    before - rules.len()
}

/// A Store app's program path holds its version, and every update of the
/// app moves it: `...\windowsapps\claude_4.4212.0.0_x64__pzs8sxrjxfjjc\app\claude.exe`.
/// A name keyed by that path is lost on the next update, so the key keeps
/// only the package family and the path inside the package:
/// `windowsapps\claude_pzs8sxrjxfjjc\app\claude.exe`. Other keys stay as
/// they are.
pub fn stable_group(group: &str) -> String {
    let lower = group.to_lowercase();
    let marker = r"\windowsapps\";
    let Some(at) = lower.find(marker) else {
        return group.to_string();
    };
    let rest = &lower[at + marker.len()..];
    let (folder, inside) = rest.split_once('\\').unwrap_or((rest, ""));
    match taskbar::package_family(&format!(r"windowsapps\{folder}")) {
        Some(family) => format!(r"windowsapps\{family}\{inside}"),
        None => group.to_string(),
    }
}

/// Whether a name belongs to what the window shows, not to the window. A
/// Claude window shows one chat and its title is that chat's, so a name
/// given to it is the chat's name: when the window shows another chat the
/// name goes, and it comes back on the window that shows its chat again.
/// In other apps the name stays on the window: a Chrome window keeps its
/// name on every tab change.
pub fn follows_title(group: &str) -> bool {
    crate::core::claude_waits::is_claude_app(group)
}

/// Brings saved names written by older versions up to date: Store app keys
/// lose their version (`stable_group`), and of two names that want the same
/// window (same app, same title) only the newer one stays, since the two
/// would block each other and neither would come back. Returns how many
/// names changed or went.
pub fn tidy(rules: &mut Vec<Rule>) -> usize {
    let mut changed = 0;
    for rule in rules.iter_mut() {
        let group = stable_group(&rule.group);
        if group != rule.group {
            rule.group = group;
            changed += 1;
        }
    }
    let newest = |rule: &Rule| rule.created.max(rule.last_used);
    let mut keep = vec![true; rules.len()];
    for a in 0..rules.len() {
        for b in 0..rules.len() {
            let same = a != b
                && rules[a].group == rules[b].group
                && rules[a].app_title == rules[b].app_title;
            if same && (newest(&rules[b]), b) > (newest(&rules[a]), a) {
                keep[a] = false;
            }
        }
    }
    let mut at = 0;
    rules.retain(|_| {
        at += 1;
        keep[at - 1]
    });
    changed + keep.iter().filter(|kept| !**kept).count()
}

/// What a renamed window's title bar says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TitleStyle {
    #[default]
    NameOnly,
    NameAndApp,
}

impl TitleStyle {
    pub const CHOICES: &'static [&'static str] = &["Name only", "Name \u{2014} app title"];

    pub fn from_setting(value: Option<&str>) -> Self {
        if value == Some(Self::CHOICES[1]) {
            TitleStyle::NameAndApp
        } else {
            TitleStyle::NameOnly
        }
    }

    pub fn title(self, custom: &str, app_title: &str) -> String {
        match self {
            TitleStyle::NameAndApp if !app_title.is_empty() => {
                format!("{custom} \u{2014} {app_title}")
            }
            _ => custom.to_string(),
        }
    }

    /// The app's own part of a title that changed while the name was on.
    /// An app that edits the title it finds, adding a "*" for unsaved
    /// changes, edits ours, and the name must not pile up in front.
    pub fn app_part<'a>(self, custom: &str, title: &'a str) -> &'a str {
        match self {
            TitleStyle::NameAndApp => title
                .strip_prefix(custom)
                .and_then(|rest| rest.strip_prefix(" \u{2014} "))
                .unwrap_or(title),
            TitleStyle::NameOnly => title,
        }
    }
}

/// A top-level window that a saved name might belong to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub hwnd: isize,
    pub group: String,
    /// The app's own title.
    pub title: String,
    pub rect: Rect,
    /// It already carries a name.
    pub named: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleState {
    /// Waiting for its window.
    Free,
    /// It named a window earlier in this session and that window closed.
    /// Only a title can bring it back: the next window of the same app is
    /// not the one that closed.
    Rested,
    /// It names a window now.
    Bound,
}

pub struct Wanted<'a> {
    pub rule: &'a Rule,
    pub state: RuleState,
}

/// Why a name went to a window, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The window has the app title the name was saved with, or still shows
    /// the name itself because WinCraft stopped without putting the app's
    /// title back.
    Title,
    /// A browser window shows the same page, whatever the product name.
    Page,
    /// It is the one window of its app, and the app has one saved name.
    Lone,
}

type Test<'a> = &'a dyn Fn(&Rule, &Window) -> bool;

/// Pairs saved names with windows as (rule index, window index, reason),
/// strongest evidence first. A name goes to at most one window and a window
/// gets at most one name. Two windows that fit a name equally, or two names
/// that fit one window, are a guess, and a guess is left alone: a missing
/// name is better than a wrong one. `lone` allows the last pass.
pub fn match_rules(
    wanted: &[Wanted],
    windows: &[Window],
    lone: bool,
) -> Vec<(usize, usize, Reason)> {
    let mut rule_used: Vec<bool> = wanted
        .iter()
        .map(|entry| entry.state == RuleState::Bound)
        .collect();
    let mut window_used: Vec<bool> = windows.iter().map(|window| window.named).collect();
    let mut pairs = Vec::new();

    let passes: [(Reason, Test); 2] = [
        (Reason::Title, &|rule, window| {
            window.title == rule.app_title || window.title == rule.name
        }),
        (Reason::Page, &|rule, window| {
            page_title(&rule.app_title).is_some_and(|page| page_title(&window.title) == Some(page))
        }),
    ];
    for (reason, test) in passes {
        for index in 0..wanted.len() {
            if rule_used[index] {
                continue;
            }
            let rule = wanted[index].rule;
            let fits = |rule: &Rule, at: usize| {
                !window_used[at] && windows[at].group == rule.group && test(rule, &windows[at])
            };
            let found: Vec<usize> = (0..windows.len()).filter(|at| fits(rule, *at)).collect();
            let rival = (0..wanted.len()).any(|other| {
                other != index
                    && !rule_used[other]
                    && found.iter().any(|at| fits(wanted[other].rule, *at))
            });
            if rival {
                continue;
            }
            let pick = match found.as_slice() {
                [only] => Some(*only),
                many => {
                    let same_place: Vec<usize> = many
                        .iter()
                        .copied()
                        .filter(|at| windows[*at].rect == rule.rect)
                        .collect();
                    (same_place.len() == 1).then(|| same_place[0])
                }
            };
            if let Some(at) = pick {
                rule_used[index] = true;
                window_used[at] = true;
                pairs.push((index, at, reason));
            }
        }
    }

    if lone {
        for index in 0..wanted.len() {
            if rule_used[index] || wanted[index].state != RuleState::Free {
                continue;
            }
            let group = &wanted[index].rule.group;
            let names_here = wanted
                .iter()
                .filter(|entry| &entry.rule.group == group)
                .count();
            let windows_here: Vec<usize> = (0..windows.len())
                .filter(|at| &windows[*at].group == group)
                .collect();
            if let ([at], 1) = (windows_here.as_slice(), names_here) {
                if !window_used[*at] {
                    rule_used[index] = true;
                    window_used[*at] = true;
                    pairs.push((index, *at, Reason::Lone));
                }
            }
        }
    }

    pairs.sort_unstable_by_key(|(index, _, _)| *index);
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTEPAD: &str = r"c:\windows\system32\notepad.exe";
    const CHROME: &str = "Chrome.UserData.Profile3";
    const FULL: Rect = [-11, -11, 3851, 2099];

    fn rule(name: &str, group: &str, app_title: &str) -> Rule {
        Rule {
            name: name.to_string(),
            group: group.to_string(),
            app_title: app_title.to_string(),
            rect: FULL,
            created: 1_000,
            last_used: 1_000,
        }
    }

    fn window(hwnd: isize, group: &str, title: &str) -> Window {
        Window {
            hwnd,
            group: group.to_string(),
            title: title.to_string(),
            rect: FULL,
            named: false,
        }
    }

    fn free(rules: &[Rule]) -> Vec<Wanted<'_>> {
        rules
            .iter()
            .map(|rule| Wanted {
                rule,
                state: RuleState::Free,
            })
            .collect()
    }

    const CLAUDE_OLD: &str = r"d:\windowsapps\claude_4.4212.0.0_x64__pzs8sxrjxfjjc\app\claude.exe";
    const CLAUDE_NEW: &str =
        r"c:\program files\windowsapps\claude_4.16336.0.0_x64__pzs8sxrjxfjjc\app\claude.exe";

    #[test]
    fn only_a_claude_windows_name_follows_its_title() {
        assert!(follows_title(
            r"windowsapps\claude_pzs8sxrjxfjjc\app\claude.exe"
        ));
        assert!(follows_title(CLAUDE_OLD));
        assert!(!follows_title(CHROME));
        assert!(!follows_title(NOTEPAD));
        assert!(!follows_title(
            r"windowsapps\microsoft.windowsterminal_8wekyb3d8bbwe\windowsterminal.exe"
        ));
    }

    #[test]
    fn a_store_app_keeps_its_key_across_an_update() {
        let key = stable_group(CLAUDE_OLD);
        assert_eq!(key, r"windowsapps\claude_pzs8sxrjxfjjc\app\claude.exe");
        assert_eq!(stable_group(CLAUDE_NEW), key);
        assert_eq!(stable_group(&key), key, "safe to run twice");
    }

    #[test]
    fn keys_that_are_not_store_paths_stay_as_they_are() {
        for key in [
            NOTEPAD,
            CHROME,
            "Claude_pzs8sxrjxfjjc!Claude",
            r"d:\windowsapps\odd\app.exe",
            r"d:\windowsapps",
        ] {
            assert_eq!(stable_group(key), key);
        }
    }

    #[test]
    fn a_name_saved_before_an_app_update_finds_the_updated_window() {
        let mut rules = vec![rule("F1-Desk", CLAUDE_OLD, "UNICOMOS - ECOM")];
        assert_eq!(tidy(&mut rules), 1);
        let windows = [window(7, &stable_group(CLAUDE_NEW), "UNICOMOS - ECOM")];
        let pairs = match_rules(&free(&rules), &windows, false);
        assert_eq!(pairs, vec![(0, 0, Reason::Title)]);
    }

    #[test]
    fn of_two_names_for_one_window_the_newer_stays() {
        let mut old = rule("F1-Desk", CLAUDE_OLD, "UNICOMOS - ECOM");
        old.last_used = 5_000;
        let mut new = rule("F-1 Desk", CLAUDE_NEW, "UNICOMOS - ECOM");
        new.created = 9_000;
        let other = rule("NewsHQ", CLAUDE_OLD, "Law news");
        let mut rules = vec![old, new, other];
        assert_eq!(tidy(&mut rules), 4, "three keys moved, one name went");
        let names: Vec<&str> = rules.iter().map(|rule| rule.name.as_str()).collect();
        assert_eq!(names, ["F-1 Desk", "NewsHQ"]);
        assert_eq!(tidy(&mut rules), 0, "safe to run twice");
    }

    #[test]
    fn names_with_equal_times_for_one_window_keep_exactly_one() {
        let mut rules = vec![rule("A", NOTEPAD, "x"), rule("B", NOTEPAD, "x")];
        assert_eq!(tidy(&mut rules), 1);
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn the_exact_app_title_in_the_same_app_finds_the_window() {
        let rules = [rule("Todo", NOTEPAD, "todo.txt - Notepad")];
        let windows = [
            window(1, NOTEPAD, "notes.txt - Notepad"),
            window(2, NOTEPAD, "todo.txt - Notepad"),
            window(3, r"c:\apps\other.exe", "todo.txt - Notepad"),
        ];
        assert_eq!(
            match_rules(&free(&rules), &windows, true),
            vec![(0, 1, Reason::Title)]
        );
    }

    #[test]
    fn another_app_with_the_same_title_never_matches() {
        let rules = [rule("Todo", NOTEPAD, "todo.txt")];
        let windows = [window(1, r"c:\apps\editor.exe", "todo.txt")];
        assert!(match_rules(&free(&rules), &windows, false).is_empty());
    }

    #[test]
    fn a_window_that_still_shows_the_name_is_found_by_it() {
        // WinCraft stopped without putting the app's title back.
        let rules = [rule("Todo", NOTEPAD, "todo.txt - Notepad")];
        let windows = [window(1, NOTEPAD, "Todo"), window(2, NOTEPAD, "other")];
        assert_eq!(
            match_rules(&free(&rules), &windows, false),
            vec![(0, 0, Reason::Title)]
        );
    }

    #[test]
    fn a_chrome_window_with_a_chrome_name_matches_by_that_name() {
        // Chrome shows the name given in "Name window..." as the whole title.
        let rules = [rule("Research", CHROME, "Thesis")];
        let windows = [
            window(1, CHROME, "Inbox - Gmail - Google Chrome"),
            window(2, CHROME, "Thesis"),
        ];
        assert_eq!(
            match_rules(&free(&rules), &windows, false),
            vec![(0, 1, Reason::Title)]
        );
        assert_eq!(page_title("Thesis"), None);
    }

    #[test]
    fn a_browser_page_matches_whatever_spelling_of_the_product_name() {
        let edge = "MSEdge";
        let rules = [rule("Docs", edge, "Handbook - Microsoft Edge")];
        let windows = [window(1, edge, "Handbook - Microsoft\u{200b} Edge")];
        assert_eq!(
            match_rules(&free(&rules), &windows, false),
            vec![(0, 0, Reason::Page)]
        );
        assert_eq!(
            page_title("Inbox - Gmail - Google Chrome"),
            Some("Inbox - Gmail")
        );
    }

    #[test]
    fn the_only_window_of_an_app_with_one_saved_name_gets_it() {
        let rules = [rule("Chats", "Telegram.TelegramDesktop", "Saved Messages")];
        let windows = [
            window(1, "Telegram.TelegramDesktop", "Family"),
            window(2, NOTEPAD, "x"),
        ];
        assert_eq!(
            match_rules(&free(&rules), &windows, true),
            vec![(0, 0, Reason::Lone)]
        );
        // Not when the last pass is off, as in a test instance.
        assert!(match_rules(&free(&rules), &windows, false).is_empty());
    }

    #[test]
    fn two_windows_or_two_names_of_one_app_are_not_guessed() {
        let one = [rule("Chats", "Telegram", "Saved Messages")];
        let two_windows = [window(1, "Telegram", "A"), window(2, "Telegram", "B")];
        assert!(match_rules(&free(&one), &two_windows, true).is_empty());

        let two = [
            rule("Chats", "Telegram", "Saved Messages"),
            rule("Work", "Telegram", "Work chat"),
        ];
        let one_window = [window(1, "Telegram", "Family")];
        assert!(match_rules(&free(&two), &one_window, true).is_empty());

        // A window that already has a name still counts as a window.
        let mut named = window(2, "Telegram", "B");
        named.named = true;
        let windows = [window(1, "Telegram", "A"), named];
        assert!(match_rules(&free(&one), &windows, true).is_empty());
    }

    #[test]
    fn a_name_that_closed_this_session_comes_back_only_by_title() {
        let rules = [rule("Todo", NOTEPAD, "todo.txt - Notepad")];
        let rested = [Wanted {
            rule: &rules[0],
            state: RuleState::Rested,
        }];
        let other = [window(1, NOTEPAD, "notes.txt - Notepad")];
        assert!(match_rules(&rested, &other, true).is_empty());
        let same = [window(2, NOTEPAD, "todo.txt - Notepad")];
        assert_eq!(
            match_rules(&rested, &same, true),
            vec![(0, 0, Reason::Title)]
        );
    }

    #[test]
    fn two_windows_with_the_title_are_told_apart_by_place_or_left_alone() {
        let mut left = window(1, NOTEPAD, "Untitled - Notepad");
        left.rect = [0, 0, 800, 600];
        let mut right = window(2, NOTEPAD, "Untitled - Notepad");
        right.rect = [800, 0, 1600, 600];
        // Saved maximized, and neither window is there now.
        let rules = [rule("Left", NOTEPAD, "Untitled - Notepad")];
        assert!(match_rules(&free(&rules), &[left.clone(), right.clone()], false).is_empty());

        let mut placed = rule("Left", NOTEPAD, "Untitled - Notepad");
        placed.rect = [0, 0, 800, 600];
        assert_eq!(
            match_rules(&free(&[placed]), &[right, left], false),
            vec![(0, 1, Reason::Title)]
        );
    }

    #[test]
    fn two_names_that_want_one_window_are_left_alone() {
        let rules = [
            rule("A", NOTEPAD, "Untitled - Notepad"),
            rule("B", NOTEPAD, "Untitled - Notepad"),
        ];
        let windows = [window(1, NOTEPAD, "Untitled - Notepad")];
        assert!(match_rules(&free(&rules), &windows, true).is_empty());
    }

    #[test]
    fn one_name_one_window_and_bound_names_are_skipped() {
        let rules = [
            rule("A", NOTEPAD, "a.txt - Notepad"),
            rule("B", NOTEPAD, "b.txt - Notepad"),
        ];
        let windows = [
            window(1, NOTEPAD, "b.txt - Notepad"),
            window(2, NOTEPAD, "a.txt - Notepad"),
        ];
        assert_eq!(
            match_rules(&free(&rules), &windows, true),
            vec![(0, 1, Reason::Title), (1, 0, Reason::Title)]
        );
        let bound = [
            Wanted {
                rule: &rules[0],
                state: RuleState::Bound,
            },
            Wanted {
                rule: &rules[1],
                state: RuleState::Free,
            },
        ];
        assert_eq!(
            match_rules(&bound, &windows, true),
            vec![(1, 0, Reason::Title)]
        );
    }

    #[test]
    fn names_unused_for_thirty_days_are_dropped() {
        let mut fresh = rule("Fresh", NOTEPAD, "a");
        fresh.last_used = 10 * 24 * 60 * 60;
        let mut old = rule("Old", NOTEPAD, "b");
        old.last_used = 0;
        let mut rules = vec![fresh, old];
        let now = KEEP_SECONDS + 1;
        assert_eq!(expire(&mut rules, now), 1);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "Fresh");
        // Exactly thirty days is kept.
        let mut edge = vec![rule("Edge", NOTEPAD, "c")];
        edge[0].last_used = 5;
        assert_eq!(expire(&mut edge, KEEP_SECONDS + 5), 0);
    }

    #[test]
    fn the_file_round_trips_and_a_bad_one_reads_as_empty() {
        let rules = vec![
            rule("Todo", NOTEPAD, "todo.txt - Notepad"),
            rule("Карточки", CHROME, "Inbox \u{2014} Gmail - Google Chrome"),
        ];
        let text = to_json(&rules);
        assert!(text.contains("\"version\": 1"), "{text}");
        assert_eq!(parse(&text).unwrap(), rules);
        assert_eq!(parse(&format!("\u{feff}{text}")).unwrap(), rules);
        assert!(parse("{ not json").is_err());
        assert!(parse("{\"version\": 1}").is_err());

        let dir = std::env::temp_dir().join(format!("wincraft-namer-test-{}", std::process::id()));
        let file = dir.join("names.json");
        assert!(load(&file).is_empty());
        config::write_atomic(&file, &to_json(&rules)).unwrap();
        assert_eq!(load(&file), rules);
        fs::write(&file, "{ broken").unwrap();
        assert!(load(&file).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_title_shows_the_name_alone_or_with_the_app_title() {
        assert_eq!(TitleStyle::from_setting(None), TitleStyle::NameOnly);
        assert_eq!(
            TitleStyle::from_setting(Some("Name only")),
            TitleStyle::NameOnly
        );
        assert_eq!(
            TitleStyle::from_setting(Some("Name \u{2014} app title")),
            TitleStyle::NameAndApp
        );
        assert_eq!(TitleStyle::NameOnly.title("Mail", "Inbox"), "Mail");
        assert_eq!(
            TitleStyle::NameAndApp.title("Mail", "Inbox"),
            "Mail \u{2014} Inbox"
        );
        assert_eq!(TitleStyle::NameAndApp.title("Mail", ""), "Mail");
    }

    #[test]
    fn an_app_that_edits_our_title_does_not_pile_the_name_up() {
        let style = TitleStyle::NameAndApp;
        assert_eq!(style.app_part("Mail", "Mail \u{2014} Inbox*"), "Inbox*");
        assert_eq!(style.app_part("Mail", "Calendar"), "Calendar");
        assert_eq!(style.app_part("Mail", "Mailbox"), "Mailbox");
        assert_eq!(TitleStyle::NameOnly.app_part("Mail", "Mail*"), "Mail*");
    }
}

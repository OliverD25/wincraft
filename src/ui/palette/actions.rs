//! What the selected row can do, as the inspector lists it and the footer
//! hints it, the marked windows, and the "Try instead" rows. No drawing here,
//! so all of it is tested without a window.

use crate::core::config::SearchConfig;
use crate::search::providers::web;
use crate::search::{ask_ai, Action, Choice, Glyph, IconRef, ResultItem};

/// Below this monitor width, in logical pixels, the inspector does not fit
/// beside the list and opens over it instead.
pub const NARROW_BELOW: f32 = 1100.0;

pub fn is_narrow(monitor_width: f32) -> bool {
    monitor_width < NARROW_BELOW
}

/// The keys an action can be run with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    ShiftEnter,
    CtrlEnter,
    Tab,
    CtrlB,
    CtrlDelete,
    CtrlK,
    CtrlC,
    ShiftRight,
    CtrlW,
    CtrlM,
    CtrlSpace,
}

impl Key {
    /// As the keycaps spell it. The arrows only exist in the monospace font.
    pub fn caps(self) -> &'static str {
        match self {
            Key::Enter => "\u{21B5}",
            Key::ShiftEnter => "Shift+\u{21B5}",
            Key::CtrlEnter => "Ctrl+\u{21B5}",
            Key::Tab => "Tab",
            Key::CtrlB => "Ctrl+B",
            Key::CtrlDelete => "Ctrl+Del",
            Key::CtrlK => "Ctrl+K",
            Key::CtrlC => "Ctrl+C",
            Key::ShiftRight => "Shift+\u{2192}",
            Key::CtrlW => "Ctrl+W",
            Key::CtrlM => "Ctrl+M",
            Key::CtrlSpace => "Ctrl+Space",
        }
    }
}

/// What running an action does.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Action(Action),
    /// Tab's completion: the text goes into the box.
    Complete(String),
    /// "Forget this pick".
    Forget,
    /// Marks or unmarks the window.
    Mark,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowAction {
    pub key: Key,
    /// The inspector's words: "Move to next monitor".
    pub label: String,
    /// The footer's: "Next monitor".
    pub short: String,
    pub run: Run,
    /// Listed in the inspector; Mark is only hinted in the footer.
    pub listed: bool,
    /// Hinted in the footer when there is room.
    pub hinted: bool,
}

impl RowAction {
    fn new(key: Key, label: &str, short: &str, run: Run) -> Self {
        Self {
            key,
            label: label.to_string(),
            short: short.to_string(),
            run,
            listed: true,
            hinted: true,
        }
    }

    fn from_choice(key: Key, choice: &Choice) -> Self {
        let short = choice.label.trim_end_matches('\u{2026}');
        Self::new(
            key,
            &choice.label,
            short,
            Run::Action(choice.action.clone()),
        )
    }

    fn quiet(mut self) -> Self {
        self.hinted = false;
        self
    }

    fn footer_only(mut self) -> Self {
        self.listed = false;
        self
    }
}

/// What else decides a row's actions besides the row itself.
#[derive(Clone, Debug, Default)]
pub struct Situation {
    /// The row's Tab action, when its provider says it can run now.
    pub tab_action: Option<Choice>,
    /// The row is the best match from the picks.
    pub best_match: bool,
    /// The row's window is marked.
    pub marked: bool,
}

/// Every action of one row, in the order the inspector lists them; the
/// first is what Enter does.
pub fn row_actions(item: &ResultItem, situation: &Situation) -> Vec<RowAction> {
    let mut actions = Vec::new();
    let window = item.window();
    if let Some(hwnd) = window {
        actions.push(RowAction::new(
            Key::Enter,
            "Switch to window",
            "Switch",
            Run::Action(Action::Activate(hwnd)),
        ));
        actions.push(RowAction::new(
            Key::ShiftRight,
            "Move to next monitor",
            "Next monitor",
            Run::Action(Action::NextMonitor(vec![hwnd])),
        ));
        if let Some(choice) = &item.ctrl_enter {
            actions.push(RowAction::new(
                Key::CtrlEnter,
                &choice.label,
                "Arrange",
                Run::Action(choice.action.clone()),
            ));
        }
        actions.push(
            RowAction::new(
                Key::CtrlW,
                "Close window",
                "Close",
                Run::Action(Action::Close(vec![hwnd])),
            )
            .quiet(),
        );
        let mark = if situation.marked { "Unmark" } else { "Mark" };
        actions.push(RowAction::new(Key::CtrlSpace, mark, mark, Run::Mark).footer_only());
    } else {
        if let Some(choice) = &item.enter {
            actions.push(RowAction::from_choice(Key::Enter, choice));
        }
        if item.answer {
            if let Some(choice) = &item.copy {
                actions.push(RowAction::from_choice(Key::CtrlC, choice));
            }
        }
        if let Some(choice) = &item.shift_enter {
            actions.push(RowAction::from_choice(Key::ShiftEnter, choice));
        }
        if let Some(choice) = &item.ctrl_enter {
            actions.push(RowAction::from_choice(Key::CtrlEnter, choice));
        }
    }
    if let Some(choice) = &situation.tab_action {
        actions.push(RowAction::from_choice(Key::Tab, choice));
    } else if let Some(tab) = item.tab.as_ref().filter(|tab| !tab.label.is_empty()) {
        let action = RowAction::new(
            Key::Tab,
            &tab.label,
            &tab.label,
            Run::Complete(tab.text.clone()),
        );
        // An answer's footer keeps to its copy and paste keys.
        actions.push(if item.answer { action.quiet() } else { action });
    }
    if let Some(choice) = &item.bookmark {
        actions.push(RowAction::from_choice(Key::CtrlB, choice));
    }
    if let Some(choice) = &item.delete {
        actions.push(RowAction::from_choice(Key::CtrlDelete, choice));
    } else if situation.best_match && item.usage_key.is_some() {
        actions.push(
            RowAction::new(Key::CtrlDelete, "Forget this pick", "Forget", Run::Forget).quiet(),
        );
    }
    if let Some(choice) = &item.shortcut {
        actions.push(RowAction::from_choice(Key::CtrlK, choice).quiet());
    }
    if !item.answer {
        if let Some(choice) = &item.copy {
            actions.push(RowAction::from_choice(Key::CtrlC, choice));
        }
    }
    actions
}

/// What the marked windows can do together; Enter arranges them.
pub fn marks_actions(windows: &[isize]) -> Vec<RowAction> {
    let all = windows.to_vec();
    let first = windows.first().copied().unwrap_or_default();
    vec![
        RowAction::new(
            Key::Enter,
            "Arrange marked in strip",
            "Arrange",
            Run::Action(Action::Arrange(first)),
        ),
        RowAction::new(
            Key::ShiftRight,
            "Move all to next monitor",
            "Next monitor",
            Run::Action(Action::NextMonitor(all.clone())),
        ),
        RowAction::new(
            Key::CtrlM,
            "Minimise all",
            "Minimise",
            Run::Action(Action::Minimize(all.clone())),
        ),
        RowAction::new(
            Key::CtrlW,
            "Close all",
            "Close all",
            Run::Action(Action::Close(all)),
        ),
    ]
}

/// The word on the selected row's Enter pill: "Switch", "Copy", "Ask".
pub fn enter_verb(label: &str) -> String {
    label
        .trim_end_matches('\u{2026}')
        .split_whitespace()
        .next()
        .unwrap_or("Run")
        .to_string()
}

/// One key hint in the footer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub key: &'static str,
    pub label: String,
    /// Drawn in the accent colour, as "Backspace Clear scope" is.
    pub accent: bool,
}

impl Hint {
    fn plain(key: &'static str, label: &str) -> Self {
        Self {
            key,
            label: label.to_string(),
            accent: false,
        }
    }
}

/// The most hints the footer shows, Esc included.
pub const MOST_HINTS: usize = 5;

/// What the footer shows for the selected row: its actions' keys, then the
/// everyday keys while there is room, and Esc last. `escape` is what Esc
/// does now: "Close", "Clear marks", "Back".
pub fn footer_hints(actions: &[RowAction], clear_scope: bool, escape: &str) -> Vec<Hint> {
    let mut hints = Vec::new();
    if clear_scope {
        hints.push(Hint {
            key: "Backspace",
            label: "Clear scope".to_string(),
            accent: true,
        });
    }
    let mut own = 0;
    for action in actions.iter().filter(|action| action.hinted) {
        hints.push(Hint::plain(action.key.caps(), &action.short));
        own += 1;
    }
    // A row that does one thing leaves room to teach the keys that always
    // work; a busier row needs the room for its own.
    if own <= 1 {
        let everyday = [
            ("\u{2191}\u{2193}", "Move"),
            ("\u{2192}", "Actions"),
            ("Ctrl+P", "Plugins"),
        ];
        for (key, label) in everyday {
            if hints.len() >= MOST_HINTS - 1 {
                break;
            }
            hints.push(Hint::plain(key, label));
        }
    }
    hints.truncate(MOST_HINTS - 1);
    hints.push(Hint::plain("Esc", escape));
    hints
}

/// A close guess at the width of one hint: the keycap, the gap, the label
/// and the space before the next hint.
pub fn hint_width(hint: &Hint) -> f32 {
    (hint.key.chars().count() as f32 * 7.0 + 14.0)
        + 6.0
        + hint.label.chars().count() as f32 * 6.2
        + 16.0
}

/// Leaves out hints from the end, never Esc, until they fit in `room`.
pub fn fit_hints(mut hints: Vec<Hint>, room: f32) -> Vec<Hint> {
    while hints.len() > 1 && hints.iter().map(hint_width).sum::<f32>() > room {
        hints.remove(hints.len() - 2);
    }
    hints
}

pub const TRY_INSTEAD: &str = "Try instead";

/// The rows shown when nothing matches what was typed.
pub fn try_instead(query: &str, search: &SearchConfig) -> Vec<ResultItem> {
    let words = query.trim();
    if words.is_empty() {
        return Vec::new();
    }
    let web_search = Choice {
        label: "Search the web".to_string(),
        action: Action::OpenUrl(web::address(web::usable_url(&search.web_url), words)),
    };
    let site = ask_ai::site(search);
    vec![
        ResultItem {
            group: TRY_INSTEAD.to_string(),
            title: format!("Ask AI about \u{201c}{words}\u{201d}"),
            subtitle: format!("Opens {} in your browser", site.name),
            icon: IconRef::Glyph(Glyph::Sparkles),
            enter: Some(Choice {
                label: "Ask".to_string(),
                action: Action::OpenUrl(ask_ai::url(search, words)),
            }),
            ctrl_enter: Some(web_search.clone()),
            ..Default::default()
        },
        ResultItem {
            group: TRY_INSTEAD.to_string(),
            title: "Search the web".to_string(),
            subtitle: "Opens in your default browser".to_string(),
            icon: IconRef::Glyph(Glyph::Globe),
            hint: "Ctrl+\u{21B5}".to_string(),
            enter: Some(web_search),
            ..Default::default()
        },
    ]
}

/// One marked window, as the inspector lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marked {
    pub hwnd: isize,
    pub title: String,
    pub place: String,
}

/// The windows marked with Ctrl+Space, Shift+click or Ctrl+A, in the order
/// they were marked.
#[derive(Clone, Debug, Default)]
pub struct Marks {
    list: Vec<Marked>,
}

impl Marks {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn contains(&self, hwnd: isize) -> bool {
        self.list.iter().any(|marked| marked.hwnd == hwnd)
    }

    pub fn list(&self) -> &[Marked] {
        &self.list
    }

    pub fn windows(&self) -> Vec<isize> {
        self.list.iter().map(|marked| marked.hwnd).collect()
    }

    /// Marks a window row, or unmarks it when it already is. A row that is
    /// not a window is left alone; returns whether anything changed.
    pub fn toggle(&mut self, item: &ResultItem, place: impl FnOnce() -> String) -> bool {
        let Some(hwnd) = item.window() else {
            return false;
        };
        if let Some(at) = self.list.iter().position(|marked| marked.hwnd == hwnd) {
            self.list.remove(at);
        } else {
            self.list.push(Marked {
                hwnd,
                title: item.title.clone(),
                place: place(),
            });
        }
        true
    }

    /// Ctrl+A: every window row shown is marked. Returns how many rows were
    /// windows, so the key can go to the text box when there are none.
    pub fn mark_all(
        &mut self,
        items: &[ResultItem],
        place: impl Fn(&ResultItem) -> String,
    ) -> usize {
        let mut windows = 0;
        for item in items {
            let Some(hwnd) = item.window() else {
                continue;
            };
            windows += 1;
            if !self.contains(hwnd) {
                self.list.push(Marked {
                    hwnd,
                    title: item.title.clone(),
                    place: place(item),
                });
            }
        }
        windows
    }

    /// Esc clears the marks before it closes the palette: true when there
    /// were marks to clear.
    pub fn clear(&mut self) -> bool {
        !std::mem::take(&mut self.list).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Completion;
    use std::path::PathBuf;

    fn window_row(hwnd: isize, title: &str) -> ResultItem {
        ResultItem {
            group: "Windows".to_string(),
            title: title.to_string(),
            subtitle: "notepad".to_string(),
            icon: IconRef::Window {
                hwnd,
                exe: PathBuf::from(r"C:\notepad.exe"),
            },
            enter: Some(Choice {
                label: "Switch to".to_string(),
                action: Action::Activate(hwnd),
            }),
            ctrl_enter: Some(Choice {
                label: "Arrange in strip\u{2026}".to_string(),
                action: Action::Arrange(hwnd),
            }),
            ..Default::default()
        }
    }

    fn choice(label: &str) -> Option<Choice> {
        Some(Choice {
            label: label.to_string(),
            action: Action::Copy(label.to_string()),
        })
    }

    fn keys(actions: &[RowAction]) -> Vec<Key> {
        actions.iter().map(|action| action.key).collect()
    }

    fn hint_keys(hints: &[Hint]) -> Vec<&str> {
        hints.iter().map(|hint| hint.key).collect()
    }

    #[test]
    fn a_window_switches_moves_arranges_and_closes() {
        let actions = row_actions(&window_row(7, "Notes"), &Situation::default());
        assert_eq!(
            keys(&actions),
            [
                Key::Enter,
                Key::ShiftRight,
                Key::CtrlEnter,
                Key::CtrlW,
                Key::CtrlSpace
            ]
        );
        assert_eq!(actions[0].label, "Switch to window");
        assert_eq!(actions[1].run, Run::Action(Action::NextMonitor(vec![7])));
        assert_eq!(actions[3].run, Run::Action(Action::Close(vec![7])));
        assert!(!actions[4].listed, "Mark is a footer hint only");
    }

    #[test]
    fn the_window_footer_reads_as_the_design_says() {
        let actions = row_actions(&window_row(7, "Notes"), &Situation::default());
        let hints = footer_hints(&actions, false, "Close");
        let words: Vec<String> = hints
            .iter()
            .map(|hint| format!("{} {}", hint.key, hint.label))
            .collect();
        assert_eq!(
            words,
            [
                "\u{21B5} Switch",
                "Shift+\u{2192} Next monitor",
                "Ctrl+\u{21B5} Arrange",
                "Ctrl+Space Mark",
                "Esc Close"
            ]
        );
    }

    #[test]
    fn a_calculator_answer_copies_then_copies_the_sum_then_pastes() {
        let answer = ResultItem {
            answer: true,
            enter: choice("Copy"),
            copy: choice("Copy with expression"),
            ctrl_enter: choice("Paste"),
            tab: Some(Completion {
                label: "Keep going".to_string(),
                text: "42".to_string(),
            }),
            ..Default::default()
        };
        let actions = row_actions(&answer, &Situation::default());
        let hints = footer_hints(&actions, false, "Close");
        let words: Vec<String> = hints
            .iter()
            .map(|hint| format!("{} {}", hint.key, hint.label))
            .collect();
        assert_eq!(
            words,
            [
                "\u{21B5} Copy",
                "Ctrl+C Copy with expression",
                "Ctrl+\u{21B5} Paste",
                "Esc Close"
            ]
        );
    }

    #[test]
    fn a_plain_command_gets_the_default_footer() {
        let command = ResultItem {
            enter: choice("Run"),
            shortcut: choice("Change shortcut\u{2026}"),
            ..Default::default()
        };
        let actions = row_actions(&command, &Situation::default());
        assert_eq!(keys(&actions), [Key::Enter, Key::CtrlK]);
        assert_eq!(actions[1].label, "Change shortcut\u{2026}");
        let hints = footer_hints(&actions, false, "Close");
        assert_eq!(
            hint_keys(&hints),
            ["\u{21B5}", "\u{2191}\u{2193}", "\u{2192}", "Ctrl+P", "Esc"]
        );
        assert_eq!(hints[0].label, "Run");
    }

    #[test]
    fn the_footer_holds_at_most_five_with_esc_last() {
        let busy = ResultItem {
            enter: choice("Open"),
            shift_enter: choice("Show in folder"),
            ctrl_enter: choice("Copy path"),
            bookmark: choice("Bookmark"),
            delete: choice("Remove"),
            tab: Some(Completion {
                label: "Browse".to_string(),
                text: "x".to_string(),
            }),
            ..Default::default()
        };
        let actions = row_actions(&busy, &Situation::default());
        let hints = footer_hints(&actions, true, "Clear marks");
        assert_eq!(hints.len(), MOST_HINTS);
        assert_eq!(hints[0].key, "Backspace");
        assert!(hints[0].accent);
        assert_eq!(hints.last(), Some(&Hint::plain("Esc", "Clear marks")));
    }

    #[test]
    fn hints_that_do_not_fit_leave_from_the_end_but_esc_stays() {
        let actions = row_actions(&window_row(1, "x"), &Situation::default());
        let hints = fit_hints(footer_hints(&actions, false, "Close"), 300.0);
        assert!(hints.iter().map(hint_width).sum::<f32>() <= 300.0);
        assert_eq!(hints.first().map(|hint| hint.key), Some("\u{21B5}"));
        assert_eq!(hints.last().map(|hint| hint.key), Some("Esc"));
        assert_eq!(fit_hints(vec![Hint::plain("Esc", "Close")], 1.0).len(), 1);
    }

    #[test]
    fn the_best_match_can_be_forgotten_unless_ctrl_delete_is_taken() {
        let picked = ResultItem {
            enter: choice("Open"),
            usage_key: Some("apps:x".to_string()),
            ..Default::default()
        };
        let best = Situation {
            best_match: true,
            ..Default::default()
        };
        let actions = row_actions(&picked, &best);
        assert_eq!(actions.last().map(|a| &a.run), Some(&Run::Forget));
        assert_eq!(actions.last().map(|a| a.key), Some(Key::CtrlDelete));
        assert!(!row_actions(&picked, &Situation::default())
            .iter()
            .any(|a| a.run == Run::Forget));
        let bookmark = ResultItem {
            delete: choice("Remove"),
            ..picked
        };
        assert!(!row_actions(&bookmark, &best)
            .iter()
            .any(|a| a.run == Run::Forget));
    }

    #[test]
    fn the_enter_verb_is_the_first_word_of_the_first_action() {
        assert_eq!(enter_verb("Switch to window"), "Switch");
        assert_eq!(enter_verb("Copy"), "Copy");
        assert_eq!(enter_verb("Arrange marked in strip"), "Arrange");
        assert_eq!(enter_verb("Restore\u{2026}"), "Restore");
        assert_eq!(enter_verb(""), "Run");
    }

    #[test]
    fn marks_toggle_mark_all_and_clear_before_closing() {
        let rows = [
            window_row(1, "One"),
            ResultItem {
                title: "An app".to_string(),
                ..Default::default()
            },
            window_row(2, "Two"),
        ];
        let mut marks = Marks::default();
        assert!(marks.toggle(&rows[0], || "Left".to_string()));
        assert!(!marks.toggle(&rows[1], String::new), "an app is not marked");
        assert_eq!(marks.windows(), [1]);
        assert_eq!(marks.list()[0].place, "Left");
        assert!(marks.toggle(&rows[0], String::new));
        assert!(marks.is_empty(), "a second press unmarks");

        assert_eq!(marks.mark_all(&rows, |_| String::new()), 2);
        assert_eq!(marks.windows(), [1, 2]);
        assert_eq!(marks.mark_all(&rows, |_| String::new()), 2, "no doubles");
        assert_eq!(marks.len(), 2);
        assert_eq!(marks.mark_all(&rows[1..2], |_| String::new()), 0);

        assert!(marks.clear(), "the first Esc clears the marks");
        assert!(!marks.clear(), "the next one closes the palette");
    }

    #[test]
    fn marked_windows_arrange_move_minimise_and_close_together() {
        let actions = marks_actions(&[3, 5]);
        assert_eq!(
            keys(&actions),
            [Key::Enter, Key::ShiftRight, Key::CtrlM, Key::CtrlW]
        );
        assert_eq!(actions[0].label, "Arrange marked in strip");
        assert_eq!(actions[2].run, Run::Action(Action::Minimize(vec![3, 5])));
        assert_eq!(actions[3].run, Run::Action(Action::Close(vec![3, 5])));
        let hints = footer_hints(&actions, false, "Clear marks");
        assert_eq!(
            hint_keys(&hints),
            ["\u{21B5}", "Shift+\u{2192}", "Ctrl+M", "Ctrl+W", "Esc"]
        );
    }

    #[test]
    fn nothing_matching_offers_ask_ai_and_a_web_search() {
        let search = SearchConfig::default();
        let rows = try_instead(" zqx ", &search);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "Ask AI about \u{201c}zqx\u{201d}");
        assert_eq!(rows[0].subtitle, "Opens Claude in your browser");
        assert_eq!(
            rows[0].enter.as_ref().map(|c| &c.action),
            Some(&Action::OpenUrl("https://claude.ai/new?q=zqx".to_string()))
        );
        let web = Action::OpenUrl("https://www.google.com/search?q=zqx".to_string());
        assert_eq!(rows[0].ctrl_enter.as_ref().map(|c| &c.action), Some(&web));
        assert_eq!(rows[1].title, "Search the web");
        assert_eq!(rows[1].enter.as_ref().map(|c| &c.action), Some(&web));
        assert!(rows.iter().all(|row| row.group == TRY_INSTEAD));
        assert!(try_instead("  ", &search).is_empty());
    }

    #[test]
    fn the_inspector_overlays_the_list_below_1100_pixels() {
        assert!(is_narrow(1024.0));
        assert!(is_narrow(1099.0));
        assert!(!is_narrow(1100.0));
        assert!(!is_narrow(2560.0));
    }
}

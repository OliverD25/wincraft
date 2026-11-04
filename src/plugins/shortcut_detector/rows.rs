//! The list's rows without any drawing: which rows show and in which order,
//! how a status reads, what Copy list puts on the clipboard, and where the
//! columns are, so the header and the rows line up.

use egui::{Color32, Rangef};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

use crate::core::hotkeys;
use crate::core::theme::Tokens;

use super::probe::{Entry, Status};

/// The parts of the modifier filter. A part keeps the rows whose
/// combination uses that modifier.
pub const MODIFIERS: [&str; 5] = ["All", "Ctrl", "Alt", "Shift", "Win"];

pub fn modifier_of(part: usize) -> Option<u32> {
    match part {
        1 => Some(MOD_CONTROL),
        2 => Some(MOD_ALT),
        3 => Some(MOD_SHIFT),
        4 => Some(MOD_WIN),
        _ => None,
    }
}

/// The theme colour a status is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Success,
    Warning,
    Info,
    Accent,
}

impl Tone {
    pub fn colour(self, tokens: &Tokens) -> Color32 {
        match self {
            Tone::Success => tokens.success,
            Tone::Warning => tokens.warning,
            Tone::Info => tokens.info,
            Tone::Accent => tokens.accent,
        }
    }
}

/// The short word on a row's tag, and its colour.
pub fn look(status: Status) -> (&'static str, Tone) {
    let tone = match status {
        Status::Free => Tone::Success,
        Status::TakenByApp => Tone::Warning,
        Status::Windows => Tone::Info,
        Status::WinCraft => Tone::Accent,
    };
    (status.label(), tone)
}

/// What the toolbar asks for.
pub struct Filter<'a> {
    /// Lowercase already.
    pub query: &'a str,
    pub modifier: Option<u32>,
    pub show_free: bool,
}

pub fn matches(entry: &Entry, filter: &Filter) -> bool {
    if !filter.show_free && entry.status == Status::Free {
        return false;
    }
    if filter
        .modifier
        .is_some_and(|wanted| entry.hotkey.modifiers & wanted == 0)
    {
        return false;
    }
    let query = filter.query;
    query.is_empty()
        || hotkeys::format(entry.hotkey).to_lowercase().contains(query)
        || entry.meaning.to_lowercase().contains(query)
        || entry.owner().to_lowercase().contains(query)
        || entry.status.label().to_lowercase().contains(query)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    #[default]
    Shortcut,
    Status,
    Meaning,
    Owner,
    Source,
}

pub const COLUMNS: [(Column, &str); 5] = [
    (Column::Shortcut, "Shortcut"),
    (Column::Status, "Status"),
    (Column::Meaning, "Meaning"),
    (Column::Owner, "Owner"),
    (Column::Source, "Source"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub descending: bool,
}

impl Sort {
    /// A click on a header: the same column turns the order round, another
    /// one sorts by it from the top.
    pub fn clicked(self, column: Column) -> Sort {
        if self.column == column {
            Sort {
                column,
                descending: !self.descending,
            }
        } else {
            Sort {
                column,
                descending: false,
            }
        }
    }
}

fn sort_key(entry: &Entry, column: Column) -> String {
    match column {
        Column::Status => entry.status.label().to_string(),
        Column::Meaning => entry.meaning.clone(),
        Column::Owner => entry.owner().to_string(),
        Column::Source => entry.source.to_string(),
        Column::Shortcut => format!(
            "{:010} {}",
            entry.hotkey.modifiers & !windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_NOREPEAT,
            hotkeys::format(entry.hotkey)
        ),
    }
}

/// The indexes of the rows to show, in the order to show them.
pub fn visible(entries: &[Entry], filter: &Filter, sort: Sort) -> Vec<usize> {
    let mut chosen: Vec<(String, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| matches(entry, filter))
        .map(|(index, entry)| (sort_key(entry, sort.column), index))
        .collect();
    chosen.sort();
    if sort.descending {
        chosen.reverse();
    }
    chosen.into_iter().map(|(_, index)| index).collect()
}

/// The Meaning cell: a dash when there is nothing to say.
pub fn meaning(entry: &Entry) -> &str {
    or_dash(&entry.meaning)
}

/// The Owner cell: a dash for a free combination.
pub fn owner(entry: &Entry) -> &str {
    or_dash(entry.owner())
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() {
        "\u{2014}"
    } else {
        text
    }
}

/// The rows shown, tab-separated with a header line, as Copy list and
/// Ctrl+C put them on the clipboard.
pub fn copy_text(entries: &[Entry], visible: &[usize]) -> String {
    let mut out = String::from("Shortcut\tStatus\tMeaning\tOwner\tSource\r\n");
    for index in visible {
        let entry = &entries[*index];
        let cells = [
            hotkeys::format(entry.hotkey),
            entry.status.label().to_string(),
            entry.meaning.clone(),
            entry.owner().to_string(),
            entry.source.to_string(),
        ];
        out.push_str(&cells.join("\t"));
        out.push_str("\r\n");
    }
    out
}

pub const SIDE: f32 = 20.0;
const GAP: f32 = 16.0;
const SHORTCUT_WIDTH: f32 = 200.0;
const STATUS_WIDTH: f32 = 110.0;
const SOURCE_WIDTH: f32 = 110.0;

/// Where each column is between `left` and `right`, in the order of
/// `COLUMNS`. The header and every row take their cells from here. Meaning
/// and Owner share the room between Status and Source half and half.
pub fn columns(left: f32, right: f32) -> [Rangef; 5] {
    let shortcut = Rangef::new(left + SIDE, left + SIDE + SHORTCUT_WIDTH);
    let status = Rangef::new(shortcut.max + GAP, shortcut.max + GAP + STATUS_WIDTH);
    let source_end = (right - SIDE).max(status.max + GAP * 2.0 + SOURCE_WIDTH);
    let source = Rangef::new(source_end - SOURCE_WIDTH, source_end);
    let flex_min = status.max + GAP;
    let flex_max = (source.min - GAP).max(flex_min);
    let half = ((flex_max - flex_min - GAP).max(0.0) / 2.0).floor();
    let meaning = Rangef::new(flex_min, flex_min + half);
    let owner_min = (meaning.max + GAP).min(source.min);
    let owner = Rangef::new(owner_min, flex_max.max(owner_min));
    [shortcut, status, meaning, owner, source]
}

#[cfg(test)]
mod tests {
    use super::super::probe::classify;
    use super::*;

    fn entries() -> Vec<Entry> {
        let key = |text: &str| hotkeys::parse(text).expect(text);
        vec![
            classify(key("Win+E"), false, None),
            {
                let mut app = classify(key("Ctrl+Alt+F9"), true, None);
                app.holder = "ShareX, probably".to_string();
                app
            },
            classify(key("Win+Alt+S"), false, Some(("ShortcutManager", "Open"))),
            classify(key("Ctrl+Shift+F7"), false, None),
        ]
    }

    fn shown(query: &str, part: usize, show_free: bool) -> Vec<usize> {
        let filter = Filter {
            query,
            modifier: modifier_of(part),
            show_free,
        };
        visible(&entries(), &filter, Sort::default())
    }

    #[test]
    fn each_status_has_a_short_word_and_a_colour() {
        assert_eq!(look(Status::Windows), ("Windows", Tone::Info));
        assert_eq!(look(Status::TakenByApp), ("App", Tone::Warning));
        assert_eq!(look(Status::WinCraft), ("WinCraft", Tone::Accent));
        assert_eq!(look(Status::Free), ("Free", Tone::Success));
        let tokens = Tokens::get(&egui::Context::default());
        assert_eq!(Tone::Info.colour(&tokens), tokens.info);
        assert_eq!(Tone::Accent.colour(&tokens), tokens.accent);
    }

    #[test]
    fn search_modifier_and_show_free_filter_together() {
        let mut all = shown("", 0, true);
        all.sort();
        assert_eq!(all, [0, 1, 2, 3]);
        assert!(!shown("", 0, false).contains(&3), "free rows hide");
        let mut ctrl = shown("", 1, true);
        ctrl.sort();
        assert_eq!(ctrl, [1, 3], "a part keeps the rows that use it");
        let mut win = shown("", 4, false);
        win.sort();
        assert_eq!(win, [0, 2]);
        assert_eq!(shown("explorer", 0, false), [0], "the meaning is searched");
        assert_eq!(shown("sharex", 0, false), [1], "the owner is searched");
        assert_eq!(shown("alt+f9", 0, false), [1], "the shortcut is searched");
        assert_eq!(shown("wincraft", 0, false), [2], "the status is searched");
        assert!(shown("explorer", 1, true).is_empty());
    }

    #[test]
    fn a_header_click_sorts_and_a_second_turns_the_order_round() {
        let status = Sort::default().clicked(Column::Status);
        assert_eq!(status.column, Column::Status);
        assert!(!status.descending);
        assert!(status.clicked(Column::Status).descending);
        let filter = Filter {
            query: "",
            modifier: None,
            show_free: true,
        };
        let rows = entries();
        let words = |sort: Sort| -> Vec<&str> {
            visible(&rows, &filter, sort)
                .iter()
                .map(|index| rows[*index].status.label())
                .collect()
        };
        assert_eq!(words(status), ["App", "Free", "WinCraft", "Windows"]);
        assert_eq!(
            words(status.clicked(Column::Status)),
            ["Windows", "WinCraft", "Free", "App"]
        );
    }

    #[test]
    fn copy_list_is_tab_separated_with_a_header() {
        let rows = entries();
        let text = copy_text(&rows, &[0, 2]);
        assert_eq!(
            text,
            "Shortcut\tStatus\tMeaning\tOwner\tSource\r\n\
             Win+E\tWindows\tOpen File Explorer\tWindows\twindows list\r\n\
             Win+Alt+S\tWinCraft\tShortcutManager: Open\tWinCraft\tWinCraft\r\n"
        );
        assert_eq!(meaning(&rows[3]), "\u{2014}");
        assert_eq!(owner(&rows[3]), "\u{2014}");
        assert_eq!(owner(&rows[1]), "ShareX, probably");
        assert_eq!(meaning(&rows[1]), "\u{2014}");
    }

    #[test]
    fn the_columns_follow_the_design_and_never_overlap() {
        let [shortcut, status, meaning, owner, source] = columns(0.0, 760.0);
        assert_eq!((shortcut.min, shortcut.max), (20.0, 220.0));
        assert_eq!((status.min, status.max), (236.0, 346.0));
        assert_eq!((source.min, source.max), (630.0, 740.0));
        assert_eq!((meaning.min, meaning.max), (362.0, 480.0));
        assert_eq!((owner.min, owner.max), (496.0, 614.0));
        // Narrower than the columns need: meaning and owner shrink to
        // nothing and the source moves right instead of covering the status.
        let [_, status, meaning, owner, source] = columns(0.0, 300.0);
        assert!(meaning.span() >= 0.0 && owner.span() >= 0.0);
        assert!(owner.max <= source.min);
        assert!(source.min >= status.max + 16.0);
    }
}

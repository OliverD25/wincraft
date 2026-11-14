//! ClaudeDeck's settings page: the fields and their groups.

use crate::core::traits::{FieldKind, SettingField};

use super::*;

pub(super) fn fields() -> Vec<SettingField> {
    vec![
        SettingField {
            key: STAY_ON_TOP_KEY,
            label: "Stay on top",
            help: "Keep ClaudeDeck above other windows. The pin in its header does the same.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: ONLY_OPEN_KEY,
            label: "Only open windows",
            help: "List only the chats that have a window open.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: FADE_KEY,
            label: "Fade when the mouse is away",
            help: "ClaudeDeck turns half see-through a few seconds after the mouse leaves it. It is solid again when the mouse comes back, when it has the keyboard, or when a chat starts to wait for you.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: FOLD_COLD_KEY,
            label: "Fold chats idle for over 1 day",
            help: "Old idle chats go into one grey line at the end.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: LIVE_KEY,
            label: "Live in the tray (no taskbar button)",
            help: "ClaudeDeck gets a tray icon of its own instead of a taskbar button, and it is not in Alt+Tab. Its \u{d7} button hides it to the tray; a click on its tray icon, its hotkey or the palette brings it back where it was. While it lives in the tray, ClaudeDeck keeps reading your chats and servers, so its count and news stay current.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: TRAY_KEY,
            label: "Show a count on the tray icon",
            help: "A red badge on ClaudeDeck's tray icon counts the chats that need approval or ask you, and its tooltip also says how many replies you have not seen, for example \"ClaudeDeck \u{2014} 2 chats wait for you, 4 new replies\". With Live in the tray off, WinCraft's tray icon says it in its tooltip instead, and only while ClaudeDeck is open.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: MARK_KEY,
            label: "Status mark",
            help: "A shape for each status, a shape and a short word, or no mark.",
            kind: FieldKind::Choice(&[MARK_SHAPES, MARK_WORDS, MARK_OFF]),
        },
        SettingField {
            key: TEXT_KEY,
            label: "A line of text (the question, or what it does)",
            help: "A second line under the name: the question when the chat waits, else what it does or what it said last. Resting the mouse on a chat shows all of it.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: TIME_KEY,
            label: "Time in the status",
            help: "How long the chat has been in its status, at the right edge.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: PROJECT_KEY,
            label: "Project and branch",
            help: "The chat's git branch after its name.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: JOBS_KEY,
            label: "Job count",
            help: "How many background jobs a chat runs.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: MODEL_KEY,
            label: "Model name",
            help: "The Claude model the chat uses.",
            kind: FieldKind::Toggle,
        },
        SettingField {
            key: SERVERS_KEY,
            label: "Servers (ssh names)",
            help: "For example rdehome1, rdehome2. Empty: no servers are read. WinCraft reads them every 30 s while the ClaudeDeck window is open, or all the time while ClaudeDeck lives in the tray, and never changes anything on them.",
            kind: FieldKind::Text,
        },
    ]
}

pub(super) fn group(key: &str) -> Option<&'static str> {
    match key {
        MARK_KEY | TEXT_KEY | TIME_KEY | PROJECT_KEY | JOBS_KEY | MODEL_KEY => Some("On each row"),
        SERVERS_KEY => Some("Homelab"),
        hook_link::KEY => Some(hook_link::GROUP),
        _ => None,
    }
}

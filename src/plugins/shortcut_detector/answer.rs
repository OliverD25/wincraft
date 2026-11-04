//! The line under the Check a shortcut box: who holds the combination, how
//! to change it, and the buttons that act on it (Change, Remove key, Press it
//! for real). `plan` decides the words and buttons without drawing, so it
//! can be tested; `strip` draws them.

use std::path::PathBuf;

use egui::{pos2, vec2, Align, Align2, Id, Layout, Rect, Sense, Ui, UiBuilder};

use crate::core::hotkeys;
use crate::core::theme::{self, Tokens};
use crate::core::traits::Hotkey;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::keycap::{self, Caps};

use super::known;
use super::lnk;
use super::owners::{Found, SOURCE_PRESS};
use super::press;
use super::probe::{Entry, Status};

pub const HEIGHT: f32 = 34.0;
pub const RESULTS_HEIGHT: f32 = 28.0;
pub const SAVED: &str =
    "Saved. Explorer may take a moment to use it; if it does not, sign out and in again.";
pub const REMOVED: &str = "Removed. The shortcut no longer has a key.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Change one of WinCraft's own hotkeys in the hotkey dialog.
    Change,
    /// Give a shortcut file a new key.
    EditLink,
    RemoveKey,
    Save,
    Cancel,
    Press,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub action: Action,
    pub label: &'static str,
    pub enabled: bool,
    pub tip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub line: String,
    pub buttons: Vec<Button>,
}

/// A shortcut file whose key is being changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkEdit {
    pub path: PathBuf,
    pub name: String,
    pub keys: Option<Hotkey>,
}

/// The answer's own state in the window.
#[derive(Default)]
pub struct AnswerUi {
    pub press: Option<press::Run>,
    /// The last press test's line, for the combination it pressed.
    pub pressed: Option<(Hotkey, String)>,
    pub link: Option<LinkEdit>,
    /// What the last Save or Remove key did.
    pub note: Option<String>,
}

impl AnswerUi {
    /// A new combination in the box: what was said about the old one goes.
    pub fn forget(&mut self) {
        self.link = None;
        self.note = None;
    }
}

fn button(action: Action, label: &'static str) -> Button {
    Button {
        action,
        label,
        enabled: true,
        tip: None,
    }
}

fn press_button(hotkey: Hotkey, test_instance: bool) -> Option<Button> {
    match press::refusal(hotkey, test_instance) {
        Some(why) if why == press::TEST_COPY => Some(Button {
            enabled: false,
            tip: Some(why.to_string()),
            ..button(Action::Press, "Press it for real")
        }),
        Some(_) => None,
        None => Some(Button {
            tip: Some(format!(
                "Sends {} once. The app that holds it really acts.",
                hotkeys::format(hotkey)
            )),
            ..button(Action::Press, "Press it for real")
        }),
    }
}

/// The words and buttons for the combination in the box.
pub fn plan(entry: &Entry, found: Option<Found>, test_instance: bool, ui: &AnswerUi) -> Plan {
    if let Some(edit) = &ui.link {
        let line = match edit.keys.map(lnk::encode) {
            Some(Err(why)) => why.to_string(),
            _ => format!("New key for \u{201c}{}\u{201d}:", edit.name),
        };
        let ready = edit.keys.is_some_and(|keys| lnk::encode(keys).is_ok());
        return Plan {
            line,
            buttons: vec![
                Button {
                    enabled: ready,
                    ..button(Action::Save, "Save")
                },
                button(Action::RemoveKey, "Remove key"),
                button(Action::Cancel, "Cancel"),
            ],
        };
    }
    if ui.press.is_some() {
        return Plan {
            line: format!(
                "Pressing {}, watching for 3 s\u{2026}",
                hotkeys::format(entry.hotkey)
            ),
            buttons: Vec::new(),
        };
    }
    if let Some(note) = &ui.note {
        return Plan {
            line: note.clone(),
            buttons: Vec::new(),
        };
    }
    let pressed = ui
        .pressed
        .as_ref()
        .filter(|(hotkey, _)| *hotkey == entry.hotkey)
        .map(|(_, line)| line.clone());
    let (line, buttons) = match (entry.status, found) {
        (Status::Free, _) => (
            "You can use it. Free combinations close to it are listed below.".to_string(),
            Vec::new(),
        ),
        (Status::WinCraft, _) => (
            format!("WinCraft uses it: {}.", entry.meaning),
            vec![button(Action::Change, "Change")],
        ),
        (Status::Windows, _) if known::is_reserved(entry.hotkey) => (
            "Windows keeps it. No app can have it.".to_string(),
            Vec::new(),
        ),
        (Status::Windows, _) => (
            "Windows uses it, so a free combination is a better choice.".to_string(),
            Vec::new(),
        ),
        (Status::TakenByApp, Some(Found::Link(link))) => (
            format!(
                "Explorer gives this key to the shortcut \u{201c}{}\u{201d}.",
                link.name
            ),
            vec![
                button(Action::EditLink, "Change"),
                button(Action::RemoveKey, "Remove key"),
            ],
        ),
        (Status::TakenByApp, found) => {
            let line = match (pressed, found) {
                (Some(line), _) => line,
                (None, Some(Found::Known { app, .. })) => format!("How to change: {}", app.change),
                (None, None) if entry.source == SOURCE_PRESS => {
                    format!("Pressing it showed {}.", entry.holder)
                }
                (None, _) => {
                    "Unknown app. Try \u{201c}Press it for real\u{201d} to see which app reacts."
                        .to_string()
                }
            };
            (
                line,
                press_button(entry.hotkey, test_instance)
                    .into_iter()
                    .collect(),
            )
        }
    };
    Plan { line, buttons }
}

/// Draws the plan in `area`; the action of a clicked button, if any.
/// `link_keys` are the keys pressed for a shortcut file so far, shown as
/// caps after the line while one is being changed.
pub fn strip(
    ui: &mut Ui,
    area: Rect,
    plan: &Plan,
    link_keys: Option<Option<Hotkey>>,
) -> Option<Action> {
    let tokens = Tokens::get(ui.ctx());
    let mut clicked = None;
    let mut right = area.right();
    for (at, item) in plan.buttons.iter().enumerate().rev() {
        let width = button::width(ui, item.label).min(area.width());
        let rect = Rect::from_min_max(pos2(right - width, area.top()), pos2(right, area.bottom()));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .id_salt(("wincraft-detector-answer", at))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.add_enabled_ui(item.enabled, |ui| {
                    let mut response = button::small(ui, item.label, Kind::Secondary);
                    if let Some(tip) = &item.tip {
                        response = response.on_hover_text(tip);
                        response = response.on_disabled_hover_text(tip);
                    }
                    if response.clicked() {
                        clicked = Some(item.action);
                    }
                });
            },
        );
        right -= width + 8.0;
    }
    let painter = ui.painter().clone();
    let room = (right - area.left() - 8.0).max(1.0);
    let galley = painter.layout_job(super::window::one_line(
        &plan.line,
        theme::regular(13.0),
        tokens.text_secondary,
        room,
    ));
    let line_right = area.left() + galley.size().x;
    let elided = galley.elided;
    painter.galley(
        pos2(area.left(), area.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_secondary,
    );
    if elided {
        let line_rect = Rect::from_min_max(area.min, pos2(line_right, area.bottom()));
        ui.interact(
            line_rect,
            Id::new("wincraft-detector-answer-line"),
            Sense::hover(),
        )
        .on_hover_text(&plan.line);
    }
    if let Some(keys) = link_keys {
        let left = line_right + 10.0;
        if left < right {
            let rect = Rect::from_min_max(pos2(left, area.top()), pos2(right, area.bottom()));
            match keys {
                Some(keys) => {
                    let text = hotkeys::format(keys);
                    let steps = vec![keycap::split(&text)
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>()];
                    ui.scope_builder(
                        UiBuilder::new()
                            .max_rect(rect)
                            .layout(Layout::left_to_right(Align::Center)),
                        |ui| keycap::caps(ui, &steps, Caps::palette()),
                    );
                }
                None => {
                    painter.text(
                        pos2(rect.left(), rect.center().y),
                        Align2::LEFT_CENTER,
                        "Press keys\u{2026}",
                        theme::regular(13.0),
                        tokens.text_disabled,
                    );
                }
            }
        }
    }
    clicked
}

/// "Results for Win+`" with a Show all link at the right; true when the
/// link was clicked.
pub fn results_line(ui: &mut Ui, area: Rect, keys: Hotkey) -> bool {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter().clone();
    painter.text(
        pos2(area.left(), area.center().y),
        Align2::LEFT_CENTER,
        format!("Results for {}", hotkeys::format(keys)),
        theme::semibold(13.0),
        tokens.text_primary,
    );
    let link = painter.layout_no_wrap(
        "Show all  Esc".to_string(),
        theme::regular(12.0),
        tokens.accent,
    );
    let rect = Rect::from_min_size(
        pos2(
            area.right() - link.size().x,
            area.center().y - link.size().y / 2.0,
        ),
        link.size(),
    )
    .expand2(vec2(4.0, 4.0));
    let response = ui
        .interact(rect, Id::new("wincraft-detector-show-all"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Back to the full list");
    let colour = if response.hovered() {
        tokens.accent_pressed
    } else {
        tokens.accent
    };
    painter.galley(rect.shrink2(vec2(4.0, 4.0)).min, link, colour);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::super::known_apps::KNOWN_APPS;
    use super::super::lnk::LinkKey;
    use super::super::probe::classify;
    use super::*;

    fn key(text: &str) -> Hotkey {
        hotkeys::parse(text).expect(text)
    }

    fn actions(plan: &Plan) -> Vec<Action> {
        plan.buttons.iter().map(|button| button.action).collect()
    }

    #[test]
    fn an_unknown_app_offers_the_press_test_and_a_test_copy_greys_it_out() {
        let entry = classify(key("Ctrl+Alt+F9"), true, None);
        let ui = AnswerUi::default();
        let real = plan(&entry, None, false, &ui);
        assert!(real.line.starts_with("Unknown app. Try"));
        assert_eq!(actions(&real), [Action::Press]);
        assert!(real.buttons[0].enabled);
        assert!(real.buttons[0]
            .tip
            .as_deref()
            .unwrap()
            .contains("Ctrl+Alt+F9"));

        let test = plan(&entry, None, true, &ui);
        assert_eq!(actions(&test), [Action::Press]);
        assert!(!test.buttons[0].enabled);
        assert_eq!(test.buttons[0].tip.as_deref(), Some(press::TEST_COPY));
    }

    #[test]
    fn a_known_app_says_how_to_change_it_and_still_offers_the_press() {
        let terminal = &KNOWN_APPS[0];
        let entry = classify(key("Win+`"), true, None);
        let found = Found::Known {
            app: terminal,
            meaning: "Open the Quake mode window",
        };
        let answer = plan(&entry, Some(found), false, &AnswerUi::default());
        assert_eq!(
            answer.line,
            "How to change: Windows Terminal: Settings \u{2192} Actions \u{2192} Quake mode"
        );
        assert_eq!(actions(&answer), [Action::Press]);
    }

    #[test]
    fn a_shortcut_file_can_be_changed_or_lose_its_key_but_is_never_pressed() {
        let link = LinkKey {
            hotkey: key("Ctrl+Alt+N"),
            name: "Notes".to_string(),
            path: PathBuf::from(r"C:\fake\Notes.lnk"),
        };
        let entry = classify(key("Ctrl+Alt+N"), true, None);
        let answer = plan(
            &entry,
            Some(Found::Link(&link)),
            false,
            &AnswerUi::default(),
        );
        assert_eq!(
            answer.line,
            "Explorer gives this key to the shortcut \u{201c}Notes\u{201d}."
        );
        assert_eq!(actions(&answer), [Action::EditLink, Action::RemoveKey]);

        let mut ui = AnswerUi {
            link: Some(LinkEdit {
                path: link.path.clone(),
                name: "Notes".to_string(),
                keys: None,
            }),
            ..AnswerUi::default()
        };
        let editing = plan(&entry, Some(Found::Link(&link)), false, &ui);
        assert_eq!(editing.line, "New key for \u{201c}Notes\u{201d}:");
        assert_eq!(
            actions(&editing),
            [Action::Save, Action::RemoveKey, Action::Cancel]
        );
        assert!(!editing.buttons[0].enabled, "nothing to save yet");
        ui.link.as_mut().unwrap().keys = Some(key("Win+Alt+N"));
        let win = plan(&entry, None, false, &ui);
        assert_eq!(win.line, "A shortcut file cannot use the Win key.");
        assert!(!win.buttons[0].enabled);
        ui.link.as_mut().unwrap().keys = Some(key("Ctrl+Alt+M"));
        assert!(plan(&entry, None, false, &ui).buttons[0].enabled);
    }

    #[test]
    fn wincraft_windows_and_free_answers() {
        let mine = classify(key("Win+Alt+S"), true, Some(("ShortcutManager", "Open")));
        let answer = plan(&mine, None, false, &AnswerUi::default());
        assert_eq!(answer.line, "WinCraft uses it: ShortcutManager: Open.");
        assert_eq!(actions(&answer), [Action::Change]);

        let windows = classify(key("Win+E"), true, None);
        assert!(actions(&plan(&windows, None, false, &AnswerUi::default())).is_empty());
        let reserved = classify(key("Win+L"), true, None);
        let answer = plan(&reserved, None, false, &AnswerUi::default());
        assert_eq!(answer.line, "Windows keeps it. No app can have it.");
        assert!(answer.buttons.is_empty(), "Win+L is never pressed");

        let free = classify(key("Win+Alt+F7"), false, None);
        assert!(plan(&free, None, false, &AnswerUi::default())
            .line
            .starts_with("You can use it."));
    }

    #[test]
    fn a_press_result_and_a_note_take_the_line() {
        let entry = classify(key("Ctrl+Alt+F9"), true, None);
        let mut ui = AnswerUi {
            pressed: Some((key("Ctrl+Alt+F9"), press::NOTHING.to_string())),
            ..AnswerUi::default()
        };
        let answer = plan(&entry, None, false, &ui);
        assert_eq!(answer.line, press::NOTHING);
        assert_eq!(actions(&answer), [Action::Press], "it can be pressed again");
        // A result for another combination does not show.
        let other = classify(key("Ctrl+Alt+F8"), true, None);
        assert!(plan(&other, None, false, &ui)
            .line
            .starts_with("Unknown app"));

        ui.note = Some(REMOVED.to_string());
        let answer = plan(&entry, None, false, &ui);
        assert_eq!(answer.line, REMOVED);
        assert!(answer.buttons.is_empty());
        ui.forget();
        assert!(ui.note.is_none() && ui.link.is_none());
    }
}

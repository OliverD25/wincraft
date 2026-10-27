//! F2 renames the picked window: its title turns into a text box on the
//! card. Enter saves, Esc cancels, and a click anywhere else cancels too, so
//! a window is only ever renamed by an Enter the user pressed.

use egui::{Align, Rect, Ui};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{ArrangeAction, ArrangeGroup};

/// What WindowNamer keeps of a name at most; the box stops there.
const MAX_CHARS: usize = 200;

/// The line shown when F2 is pressed while no plugin keeps window names.
pub const NEEDS_NAMER: &str = "WinCraft: switch WindowNamer on in Plugins to rename windows.";

/// How a finished edit ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    Save(String),
    Cancel,
}

/// One title being edited.
#[derive(Debug)]
pub struct Rename {
    pub hwnd: isize,
    /// The edit is on a ghost, not on the window's own card.
    pub ghost: bool,
    text: String,
    /// The box has had the keyboard once. Before that a missing focus only
    /// means the window has not handed it over yet.
    focused: bool,
    fresh: bool,
    done: Option<Done>,
}

/// What F2 does.
#[derive(Debug)]
pub enum Start {
    Edit(Rename),
    /// No plugin keeps window names: say so.
    NeedsNamer,
    Nothing,
}

/// F2 on the picked card or ghost. `leaving` is true for a window that is
/// already going away.
pub fn start(
    can_rename: bool,
    cursor: Option<isize>,
    ghost: bool,
    group: &ArrangeGroup,
    leaving: &dyn Fn(isize) -> bool,
) -> Start {
    let Some(window) = cursor
        .and_then(|hwnd| group.windows.iter().find(|window| window.hwnd == hwnd))
        .filter(|window| !leaving(window.hwnd))
    else {
        return Start::Nothing;
    };
    if !can_rename {
        return Start::NeedsNamer;
    }
    Start::Edit(Rename::new(window.hwnd, ghost, &window.label))
}

/// Whether the box is finished: Enter saves, losing the keyboard cancels.
/// `seen` is whether the box has had the keyboard before this frame.
fn decide(enter: bool, has_focus: bool, seen: bool) -> Option<bool> {
    if !seen {
        return None;
    }
    if enter {
        return Some(true);
    }
    (!has_focus).then_some(false)
}

/// The typed name as the action that sets it. A name with nothing but
/// spaces removes the window's name.
pub fn action(hwnd: isize, text: &str) -> ArrangeAction {
    let name = text.trim();
    ArrangeAction::Rename {
        hwnd,
        name: (!name.is_empty()).then(|| name.to_string()),
    }
}

impl Rename {
    pub fn new(hwnd: isize, ghost: bool, name: &str) -> Self {
        Self {
            hwnd,
            ghost,
            text: name.to_string(),
            focused: false,
            fresh: true,
            done: None,
        }
    }

    /// How the edit ended, once it did.
    pub fn take_done(&mut self) -> Option<Done> {
        self.done.take()
    }

    /// Draws the box in `rect`, in a font of `size`, and notes how it ended.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, size: f32) {
        let ctx = ui.ctx().clone();
        let tokens = Tokens::get(&ctx);
        ui.painter()
            .rect_filled(rect, tokens.radius_row, tokens.panel_bg);
        let edit = egui::TextEdit::singleline(&mut self.text)
            .id_salt(("arrange-rename", self.hwnd))
            .font(theme::regular(size))
            .text_color(tokens.text_primary)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::symmetric(4, 0))
            .vertical_align(Align::Center)
            .char_limit(MAX_CHARS)
            .desired_width(rect.width() - 8.0);
        let response = ui.put(rect, edit);
        let has_focus = response.has_focus();
        // Asked for on every frame until it arrives: the window may take a
        // frame or two to hand the keyboard over.
        if !self.focused && !has_focus {
            response.request_focus();
        }
        if std::mem::take(&mut self.fresh) {
            if let Some(mut state) = egui::TextEdit::load_state(&ctx, response.id) {
                let end = egui::text::CCursor::new(self.text.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        end,
                    )));
                state.store(&ctx, response.id);
            }
        }
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let verdict = decide(enter, has_focus, self.focused);
        self.focused |= has_focus;
        ui.painter().rect_stroke(
            rect,
            tokens.radius_row,
            theme::line(&ctx, tokens.accent),
            egui::StrokeKind::Inside,
        );
        match verdict {
            Some(true) => self.done = Some(Done::Save(self.text.clone())),
            Some(false) => self.done = Some(Done::Cancel),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::ArrangeWindow;

    fn group() -> ArrangeGroup {
        ArrangeGroup {
            key: "chrome.exe".to_string(),
            label: "Chrome".to_string(),
            windows: [(1, "Docs"), (2, "Mail")]
                .into_iter()
                .map(|(hwnd, label)| ArrangeWindow {
                    hwnd,
                    label: label.to_string(),
                    desktop: String::new(),
                    monitor: None,
                    on_monitor: None,
                    saved_monitor: None,
                    renamed: false,
                    same_title: false,
                    attention: false,
                })
                .collect(),
        }
    }

    fn never(_: isize) -> bool {
        false
    }

    #[test]
    fn f2_edits_the_picked_card_with_its_name_in_the_box() {
        let Start::Edit(edit) = start(true, Some(2), false, &group(), &never) else {
            panic!("F2 should start an edit");
        };
        assert_eq!(
            (edit.hwnd, edit.ghost, edit.text.as_str()),
            (2, false, "Mail")
        );
    }

    #[test]
    fn f2_edits_a_picked_ghost_too() {
        let Start::Edit(edit) = start(true, Some(1), true, &group(), &never) else {
            panic!("F2 should start an edit");
        };
        assert!(edit.ghost);
    }

    #[test]
    fn f2_with_windownamer_off_shows_the_line_and_no_box() {
        assert!(matches!(
            start(false, Some(1), false, &group(), &never),
            Start::NeedsNamer
        ));
        assert!(NEEDS_NAMER.contains("WinCraft") && NEEDS_NAMER.contains("WindowNamer"));
    }

    #[test]
    fn f2_needs_a_picked_window_that_is_not_leaving() {
        assert!(matches!(
            start(true, None, false, &group(), &never),
            Start::Nothing
        ));
        assert!(matches!(
            start(true, Some(9), false, &group(), &never),
            Start::Nothing
        ));
        assert!(matches!(
            start(true, Some(1), false, &group(), &|hwnd| hwnd == 1),
            Start::Nothing
        ));
    }

    #[test]
    fn enter_saves_and_losing_the_keyboard_cancels() {
        assert_eq!(decide(true, false, true), Some(true));
        assert_eq!(decide(false, false, true), Some(false), "a click elsewhere");
        assert_eq!(decide(false, true, true), None, "still typing");
    }

    #[test]
    fn nothing_ends_before_the_box_had_the_keyboard() {
        assert_eq!(decide(false, false, false), None);
        assert_eq!(decide(true, false, false), None);
    }

    #[test]
    fn the_name_is_trimmed_and_an_empty_one_removes_the_name() {
        assert_eq!(
            action(7, "  Work notes "),
            ArrangeAction::Rename {
                hwnd: 7,
                name: Some("Work notes".to_string())
            }
        );
        assert_eq!(
            action(7, "   "),
            ArrangeAction::Rename {
                hwnd: 7,
                name: None
            }
        );
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            focused: true,
            events,
            ..Default::default()
        }
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Runs one frame of the box with these events.
    fn frame(ctx: &egui::Context, edit: &mut Rename, events: Vec<egui::Event>) {
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 20.0));
        let mut output = ctx.run_ui(input(events), |ui| edit.show(ui, rect, 13.0));
        // There is no renderer in a test to upload the font atlas to.
        output.textures_delta.clear();
    }

    #[test]
    fn the_box_takes_the_keyboard_and_enter_saves_the_text() {
        let ctx = egui::Context::default();
        let mut edit = Rename::new(5, false, "Docs");
        frame(&ctx, &mut edit, vec![]);
        frame(&ctx, &mut edit, vec![]);
        assert_eq!(edit.take_done(), None, "still editing");
        frame(&ctx, &mut edit, vec![key(egui::Key::Enter)]);
        assert_eq!(edit.take_done(), Some(Done::Save("Docs".to_string())));
    }

    #[test]
    fn typing_replaces_the_selected_name() {
        let ctx = egui::Context::default();
        let mut edit = Rename::new(5, false, "Docs");
        frame(&ctx, &mut edit, vec![]);
        frame(&ctx, &mut edit, vec![]);
        frame(
            &ctx,
            &mut edit,
            vec![egui::Event::Text("Plans".to_string())],
        );
        frame(&ctx, &mut edit, vec![key(egui::Key::Enter)]);
        assert_eq!(edit.take_done(), Some(Done::Save("Plans".to_string())));
    }
}

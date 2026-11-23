//! A note on a chat (design section 17, option A): one line of the user's
//! own text under the chat's row, on an amber tint, with a small note icon.
//! The menu, the N key or a double-click on the note opens a one-line box
//! on the row; Enter saves, Esc cancels, and an empty box changes nothing.
//! A small × at the end of the note, shown while the mouse is on the row,
//! removes it. A note is ClaudeDeck's own: no real chat is changed.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Color32, Galley, Key, Painter, Pos2, Rect, Shape, Stroke, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::model::{ChatLine, Line};
use super::rows::{self, Look};
use super::store::Saved;
use super::window::DeckUi;

/// The most characters a note can have.
pub const LIMIT: usize = 120;
/// The empty box's text.
pub const HINT: &str = "A note for this chat";
/// At the right of the box until something is typed; then the count.
pub const KEYS_HINT: &str = "Enter saves \u{b7} Esc cancels";
const SIZE: f32 = 12.0;
const CHIP: f32 = 18.0;
const PAD: f32 = 6.0;
pub(super) const ICON: f32 = 12.0;
const ICON_GAP: f32 = 5.0;
const CROSS: f32 = 12.0;
const CROSS_GAP: f32 = 6.0;
/// The amber accent behind the note, as on the design page.
const TINT: f32 = 0.14;

/// The note being typed in its box on the row.
pub(super) struct NoteEdit {
    pub id: String,
    pub text: String,
    /// The text is to be selected whole on the next frame.
    pub fresh: bool,
    /// The box has had the keyboard; losing it again saves.
    pub focused: bool,
}

/// A chat that can have a note: not a stand-in, not a hidden chat, and
/// not a fork that runs in a terminal (its menu stays short).
pub fn can_note(line: &ChatLine) -> bool {
    !line.stand_in && !line.hidden && line.chat.terminal.is_none()
}

/// The text as it is saved: one line, no spaces at the ends, at most
/// `LIMIT` characters.
pub fn clean(text: &str) -> String {
    let one_line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    one_line.trim().chars().take(LIMIT).collect()
}

/// Saves a chat's note. An empty text changes nothing.
pub fn set(saved: &mut Saved, id: &str, text: &str) {
    let text = clean(text);
    if !text.is_empty() {
        saved.notes.insert(id.to_string(), text);
    }
}

pub fn remove(saved: &mut Saved, id: &str) {
    saved.notes.remove(id);
}

/// Opens the box on the chat's row, with its note (if any) selected.
pub(super) fn start(state: &mut DeckUi, id: &str) {
    state.selected = Some(super::window::Pick::Chat(id.to_string()));
    state.noting = Some(NoteEdit {
        id: id.to_string(),
        text: state.saved.notes.get(id).cloned().unwrap_or_default(),
        fresh: true,
        focused: false,
    });
}

/// The extra height of a row whose box is open but has no note yet.
pub(super) fn extra(line: &Line, noting: Option<&str>) -> f32 {
    match line {
        Line::Chat(c) if c.note.is_none() && noting == Some(c.chat.id.as_str()) => rows::NOTE,
        _ => 0.0,
    }
}

/// A row being noted was scrolled out of view: the note is saved and the
/// box closes, so the list's keys work again.
pub(super) fn end_hidden(line: &Line, state: &mut DeckUi) {
    let Line::Chat(chat) = line else {
        return;
    };
    if let Some(edit) = state.noting.take_if(|n| n.id == chat.chat.id) {
        set(&mut state.saved, &edit.id, &edit.text);
    }
}

/// Where the painted note is: its tinted rect and, while the mouse is on
/// the row, the × at its end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chip {
    pub rect: Rect,
    pub cross: Option<Rect>,
}

fn chip_rect(room: Rect, width: f32) -> Rect {
    let top = room.center().y - CHIP / 2.0;
    Rect::from_min_size(pos2(room.left(), top), vec2(width.min(room.width()), CHIP))
}

fn words_galley(painter: &Painter, words: &str, colour: Color32, width: f32) -> Arc<Galley> {
    let mut job: LayoutJob = text::job(words, theme::regular(SIZE), colour, None);
    job.wrap = TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    painter.layout_job(job)
}

fn laid(
    painter: &Painter,
    room: Rect,
    words: &str,
    tokens: &Tokens,
    cross: bool,
) -> (Chip, Arc<Galley>) {
    let cross_room = if cross { CROSS_GAP + CROSS } else { 0.0 };
    let fixed = PAD + ICON + ICON_GAP + cross_room + PAD;
    let galley = words_galley(painter, words, tokens.text_primary, room.width() - fixed);
    let rect = chip_rect(room, fixed + galley.size().x);
    let cross = cross.then(|| {
        Rect::from_center_size(
            pos2(rect.right() - PAD - CROSS / 2.0, rect.center().y),
            vec2(CROSS, CROSS),
        )
    });
    (Chip { rect, cross }, galley)
}

/// Where the note and its × are, without painting.
pub fn hit(painter: &Painter, room: Rect, words: &str, tokens: &Tokens, cross: bool) -> Chip {
    laid(painter, room, words, tokens, cross).0
}

/// Paints the note in `room` (from `rows::note_room`); `cross` adds the ×.
pub fn paint(painter: &Painter, room: Rect, words: &str, tokens: &Tokens, cross: bool) -> Chip {
    let (chip, galley) = laid(painter, room, words, tokens, cross);
    painter.rect_filled(chip.rect, 4.0, tokens.accent.gamma_multiply(TINT));
    icon(
        painter,
        pos2(chip.rect.left() + PAD, chip.rect.center().y),
        tokens.accent,
    );
    let x = chip.rect.left() + PAD + ICON + ICON_GAP;
    let y = chip.rect.center().y - galley.size().y / 2.0;
    painter.galley(pos2(x, y), galley, Color32::PLACEHOLDER);
    if let Some(cross) = chip.cross {
        cross_mark(painter, cross, tokens.text_secondary);
    }
    chip
}

/// The design page's note icon: a page with its corner turned, 12 px.
pub(super) fn icon(painter: &Painter, left_middle: Pos2, colour: Color32) {
    let scale = ICON / 24.0;
    let at = |x: f32, y: f32| {
        pos2(
            left_middle.x + x * scale,
            left_middle.y + (y - 12.0) * scale,
        )
    };
    let stroke = Stroke::new(1.3, colour);
    let page = vec![
        at(15.5, 3.0),
        at(4.0, 3.0),
        at(3.0, 4.0),
        at(3.0, 20.0),
        at(4.0, 21.0),
        at(20.0, 21.0),
        at(21.0, 20.0),
        at(21.0, 8.5),
        at(15.5, 3.0),
    ];
    painter.add(Shape::line(page, stroke));
    let corner = vec![at(15.0, 3.0), at(15.0, 8.0), at(16.0, 9.0), at(21.0, 9.0)];
    painter.add(Shape::line(corner, stroke));
}

fn cross_mark(painter: &Painter, rect: Rect, colour: Color32) {
    let r = rect.shrink(3.0);
    let stroke = Stroke::new(1.4, colour);
    painter.line_segment([r.left_top(), r.right_bottom()], stroke);
    painter.line_segment([r.right_top(), r.left_bottom()], stroke);
}

/// The count shown once something is typed: "34 / 120".
pub fn count(text: &str) -> String {
    format!("{} / {LIMIT}", text.chars().count())
}

/// The open box in `room`: Enter or a click elsewhere saves, Esc keeps the
/// old note, and an empty box changes nothing.
pub(super) fn field(ui: &mut Ui, room: Rect, state: &mut DeckUi, look: &Look) {
    let Some(edit) = state.noting.as_mut() else {
        return;
    };
    let tokens = look.tokens;
    let painter = ui.painter_at(room.expand(1.0));
    let rect = chip_rect(room, room.width());
    painter.rect_filled(rect, 4.0, tokens.accent.gamma_multiply(TINT));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, tokens.accent),
        egui::StrokeKind::Inside,
    );
    icon(
        &painter,
        pos2(rect.left() + PAD, rect.center().y),
        tokens.accent,
    );
    let right = if edit.text.is_empty() {
        KEYS_HINT.to_string()
    } else {
        count(&edit.text)
    };
    let hint = painter.layout_job(text::job(
        &right,
        theme::regular(SIZE),
        tokens.text_secondary,
        None,
    ));
    let hint_width = hint.size().x;
    painter.galley(
        pos2(
            rect.right() - PAD - hint_width,
            rect.center().y - hint.size().y / 2.0,
        ),
        hint,
        Color32::PLACEHOLDER,
    );
    let left = rect.left() + PAD + ICON + ICON_GAP;
    let boxed = Rect::from_min_max(
        pos2(left, rect.top()),
        pos2(rect.right() - PAD - hint_width - CROSS_GAP, rect.bottom()),
    );
    let hint_text = egui::RichText::new(HINT)
        .font(theme::regular(SIZE))
        .color(tokens.text_disabled);
    let text_edit = egui::TextEdit::singleline(&mut edit.text)
        .id_salt(("cd-note", edit.id.as_str()))
        .font(theme::regular(SIZE))
        .text_color(tokens.text_primary)
        .hint_text(hint_text)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .vertical_align(egui::Align::Center)
        .char_limit(LIMIT)
        .desired_width(boxed.width().max(0.0));
    let response = ui.put(boxed, text_edit);
    let has_focus = response.has_focus();
    if !edit.focused && !has_focus {
        response.request_focus();
    }
    if std::mem::take(&mut edit.fresh) {
        if let Some(mut text) = egui::TextEdit::load_state(ui.ctx(), response.id) {
            let end = egui::text::CCursor::new(edit.text.chars().count());
            text.cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    end,
                )));
            text.store(ui.ctx(), response.id);
        }
    }
    let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    let was_focused = edit.focused;
    edit.focused |= has_focus;
    if esc {
        state.noting = None;
        state.renamed = true;
    } else if enter || (was_focused && !has_focus) {
        state.renamed = true;
        let (id, text) = (edit.id.clone(), edit.text.clone());
        set(&mut state.saved, &id, &text);
        state.noting = None;
    }
}

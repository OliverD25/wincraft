//! A chat's Quick access number from QuickJump: the box with its digit on
//! the chat's row, and the asks the right-click menu sends. QuickJump
//! keeps the numbers; ClaudeDeck only reads them and asks through
//! `core::quick_pins`.

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Response, Stroke, StrokeKind};

use crate::core::quick_pins::{self, Ask};
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::marks;

const TEXT: f32 = 12.0;

/// The chat's place in Quick access; None for no number or QuickJump off.
pub fn slot_of(session: &str) -> Option<usize> {
    quick_pins::number_of(session).flatten()
}

/// For the menu: None while QuickJump is off, else the chat's place if
/// it has one.
pub fn state(session: &str) -> Option<Option<usize>> {
    quick_pins::number_of(session)
}

/// The tip of the number box, while the mouse is on it.
pub fn tip(response: &Response, quick_box: Option<Rect>, session: &str) {
    let on_box = quick_box
        .zip(response.hover_pos())
        .is_some_and(|(rect, at)| rect.contains(at));
    if let Some(slot) = on_box.then(|| slot_of(session)).flatten() {
        response.clone().on_hover_text(quick_pins::tip(slot));
    }
}

/// The number's box: the size of the keyboard digit's box, but outlined
/// in the accent colour with the digit in it, so the two never look
/// alike. Returns its rect.
pub fn paint(painter: &Painter, at: Pos2, slot: usize, tokens: &Tokens) -> Rect {
    let rect = Rect::from_min_size(at, vec2(marks::SIZE, marks::SIZE));
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, tokens.accent),
        StrokeKind::Inside,
    );
    let galley = painter.layout_job(text::job(
        &quick_pins::digit_of_slot(slot).to_string(),
        theme::regular(TEXT),
        tokens.accent,
        None,
    ));
    let size = galley.size();
    let corner = rect.center() - size / 2.0;
    painter.galley(pos2(corner.x, corner.y), galley, Color32::PLACEHOLDER);
    rect
}

/// The menu asked for a number: `at`, or the first free one when None.
pub fn put(session: &str, name: &str, window: Option<isize>, at: Option<usize>) {
    quick_pins::ask(Ask::Put {
        session: session.to_string(),
        name: name.to_string(),
        window,
        at,
    });
}

pub fn remove(session: &str) {
    quick_pins::ask(Ask::Remove {
        session: session.to_string(),
    });
}

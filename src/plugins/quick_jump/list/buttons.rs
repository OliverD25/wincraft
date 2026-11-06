//! The buttons of Quick access that the shared button cannot draw: one
//! with its key in a chip at the right end ("Unpin  Del", "Done  ↵"), and
//! one with a plus before the label.

use egui::{pos2, vec2, Align2, CursorIcon, Rect, Response, Sense, Stroke, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::button::Kind;
use crate::ui::widgets::focus;

const HEIGHT: f32 = 32.0;
const PAD_LEFT: f32 = 12.0;
const PAD_RIGHT_CHIP: f32 = 6.0;
const PAD_RIGHT: f32 = 12.0;
const GAP: f32 = 8.0;
const PLUS: f32 = 14.0;
const CHIP_H: f32 = 20.0;
const CHIP_PAD: f32 = 5.0;

/// What a button shows besides its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extra<'a> {
    /// The key that does the same, in a chip after the label.
    Key(&'a str),
    /// A plus before the label.
    Plus,
}

/// The width of a button: padding, label and what comes with it.
pub fn width(label_w: f32, extra: Extra, chip_w: f32) -> f32 {
    match extra {
        Extra::Key(_) => PAD_LEFT + label_w + GAP + chip_w + PAD_RIGHT_CHIP,
        Extra::Plus => PAD_LEFT + PLUS + GAP + label_w + PAD_RIGHT,
    }
}

pub fn draw(ui: &mut Ui, label: &str, kind: Kind, extra: Extra) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::regular(14.0), tokens.text_primary);
    let chip = match extra {
        Extra::Key(key) => Some(ui.painter().layout_no_wrap(
            key.to_string(),
            // The return arrow exists only in the monospace face.
            theme::mono(12.0),
            tokens.text_secondary,
        )),
        _ => None,
    };
    let chip_w = chip
        .as_ref()
        .map_or(0.0, |galley| galley.size().x + 2.0 * CHIP_PAD + 2.0);
    let size = vec2(width(galley.size().x, extra, chip_w), HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
        let (stroke, text, fill) = match kind {
            Kind::Primary => (
                tokens.accent,
                tokens.accent,
                tokens.accent_tint.gamma_multiply(hover),
            ),
            Kind::Secondary => (
                tokens.border,
                tokens.text_primary,
                tokens.hover_bg.gamma_multiply(hover),
            ),
            Kind::Danger => (
                tokens.danger,
                tokens.danger,
                tokens.hover_bg.gamma_multiply(hover),
            ),
        };
        let fill = if response.is_pointer_button_down_on() {
            tokens.selection_bg
        } else {
            fill
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            tokens.radius_row,
            fill,
            theme::line(ui.ctx(), stroke),
            StrokeKind::Inside,
        );
        let mut x = rect.left() + PAD_LEFT;
        if extra == Extra::Plus {
            let centre = pos2(x + PLUS / 2.0, rect.center().y);
            let line = Stroke::new(1.75, text);
            painter.hline(centre.x - 5.0..=centre.x + 5.0, centre.y, line);
            painter.vline(centre.x, centre.y - 5.0..=centre.y + 5.0, line);
            x += PLUS + GAP;
        }
        let label_pos = pos2(x, rect.center().y - galley.size().y / 2.0);
        let after = x + galley.size().x;
        painter.galley(label_pos, galley, text);
        if let Some(chip) = chip {
            let cap = Rect::from_min_size(
                pos2(after + GAP, rect.center().y - CHIP_H / 2.0),
                vec2(chip_w, CHIP_H),
            );
            painter.rect_stroke(
                cap,
                4,
                theme::line(ui.ctx(), tokens.border),
                StrokeKind::Inside,
            );
            painter.text(
                cap.center(),
                Align2::CENTER_CENTER,
                chip.text(),
                theme::mono(12.0),
                tokens.text_secondary,
            );
        }
        focus::ring(ui, &response, f32::from(tokens.radius_row));
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_chip_widens_the_button_by_its_own_width_and_gap() {
        let keyed = width(40.0, Extra::Key("Del"), 30.0);
        assert_eq!(keyed, 12.0 + 40.0 + 8.0 + 30.0 + 6.0);
        let plus = width(40.0, Extra::Plus, 0.0);
        assert_eq!(plus, 12.0 + 14.0 + 8.0 + 40.0 + 12.0);
    }
}

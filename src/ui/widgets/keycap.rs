use egui::{pos2, vec2, Rect, Response, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};

/// "Win+Alt+F1" → ["Win", "Alt", "F1"].
pub fn split(binding: &str) -> Vec<&str> {
    binding
        .split('+')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .collect()
}

/// One key: mono text, padding 1 6, 1 px border with a 2 px bottom edge,
/// radius 3. The thicker bottom edge is what makes it read as a key cap.
pub fn chip(ui: &mut Ui, key: &str, size: f32, accent: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let colour = if accent {
        tokens.accent
    } else {
        tokens.text_secondary
    };
    let stroke_colour = if accent { tokens.accent } else { tokens.border };
    let ppp = ui.ctx().pixels_per_point();
    let top = theme::snap(1.0, ppp);
    let bottom = theme::snap(2.0, ppp);

    let galley = ui
        .painter()
        .layout_no_wrap(key.to_string(), theme::mono(size), colour);
    let size = vec2(
        galley.size().x + 12.0 + top * 2.0,
        galley.size().y + 2.0 + top + bottom,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_stroke(
            rect,
            3,
            theme::stroke(ui.ctx(), 1.0, stroke_colour),
            StrokeKind::Inside,
        );
        let band = Rect::from_min_max(
            pos2(rect.left() + 3.0, rect.bottom() - bottom),
            pos2(rect.right() - 3.0, rect.bottom() - top),
        );
        painter.rect_filled(band, 0, stroke_colour);
        let text_pos = pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.top() + top + 1.0,
        );
        painter.galley(text_pos, galley, colour);
    }
    response
}

/// All keys of a binding, 4 px apart.
pub fn chips(ui: &mut Ui, binding: &str, size: f32, accent: bool) -> Response {
    let keys = split(binding);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.horizontal(|ui| {
            for key in keys {
                chip(ui, key, size, accent);
            }
        })
        .response
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_splits_into_one_chip_per_key() {
        assert_eq!(split("Win+Alt+F1"), vec!["Win", "Alt", "F1"]);
        assert_eq!(split("Ctrl + Shift + ,"), vec!["Ctrl", "Shift", ","]);
        assert!(split("").is_empty());
    }
}

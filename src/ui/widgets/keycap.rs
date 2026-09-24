use egui::{vec2, Color32, FontId, Response, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};

/// "Win+Alt+F1" → ["Win", "Alt", "F1"].
pub fn split(binding: &str) -> Vec<&str> {
    binding
        .split('+')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .collect()
}

/// "Win+Alt+F1" → "Win + Alt + F1", the way a chip spells a binding.
pub fn spaced(binding: &str) -> String {
    split(binding).join(" + ")
}

/// Modifiers held so far during a capture, open-ended: "Win + Alt +".
pub fn held(modifiers: &str) -> String {
    let keys = spaced(modifiers);
    if keys.is_empty() {
        keys
    } else {
        format!("{keys} +")
    }
}

/// One chip for a whole binding: 13 regular, padding 3 8, panel fill, 1 px
/// border, radius 4, secondary text. While capturing it is outlined and
/// written in the accent colour.
pub fn binding(ui: &mut Ui, text: &str, accent: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (text_colour, stroke) = if accent {
        (tokens.accent, tokens.accent)
    } else {
        (tokens.text_secondary, tokens.border)
    };
    chip(ui, text, theme::regular(13.0), text_colour, stroke)
}

/// The same chip at the footer's 12 px, in any font: the return arrow only
/// exists in egui's monospace fallback.
pub fn hint(ui: &mut Ui, text: &str, font: FontId) -> Response {
    let tokens = Tokens::get(ui.ctx());
    chip(ui, text, font, tokens.text_secondary, tokens.border)
}

fn chip(ui: &mut Ui, text: &str, font: FontId, text_colour: Color32, stroke: Color32) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, text_colour);
    let size = vec2(galley.size().x + 16.0, galley.size().y + 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect(
            rect,
            4,
            tokens.panel_bg,
            theme::stroke(ui.ctx(), 1.0, stroke),
            StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, text_colour);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_is_written_as_one_spaced_chip() {
        assert_eq!(split("Win+Alt+F1"), vec!["Win", "Alt", "F1"]);
        assert_eq!(spaced("Win+Alt+F1"), "Win + Alt + F1");
        assert_eq!(spaced("Ctrl + Shift + ,"), "Ctrl + Shift + ,");
        assert_eq!(spaced(""), "");
    }

    #[test]
    fn held_modifiers_stay_open_ended() {
        assert_eq!(held("Win+Alt"), "Win + Alt +");
        assert_eq!(held(""), "");
    }
}

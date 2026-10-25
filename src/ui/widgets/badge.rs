use egui::{vec2, Color32, Response, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};

/// 20 high outlined chip with 12 px text. The status lives in the colour of
/// the outline and the words, never in a fill.
pub fn badge(ui: &mut Ui, text: &str, stroke: Color32, text_colour: Color32) -> Response {
    sized(ui, text, stroke, text_colour, 20.0)
}

/// The store tile's badge: the same chip, 22 high.
pub fn tall(ui: &mut Ui, text: &str, stroke: Color32, text_colour: Color32) -> Response {
    sized(ui, text, stroke, text_colour, 22.0)
}

fn sized(ui: &mut Ui, text: &str, stroke: Color32, text_colour: Color32, height: f32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::regular(12.0), text_colour);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 16.0, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_stroke(
            rect,
            Tokens::get(ui.ctx()).radius_row,
            theme::line(ui.ctx(), stroke),
            StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() / 2.0, galley, text_colour);
    }
    response
}

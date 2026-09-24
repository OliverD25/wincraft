use egui::{vec2, Color32, Response, Sense, StrokeKind, Ui};

use crate::core::theme;

/// 20 high outlined chip with 11 px text. The status lives in the colour of
/// the outline and the words, never in a fill.
pub fn badge(ui: &mut Ui, text: &str, stroke: Color32, text_colour: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::lora(11.0), text_colour);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 16.0, 20.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_stroke(
            rect,
            4,
            theme::stroke(ui.ctx(), 1.0, stroke),
            StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() / 2.0, galley, text_colour);
    }
    response
}

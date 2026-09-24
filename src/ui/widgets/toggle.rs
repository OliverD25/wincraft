use egui::{pos2, vec2, CursorIcon, Response, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::focus;

const WIDTH: f32 = 40.0;
const HEIGHT: f32 = 20.0;
const KNOB: f32 = 12.0;
const INSET: f32 = 3.0;

/// 40×20 pill. The knob slides over 120 ms; the stroke colour switches at once,
/// so the state reads immediately and the motion only confirms it.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, mut response) = ui.allocate_exact_size(vec2(WIDTH, HEIGHT), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let position = ui
            .ctx()
            .animate_bool_with_time(response.id.with("knob"), *on, 0.12);
        let colour = if *on {
            tokens.accent
        } else {
            tokens.text_disabled
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            10,
            egui::Color32::TRANSPARENT,
            theme::stroke(ui.ctx(), 1.0, colour),
            StrokeKind::Inside,
        );
        let left = rect.left() + INSET + KNOB / 2.0;
        let right = rect.right() - INSET - KNOB / 2.0;
        let centre = pos2(left + (right - left) * position, rect.center().y);
        painter.circle_filled(centre, KNOB / 2.0, colour);
        focus::ring(ui, &response, 10.0);
    }

    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if ui.is_enabled() {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

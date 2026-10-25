use egui::{pos2, vec2, Color32, CursorIcon, Response, Sense, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::focus;

const HEIGHT: f32 = 30.0;
const PADDING: f32 = 14.0;
const GAP: f32 = 6.0;

/// A filter pill: the label in 13 px and the number of matches after it in
/// 12 px. The chosen one has accent text, an accent border and the selection
/// fill; the others are outlined only.
pub fn chip(ui: &mut Ui, label: &str, count: usize, selected: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let label_colour = if selected {
        tokens.accent
    } else {
        tokens.text_primary
    };
    let name = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::regular(13.0), label_colour);
    let number = ui.painter().layout_no_wrap(
        count.to_string(),
        theme::regular(12.0),
        tokens.text_disabled,
    );
    let width = PADDING * 2.0 + name.size().x + GAP + number.size().x;
    let (rect, response) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::click());

    if ui.is_rect_visible(rect) {
        let hover = ui.ctx().animate_bool_with_time(
            response.id.with("hover"),
            response.hovered() && !selected,
            0.08,
        );
        let (fill, stroke) = if selected {
            (tokens.selection_bg, tokens.accent)
        } else {
            (tokens.hover_bg.gamma_multiply(hover), tokens.border)
        };
        let painter = ui.painter();
        painter.rect(
            rect,
            HEIGHT / 2.0,
            fill,
            theme::line(ui.ctx(), stroke),
            StrokeKind::Inside,
        );
        let x = rect.left() + PADDING;
        painter.galley(
            pos2(x, rect.center().y - name.size().y / 2.0),
            name.clone(),
            Color32::PLACEHOLDER,
        );
        painter.galley(
            pos2(
                x + name.size().x + GAP,
                rect.center().y - number.size().y / 2.0,
            ),
            number,
            Color32::PLACEHOLDER,
        );
        focus::ring(ui, &response, HEIGHT / 2.0);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

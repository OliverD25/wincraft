use egui::{Shape, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

/// A dashed box with a 15 px semibold title and one 12 px line. Dashed rather
/// than solid so it never reads as a row that failed to load its controls.
pub fn empty_state(ui: &mut Ui, title: &str, line: &str) {
    let tokens = Tokens::get(ui.ctx());
    let response = egui::Frame::NONE
        .inner_margin(egui::Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            text::single(
                ui,
                text::job(title, theme::semibold(15.0), tokens.text_primary, None),
            );
            text::caption(ui, line, tokens.text_secondary);
        })
        .response;

    let rect = response.rect.shrink(0.5);
    let stroke = theme::stroke(ui.ctx(), 1.0, tokens.border);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    ui.painter()
        .extend(Shape::dashed_line(&corners, stroke, 4.0, 3.0));
}

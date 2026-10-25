use egui::{
    pos2, vec2, Align, Id, Layout, Margin, Rect, Response, Sense, StrokeKind, TextEdit, Ui,
};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::keycap;

const HEIGHT: f32 = 36.0;
const ICON: f32 = 16.0;
const SIDE: f32 = 10.0;
const HINT: &str = "Ctrl+F";

/// 36 high, 1 px border, radius 4: a search icon, the text, and a "Ctrl+F"
/// chip at the right that tells where the shortcut is. The border turns
/// accent while it has focus. `id` lets the page focus it on Ctrl+F.
pub fn search_box(
    ui: &mut Ui,
    id: Id,
    query: &mut String,
    placeholder: &str,
    width: f32,
) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover());

    let hint_width = ui
        .painter()
        .layout_no_wrap(
            HINT.to_string(),
            theme::regular(12.0),
            tokens.text_secondary,
        )
        .size()
        .x
        + 16.0;
    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + SIDE + ICON / 2.0, rect.center().y),
        vec2(ICON, ICON),
    );
    let edit_rect = Rect::from_min_max(
        pos2(icon_rect.right() + 8.0, rect.top()),
        pos2(rect.right() - SIDE - hint_width - 8.0, rect.bottom()),
    );

    let hint = egui::RichText::new(placeholder)
        .font(theme::regular(14.0))
        .color(tokens.text_disabled);
    let edit = TextEdit::singleline(query)
        .id(id)
        .font(theme::regular(14.0))
        .text_color(tokens.text_primary)
        .hint_text(hint)
        .frame(egui::Frame::NONE)
        .margin(Margin::symmetric(0, 0))
        .vertical_align(Align::Center)
        .desired_width(edit_rect.width());
    let response = ui.put(edit_rect, edit);

    let border = if response.has_focus() {
        tokens.accent
    } else {
        tokens.border
    };
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        tokens.radius_row,
        theme::line(ui.ctx(), border),
        StrokeKind::Inside,
    );
    icons::paint(painter, icon_rect, Icon::Search, tokens.text_disabled);

    let hint_rect = Rect::from_min_max(
        pos2(rect.right() - SIDE - hint_width, rect.top()),
        pos2(rect.right() - SIDE, rect.bottom()),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(hint_rect)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| keycap::hint(ui, HINT, theme::regular(12.0)),
    );
    response
}

//! How a row nested under the best-match app is drawn: 32 px high,
//! indented 40 px, on a 1 px guide line, with a small window glyph and the
//! window's title only (design "Open or Switch Options", 1a and 3a).

use super::*;
use crate::search::Nested;

pub(super) const NESTED_HEIGHT: f32 = 32.0;
const INDENT: f32 = 40.0;
const GLYPH: f32 = 12.0;

/// One nested row. `last` ends the guide line at the row's middle.
pub(super) fn nested_row(
    ui: &mut egui::Ui,
    entry: &ResultItem,
    look: &RowLook,
    last: bool,
) -> RowResponse {
    let tokens = Tokens::get(ui.ctx());
    let inert = entry.nested == Some(Nested::NoMatch);
    let sense = if look.interactive && !inert {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), NESTED_HEIGHT), sense);
    let answer = RowResponse {
        clicked: response.clicked(),
        shift: response.clicked() && ui.input(|input| input.modifiers.shift),
        rect,
    };
    if !ui.is_rect_visible(rect) {
        return answer;
    }
    let painter = ui.painter().clone();
    // The guide runs down from the app's icon.
    let guide_x = rect.left() + ROW_PADDING + ICON_SLOT / 2.0;
    let guide_end = if last { rect.center().y } else { rect.bottom() };
    painter.vline(
        guide_x,
        rect.top()..=guide_end,
        theme::line(ui.ctx(), tokens.border),
    );
    let body = Rect::from_min_max(pos2(rect.left() + INDENT - 8.0, rect.top()), rect.max);
    if look.selected {
        row::paint_selection(ui, body);
    } else if response.hovered() && !inert {
        painter.rect_filled(body, tokens.radius_row, tokens.hover_bg);
    }
    let mut left = rect.left() + INDENT;
    let window = matches!(entry.nested, Some(Nested::Window { .. }));
    if window {
        let glyph = Rect::from_center_size(
            pos2(left + GLYPH / 2.0, rect.center().y),
            vec2(GLYPH, GLYPH - 2.0),
        );
        let colour = if look.selected {
            tokens.text_primary
        } else {
            tokens.text_secondary
        };
        painter.rect_stroke(glyph, 2, theme::line(ui.ctx(), colour), StrokeKind::Inside);
        painter.hline(
            glyph.x_range(),
            glyph.top() + 3.0,
            theme::line(ui.ctx(), colour),
        );
        left = glyph.right() + 10.0;
    }

    let inner = Rect::from_min_max(
        pos2(left, rect.top()),
        pos2(rect.right() - ROW_PADDING, rect.bottom()),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let mut right = inner.right();
    if let Some(verb) = look.verb.filter(|_| look.selected) {
        right = enter_pill(&mut child, verb).left() - 12.0;
    } else if !entry.hint.is_empty() {
        right = keycap::binding(&mut child, &keycap::spaced(&entry.hint), false)
            .rect
            .left()
            - 12.0;
    }

    let colour = if inert {
        tokens.text_disabled
    } else if look.selected {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    let mut title = label_job(
        &entry.title,
        if window { look.needle } else { "" },
        theme::regular(13.0),
        colour,
        TITLE_LINE,
        &tokens,
    );
    if !window && !entry.subtitle.is_empty() {
        let mut buffer = String::from("   ");
        buffer.push_str(&entry.subtitle);
        title.append(
            &buffer,
            0.0,
            text::format(theme::regular(12.0), tokens.text_disabled, Some(TITLE_LINE)),
        );
    }
    title.wrap = one_line((right - left).max(0.0));
    let title = painter.layout_job(title);
    painter.galley(
        pos2(left, rect.center().y - title.size().y / 2.0),
        title,
        colour,
    );
    answer
}

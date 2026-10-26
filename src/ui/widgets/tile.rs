use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align, Color32, CursorIcon, Id, Layout, Rect, Response, Sense, StrokeKind, Ui,
    UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::text;

/// Every tile has the same height, so a row of tiles lines up whatever the
/// text: 14 padding, name 20, 3, two description lines of 17.5, 14 padding.
pub const HEIGHT: f32 = 86.0;
pub const MIN_WIDTH: f32 = 290.0;
pub const GAP: f32 = 12.0;

const PAD: f32 = 14.0;
const PART_GAP: f32 = 14.0;
const ICON_BOX: f32 = 52.0;
const ICON_BOX_RADIUS: u8 = 12;
const GLYPH: f32 = 26.0;
const NAME_ROW: f32 = 20.0;
const NAME_TO_DESCRIPTION: f32 = 3.0;
const DESCRIPTION_LINE: f32 = 17.5;

pub struct Tile<'a> {
    pub icon: Icon,
    pub name: &'a str,
    pub description: &'a str,
}

/// How many tiles fit in a row of `width`, as CSS `auto-fill` with a 290
/// minimum would count them.
pub fn columns(width: f32) -> usize {
    (((width + GAP) / (MIN_WIDTH + GAP)).floor() as usize).max(1)
}

/// The width each tile grows to so a row fills `width`.
pub fn tile_width(width: f32, columns: usize) -> f32 {
    (width - GAP * (columns.saturating_sub(1)) as f32) / columns.max(1) as f32
}

fn clamped(
    text: &str,
    font: egui::FontId,
    colour: Color32,
    line: f32,
    width: f32,
    rows: usize,
) -> LayoutJob {
    let mut job = text::job(text, font, colour, Some(line));
    job.wrap.max_width = width.max(1.0);
    job.wrap.max_rows = rows;
    job
}

/// One tile in `rect`: the icon box, the text, and at the right end whatever
/// `control` draws. The whole tile is a click target; the control sits on top
/// of it and takes its own clicks. `selected` paints the keyboard-chosen tile
/// with the accent border and the selection fill, which also shows where focus
/// is.
pub fn show(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    tile: &Tile,
    selected: bool,
    control: impl FnOnce(&mut Ui),
) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let hover =
        ui.ctx()
            .animate_bool_with_time(id.with("hover"), response.hovered() && !selected, 0.08);
    let painter = ui.painter().clone();

    let (fill, border) = if selected {
        (tokens.selection_bg, tokens.accent)
    } else {
        (
            tokens.hover_bg.gamma_multiply(hover),
            tokens.border.lerp_to_gamma(tokens.text_disabled, hover),
        )
    };
    painter.rect(
        rect,
        tokens.radius_tile,
        fill,
        theme::line(ui.ctx(), border),
        StrokeKind::Inside,
    );

    let inner = rect.shrink(PAD);
    let icon_box = Rect::from_center_size(
        pos2(inner.left() + ICON_BOX / 2.0, inner.center().y),
        vec2(ICON_BOX, ICON_BOX),
    );
    let glyph = Rect::from_center_size(icon_box.center(), vec2(GLYPH, GLYPH));
    if tokens.icon_tiles {
        icons::paint_on_tile(
            &painter,
            icon_box,
            glyph,
            tile.icon,
            f32::from(ICON_BOX_RADIUS),
        );
    } else {
        if tokens.tile_fill {
            painter.rect_filled(icon_box, ICON_BOX_RADIUS, tokens.panel_bg);
        }
        painter.rect_stroke(
            icon_box,
            ICON_BOX_RADIUS,
            theme::line(ui.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        icons::paint_weighted(&painter, glyph, tile.icon, tokens.accent, 1.5);
    }

    // The control is drawn first so the text gets the room that is left.
    let placed = ui.scope_builder(
        UiBuilder::new()
            .id_salt(id.with("control"))
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
        control,
    );
    let control_left = placed.response.rect.left();
    let right = if placed.response.rect.width() > 0.5 {
        control_left - PART_GAP
    } else {
        inner.right()
    };

    let left = icon_box.right() + PART_GAP;
    let width = right - left;
    let block = NAME_ROW + NAME_TO_DESCRIPTION + DESCRIPTION_LINE * 2.0;
    let top = inner.center().y - block / 2.0;
    let name = painter.layout_job(clamped(
        tile.name,
        theme::semibold(14.0),
        tokens.text_primary,
        NAME_ROW,
        width,
        1,
    ));
    painter.galley(pos2(left, top), name, tokens.text_primary);
    let description = painter.layout_job(clamped(
        tile.description,
        theme::regular(12.5),
        tokens.text_secondary,
        DESCRIPTION_LINE,
        width,
        2,
    ));
    painter.galley(
        pos2(left, top + NAME_ROW + NAME_TO_DESCRIPTION),
        description,
        tokens.text_secondary,
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fits_as_many_290_wide_tiles_as_the_row_allows() {
        assert_eq!(columns(100.0), 1);
        assert_eq!(columns(290.0), 1);
        assert_eq!(columns(591.0), 1);
        assert_eq!(columns(592.0), 2);
        assert_eq!(columns(894.0), 3);
        assert_eq!(columns(1196.0), 4);
    }

    #[test]
    fn tiles_grow_to_fill_the_row_and_never_below_the_minimum() {
        for width in [290.0, 592.0, 700.0, 894.0, 1000.0] {
            let n = columns(width);
            let each = tile_width(width, n);
            assert!(each >= MIN_WIDTH - 0.01, "{each} at {width}");
            let used = each * n as f32 + GAP * (n - 1) as f32;
            assert!((used - width).abs() < 0.01);
        }
    }
}

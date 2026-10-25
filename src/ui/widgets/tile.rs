use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, CursorIcon, Id, Layout, Rect, Response, Sense,
    StrokeKind, Ui, UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::text;

/// Every tile has the same height, so a row of tiles lines up whatever the
/// text: 132 picture, 14 padding, name 20, meta 18, 10, description 58, 10,
/// footer 41 and 16 padding.
pub const HEIGHT: f32 = 320.0;
pub const MIN_WIDTH: f32 = 260.0;
pub const GAP: f32 = 16.0;

const BAND: f32 = 132.0;
const SIDE: f32 = 16.0;
const NAME_ROW: f32 = 20.0;
const DESCRIPTION: f32 = 58.5;
const FOOTER_PAD: f32 = 12.0;
const FOOTER_ROW: f32 = 28.0;
const SWITCH_ROOM: f32 = 48.0;

pub struct Tile<'a> {
    pub icon: Icon,
    pub name: &'a str,
    pub meta: &'a str,
    pub description: &'a str,
    pub category: &'a str,
    /// Leave room beside the name for the control drawn for `Part::Header`.
    pub switch: bool,
}

/// The two places a tile lets its owner draw controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Right of the name.
    Header,
    /// Right end of the footer.
    Footer,
}

/// How many tiles fit in a row of `width`, as CSS `auto-fill` with a 260
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

/// One tile in `rect`. The whole tile is a click target; the controls that
/// `controls` draws for each `Part` sit on top of it and take their own
/// clicks. `selected` paints the keyboard-chosen tile with
/// the accent border and the selection fill, which also shows where focus is.
pub fn show(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    tile: &Tile,
    selected: bool,
    mut controls: impl FnMut(&mut Ui, Part),
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
    let line = theme::stroke(ui.ctx(), 1.0, tokens.border);
    let painter = ui.painter().clone();

    let (fill, border) = if selected {
        (tokens.selection_bg, tokens.accent)
    } else {
        (tokens.hover_bg.gamma_multiply(hover), tokens.border)
    };
    painter.rect(
        rect,
        6,
        fill,
        theme::stroke(ui.ctx(), 1.0, border),
        StrokeKind::Inside,
    );

    let band = Rect::from_min_size(rect.min, vec2(rect.width(), BAND)).shrink(0.5);
    painter.rect_filled(
        band,
        CornerRadius {
            nw: 5,
            ne: 5,
            sw: 0,
            se: 0,
        },
        tokens.panel_bg,
    );
    painter.hline(
        band.x_range(),
        band.bottom() - line.width / 2.0,
        theme::stroke(ui.ctx(), 1.0, border),
    );
    icons::paint_weighted(
        &painter,
        Rect::from_center_size(band.center(), vec2(40.0, 40.0)),
        tile.icon,
        tokens.accent,
        1.5,
    );

    let left = rect.left() + SIDE;
    let inner = rect.width() - SIDE * 2.0;
    let name_top = rect.top() + BAND + 14.0;
    let name_width = if tile.switch {
        inner - SWITCH_ROOM
    } else {
        inner
    };
    let name = painter.layout_job(clamped(
        tile.name,
        theme::semibold(15.0),
        tokens.text_primary,
        NAME_ROW,
        name_width,
        1,
    ));
    painter.galley(pos2(left, name_top), name, tokens.text_primary);
    let meta = painter.layout_job(clamped(
        tile.meta,
        theme::mono(12.0),
        tokens.text_disabled,
        16.0,
        inner,
        1,
    ));
    painter.galley(
        pos2(left, name_top + NAME_ROW + 2.0),
        meta,
        tokens.text_disabled,
    );

    let description_top = name_top + NAME_ROW + 2.0 + 16.0 + 10.0;
    let description = painter.layout_job(clamped(
        tile.description,
        theme::regular(13.0),
        tokens.text_secondary,
        19.5,
        inner,
        3,
    ));
    painter.galley(
        pos2(left, description_top),
        description,
        tokens.text_secondary,
    );

    let footer_top = description_top + DESCRIPTION + 10.0;
    painter.hline(left..=left + inner, footer_top + line.width / 2.0, line);

    let row_top = footer_top + FOOTER_PAD;
    if tile.switch {
        let switch_rect = Rect::from_min_size(pos2(left, name_top), vec2(inner, NAME_ROW));
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(id.with("header"))
                .max_rect(switch_rect)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| controls(ui, Part::Header),
        );
    }
    let footer_rect = Rect::from_min_size(pos2(left, row_top), vec2(inner, FOOTER_ROW));
    let placed = ui.scope_builder(
        UiBuilder::new()
            .id_salt(id.with("footer"))
            .max_rect(footer_rect)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| controls(ui, Part::Footer),
    );
    let room = (placed.response.rect.left() - left - 8.0).min(inner);
    if room > 24.0 {
        let category = painter.layout_job(clamped(
            tile.category,
            theme::regular(12.0),
            tokens.text_disabled,
            16.0,
            room,
            1,
        ));
        painter.galley(
            pos2(left, footer_rect.center().y - category.size().y / 2.0),
            category,
            tokens.text_disabled,
        );
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_fits_as_many_260_wide_tiles_as_the_row_allows() {
        assert_eq!(columns(100.0), 1);
        assert_eq!(columns(260.0), 1);
        assert_eq!(columns(535.0), 1);
        assert_eq!(columns(536.0), 2);
        assert_eq!(columns(812.0), 3);
        assert_eq!(columns(1088.0), 4);
    }

    #[test]
    fn tiles_grow_to_fill_the_row_and_never_below_the_minimum() {
        for width in [260.0, 536.0, 700.0, 812.0, 1000.0] {
            let n = columns(width);
            let each = tile_width(width, n);
            assert!(each >= MIN_WIDTH - 0.01, "{each} at {width}");
            let used = each * n as f32 + GAP * (n - 1) as f32;
            assert!((used - width).abs() < 0.01);
        }
    }
}

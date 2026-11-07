//! The compact card: the number and the title on their own line on top, the
//! picture below on a box as wide as the card. The compact popup draws its
//! cards with it, and so does the strip of a monitor that is not the main
//! one when all its cards fit on one line.

use super::*;

/// The room around a card's title line and picture.
pub(super) const CARD_PAD: f32 = 6.0;
/// The height of the title line, the number chip's size.
pub(super) const KEYCAP: f32 = 22.0;
/// A picture in a monitor's shape is never flatter or taller than these.
pub(super) const THUMB_MIN: f32 = 90.0;
pub(super) const THUMB_MAX: f32 = 240.0;

/// How tall a picture `width` wide is in the shape of a monitor `size` big,
/// held between `THUMB_MIN` and `THUMB_MAX`.
pub fn thumb_height(width: f32, size: Option<Vec2>) -> f32 {
    let shape = size
        .filter(|size| size.x > 0.0 && size.y > 0.0)
        .unwrap_or(DEFAULT_SHAPE);
    (width * shape.y / shape.x)
        .round()
        .clamp(THUMB_MIN, THUMB_MAX)
}

/// A card's height for a picture `thumb` tall.
pub(super) fn card_height(thumb: f32) -> f32 {
    CARD_PAD + KEYCAP + CARD_PAD + thumb + CARD_PAD
}

/// Where a card whose top is `top` starts its title row and its picture.
pub(super) fn card_rows(top: f32) -> (f32, f32) {
    let title = top + CARD_PAD;
    (title, title + KEYCAP + CARD_PAD)
}

/// The box the picture sits on, in a card `rect` with pictures `thumb` tall.
pub(super) fn picture_box(rect: Rect, thumb: f32) -> Rect {
    let (_, thumb_top) = card_rows(rect.top());
    Rect::from_min_size(
        pos2(rect.left() + CARD_PAD, thumb_top),
        vec2(rect.width() - 2.0 * CARD_PAD, thumb),
    )
}

/// The colours a card is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fill {
    /// The compact popup's: no fill but under the mouse or the keys.
    Popup,
    /// The strip's: the panel colour, the raised one when selected, and the
    /// picture on the window colour.
    Strip,
}

/// How one card is drawn.
#[derive(Clone, Copy)]
pub(super) struct Look<'a> {
    pub thumb: f32,
    pub number: usize,
    /// The card the keys are on.
    pub picked: bool,
    /// Another desktop's window that is not picked.
    pub faded: bool,
    /// Under a mouse that has moved.
    pub hovered: bool,
    /// The window wants attention: the orange tint and frame.
    pub attention: bool,
    pub fill: Fill,
    /// A live picture is put on the box later; without one the box shows
    /// the window's name.
    pub has_picture: bool,
    /// The picture's and the name card's opacity.
    pub opacity: f32,
    /// The "Same title" badge, and whether it can offer to rename. None
    /// without the badge.
    pub same_title: Option<bool>,
    /// "Waiting for <app>": the window did not close when asked.
    pub waiting: Option<&'a str>,
}

/// What `paint` drew.
pub(super) struct Painted {
    /// Where the live picture goes.
    pub picture: Rect,
    /// The badges' rectangle, kept clear of the picture.
    pub clear: Rect,
    /// The "Same title" badge was clicked to rename.
    pub renamed: bool,
    /// The title is a text box after F2.
    pub editing: bool,
}

/// Paints one card in `rect` with `painter` (faded as the card fades). The
/// card's `response` owns the badges' tooltips; `edit` is the title box
/// after F2.
pub(super) fn paint(
    ui: &mut Ui,
    painter: &egui::Painter,
    response: &Response,
    rect: Rect,
    window: &ArrangeWindow,
    look: Look,
    edit: Option<&mut rename::Rename>,
) -> Painted {
    let tokens = Tokens::get(ui.ctx());
    paint_back(ui.ctx(), painter, rect, &tokens, look);
    // The title row is first and the picture last, so the pictures of all
    // cards sit on one line at the bottom.
    let (title_top, _) = card_rows(rect.top());
    let thumb = picture_box(rect, look.thumb);
    let box_fill = match look.fill {
        Fill::Popup => tokens.panel_bg,
        Fill::Strip => tokens.window_bg,
    };
    painter.rect(
        thumb,
        tokens.radius_row,
        box_fill,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let picture = thumb.shrink(1.0);
    let mut badges = Vec::new();
    let waiting = look.waiting;
    if waiting.is_some() {
        badges.push(Badge::Waiting);
    }
    if look.same_title.is_some() {
        badges.push(Badge::SameTitle);
    }
    let shown = paint_badges(
        ui,
        painter,
        response.id,
        thumb,
        &badges,
        look.same_title.unwrap_or(false),
        waiting,
    );
    if !look.has_picture {
        let mut faded = painter.clone();
        faded.multiply_opacity(look.opacity);
        name_card(&faded, picture, &window.label, "");
    }
    let chip = number_chip(
        painter,
        pos2(rect.left() + CARD_PAD, title_top),
        KEYCAP,
        look.number,
    );
    let colour = if look.faded {
        tokens.text_secondary
    } else {
        tokens.text_primary
    };
    let left = chip.right() + 6.0;
    let mut painted = Painted {
        picture,
        clear: shown.clear,
        renamed: shown.renamed,
        editing: false,
    };
    // F2: the title is a text box, which owns the mouse on this card.
    if let Some(edit) = edit {
        let field = Rect::from_min_max(
            pos2(left, title_top),
            pos2(rect.right() - CARD_PAD, title_top + KEYCAP),
        );
        edit.show(ui, field, 13.0);
        painted.editing = true;
        return painted;
    }
    let mut job = text::job(&window.label, theme::regular(13.0), colour, None);
    job.wrap = TextWrapping {
        max_width: (rect.right() - CARD_PAD - left).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    let elided = galley.elided;
    painter.galley(
        pos2(left, title_top + (KEYCAP - galley.size().y) / 2.0),
        galley,
        colour,
    );
    if elided {
        title_tooltip(response, title_top, &window.label);
    }
    painted
}

/// The card's fill, the attention look and the selected ring.
fn paint_back(
    ctx: &egui::Context,
    painter: &egui::Painter,
    rect: Rect,
    tokens: &Tokens,
    look: Look,
) {
    let radius = tokens.radius_tile;
    let lit = match look.fill {
        Fill::Popup if look.picked => Some(selected_fill(tokens)),
        Fill::Popup if look.hovered => Some(tokens.hover_bg),
        Fill::Popup => None,
        Fill::Strip => {
            let base = if look.picked {
                tokens.elevated_bg
            } else {
                tokens.panel_bg
            };
            painter.rect_filled(rect, radius, base);
            (look.hovered && !look.picked).then_some(tokens.hover_bg)
        }
    };
    // The compact popup has never had the attention look.
    let attention = look.attention && look.fill == Fill::Strip;
    paint_look(painter, rect, radius, card_look(tokens, lit, attention));
    if look.picked {
        paint_ring(painter, ctx, rect, radius, selected_ring(tokens));
    }
}

/// The full title as a tooltip under the title row. Anchored to the row and
/// not to the whole card: the card ends at the bottom of the popup, so a
/// tooltip under it would not fit and would open upwards over the button.
fn title_tooltip(card: &Response, title_top: f32, title: &str) {
    let mut row = card.clone();
    row.rect = Rect::from_min_size(
        pos2(card.rect.left(), title_top),
        vec2(card.rect.width(), KEYCAP),
    );
    egui::Tooltip::for_enabled(&row).show(|ui| {
        ui.set_max_width(ui.spacing().tooltip_width);
        ui.add(egui::Label::new(title));
    });
}

//! The token mark on a chat row, as the **Token mark** setting picks it
//! (design section 16): a thin line at the bottom of the row as long as
//! the chat's share (the chat that used most fills the row), a grey fill
//! behind the row from the left, or four small signal bars in the gap
//! left of the status mark. Mini mode shows none.

use egui::{pos2, vec2, Painter, Rect};

use crate::core::theme::Tokens;
use crate::plugins::claude_deck::token_mark::{level, Style};

use super::view::Share;

/// The line's height and its room from the row's sides.
pub const HEIGHT: f32 = 2.0;
const SIDE: f32 = 12.0;
/// Even the smallest share shows this much.
const LEAST: f32 = 2.0;
/// The fill: the secondary text colour at 9%.
const FILL_STRENGTH: f32 = 0.09;
/// Signal bars: 2 px wide, 1 px apart, 3, 6, 9 and 12 px high.
pub const SIGNAL_W: f32 = 2.0;
const SIGNAL_GAP: f32 = 1.0;
const SIGNAL_STEP: f32 = 3.0;
/// From the row's left edge, inside its 12 px padding.
const SIGNAL_LEFT: f32 = 1.0;

/// Where the line goes in a row, for a length of 0 to 1.
pub fn rect(row: Rect, length: f32) -> Rect {
    let room = (row.width() - 2.0 * SIDE).max(0.0);
    let width = (room * length.clamp(0.0, 1.0)).max(LEAST.min(room));
    Rect::from_min_size(
        pos2(row.left() + SIDE, row.bottom() - HEIGHT - 1.0),
        vec2(width, HEIGHT),
    )
}

/// The fill: the row's full height, from its left edge.
pub fn fill_rect(row: Rect, length: f32) -> Rect {
    Rect::from_min_size(
        row.min,
        vec2(row.width() * length.clamp(0.0, 1.0), row.height()),
    )
}

/// The four signal bars standing on `bottom`, and how many are lit.
pub fn signal_rects(row: Rect, bottom: f32, percent: f64) -> ([Rect; 4], u8) {
    let rects = std::array::from_fn(|at| {
        let height = SIGNAL_STEP * (at + 1) as f32;
        let x = row.left() + SIGNAL_LEFT + at as f32 * (SIGNAL_W + SIGNAL_GAP);
        Rect::from_min_size(pos2(x, bottom - height), vec2(SIGNAL_W, height))
    });
    (rects, level(percent))
}

/// What goes behind the row's content: the fill.
pub fn under(painter: &Painter, row: Rect, style: Style, share: Option<Share>, tokens: &Tokens) {
    if let (Style::Fill, Some(share)) = (style, share) {
        painter.rect_filled(
            fill_rect(row, share.length),
            0.0,
            tokens.text_secondary.gamma_multiply(FILL_STRENGTH),
        );
    }
}

/// What goes over the row: the line or the signal bars. `mark_bottom` is
/// the bottom of the status mark, where the bars stand.
pub fn over(
    painter: &Painter,
    row: Rect,
    mark_bottom: f32,
    style: Style,
    share: Option<Share>,
    tokens: &Tokens,
) {
    let Some(share) = share else {
        return;
    };
    match style {
        Style::Line => {
            painter.rect_filled(
                rect(row, share.length),
                HEIGHT / 2.0,
                tokens.accent.gamma_multiply(0.7),
            );
        }
        Style::Signal => {
            let (bars, lit) = signal_rects(row, mark_bottom, share.percent);
            for (at, bar) in bars.into_iter().enumerate() {
                let colour = if (at as u8) < lit {
                    tokens.accent
                } else {
                    tokens.border
                };
                painter.rect_filled(bar, 1.0, colour);
            }
        }
        Style::Fill | Style::Off => {}
    }
}

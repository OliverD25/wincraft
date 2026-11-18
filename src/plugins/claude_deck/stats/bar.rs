//! The token bar: a thin line at the bottom of a chat row, as long as the
//! chat's share of the last 7 days (the chat that used most fills the
//! row). It is drawn only with the Token bar setting on.

use egui::{pos2, vec2, Painter, Rect};

use crate::core::theme::Tokens;

/// The bar's height and its room from the row's sides.
pub const HEIGHT: f32 = 2.0;
const SIDE: f32 = 12.0;
/// Even the smallest share shows this much.
const LEAST: f32 = 2.0;

/// Where the bar goes in a row, for a share of 0 to 1.
pub fn rect(row: Rect, share: f32) -> Rect {
    let room = (row.width() - 2.0 * SIDE).max(0.0);
    let width = (room * share.clamp(0.0, 1.0)).max(LEAST.min(room));
    Rect::from_min_size(
        pos2(row.left() + SIDE, row.bottom() - HEIGHT - 1.0),
        vec2(width, HEIGHT),
    )
}

/// Draws a chat row's bar, when it has a share.
pub fn draw(painter: &Painter, row: Rect, share: Option<f32>, tokens: &Tokens) {
    if let Some(share) = share {
        painter.rect_filled(
            rect(row, share),
            HEIGHT / 2.0,
            tokens.accent.gamma_multiply(0.7),
        );
    }
}

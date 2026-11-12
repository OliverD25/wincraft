//! The hover panel (design section 8): resting the mouse on a chat shows
//! its full name, status and time, its project and branch, the full
//! question or last result, and "No open window" when a click would do
//! nothing. Painted on the tooltip layer, inside ClaudeDeck's window.

use std::sync::Arc;

use egui::epaint::Shadow;
use egui::{pos2, vec2, Color32, Context, Galley, Id, LayerId, Order, Painter, Rect, Stroke};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::labels::{status_words, time_label, NO_WINDOW};
use super::marks;
use super::model::{group_name, Chat, NEW_GROUP};
use super::rows::Look;
use super::MarkStyle;

/// The mouse rests this long before the panel shows, in seconds.
pub const DELAY: f32 = 0.5;
const WIDTH: f32 = 300.0;
const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 10.0;
const RADIUS: u8 = 8;
/// From the row: the panel's left edge in, and its gap below.
const IN_FROM_LEFT: f32 = 60.0;
const BELOW: f32 = 6.0;
/// The panel keeps this far from the window's edges.
const MARGIN: f32 = 6.0;
const HEAD_SIZE: f32 = 12.0;
const BODY_SIZE: f32 = 13.0;
const BODY_LINE: f32 = 13.0 * 1.45;
const GAP: f32 = 6.0;

/// The panel's first line: "WinCraft design · asks you · 4 min".
pub fn head_line(chat: &Chat, now: u64) -> String {
    format!(
        "{} \u{b7} {} \u{b7} {}",
        chat.title,
        status_words(chat.mark),
        time_label(chat.since, now)
    )
}

/// The project and the branch, as far as they are known.
pub fn place_line(chat: &Chat) -> String {
    let project = chat
        .folder
        .as_deref()
        .map_or_else(|| NEW_GROUP.to_string(), group_name);
    match &chat.branch {
        Some(branch) => format!("{project} \u{b7} {branch}"),
        None => project,
    }
}

fn wrapped(painter: &Painter, words: &str, size: f32, colour: Color32, width: f32) -> Arc<Galley> {
    let line = (size == BODY_SIZE).then_some(BODY_LINE);
    let mut job = text::job(words, theme::regular(size), colour, line);
    job.wrap.max_width = width.max(0.0);
    painter.layout_job(job)
}

/// Paints the panel for `chat`, hovered in `row`, inside `area` (the list).
pub fn show(ctx: &Context, row: Rect, area: Rect, chat: &Chat, look: &Look) {
    let tokens: &Tokens = look.tokens;
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("claude-deck-hover")));
    let width = WIDTH.min(area.width() - 2.0 * MARGIN).max(80.0);
    let inner = width - 2.0 * PAD_X;
    let head = wrapped(
        &painter,
        &head_line(chat, look.now),
        HEAD_SIZE,
        tokens.text_secondary,
        inner - marks::SIZE - GAP,
    );
    let place = wrapped(
        &painter,
        &place_line(chat),
        HEAD_SIZE,
        tokens.text_secondary,
        inner,
    );
    let body = chat
        .full
        .as_deref()
        .map(|words| wrapped(&painter, words, BODY_SIZE, tokens.text_primary, inner));
    let no_window = chat
        .window
        .is_none()
        .then(|| wrapped(&painter, NO_WINDOW, HEAD_SIZE, tokens.text_disabled, inner));
    let mut height = PAD_Y * 2.0 + head.size().y.max(marks::SIZE) + GAP + place.size().y;
    for part in body.iter().chain(no_window.iter()) {
        height += GAP + part.size().y;
    }
    let left = (row.left() + IN_FROM_LEFT)
        .min(area.right() - MARGIN - width)
        .max(area.left() + MARGIN);
    let mut top = row.bottom() + BELOW;
    if top + height > area.bottom() - MARGIN {
        top = (row.top() - BELOW - height).max(area.top() + MARGIN);
    }
    let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
    if tokens.shadows {
        let shadow = Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: tokens.palette_shadow,
        };
        painter.add(shadow.as_shape(rect, RADIUS));
    }
    painter.rect(
        rect,
        RADIUS,
        tokens.elevated_bg,
        Stroke::new(1.0, tokens.border),
        egui::StrokeKind::Inside,
    );
    let x = rect.left() + PAD_X;
    let mut y = rect.top() + PAD_Y;
    marks::paint(
        &painter,
        pos2(x, y),
        chat.mark,
        MarkStyle::Shapes,
        tokens,
        look.turn,
    );
    let head_height = head.size().y.max(marks::SIZE);
    painter.galley(pos2(x + marks::SIZE + GAP, y), head, Color32::PLACEHOLDER);
    y += head_height + GAP;
    let place_height = place.size().y;
    painter.galley(pos2(x, y), place, Color32::PLACEHOLDER);
    y += place_height;
    for part in body.into_iter().chain(no_window) {
        y += GAP;
        let part_height = part.size().y;
        painter.galley(pos2(x, y), part, Color32::PLACEHOLDER);
        y += part_height;
    }
}

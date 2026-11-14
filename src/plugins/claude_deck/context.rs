//! The context ring (design section 15): how full a chat's context is,
//! where 100% is the point where Claude Code compacts the chat. A 12 px
//! ring fills clockwise from the top over a faint track; its colour tells
//! the band. It sits just before a row's time, and the rings of all rows
//! line up in one column.

use std::f32::consts::{FRAC_PI_2, TAU};

use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::core::claude_turns::context::{self, Context, ONE_MILLION};
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

/// The ring's size, its radius and its stroke, from the design page.
pub const SIZE: f32 = 12.0;
const RADIUS: f32 = SIZE / 2.0 - 1.6;
const STROKE: f32 = 2.0;
const TRACK_OPACITY: f32 = 0.28;
/// Even a nearly empty chat shows this much of an arc, in percent.
const LEAST_ARC: u64 = 3;
const STEPS: usize = 48;
/// The time column is at least as wide as this label, so the rings of
/// rows with a short time ("30 s") line up with those of a long one.
pub const WIDEST_TIME: &str = "00 min";
/// The gap between the ring and its neighbours, as between a row's parts.
const GAP: f32 = 6.0;
const TIME_SIZE: f32 = 12.0;

/// A chat's context, when its size is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    /// The context of the last request, in tokens.
    pub tokens: u64,
    /// Where the chat is compacted, in tokens.
    pub limit: u64,
    /// How many times the chat was compacted.
    pub compactions: u32,
}

/// The fill of a chat from what its transcript tells. `setting` is the
/// user's `autoCompactWindow`; `model` the session's model id.
pub fn fill(context: &Context, setting: Option<u64>, model: Option<&str>) -> Option<Fill> {
    let tokens = context.tokens?;
    let one_million = context.one_million || model.is_some_and(|model| model.contains(ONE_MILLION));
    Some(Fill {
        tokens,
        limit: context::compaction_point(setting, one_million, context.most),
        compactions: context.compactions,
    })
}

/// The fill in whole percent, rounded; over 100 when past the limit.
pub fn percent(fill: Fill) -> u64 {
    if fill.limit == 0 {
        return 100;
    }
    (fill.tokens * 100 + fill.limit / 2) / fill.limit
}

/// The four colours of the ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// Under 15%: grey.
    Fresh,
    /// 15 to 45%: green.
    Fine,
    /// 45 to 75%: amber.
    GettingFull,
    /// Over 75%: red.
    NearCompaction,
}

pub fn band(percent: u64) -> Band {
    match percent {
        0..15 => Band::Fresh,
        15..45 => Band::Fine,
        45..75 => Band::GettingFull,
        _ => Band::NearCompaction,
    }
}

pub fn colour(band: Band, tokens: &Tokens) -> Color32 {
    match band {
        Band::Fresh => tokens.text_secondary,
        Band::Fine => tokens.success,
        Band::GettingFull => tokens.accent,
        Band::NearCompaction => tokens.danger,
    }
}

/// How much of the ring the arc covers, 0.03 to 1.
pub fn arc(percent: u64) -> f32 {
    percent.clamp(LEAST_ARC, 100) as f32 / 100.0
}

/// Tokens as the hover line writes them: "318k".
pub fn thousands(tokens: u64) -> String {
    let k = (tokens + 500) / 1000;
    format!("{}k", if tokens > 0 { k.max(1) } else { 0 })
}

/// The hover panel's line: "Context 318k of 777k (41%) · compacted
/// 2 times", or "Context size not known yet".
pub fn hover_text(fill: Option<Fill>) -> String {
    let Some(fill) = fill else {
        return "Context size not known yet".to_string();
    };
    let mut line = format!(
        "Context {} of {} ({}%)",
        thousands(fill.tokens),
        thousands(fill.limit),
        percent(fill)
    );
    match fill.compactions {
        0 => {}
        1 => line.push_str(" \u{b7} compacted 1 time"),
        n => line.push_str(&format!(" \u{b7} compacted {n} times")),
    }
    line
}

/// Where the ring goes in a row: its right edge at `right` (what the time
/// left free), but never right of `column` (the shared column's edge),
/// and its middle on the text's middle.
pub fn ring_rect(right: f32, column: f32, middle: f32) -> Rect {
    let edge = right.min(column);
    Rect::from_center_size(pos2(edge - SIZE / 2.0, middle), vec2(SIZE, SIZE))
}

/// Paints a row's ring before its time and returns the room left on its
/// left. `right` is what the time left free; `edge` the row's right edge
/// inside its padding.
pub fn in_row(
    painter: &Painter,
    right: f32,
    edge: f32,
    time_shown: bool,
    middle: f32,
    fill: Option<Fill>,
    tokens: &Tokens,
) -> f32 {
    let column = if time_shown {
        let widest = painter.layout_job(text::job(
            WIDEST_TIME,
            theme::regular(TIME_SIZE),
            tokens.text_secondary,
            None,
        ));
        edge - widest.size().x - GAP
    } else {
        edge
    };
    let rect = ring_rect(right, column, middle);
    paint(painter, rect, fill, tokens);
    rect.left() - GAP
}

/// Paints the ring in `rect`: the track, and the arc when the size is
/// known.
pub fn paint(painter: &Painter, rect: Rect, fill: Option<Fill>, tokens: &Tokens) {
    let centre = rect.center();
    let track = tokens.text_secondary.gamma_multiply(TRACK_OPACITY);
    painter.circle_stroke(centre, RADIUS, Stroke::new(STROKE, track));
    let Some(fill) = fill else {
        return;
    };
    let percent = percent(fill);
    let stroke = Stroke::new(STROKE, colour(band(percent), tokens));
    let part = arc(percent);
    if part >= 1.0 {
        painter.circle_stroke(centre, RADIUS, stroke);
        return;
    }
    let steps = ((STEPS as f32 * part).ceil() as usize).max(2);
    painter.add(Shape::line(arc_points(centre, part, steps), stroke));
}

/// The arc's points, clockwise from the top (y grows downwards).
fn arc_points(centre: Pos2, part: f32, steps: usize) -> Vec<Pos2> {
    (0..=steps)
        .map(|step| {
            let angle = -FRAC_PI_2 + TAU * part * step as f32 / steps as f32;
            centre + vec2(angle.cos(), angle.sin()) * RADIUS
        })
        .collect()
}

#[cfg(test)]
mod tests;

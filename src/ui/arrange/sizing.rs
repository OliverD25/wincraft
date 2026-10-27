use super::*;

/// How wide a picture `height` tall is for a window whose picture shows
/// `source` (its size in pixels): the window's own shape, kept between
/// NARROWEST and WIDEST of the height, 16:9 when the size is not known.
/// Whole points, so a card does not change width while nothing changed.
pub fn picture_width(source: Option<Vec2>, height: f32) -> f32 {
    let shape = source
        .filter(|size| size.x > 0.0 && size.y > 0.0)
        .unwrap_or(DEFAULT_SHAPE);
    (height * (shape.x / shape.y).clamp(NARROWEST, WIDEST)).round()
}

/// Cards of these widths in lines no wider than `available`, in order:
/// each line holds as many as fit, and at least one.
pub fn wrap(widths: &[f32], available: f32) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (index, width) in widths.iter().enumerate() {
        let wanted = if index == start {
            *width
        } else {
            used + CARD_GAP + width
        };
        // Half a point of room: a panel sized to fit exactly can round down.
        if index > start && wanted > available + 0.5 {
            lines.push(start..index);
            start = index;
            used = *width;
        } else {
            used = wanted;
        }
    }
    if start < widths.len() {
        lines.push(start..widths.len());
    }
    lines
}

/// How wide cards of these widths are on one line.
pub fn line_width(widths: &[f32]) -> f32 {
    widths.iter().sum::<f32>() + CARD_GAP * widths.len().saturating_sub(1) as f32
}

/// A Flat card's width for a window whose picture shows `source`.
pub(super) fn card_width(source: Option<Vec2>) -> f32 {
    picture_width(source, THUMB_HEIGHT) + 2.0 * CARD_INSET
}

/// The cards' widths in a row, or the empty row's placeholder.
fn row_widths(row: &Row, widths: &[f32]) -> Vec<f32> {
    if row.members.is_empty() {
        vec![card_width(None)]
    } else {
        row.members.iter().map(|member| widths[*member]).collect()
    }
}

/// The panel size for the rows, whose cards are `widths` wide (one per
/// window of the group): as wide as the widest row needs, at least
/// `min_width` and at most `limit.x`, with longer rows wrapping onto more
/// lines; as tall as that makes it, at most `limit.y`, beyond which the
/// rows scroll. `link` makes room for the line that opens the other
/// desktops, `note` for the note on their order under the rows.
pub fn panel_size(
    rows: &[Row],
    widths: &[f32],
    link: bool,
    note: bool,
    min_width: f32,
    limit: Vec2,
) -> Vec2 {
    let widest = rows
        .iter()
        .map(|row| line_width(&row_widths(row, widths)))
        .fold(card_width(None), f32::max);
    let width = (2.0 * PADDING + widest + SLACK).min(limit.x);
    let wrapped: Vec<(Vec<f32>, Vec<std::ops::Range<usize>>)> = rows
        .iter()
        .map(|row| {
            let cards = row_widths(row, widths);
            let lines = wrap(&cards, width - 2.0 * PADDING);
            (cards, lines)
        })
        .collect();
    // Rows that wrap leave the panel only as wide as their longest line; at
    // that width they wrap the same way.
    let longest = wrapped
        .iter()
        .flat_map(|(cards, lines)| lines.iter().map(|line| line_width(&cards[line.clone()])))
        .fold(0.0, f32::max);
    let width = (2.0 * PADDING + longest + SLACK)
        .min(width)
        .max(min_width)
        .min(limit.x);
    let lines: f32 = wrapped
        .iter()
        .map(|(_, lines)| {
            let lines = lines.len() as f32;
            HEADER + HEADER_GAP + lines * CARD_HEIGHT + (lines - 1.0) * CARD_GAP
        })
        .sum();
    let count = rows.len().max(1) as f32;
    let footer = if note && rows.len() > 1 { FOOTER } else { 0.0 };
    let link = if link { others::LINK_HEIGHT } else { 0.0 };
    let height = 2.0 * PADDING + lines + (count - 1.0) * ROW_GAP + link + footer + SLACK;
    vec2(width, height.min(limit.y))
}

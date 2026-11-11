//! How the Claude cards' marks and the toolbar's counters are painted.

use super::*;

/// The chip's colours: (text and mark, dot) by status.
pub(in crate::ui::arrange) fn status_colour(tokens: &Tokens, status: Status) -> Color32 {
    match status {
        Status::Waiting => tokens.warning,
        Status::Working => tokens.info,
        Status::Idle => tokens.text_secondary,
    }
}

pub(in crate::ui::arrange) const CHIP_HEIGHT: f32 = 20.0;
pub(in crate::ui::arrange) const CHIP_INSET: f32 = 6.0;
pub(in crate::ui::arrange) const MARK_SIZE: f32 = 12.0;

/// The status chip in the top left corner of the picture box `thumb`.
/// Returns its rectangle, which the live picture keeps clear of (DWM draws
/// the picture over anything painted under it), or NOTHING.
pub(in crate::ui::arrange) fn paint_chip(
    ui: &Ui,
    painter: &egui::Painter,
    thumb: Rect,
    mark: &Mark,
) -> Rect {
    let (Some(status), Some(text)) = (mark.status, mark.chip.as_deref()) else {
        return Rect::NOTHING;
    };
    let tokens = Tokens::get(ui.ctx());
    let colour = status_colour(&tokens, status);
    let galley = painter.layout_no_wrap(text.to_string(), theme::regular(12.0), colour);
    let width = 8.0 + MARK_SIZE + 5.0 + galley.size().x + 8.0;
    let width = width.min((thumb.width() - 2.0 * CHIP_INSET).max(0.0));
    let rect = Rect::from_min_size(
        thumb.min + Vec2::splat(CHIP_INSET),
        vec2(width, CHIP_HEIGHT),
    );
    painter.rect(
        rect,
        (CHIP_HEIGHT / 2.0) as u8,
        theme::with_alpha(tokens.window_bg, 0.92),
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let mark_rect = Rect::from_center_size(
        pos2(rect.left() + 8.0 + MARK_SIZE / 2.0, rect.center().y),
        Vec2::splat(MARK_SIZE),
    );
    match status {
        Status::Working => {
            let turn = if mark.animate {
                ui.ctx().input(|i| i.time) as f32 * std::f32::consts::TAU / 1.2
            } else {
                0.0
            };
            ring_mark(painter, mark_rect, colour, turn);
        }
        Status::Waiting => {
            painter.circle_filled(mark_rect.center(), 4.0, colour);
        }
        Status::Idle => {
            painter.circle_filled(mark_rect.center(), 4.0, tokens.text_disabled);
        }
    }
    let text_left = mark_rect.right() + 5.0;
    let clip = Rect::from_min_max(
        pos2(text_left, rect.top()),
        pos2(rect.right() - 6.0, rect.bottom()),
    );
    painter.with_clip_rect(clip).galley(
        pos2(text_left, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    rect
}

/// The progress ring of a working chip: a faint full circle and a 2/3 arc
/// that starts at `turn` radians.
pub(in crate::ui::arrange) fn ring_mark(
    painter: &egui::Painter,
    rect: Rect,
    colour: Color32,
    turn: f32,
) {
    let radius = rect.width() / 2.0 - 2.0;
    let centre = rect.center();
    let width = 2.0;
    painter.circle_stroke(
        centre,
        radius,
        egui::Stroke::new(width, theme::with_alpha(colour, 0.3)),
    );
    let steps = 24;
    let sweep = std::f32::consts::TAU * 0.65;
    let start = turn - std::f32::consts::FRAC_PI_2;
    let points: Vec<Pos2> = (0..=steps)
        .map(|step| {
            let angle = start + sweep * step as f32 / steps as f32;
            centre + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(points, egui::Stroke::new(width, colour)));
}

/// A small snowflake: three crossing lines with short ticks.
pub(in crate::ui::arrange) fn snowflake(
    painter: &egui::Painter,
    centre: Pos2,
    size: f32,
    colour: Color32,
) {
    let stroke = egui::Stroke::new(1.3, colour);
    let reach = size / 2.0;
    for step in 0..3 {
        let angle = std::f32::consts::FRAC_PI_2 + step as f32 * std::f32::consts::PI / 3.0;
        let along = vec2(angle.cos(), angle.sin());
        painter.line_segment([centre - along * reach, centre + along * reach], stroke);
        for end in [-1.0f32, 1.0] {
            let tip = centre + along * reach * 0.6 * end;
            let side = vec2(-along.y, along.x) * reach * 0.3;
            painter.line_segment([tip, tip + along * reach * 0.3 * end + side], stroke);
            painter.line_segment([tip, tip + along * reach * 0.3 * end - side], stroke);
        }
    }
}

/// The age at the right end of a card's title line, between `left` and
/// `right`, centred on `centre_y`; a snowflake before it when frozen.
/// Returns where it starts, so the title ends before it.
pub(in crate::ui::arrange) fn paint_age(
    painter: &egui::Painter,
    tokens: &Tokens,
    right: f32,
    centre_y: f32,
    mark: &Mark,
) -> f32 {
    let Some(text) = mark.age_text.as_deref() else {
        return right;
    };
    let colour = mark.age_colour(tokens);
    let galley = painter.layout_no_wrap(text.to_string(), theme::regular(12.0), colour);
    let mut left = right - galley.size().x;
    painter.galley(pos2(left, centre_y - galley.size().y / 2.0), galley, colour);
    if mark.age == Some(Age::Frozen) {
        let size = 11.0;
        let centre = pos2(left - 3.0 - size / 2.0, centre_y);
        snowflake(painter, centre, size, colour);
        left -= 3.0 + size;
    }
    left - 6.0
}

/// The light blue tint behind a cold card's faded picture.
pub(in crate::ui::arrange) fn paint_tint(
    painter: &egui::Painter,
    tokens: &Tokens,
    thumb: Rect,
    mark: &Mark,
) {
    if mark.tinted() {
        painter.rect_filled(thumb, 3, theme::with_alpha(tokens.info, 0.18));
    }
}

/// What a click in the toolbar's Claude part asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::arrange) enum Click {
    Counter(Filter),
    Board,
}

pub(in crate::ui::arrange) const COUNTER_HEIGHT: f32 = 32.0;
pub(in crate::ui::arrange) const COUNTER_GAP: f32 = 6.0;

/// The counters' and the Board button's sizes: (counter widths, board
/// width, whole width with the divider before them).
pub(in crate::ui::arrange) fn bar_widths(
    painter: &egui::Painter,
    counts: [usize; 4],
) -> (Vec<f32>, f32, f32) {
    let text = |words: String, font: egui::FontId| {
        painter.layout_no_wrap(words, font, Color32::WHITE).size().x
    };
    let counters: Vec<f32> = Filter::ALL
        .iter()
        .zip(counts)
        .map(|(filter, count)| {
            10.0 + MARK_SIZE
                + 6.0
                + text(filter.label().to_string(), theme::regular(14.0))
                + 6.0
                + text(count.to_string(), theme::semibold(14.0))
                + 10.0
        })
        .collect();
    let board = 10.0 + text("Board".into(), theme::regular(14.0)) + 8.0 + 20.0 + 10.0;
    let whole = 1.0 + 12.0 + counters.iter().sum::<f32>() + COUNTER_GAP * 3.0 + 12.0 + board;
    (counters, board, whole)
}

/// The width the counters and the Board button take in the toolbar.
pub(in crate::ui::arrange) fn bar_width(ui: &Ui, counts: [usize; 4]) -> f32 {
    bar_widths(ui.painter(), counts).2
}

/// Draws the divider, the four counters and the Board button from `left`,
/// centred on `centre_y`. Returns what was clicked.
pub(in crate::ui::arrange) fn bar(
    ui: &mut Ui,
    left: f32,
    centre_y: f32,
    counts: [usize; 4],
    lit: Option<Filter>,
    board_open: bool,
) -> Option<Click> {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter().clone();
    let (widths, board_width, _) = bar_widths(&painter, counts);
    let top = centre_y - COUNTER_HEIGHT / 2.0;
    painter.line_segment(
        [
            pos2(left + 0.5, centre_y - 12.0),
            pos2(left + 0.5, centre_y + 12.0),
        ],
        egui::Stroke::new(1.0, tokens.border),
    );
    let mut x = left + 1.0 + 12.0;
    let mut clicked = None;
    for ((filter, count), width) in Filter::ALL.into_iter().zip(counts).zip(widths) {
        let rect = Rect::from_min_size(pos2(x, top), vec2(width, COUNTER_HEIGHT));
        x += width + COUNTER_GAP;
        let response = ui
            .interact(
                rect,
                egui::Id::new(("claude-counter", filter.label())),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(counter_hint(filter));
        let colour = match filter {
            Filter::Waiting => tokens.warning,
            Filter::Working | Filter::Cold => tokens.info,
            Filter::Idle => tokens.text_secondary,
        };
        let pressed = lit == Some(filter);
        let (fill, outline) = if pressed {
            (theme::with_alpha(colour, 0.14), colour)
        } else if response.hovered() {
            (tokens.hover_bg, tokens.border)
        } else {
            (tokens.panel_bg, tokens.border)
        };
        painter.rect(
            rect,
            tokens.radius_row,
            fill,
            theme::line(ui.ctx(), outline),
            StrokeKind::Inside,
        );
        let mark = Rect::from_center_size(
            pos2(rect.left() + 10.0 + MARK_SIZE / 2.0, rect.center().y),
            Vec2::splat(MARK_SIZE),
        );
        match filter {
            Filter::Waiting => {
                painter.circle_filled(mark.center(), 4.0, tokens.warning);
            }
            Filter::Working => ring_mark(&painter, mark, tokens.info, 0.0),
            Filter::Idle => {
                painter.circle_filled(mark.center(), 4.0, tokens.text_disabled);
            }
            Filter::Cold => snowflake(&painter, mark.center(), 11.0, tokens.info),
        }
        let label = painter.layout_no_wrap(
            filter.label().to_string(),
            theme::regular(14.0),
            tokens.text_primary,
        );
        let label_left = mark.right() + 6.0;
        let number_left = label_left + label.size().x + 6.0;
        painter.galley(
            pos2(label_left, rect.center().y - label.size().y / 2.0),
            label,
            tokens.text_primary,
        );
        let number = painter.layout_no_wrap(count.to_string(), theme::semibold(14.0), colour);
        painter.galley(
            pos2(number_left, rect.center().y - number.size().y / 2.0),
            number,
            colour,
        );
        if response.clicked() {
            clicked = Some(Click::Counter(filter));
        }
    }
    let rect = Rect::from_min_size(
        pos2(x - COUNTER_GAP + 12.0, top),
        vec2(board_width, COUNTER_HEIGHT),
    );
    let response = ui
        .interact(rect, egui::Id::new("claude-board-button"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("The same chats, listed by status. Your cards do not move.");
    let fill = if board_open {
        selected_fill(&tokens)
    } else if response.hovered() {
        tokens.hover_bg
    } else {
        tokens.panel_bg
    };
    painter.rect(
        rect,
        tokens.radius_row,
        fill,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let label = painter.layout_no_wrap("Board".into(), theme::regular(14.0), tokens.text_primary);
    let label_left = rect.left() + 10.0;
    let key_left = label_left + label.size().x + 8.0;
    painter.galley(
        pos2(label_left, rect.center().y - label.size().y / 2.0),
        label,
        tokens.text_primary,
    );
    keycap(&painter, &tokens, pos2(key_left, rect.center().y), "B");
    if response.clicked() {
        clicked = Some(Click::Board);
    }
    clicked
}

/// A small key label such as "B", its left edge at `at.x`, centred on
/// `at.y`.
pub(in crate::ui::arrange) fn keycap(
    painter: &egui::Painter,
    tokens: &Tokens,
    at: Pos2,
    key: &str,
) {
    let galley = painter.layout_no_wrap(key.into(), theme::regular(12.0), tokens.text_secondary);
    let rect = Rect::from_min_size(
        pos2(at.x, at.y - 9.0),
        vec2((galley.size().x + 12.0).max(20.0), 18.0),
    );
    painter.rect_stroke(
        rect,
        4,
        theme::line(painter.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
}

pub(in crate::ui::arrange) fn counter_hint(filter: Filter) -> &'static str {
    match filter {
        Filter::Waiting => "Light up the chats that wait for you. S steps through the counters.",
        Filter::Working => "Light up the chats Claude works on now. S steps through the counters.",
        Filter::Idle => "Light up the chats with nothing to do. S steps through the counters.",
        Filter::Cold => "Light up the chats idle for 1 day or more. S steps through the counters.",
    }
}

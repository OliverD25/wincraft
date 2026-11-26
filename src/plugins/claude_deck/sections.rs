//! The sections of the list (design section 18, option A): the line above
//! a group's header, and the cold area at the end, a darker shelf with its
//! own header that folds. The Servers tab draws a server's cold chats with
//! the same header.

use egui::{pos2, vec2, Color32, Painter, Rect, Sense, Stroke, StrokeKind, Ui};

use crate::core::claude_sessions::Age;
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::labels::{chats_label, days_label};
use super::marks;
use super::model::{ColdLine, Line, Mark};
use super::rows::{chevron, cut, put, Look, CHEVRON};
use super::window::{DeckUi, Pick};

/// A group's header and the cold header.
pub const HEADER: f32 = 30.0;
/// The space above a group's header, but not the first.
pub const GROUP_GAP: f32 = 8.0;
/// The space between the last group and the cold area.
pub const COLD_GAP: f32 = 12.0;
pub const GROUP_NAME: f32 = 14.0;
const COLD_NAME: f32 = 13.0;
const COUNT: f32 = 12.0;
const PAD: f32 = 10.0;
const PART_GAP: f32 = 7.0;
/// The line over the cold area is twice as thick as the one over a group.
const COLD_RULE: f32 = 2.0;

pub const COLD_TITLE: &str = "Cold chats";

/// "3 chats · 1 – 3 days", "1 chat · 2 days": how many cold chats, and
/// how long the youngest and the oldest have been idle.
pub fn cold_count(chats: usize, days: (u64, u64)) -> String {
    format!("{} \u{b7} {}", chats_label(chats), days_label(days))
}

/// The ground of the cold area, darker than the window.
pub fn shelf_bg(tokens: &Tokens) -> Color32 {
    tokens.sunken_bg()
}

/// The tokens a cold chat's row is drawn with: its name is grey, so the
/// eye goes to the active chats first.
pub fn cold_tokens(tokens: &Tokens) -> Tokens {
    Tokens {
        text_primary: tokens.text_secondary,
        ..*tokens
    }
}

/// The 1 px line across the top of a group's header (not the first).
pub fn group_rule(painter: &Painter, header: Rect, tokens: &Tokens) {
    let stroke = theme::line(painter.ctx(), tokens.border);
    painter.hline(header.x_range(), header.top() + stroke.width / 2.0, stroke);
}

/// The shelf behind the cold header and its rows, from the header's top
/// to `bottom`, with the 2 px line across its top.
pub fn shelf(painter: &Painter, top: Rect, bottom: f32, tokens: &Tokens) {
    let area = Rect::from_min_max(top.min, pos2(top.right(), bottom.max(top.bottom())));
    painter.rect_filled(area, 0.0, shelf_bg(tokens));
    let rule = Rect::from_min_size(top.min, vec2(top.width(), COLD_RULE));
    painter.rect_filled(rule, 0.0, tokens.border);
}

/// The cold header: the fold arrow, the snowflake, "Cold chats" in the
/// blue of the cold mark, and at the right the count and the age range.
pub fn cold(
    painter: &Painter,
    rect: Rect,
    line: &ColdLine,
    look: &Look,
    hovered: bool,
    selected: bool,
) {
    let tokens = look.tokens;
    if selected {
        painter.rect_filled(rect, 0.0, tokens.selection_bg);
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, tokens.accent),
            StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
    let middle = rect.center().y;
    let mut left = rect.left() + PAD;
    chevron(
        painter,
        pos2(left, middle - CHEVRON / 2.0),
        !line.open,
        tokens.text_secondary,
    );
    left += CHEVRON + PART_GAP;
    marks::shape(
        painter,
        Rect::from_min_size(
            pos2(left, middle - marks::SIZE / 2.0),
            vec2(marks::SIZE, marks::SIZE),
        ),
        Mark::Idle(Age::Frozen),
        tokens,
        0.0,
    );
    left += marks::SIZE + PART_GAP;
    let count = painter.layout_job(text::job(
        &cold_count(line.chats, line.days),
        theme::regular(COUNT),
        tokens.text_secondary,
        None,
    ));
    let right = rect.right() - PAD - count.size().x;
    put(painter, right, middle, count);
    let title = text::job(COLD_TITLE, theme::semibold(COLD_NAME), tokens.info, None);
    put(
        painter,
        left,
        middle,
        cut(painter, title, right - PART_GAP - left),
    );
}

/// Where the cold area ends: under the last cold chat after the cold
/// line at `at`, whose bottom is `top`.
pub(super) fn cold_bottom(
    lines: &[Line],
    at: usize,
    top: f32,
    height_of: &dyn Fn(usize, &Line) -> f32,
) -> f32 {
    top + lines
        .iter()
        .enumerate()
        .skip(at + 1)
        .take_while(|(_, line)| matches!(line, Line::Chat(_)))
        .map(|(index, line)| height_of(index, line))
        .sum::<f32>()
}

/// The cold header: a click folds or opens the cold chats and picks it,
/// so the arrow keys and Space can fold it too.
pub(super) fn cold_line(
    ui: &mut Ui,
    painter: &Painter,
    rect: Rect,
    line: &ColdLine,
    look: &Look,
    state: &mut DeckUi,
) {
    let response = ui.interact(rect, ui.id().with("cd-cold"), Sense::click());
    let selected = state.selected == Some(Pick::Cold);
    cold(painter, rect, line, look, response.hovered(), selected);
    if selected && std::mem::take(&mut state.scroll_to_selected) {
        ui.scroll_to_rect(rect, None);
    }
    if response.clicked() {
        state.saved.cold_open = !state.saved.cold_open;
        state.selected = Some(Pick::Cold);
    }
}

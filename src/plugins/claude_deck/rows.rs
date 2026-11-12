//! One line of the list, painted: a group's header, a chat's row with its
//! tree lines, or the line of cold chats. Sizes are the design page's: a
//! 22 px row (34 px with its line of text), a 24 px group header and cold
//! line, 14 px per tree level.

use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Color32, Galley, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};
use std::sync::Arc;

use crate::core::claude_sessions::Age;
use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::labels::{
    chats_label, cold_label, days_label, forks_label, jobs_label, model_label, time_label,
};
use super::marks;
use super::model::{ChatLine, ColdLine, Fold, GroupLine, Line, Mark};
use super::{MarkStyle, Settings};

pub const ROW: f32 = 22.0;
/// A row with its line of text under the name.
pub const ROW_TWO: f32 = 34.0;
pub const GROUP: f32 = 24.0;
/// The space above a group's header (not the first) and the cold line.
pub const GROUP_GAP: f32 = 4.0;
const PAD_LEFT: f32 = 12.0;
const PAD_RIGHT: f32 = 10.0;
const GROUP_PAD: f32 = 10.0;
const COLD_PAD: f32 = 27.0;
const LEVEL: f32 = 14.0;
const GAP: f32 = 5.0;
const PART_GAP: f32 = 6.0;
const CHEVRON: f32 = 10.0;
/// From a row's top: the chevron, the mark, and the middle of the text.
const CHEVRON_TOP: f32 = 7.0;
const MARK_TOP: f32 = 5.0;
const TEXT_MIDDLE: f32 = 11.5;
/// The middle of the line of text in a two-line row.
const LINE_TWO_MIDDLE: f32 = 25.5;
/// Where a tree line turns towards its fork.
const ELBOW: f32 = 11.0;
const NAME_SIZE: f32 = 13.0;
const SMALL: f32 = 12.0;
/// A branch or model squeezed below this is left out instead.
const SMALLEST_TAG: f32 = 24.0;
const STAND_IN_OPACITY: f32 = 0.6;

/// What every line needs to be painted.
pub struct Look<'a> {
    pub tokens: &'a Tokens,
    pub settings: &'a Settings,
    pub now: u64,
    /// Where the Working ring has got to, 0 to 1.
    pub turn: f32,
}

/// The height a line takes, its gap above included. `two` is the setting
/// for the line of text; every chat row then has it, so rows line up.
pub fn height(line: &Line, first: bool, two: bool) -> f32 {
    match line {
        Line::Chat(_) if two => ROW_TWO,
        Line::Chat(_) => ROW,
        Line::Group(_) if first => GROUP,
        Line::Group(_) | Line::Cold(_) => GROUP + GROUP_GAP,
    }
}

/// The part of a line's rect that is drawn, without its gap above.
pub fn body(line: &Line, rect: Rect) -> Rect {
    match line {
        Line::Chat(_) => rect,
        _ => Rect::from_min_max(pos2(rect.left(), rect.bottom() - GROUP), rect.max),
    }
}

/// The fold arrow of a chat row, which folds its forks.
pub fn chevron_rect(row: Rect, depth: usize) -> Rect {
    let left = row.left() + PAD_LEFT + indent(depth);
    Rect::from_min_size(pos2(left, row.top() + CHEVRON_TOP), vec2(CHEVRON, CHEVRON)).expand(3.0)
}

fn indent(depth: usize) -> f32 {
    if depth == 0 {
        0.0
    } else {
        LEVEL * depth as f32 + GAP
    }
}

/// The fold arrow: pointing down when open, right when folded.
fn chevron(painter: &Painter, at: Pos2, folded: bool, colour: Color32) {
    let points: [(f32, f32); 3] = if folded {
        [(3.5, 8.0), (6.5, 5.0), (3.5, 2.0)]
    } else {
        [(2.0, 3.5), (5.0, 6.5), (8.0, 3.5)]
    };
    let points = points
        .iter()
        .map(|(x, y)| pos2(at.x + x, at.y + y))
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.4, colour)));
}

fn galley(painter: &Painter, words: &str, size: f32, colour: Color32) -> Arc<Galley> {
    painter.layout_job(text::job(words, theme::regular(size), colour, None))
}

/// One line of text cut to `width` with an ellipsis.
fn cut(painter: &Painter, mut job: LayoutJob, width: f32) -> Arc<Galley> {
    job.wrap = TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    painter.layout_job(job)
}

fn put(painter: &Painter, left: f32, middle: f32, galley: Arc<Galley>) -> f32 {
    let size = galley.size();
    painter.galley(
        pos2(left, middle - size.y / 2.0),
        galley,
        Color32::PLACEHOLDER,
    );
    size.x
}

/// The tree lines in front of a fork, as in a file tree.
fn tree_lines(painter: &Painter, row: Rect, line: &ChatLine, colour: Color32) {
    if line.depth == 0 {
        return;
    }
    let stroke = Stroke::new(1.0, colour);
    let left = row.left() + PAD_LEFT;
    let x = |level: usize| (left + LEVEL * level as f32 + 6.0).round() + 0.5;
    for (level, goes_on) in line.rails.iter().enumerate() {
        if *goes_on {
            painter.vline(x(level), row.y_range(), stroke);
        }
    }
    let own = line.depth - 1;
    let elbow = row.top() + ELBOW;
    let bottom = if line.last { elbow } else { row.bottom() };
    painter.vline(x(own), row.top()..=bottom, stroke);
    painter.hline(x(own)..=x(own) + 7.0, elbow + 0.5, stroke);
}

/// A chat's row.
pub fn chat(
    painter: &Painter,
    row: Rect,
    line: &ChatLine,
    look: &Look,
    hovered: bool,
    selected: bool,
) {
    let tokens = look.tokens;
    let settings = look.settings;
    if selected {
        painter.rect_filled(row, 0.0, tokens.selection_bg);
        painter.rect_stroke(
            row,
            0.0,
            Stroke::new(1.0, tokens.accent),
            StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(row, 0.0, tokens.hover_bg);
    }
    let mut painter = painter.clone();
    if line.stand_in {
        painter.multiply_opacity(STAND_IN_OPACITY);
    }
    tree_lines(&painter, row, line, tokens.border);
    let mut x = row.left() + PAD_LEFT + indent(line.depth);
    if line.fold != Fold::None {
        let folded = matches!(line.fold, Fold::Folded(_));
        chevron(
            &painter,
            pos2(x, row.top() + CHEVRON_TOP),
            folded,
            tokens.text_secondary,
        );
    }
    x += CHEVRON + GAP;
    let mark_at = pos2(x, row.top() + MARK_TOP);
    if line.stand_in && !matches!(line.fold, Fold::Folded(_)) {
        if settings.mark != MarkStyle::Off {
            marks::stand_in(
                &painter,
                Rect::from_min_size(mark_at, vec2(marks::SIZE, marks::SIZE)),
                tokens,
            );
            x += marks::width(settings.mark) + GAP;
        }
    } else {
        let used = marks::paint(
            &painter,
            mark_at,
            line.chat.mark,
            settings.mark,
            tokens,
            look.turn,
        );
        if used > 0.0 {
            x += used + GAP;
        }
    }
    parts(&painter, row, x, line, look);
    if settings.text {
        if let Some(words) = &line.chat.text {
            let job = text::job(words, theme::regular(SMALL), tokens.text_secondary, None);
            let room = row.right() - PAD_RIGHT - x;
            put(
                &painter,
                x,
                row.top() + LINE_TWO_MIDDLE,
                cut(&painter, job, room),
            );
        }
    }
}

/// The name and its parts after the mark, and the time at the right edge.
fn parts(painter: &Painter, row: Rect, left: f32, line: &ChatLine, look: &Look) {
    let tokens = look.tokens;
    let settings = look.settings;
    let chat = &line.chat;
    let middle = row.top() + TEXT_MIDDLE;
    let mut right = row.right() - PAD_RIGHT;
    if settings.time && !line.stand_in {
        let time = galley(
            painter,
            &time_label(chat.since, look.now),
            SMALL,
            tokens.text_secondary,
        );
        right -= time.size().x;
        put(painter, right, middle, time);
        right -= PART_GAP;
    }
    let room = (right - left).max(0.0);
    let mut fixed: Vec<Arc<Galley>> = Vec::new();
    if settings.jobs && chat.jobs > 0 {
        fixed.push(galley(painter, &jobs_label(chat.jobs), SMALL, tokens.info));
    }
    let roll = match line.fold {
        Fold::Folded(forks) if !line.stand_in => Some(galley(
            painter,
            &forks_label(forks),
            SMALL,
            tokens.text_secondary,
        )),
        _ => None,
    };
    let mut tags: Vec<String> = Vec::new();
    if settings.project {
        tags.extend(chat.branch.clone());
    }
    if settings.model {
        tags.extend(chat.model.as_deref().map(|m| model_label(m).to_string()));
    }
    let tag = tags.join(" \u{b7} ");
    let name_job = {
        let colour = if line.stand_in {
            tokens.text_secondary
        } else {
            tokens.text_primary
        };
        let mut job = text::job(&chat.title, theme::regular(NAME_SIZE), colour, None);
        if line.stand_in {
            for section in &mut job.sections {
                section.format.italics = true;
            }
        }
        job
    };
    let name_width = painter.layout_job(name_job.clone()).size().x;
    let fixed_width: f32 = fixed
        .iter()
        .chain(roll.iter())
        .map(|g| g.size().x + PART_GAP)
        .sum();
    let tag_job = text::job(&tag, theme::regular(SMALL), tokens.text_disabled, None);
    let tag_natural = if tag.is_empty() {
        0.0
    } else {
        painter.layout_job(tag_job.clone()).size().x + PART_GAP
    };
    let tag_room = (room - name_width - fixed_width).min(tag_natural);
    let tag_room = if tag_room < SMALLEST_TAG {
        0.0
    } else {
        tag_room
    };
    let name_room = (room - fixed_width - tag_room).max(0.0);
    let mut x = left;
    x += put(painter, x, middle, cut(painter, name_job, name_room)) + PART_GAP;
    for part in fixed {
        x += put(painter, x, middle, part) + PART_GAP;
    }
    if tag_room > 0.0 {
        x += put(
            painter,
            x,
            middle,
            cut(painter, tag_job, tag_room - PART_GAP),
        ) + PART_GAP;
    }
    if let Some(roll) = roll {
        put(painter, x, middle, roll);
    }
}

/// A group's header: the fold arrow, the name, and the count, or while
/// folded the most urgent mark and "N chats".
pub fn group(painter: &Painter, rect: Rect, line: &GroupLine, look: &Look, hovered: bool) {
    let tokens = look.tokens;
    if hovered {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
    let middle = rect.center().y;
    let left = rect.left() + GROUP_PAD;
    chevron(
        painter,
        pos2(left, middle - CHEVRON / 2.0),
        line.folded,
        tokens.text_secondary,
    );
    let mut right = rect.right() - GROUP_PAD;
    let count = if line.folded {
        chats_label(line.chats)
    } else {
        line.chats.to_string()
    };
    let count = galley(painter, &count, SMALL, tokens.text_secondary);
    right -= count.size().x;
    put(painter, right, middle, count);
    if line.folded && look.settings.mark != MarkStyle::Off {
        right -= PART_GAP + marks::SIZE;
        marks::shape(
            painter,
            Rect::from_min_size(
                pos2(right, middle - marks::SIZE / 2.0),
                vec2(marks::SIZE, marks::SIZE),
            ),
            line.roll,
            tokens,
            look.turn,
        );
    }
    let name_left = left + CHEVRON + PART_GAP;
    let job = text::job(
        &line.name,
        theme::semibold(SMALL),
        tokens.text_secondary,
        None,
    );
    put(
        painter,
        name_left,
        middle,
        cut(painter, job, right - PART_GAP - name_left),
    );
}

/// The grey line that holds the chats idle for over a day.
pub fn cold(painter: &Painter, rect: Rect, line: &ColdLine, look: &Look, hovered: bool) {
    let tokens = look.tokens;
    if hovered {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
    painter.extend(Shape::dashed_line(
        &[rect.left_top(), rect.right_top()],
        Stroke::new(1.0, tokens.border),
        3.0,
        3.0,
    ));
    let middle = rect.center().y;
    let left = rect.left() + COLD_PAD;
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
    let days = galley(painter, &days_label(line.days), SMALL, tokens.info);
    let right = rect.right() - PAD_RIGHT - days.size().x;
    put(painter, right, middle, days);
    let label = text::job(
        &cold_label(line.chats),
        theme::regular(SMALL),
        tokens.info,
        None,
    );
    let label_left = left + marks::SIZE + PART_GAP;
    put(
        painter,
        label_left,
        middle,
        cut(painter, label, right - PART_GAP - label_left),
    );
}

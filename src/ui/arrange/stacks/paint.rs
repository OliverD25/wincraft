//! How groups look: a stack's card (two edges behind it, a count chip, the
//! other windows' names), the mark of a group with one window open, the
//! check mark of a picked card, the frame round an open group, the list
//! under a selected stack and the "Drop to group" mark.

use super::*;

/// The two lines of names under a stack's title; its picture is that much
/// shorter, so a stack is as tall as a card.
pub const LINES: f32 = 30.0;
const LINE_FONT: f32 = 12.0;
/// How far each edge behind a stack peeks out.
const EDGE_STEP: f32 = 4.0;
const CHIP_HEIGHT: f32 = 16.0;
/// Round an open group's cards.
const FRAME_PAD: f32 = 6.0;
pub const TAB_HEIGHT: f32 = 16.0;
const LIST_WIDTH: f32 = 260.0;
const LIST_ROW: f32 = 20.0;
const LIST_PAD: f32 = 8.0;

/// A card's picture height: shorter on a stack, to make room for its lines.
pub fn thumb_height(stacks: &Stacks, hwnd: isize, full: f32) -> f32 {
    if stacks.closed(hwnd).is_some() {
        full - LINES
    } else {
        full
    }
}

/// Two card edges behind a stack, peeking out at the top right.
pub fn behind(painter: &egui::Painter, stacks: &Stacks, hwnd: isize, rect: Rect) {
    if stacks.closed(hwnd).is_none() {
        return;
    }
    let tokens = Tokens::get(painter.ctx());
    for step in [2.0, 1.0] {
        let edge = rect.translate(vec2(EDGE_STEP * step, -EDGE_STEP * step));
        painter.rect(
            edge,
            4,
            tokens.panel_bg,
            theme::line(painter.ctx(), tokens.border),
            StrokeKind::Inside,
        );
    }
    // The card itself hides the edges' lower left parts.
    painter.rect(
        rect,
        4,
        tokens.window_bg,
        theme::line(painter.ctx(), tokens.border),
        StrokeKind::Inside,
    );
}

/// The marks on a card's picture: a stack's count chip, a lone group
/// window's small stack mark, a picked card's check mark. Returns the part
/// of the picture they cover, joined to `clear`, so DWM leaves it free.
pub fn marks(
    painter: &egui::Painter,
    stacks: &Stacks,
    hwnd: isize,
    thumb: Rect,
    clear: Rect,
) -> Rect {
    let tokens = Tokens::get(painter.ctx());
    let mut clear = clear;
    let mut cover = |rect: Rect| {
        clear = if clear.is_positive() {
            clear.union(rect)
        } else {
            rect
        };
    };
    let corner = pos2(thumb.right() - 4.0, thumb.top() + 4.0);
    if let Some(stack) = stacks.closed(hwnd) {
        let galley = painter.layout_no_wrap(
            stack.members.len().to_string(),
            theme::semibold(12.0),
            tokens.text_primary,
        );
        let chip = Rect::from_min_max(
            pos2(corner.x - (galley.size().x + 12.0).max(22.0), corner.y),
            pos2(corner.x, corner.y + CHIP_HEIGHT),
        );
        painter.rect(
            chip,
            8,
            tokens.window_bg,
            theme::line(painter.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        painter.galley(
            chip.center() - galley.size() / 2.0,
            galley,
            tokens.text_primary,
        );
        cover(chip);
    } else if stacks.lone(hwnd).is_some() {
        let mark = Rect::from_min_max(
            pos2(corner.x - 18.0, corner.y),
            pos2(corner.x, corner.y + 14.0),
        );
        painter.rect_filled(mark, 3, tokens.window_bg);
        for step in [0.0, 3.0] {
            let sheet = Rect::from_min_size(
                pos2(mark.left() + 3.0 + step, mark.top() + 6.0 - step),
                vec2(9.0, 6.0),
            );
            painter.rect(
                sheet,
                1,
                tokens.window_bg,
                theme::line(painter.ctx(), tokens.text_secondary),
                StrokeKind::Inside,
            );
        }
        cover(mark);
    }
    if stacks.is_picked(hwnd) {
        let centre = pos2(thumb.left() + 13.0, thumb.top() + 13.0);
        painter.circle(
            centre,
            9.0,
            tokens.text_primary,
            theme::line(painter.ctx(), tokens.window_bg),
        );
        let stroke = egui::Stroke::new(2.0, tokens.window_bg);
        painter.line_segment([centre + vec2(-4.0, 0.0), centre + vec2(-1.0, 3.0)], stroke);
        painter.line_segment([centre + vec2(-1.0, 3.0), centre + vec2(4.0, -3.0)], stroke);
        cover(Rect::from_center_size(centre, Vec2::splat(20.0)));
    }
    clear
}

/// The text under a stack's title: the other windows' short names, one
/// after the other, ending in "+N more" when they do not fit in two lines.
pub fn names_line(stack: &Found, fits: &dyn Fn(&str) -> bool) -> String {
    let names: Vec<String> = stack
        .members
        .iter()
        .filter(|window| window.hwnd != stack.rep)
        .map(|window| deck_groups::short_name(&window.label, &stack.name))
        .collect();
    for shown in (0..=names.len()).rev() {
        let mut text = names[..shown].join(" \u{b7} ");
        let more = names.len() - shown;
        if more > 0 {
            if !text.is_empty() {
                text.push_str("  ");
            }
            text.push_str(&format!("+{more} more"));
        }
        if fits(&text) || shown == 0 {
            return text;
        }
    }
    String::new()
}

/// Draws the stack's names under its title, in `area`.
pub fn lines(painter: &egui::Painter, stacks: &Stacks, hwnd: isize, area: Rect) {
    let Some(stack) = stacks.closed(hwnd) else {
        return;
    };
    let tokens = Tokens::get(painter.ctx());
    let job = |text: &str| {
        let mut job = text::job(text, theme::regular(LINE_FONT), tokens.text_secondary, None);
        job.wrap = TextWrapping {
            max_width: area.width().max(0.0),
            max_rows: 2,
            break_anywhere: false,
            overflow_character: Some('\u{2026}'),
        };
        job
    };
    let fits = |text: &str| !painter.layout_job(job(text)).elided;
    let text = names_line(stack, &fits);
    painter.galley(
        area.min,
        painter.layout_job(job(&text)),
        tokens.text_secondary,
    );
}

/// An open group's window gets a solid number chip over its usual one.
pub fn member_chip(
    painter: &egui::Painter,
    stacks: &Stacks,
    hwnd: isize,
    chip: Rect,
    number: usize,
) {
    let Some(stack) = stacks.open_stack() else {
        return;
    };
    if !stack.members.iter().any(|w| w.hwnd == hwnd) {
        return;
    }
    let tokens = Tokens::get(painter.ctx());
    painter.rect_filled(chip, tokens.radius_row, tokens.text_primary);
    let galley =
        painter.layout_no_wrap(number.to_string(), theme::semibold(12.0), tokens.window_bg);
    painter.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.window_bg,
    );
}

/// The open group's cards in runs next to each other, from last frame's
/// card places: a run ends where a line of cards wraps or a column ends.
pub fn runs(rects: &[Rect]) -> Vec<Rect> {
    let mut sorted = rects.to_vec();
    sorted.sort_by(|a, b| {
        a.top()
            .total_cmp(&b.top())
            .then(a.left().total_cmp(&b.left()))
    });
    let mut runs: Vec<Rect> = Vec::new();
    for rect in sorted {
        match runs.last_mut() {
            Some(run)
                if (run.top() - rect.top()).abs() < 1.0
                    && rect.left() - run.right() <= 2.0 * CARD_GAP =>
            {
                *run = run.union(rect)
            }
            _ => runs.push(rect),
        }
    }
    runs
}

/// Where the open group's windows were drawn last frame: a window with a
/// ghost in the group's column counts by its ghost, not its own card.
fn member_rects(shared: &Shared, stack: &Found) -> Vec<Rect> {
    let ghost_at = |hwnd: isize| {
        let group = shared.group()?;
        shared
            .items
            .iter()
            .find(|(item, _)| {
                matches!(item, super::super::by_monitor::Item::Ghost(_))
                    && group
                        .windows
                        .get(item.member())
                        .is_some_and(|w| w.hwnd == hwnd)
            })
            .map(|(_, rect)| *rect)
    };
    stack
        .members
        .iter()
        .filter_map(|w| {
            if shared.stacks.is_ghosted(w.hwnd) {
                ghost_at(w.hwnd)
            } else {
                hit_rect(&shared.card_hits, w.hwnd)
            }
        })
        .collect()
}

/// The frame round a run of an open group's cards. With `head` (the room
/// the By monitor view leaves above each line while a group is open) the
/// frame reaches up into that room, so the header line sits inside it.
pub fn frame_rect(run: Rect, head: f32) -> Rect {
    let mut frame = run.expand(FRAME_PAD);
    if head > 0.0 {
        frame.min.y = run.top() - head - 2.0;
    }
    frame
}

/// Where the header line goes on `frame`: inside its top padding when
/// there is room for it, else across its top edge.
pub fn tab_at(frame: Rect, head: f32) -> Pos2 {
    if head > 0.0 {
        pos2(frame.left() + 8.0, frame.top() + 2.0)
    } else {
        pos2(frame.left() + 8.0, frame.top() - TAB_HEIGHT / 2.0)
    }
}

/// The frame round an open group, behind its cards: a light fill and a
/// ring. Drawn from last frame's card places, before the cards.
pub fn open_frame(painter: &egui::Painter, shared: &Shared) {
    let Some(stack) = shared.stacks.open_stack() else {
        return;
    };
    let tokens = Tokens::get(painter.ctx());
    let rects = member_rects(shared, stack);
    for run in runs(&rects) {
        painter.rect(
            frame_rect(run, shared.stacks.head),
            8,
            theme::with_alpha(tokens.text_primary, 0.05),
            theme::line(painter.ctx(), theme::with_alpha(tokens.text_primary, 0.35)),
            StrokeKind::Inside,
        );
    }
}

/// The open group's name and keys, on the top edge of its first run.
fn open_tab(ctx: &egui::Context, shared: &Shared, frame: &mut Frame) {
    let Some(stack) = shared.stacks.open_stack() else {
        return;
    };
    let rects = member_rects(shared, stack);
    let Some(run) = runs(&rects).first().copied() else {
        return;
    };
    let first = frame_rect(run, shared.stacks.head);
    let tokens = Tokens::get(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("deck-group-tab"),
    ));
    let text = format!(
        "{} \u{b7} 1 \u{2013} {} opens a window \u{b7} Esc closes the group",
        stack.name,
        stack.members.len()
    );
    let mut job = text::job(&text, theme::regular(12.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: (first.width() - 16.0).max(40.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    let tab = Rect::from_min_size(
        tab_at(first, shared.stacks.head),
        vec2(galley.size().x + 12.0, TAB_HEIGHT),
    );
    painter.rect(
        tab,
        8,
        tokens.window_bg,
        theme::line(ctx, theme::with_alpha(tokens.text_primary, 0.35)),
        StrokeKind::Inside,
    );
    painter.galley(
        pos2(tab.left() + 6.0, tab.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_primary,
    );
    // Only the part above the cards: a picture starts right at a ghost's
    // top, and covering it would hide it.
    let mut covered = tab;
    covered.max.y = covered.max.y.min(run.top() - 1.0);
    cover(frame, covered);
}

fn cover(frame: &mut Frame, rect: Rect) {
    frame.covers.push(rect);
}

/// After the cards are drawn: the open group's tab, the list under a
/// selected or pointed-at stack, and the "Drop to group" mark.
pub fn after_cards(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    frame: &mut Frame,
    dragging: bool,
) {
    let on = shared
        .snapshot
        .as_ref()
        .is_some_and(|s| s.deck_groups.is_some());
    if !on {
        return;
    }
    open_tab(ctx, shared, frame);
    let tokens = Tokens::get(ctx);
    if let Some(rect) = shared
        .stacks
        .drop_on
        .filter(|_| dragging)
        .and_then(|card| hit_rect(&shared.card_hits, card))
    {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("deck-drop-to-group"),
        ));
        painter.rect_filled(rect, 4, Color32::from_black_alpha(150));
        painter.rect_stroke(
            rect,
            4,
            theme::stroke(ctx, 2.0, tokens.accent),
            StrokeKind::Inside,
        );
        let label = painter.layout_no_wrap(
            "Drop to group".to_string(),
            theme::semibold(12.0),
            Color32::WHITE,
        );
        painter.galley(rect.min + vec2(8.0, 6.0), label, Color32::WHITE);
        cover(frame, rect);
    }
    if dragging || shared.stacks.dialog.is_some() {
        return;
    }
    let shown = frame
        .hovered
        .filter(|hwnd| shared.stacks.closed(*hwnd).is_some())
        .or(shared
            .cursor
            .filter(|hwnd| !shared.cursor_ghost && shared.stacks.closed(*hwnd).is_some()));
    let Some(hwnd) = shown else {
        return;
    };
    let (Some(stack), Some(card)) = (
        shared.stacks.closed(hwnd).cloned(),
        hit_rect(&shared.card_hits, hwnd),
    ) else {
        return;
    };
    let numbers = taskbar_numbers(group, desktops);
    let number = group
        .windows
        .iter()
        .position(|w| w.hwnd == hwnd)
        .map_or(0, |at| numbers[at]);
    let height = LIST_PAD * 2.0 + LIST_ROW * (stack.members.len() + 1) as f32;
    let screen = ctx.content_rect();
    let left = card
        .left()
        .min(screen.right() - LIST_WIDTH - 4.0)
        .max(screen.left() + 4.0);
    let top = if card.bottom() + 6.0 + height <= screen.bottom() {
        card.bottom() + 6.0
    } else {
        (card.top() - 6.0 - height).max(screen.top())
    };
    let list = Rect::from_min_size(pos2(left, top), vec2(LIST_WIDTH, height));
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("deck-stack-list"),
    ));
    painter.rect(
        list,
        6,
        tokens.window_bg,
        theme::line(ctx, tokens.border),
        StrokeKind::Inside,
    );
    let row = |at: usize| {
        Rect::from_min_size(
            pos2(
                list.left() + LIST_PAD,
                list.top() + LIST_PAD + LIST_ROW * at as f32,
            ),
            vec2(LIST_WIDTH - 2.0 * LIST_PAD, LIST_ROW),
        )
    };
    let header = format!("In {} \u{b7} press {number}, then a number", stack.name);
    let one_line = |text: &str, colour: Color32, width: f32| {
        let mut job = text::job(text, theme::regular(12.0), colour, None);
        job.wrap = TextWrapping {
            max_width: width,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        painter.layout_job(job)
    };
    let first = row(0);
    let galley = one_line(&header, tokens.text_secondary, first.width());
    painter.galley(
        pos2(first.left(), first.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_secondary,
    );
    for (at, window) in stack.members.iter().enumerate() {
        let line = row(at + 1);
        let chip = number_chip(&painter, line.min + vec2(0.0, 1.0), LIST_ROW - 2.0, at + 1);
        let mut left = chip.right() + NUMBER_GAP;
        if window.attention {
            painter.circle_filled(pos2(left + 4.0, line.center().y), 4.0, tokens.warning);
            left += 12.0;
        }
        let galley = one_line(&window.label, tokens.text_primary, line.right() - left);
        painter.galley(
            pos2(left, line.center().y - galley.size().y / 2.0),
            galley,
            tokens.text_primary,
        );
    }
    cover(frame, list);
}

/// The Flat view's part before its cards: it has no ghosts and no "Drop to
/// group", and the open group's frame goes behind the cards.
pub fn before_flat_cards(ui: &Ui, shared: &mut Shared, area: Rect) {
    shared.stacks.ghosted.clear();
    shared.stacks.head = 0.0;
    shared.stacks.drop_on = None;
    open_frame(&ui.painter().with_clip_rect(area), shared);
}

/// A Flat card's picture area: shorter on a stack, to make room for the
/// names under its title.
pub fn flat_picture(stacks: &Stacks, hwnd: isize, area: Rect) -> Rect {
    let mut area = area;
    area.max.y = area.min.y + thumb_height(stacks, hwnd, area.height()).max(0.0);
    area
}

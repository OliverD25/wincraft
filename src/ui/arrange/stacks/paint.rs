//! How groups look: a stack's card (two edges behind it, a count chip, the
//! other windows' names), the mark of a group with one window open, the
//! check mark of a picked card, the list under a selected stack and the
//! "Drop to group" mark. An open group's layer is in `overlay`.

use super::*;

/// The two lines of names under a stack's title; its picture is that much
/// shorter, so a stack is as tall as a card.
pub const LINES: f32 = 30.0;
const LINE_FONT: f32 = 12.0;
/// How far each edge behind a stack peeks out.
const EDGE_STEP: f32 = 4.0;
const CHIP_HEIGHT: f32 = 16.0;

/// A card's picture height: shorter on a stack, to make room for its lines.
pub fn thumb_height(stacks: &Stacks, hwnd: isize, full: f32) -> f32 {
    if stacks.card_stack(hwnd).is_some() {
        full - LINES
    } else {
        full
    }
}

/// Two card edges behind a stack, peeking out at the top right.
pub fn behind(painter: &egui::Painter, stacks: &Stacks, hwnd: isize, rect: Rect) {
    if stacks.card_stack(hwnd).is_none() {
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
    if let Some(stack) = stacks.card_stack(hwnd) {
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
    let Some(stack) = stacks.card_stack(hwnd) else {
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

/// A click at `at` is outside the open group's layer (`layer`, as drawn
/// last frame): it closes the group and does nothing else.
pub fn outside_open_group(at: Pos2, layer: Option<Rect>) -> bool {
    layer.is_some_and(|layer| !layer.contains(at))
}

fn cover(frame: &mut Frame, rect: Rect) {
    frame.covers.push(rect);
}

/// After the cards are drawn: the open group's layer, the list under a
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
    if shared.stacks.open.is_some() {
        shared.stacks.list = None;
        shared.stacks.drop_on = None;
        super::overlay::show(ctx, shared, group, desktops, frame);
        return;
    }
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
        drop_badge(ctx, &tokens, rect);
        cover(frame, rect);
    }
    if dragging || shared.stacks.dialog.is_some() {
        shared.stacks.list = None;
        return;
    }
    super::list::show(ctx, shared, group, desktops, frame);
}

/// The "Drop to group" badge: an accent pill on the stack's top edge. It is
/// painted above the dragged card (which sits on the Tooltip layer), so the
/// card under the mouse never hides it.
fn drop_badge(ctx: &egui::Context, tokens: &Tokens, rect: Rect) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Debug,
        egui::Id::new("deck-drop-to-group-badge"),
    ));
    let label = painter.layout_no_wrap(
        "Drop to group".to_string(),
        theme::semibold(14.0),
        Color32::WHITE,
    );
    let size = label.size() + vec2(24.0, 12.0);
    let screen = ctx.content_rect();
    let top = (rect.min.y - size.y / 2.0).max(screen.min.y + 4.0);
    let badge = Rect::from_min_size(pos2(rect.center().x - size.x / 2.0, top), size);
    painter.rect_filled(badge, size.y / 2.0, tokens.accent);
    painter.rect_stroke(
        badge,
        size.y / 2.0,
        theme::stroke(ctx, 1.0, Color32::from_white_alpha(90)),
        StrokeKind::Inside,
    );
    let at = badge.center() - label.size() / 2.0;
    painter.galley(at, label, Color32::WHITE);
}

/// The Flat view's part before its cards: it has no "Drop to group".
pub fn before_flat_cards(shared: &mut Shared) {
    shared.stacks.drop_on = None;
}

/// A Flat card's picture area: shorter on a stack, to make room for the
/// names under its title.
pub fn flat_picture(stacks: &Stacks, hwnd: isize, area: Rect) -> Rect {
    let mut area = area;
    area.max.y = area.min.y + thumb_height(stacks, hwnd, area.height()).max(0.0);
    area
}

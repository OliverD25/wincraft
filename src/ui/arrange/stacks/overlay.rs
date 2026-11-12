//! An open group's layer: an opaque panel above the row, anchored on the
//! group's card, with the group's windows as cards numbered 1, 2, 3. The
//! row under it keeps every card where it is (Principle 1, Never surprise
//! the user). "Open a group as" picks how the cards lie: in one row centred
//! on the stack, or as a fan that starts on the stack's place.

use super::*;

/// The layer stays this far inside the WindowDeck window.
pub const MARGIN: f32 = 4.0;
const PAD_X: f32 = 10.0;
const PAD_TOP: f32 = 8.0;
/// The header line: the group's name and its keys.
const HEAD: f32 = 16.0;
const HEAD_GAP: f32 = 8.0;
const PAD_BOTTOM: f32 = 10.0;
const GAP: f32 = 8.0;
/// A row too wide for the window narrows its cards, down to this: the
/// number and the start of the name still fit at 12 px.
const MIN_CARD: f32 = 96.0;
/// A fan's step, as a part of a card's width: 64 px of a 162 px card.
const FAN_STEP: f32 = 0.4;
/// A fan too wide for the window steps less, down to this: a number chip
/// and a little air.
const MIN_STEP: f32 = 32.0;
const CARD_PAD: f32 = 4.0;
const TITLE_GAP: f32 = 6.0;
const TITLE: f32 = 18.0;
const RADIUS: u8 = 4;

/// The keys that move or change cards in the row. While a group is open the
/// row is under its layer, so they do nothing.
pub const ROW_KEYS: &[egui::Key] = &[
    egui::Key::ArrowUp,
    egui::Key::ArrowDown,
    egui::Key::ArrowLeft,
    egui::Key::ArrowRight,
    egui::Key::Enter,
    egui::Key::Space,
    egui::Key::Tab,
    egui::Key::Home,
    egui::Key::End,
    egui::Key::PageUp,
    egui::Key::PageDown,
    egui::Key::Delete,
    egui::Key::F2,
];

/// How an open group lies on its layer ("Open a group as").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// "A row centred on the stack".
    Row,
    /// "A fan".
    Fan,
}

/// Where the layer goes: the panel, and each window's card in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub panel: Rect,
    pub cards: Vec<Rect>,
}

/// How far a span from `lo` to `hi` moves to lie between `min` and `max`:
/// only as far as it must. A span wider than the room starts at `min`.
fn slide(lo: f32, hi: f32, min: f32, max: f32) -> f32 {
    if hi - lo > max - min || lo < min {
        min - lo
    } else if hi > max {
        max - hi
    } else {
        0.0
    }
}

/// Places the layer of a group of `n` windows whose card is `stack`, with
/// cards of the row's `card` size, inside `window` (the WindowDeck window).
/// The cards' tops are level with the stack's top; the header line goes
/// above them, inside the panel.
pub fn place(stack: Rect, n: usize, card: Vec2, window: Rect, style: Style) -> Placed {
    let room = window.shrink(MARGIN);
    let inner = (room.width() - 2.0 * PAD_X).max(0.0);
    let gaps = n.saturating_sub(1) as f32;
    let (size, step) = match style {
        Style::Row => {
            let fits = (inner - GAP * gaps) / n.max(1) as f32;
            let width = fits.min(card.x).max(MIN_CARD.min(card.x));
            (vec2(width, card.y), width + GAP)
        }
        Style::Fan => {
            let mut step = (card.x * FAN_STEP).floor();
            if gaps > 0.0 {
                step = step.min((inner - card.x) / gaps).max(MIN_STEP);
            }
            (card, step)
        }
    };
    let span = size.x + step * gaps;
    let left = match style {
        Style::Row => stack.center().x - span / 2.0,
        Style::Fan => stack.left(),
    };
    let top = stack.top();
    let panel = Rect::from_min_max(
        pos2(left - PAD_X, top - HEAD_GAP - HEAD - PAD_TOP),
        pos2(left + span + PAD_X, top + size.y + PAD_BOTTOM),
    );
    let shift = vec2(
        slide(panel.left(), panel.right(), room.left(), room.right()),
        slide(panel.top(), panel.bottom(), room.top(), room.bottom()),
    );
    let cards = (0..n)
        .map(|at| Rect::from_min_size(pos2(left + step * at as f32, top) + shift, size))
        .collect();
    Placed {
        panel: panel.translate(shift),
        cards,
    }
}

/// The order a fan's cards are drawn in, back to front: the card in
/// `front` last, and the nearer a card is to it, the later.
pub fn draw_order(n: usize, front: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|at| std::cmp::Reverse(at.abs_diff(front)));
    order
}

/// The part of each fan card that shows: the front card whole, a card left
/// of it up to where the next card starts, a card right of it from where
/// the card before ends. These parts never overlap, so each takes the mouse
/// where it shows.
pub fn shown_parts(cards: &[Rect], front: usize) -> Vec<Rect> {
    cards
        .iter()
        .enumerate()
        .map(|(at, card)| {
            let mut part = *card;
            if at < front {
                part.max.x = cards[at + 1].left();
            } else if at > front {
                part.min.x = cards[at - 1].right();
            }
            part
        })
        .collect()
}

/// Where the selection starts when a group opens with the mouse over its
/// layer (`pointer`): the window under it, or the nearest one when it is
/// between or beside the windows. In a row that is usually the middle one,
/// as the row is centred on the stack that was clicked. None when the
/// pointer is not over the layer, as when Enter opened the group: then it
/// starts on the first window.
pub fn start_at(placed: &Placed, pointer: Pos2) -> Option<usize> {
    if !placed.panel.contains(pointer) {
        return None;
    }
    placed
        .cards
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let a = (a.center().x - pointer.x).abs();
            let b = (b.center().x - pointer.x).abs();
            a.total_cmp(&b)
        })
        .map(|(at, _)| at)
}

/// Left and Right among `count` windows, from `front`; no wrap.
pub fn step(front: usize, count: usize, left: bool, right: bool) -> usize {
    let last = count.saturating_sub(1);
    if left {
        front.saturating_sub(1)
    } else if right {
        (front + 1).min(last)
    } else {
        front.min(last)
    }
}

/// The part of a row picture at `dest` that the layer at `cover` leaves
/// free, when it is a rectangle: the layer covers one whole side of it. A
/// picture the layer does not touch keeps all of it; one it covers whole,
/// or cuts into another shape, keeps nothing.
pub fn left_over(dest: Rect, cover: Rect) -> Rect {
    if !dest.intersects(cover) {
        return dest;
    }
    let mut left = dest;
    let tall = cover.top() <= dest.top() && cover.bottom() >= dest.bottom();
    let wide = cover.left() <= dest.left() && cover.right() >= dest.right();
    if tall && cover.left() <= dest.left() && cover.right() < dest.right() {
        left.min.x = cover.right();
    } else if tall && cover.right() >= dest.right() && cover.left() > dest.left() {
        left.max.x = cover.left();
    } else if wide && cover.top() <= dest.top() && cover.bottom() < dest.bottom() {
        left.min.y = cover.bottom();
    } else if wide && cover.bottom() >= dest.bottom() && cover.top() > dest.top() {
        left.max.y = cover.top();
    } else {
        return Rect::NOTHING;
    }
    left
}

/// The group's windows put back in place of its card, so a window's menu
/// knows its real place in the row.
fn unfolded(group: &ArrangeGroup, stack: &Found) -> ArrangeGroup {
    let mut group = group.clone();
    if let Some(at) = group.windows.iter().position(|w| w.hwnd == stack.rep) {
        group.windows.splice(at..=at, stack.members.iter().cloned());
    }
    group
}

/// Draws the open group's layer above the row, takes the mouse there, and
/// puts its windows' pictures on it. Called after the row's cards.
pub fn show(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    frame: &mut Frame,
) {
    let Some(stack) = shared.stacks.open_stack().cloned() else {
        shared.stacks.layer = None;
        return;
    };
    let Some(card) = hit_rect(&shared.card_hits, stack.rep) else {
        // The row has not placed the group's card yet.
        ctx.request_repaint();
        return;
    };
    let style = if shared.snapshot.as_ref().is_some_and(|s| s.group_fan) {
        Style::Fan
    } else {
        Style::Row
    };
    let n = stack.members.len();
    let window = ctx.content_rect();
    let placed = place(card, n, card.size(), window, style);
    // The group's first frame: with the mouse over the layer the selection
    // starts on the window under it, so one window is lit, not two.
    if shared.stacks.layer.is_none() {
        if let Some(at) = ctx
            .input(|i| i.pointer.hover_pos())
            .and_then(|pointer| start_at(&placed, pointer))
        {
            shared.stacks.front = at;
        }
    }
    let front = shared.stacks.front.min(n.saturating_sub(1));
    let (order, parts) = match style {
        Style::Row => ((0..n).collect::<Vec<_>>(), placed.cards.clone()),
        Style::Fan => (draw_order(n, front), shown_parts(&placed.cards, front)),
    };
    let quiet = egui::Popup::is_any_open(ctx) || shared.stacks.dialog.is_some();
    let pictures: Vec<bool> = stack
        .members
        .iter()
        .map(|w| {
            !quiet && thumbnail(shared, w.hwnd).is_some_and(|thumb| thumb.source_size().is_some())
        })
        .collect();
    let full = unfolded(group, &stack);
    // The same menu as a card in the row, Rename included (WindowNamer on).
    let can_rename = shared.snapshot.as_ref().is_some_and(|s| s.can_rename);
    let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO);
    let tokens = Tokens::get(ctx);
    let id = egui::Id::new("deck-group-layer");
    let mut hovered = None;
    let mut clicked = None;
    let mut middle = None;
    let mut chosen = None;
    let mut drawn: Vec<(usize, Rect)> = Vec::new();
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(placed.panel.min)
        .movable(false)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(placed.panel.size());
            // The whole panel takes the mouse, so the row under it does not.
            ui.interact(placed.panel, id.with("back"), Sense::click());
            for (at, window) in stack.members.iter().enumerate() {
                let response = ui.interact(parts[at], id.with(window.hwnd), Sense::click());
                if response.hovered() {
                    hovered = Some(at);
                }
                if response.clicked() {
                    clicked = Some(window.hwnd);
                }
                if response.middle_clicked() {
                    middle = Some(window.hwnd);
                }
                let member = full.windows.iter().position(|w| w.hwnd == window.hwnd);
                response.context_menu(|ui| {
                    if let Some(member) = member {
                        let names = can_rename.then_some(full.windows[member].renamed);
                        if let Some(action) = card_menu(ui, &full, member, desktops, names) {
                            chosen = Some(action);
                        }
                    }
                    group_menu(ui, shared, &full, window.hwnd, false);
                });
            }
            let painter = ui.painter().clone();
            if tokens.shadows {
                let shadow = egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: tokens.palette_shadow,
                };
                painter.add(shadow.as_shape(placed.panel, 10));
            }
            painter.rect(
                placed.panel,
                10,
                tokens.window_bg,
                theme::line(ctx, theme::with_alpha(tokens.text_primary, 0.35)),
                StrokeKind::Inside,
            );
            header(&painter, &tokens, placed.panel, &stack, style);
            let front = match (style, hovered) {
                (Style::Row, Some(at)) if moved => at,
                _ => front,
            };
            for at in &order {
                let rect = placed.cards[*at];
                let chip_left = if style == Style::Fan && *at > front {
                    parts[*at].left() + CARD_PAD + 2.0
                } else {
                    rect.left() + CARD_PAD + 2.0
                };
                let picture = member_card(
                    &painter,
                    &tokens,
                    rect,
                    &stack.members[*at],
                    MemberLook {
                        number: at + 1,
                        chip_left,
                        selected: *at == front,
                        picture: pictures[*at],
                        edged: style == Style::Fan,
                    },
                );
                drawn.push((*at, picture));
            }
        });
    if moved {
        if let Some(at) = hovered.filter(|at| *at != shared.stacks.front) {
            shared.stacks.front = at;
            ctx.request_repaint();
        }
    }
    // The group's own card in the row gives its picture to the layer, and
    // the row's pictures keep only what the layer leaves free: DWM draws
    // pictures above everything.
    let members = stack.hwnds();
    frame
        .pending
        .retain(|item| item.ghost || !members.contains(&item.hwnd));
    for item in frame.pending.iter_mut().filter(|item| !item.layer) {
        item.clip = item.clip.intersect(left_over(item.dest, placed.panel));
    }
    for (at, picture) in drawn {
        if !pictures[at] {
            continue;
        }
        frame.pending.push(Pending {
            hwnd: stack.members[at].hwnd,
            dest: picture,
            clip: parts[at].intersect(window),
            opacity: 255,
            clear: Rect::NOTHING,
            ghost: false,
            layer: true,
        });
    }
    if !quiet {
        frame.list_row = hovered.map(|at| stack.members[at].hwnd);
    }
    if let Some(hwnd) = clicked {
        frame.activate = Some(hwnd);
    }
    if let Some(hwnd) = middle.filter(|hwnd| !frame.close.contains(hwnd)) {
        frame.close.push(hwnd);
    }
    if let Some(action) = chosen {
        frame.actions.push(action);
    }
    shared.stacks.layer = Some(placed.panel);
}

/// The group's name and its keys, along the top of the panel.
fn header(painter: &egui::Painter, tokens: &Tokens, panel: Rect, stack: &Found, style: Style) {
    let at = pos2(panel.left() + PAD_X, panel.top() + PAD_TOP);
    let width = panel.width() - 2.0 * PAD_X;
    let one_line = |text: &str, font: egui::FontId, colour: Color32, width: f32| {
        let mut job = text::job(text, font, colour, None);
        job.wrap = TextWrapping {
            max_width: width.max(0.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        painter.layout_job(job)
    };
    let name = one_line(
        &stack.name,
        theme::semibold(12.0),
        tokens.text_primary,
        width,
    );
    let n = stack.members.len();
    let keys = match style {
        Style::Row => {
            format!(" \u{b7} 1 \u{2013} {n} opens a window \u{b7} Esc closes the group")
        }
        Style::Fan => format!(" \u{b7} 1 \u{2013} {n} or \u{2190} \u{2192} \u{b7} Esc closes"),
    };
    let name_width = name.size().x;
    let rest = one_line(
        &keys,
        theme::regular(12.0),
        tokens.text_secondary,
        width - name_width,
    );
    let middle = at.y + HEAD / 2.0;
    painter.galley(
        pos2(at.x, middle - name.size().y / 2.0),
        name,
        tokens.text_primary,
    );
    painter.galley(
        pos2(at.x + name_width, middle - rest.size().y / 2.0),
        rest,
        tokens.text_secondary,
    );
}

struct MemberLook {
    number: usize,
    /// Where the number chip starts: on a fan card right of the front one,
    /// at the start of the part that shows.
    chip_left: f32,
    selected: bool,
    picture: bool,
    /// A fan's cards get an edge, so each shows where it starts.
    edged: bool,
}

/// One window's card on the layer: an opaque ground, its picture area, and
/// a solid number chip and its name under it. Returns the picture's place.
fn member_card(
    painter: &egui::Painter,
    tokens: &Tokens,
    rect: Rect,
    window: &ArrangeWindow,
    look: MemberLook,
) -> Rect {
    let ctx = painter.ctx();
    painter.rect_filled(rect, RADIUS, tokens.window_bg);
    let fill = selected_fill(tokens);
    paint_look(
        painter,
        rect,
        RADIUS,
        card_look(tokens, look.selected.then_some(fill), window.attention),
    );
    if look.edged {
        painter.rect_stroke(
            rect,
            RADIUS,
            theme::line(ctx, tokens.border),
            StrokeKind::Inside,
        );
    }
    if look.selected {
        paint_ring(painter, ctx, rect, RADIUS, selected_ring(tokens));
    }
    let area = Rect::from_min_size(
        rect.min + vec2(CARD_PAD, CARD_PAD),
        vec2(
            rect.width() - 2.0 * CARD_PAD,
            (rect.height() - 2.0 * CARD_PAD - TITLE_GAP - TITLE).max(0.0),
        ),
    );
    let border = if look.selected {
        selected_ring(tokens)
    } else {
        tokens.border
    };
    painter.rect(
        area,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(ctx, border),
        StrokeKind::Inside,
    );
    let picture = area.shrink(1.0);
    if !look.picture {
        name_card(painter, picture, &window.label, "");
    }
    let top = area.bottom() + TITLE_GAP;
    let number = painter.layout_no_wrap(
        look.number.to_string(),
        theme::semibold(12.0),
        tokens.window_bg,
    );
    let chip = Rect::from_min_size(
        pos2(look.chip_left, top),
        vec2((number.size().x + 10.0).max(TITLE), TITLE),
    );
    painter.rect_filled(chip, tokens.radius_row, tokens.text_primary);
    painter.galley(
        chip.center() - number.size() / 2.0,
        number,
        tokens.window_bg,
    );
    let left = chip.right() + NUMBER_GAP;
    let right = rect.right() - CARD_PAD - 2.0;
    let mut job = text::job(
        &window.label,
        theme::regular(12.0),
        tokens.text_primary,
        None,
    );
    job.wrap = TextWrapping {
        max_width: (right - left).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(left, chip.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_primary,
    );
    picture
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;

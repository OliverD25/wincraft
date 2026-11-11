//! The list under a selected or pointed-at stack: one row per window, with
//! its number. It opens when the stack is selected or the mouse is on it,
//! and stays open while the mouse moves from the stack into the list. A row
//! lights up under the mouse and a click on it switches to that window.

use super::*;

const LIST_WIDTH: f32 = 260.0;
const LIST_ROW: f32 = 20.0;
const LIST_PAD: f32 = 8.0;

/// The list shown last frame, and where the mouse keeps it open.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui::arrange) struct Shown {
    pub hwnd: isize,
    /// The stack's card, the list and the strip between them.
    pub keeps: Vec<Rect>,
    /// The selected card when the list opened: a new selection ends it.
    pub cursor: Option<isize>,
}

/// The rectangles that keep a list open: the card, the list, and the strip
/// between them, so the mouse can cross the gap without closing it.
pub fn keep_area(card: Rect, list: Rect) -> Vec<Rect> {
    let left = card.left().max(list.left());
    let right = card.right().min(list.right()).max(left);
    let (top, bottom) = if list.top() >= card.bottom() {
        (card.bottom(), list.top())
    } else {
        (list.bottom(), card.top())
    };
    let gap = Rect::from_min_max(pos2(left, top), pos2(right, bottom.max(top)));
    vec![card, list, gap]
}

/// Which stack's list shows now: the one the mouse still holds open, else
/// the stack under the mouse, else the selected stack.
pub fn which(
    kept: Option<&Shown>,
    pointer: Option<Pos2>,
    cursor: Option<isize>,
    hovered: Option<isize>,
    selected: Option<isize>,
) -> Option<isize> {
    let held = kept
        .filter(|shown| shown.cursor == cursor)
        .filter(|shown| pointer.is_some_and(|at| shown.keeps.iter().any(|r| r.contains(at))))
        .map(|shown| shown.hwnd);
    held.or(hovered).or(selected)
}

pub fn show(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    frame: &mut Frame,
) {
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    let kept = shared.stacks.list.take();
    let stack_of = |hwnd: &isize| shared.stacks.closed(*hwnd).is_some();
    let kept = kept.filter(|shown| stack_of(&shown.hwnd));
    let shown = which(
        kept.as_ref(),
        pointer,
        shared.cursor,
        frame.hovered.filter(stack_of),
        shared
            .cursor
            .filter(|hwnd| !shared.cursor_ghost && stack_of(hwnd)),
    );
    let Some(hwnd) = shown else {
        return;
    };
    let (Some(stack), Some(card)) = (
        shared.stacks.closed(hwnd).cloned(),
        hit_rect(&shared.card_hits, hwnd),
    ) else {
        return;
    };
    let tokens = Tokens::get(ctx);
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
    let id = egui::Id::new("deck-stack-list");
    let mut chosen = None;
    let mut row_under = None;
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(list.min)
        .movable(false)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(list.size());
            // The whole list takes the mouse, so the card under it does not.
            ui.interact(list, id.with("back"), egui::Sense::click());
            let painter = ui.painter().clone();
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
            // The row light is the right-click menu's.
            let lit = ui.visuals().widgets.hovered;
            for (at, window) in stack.members.iter().enumerate() {
                let line = row(at + 1);
                let response = ui.interact(
                    line.expand2(vec2(4.0, 0.0)),
                    id.with(window.hwnd),
                    egui::Sense::click(),
                );
                if response.hovered() {
                    row_under = Some(window.hwnd);
                    painter.rect_filled(
                        line.expand2(vec2(4.0, 0.0)),
                        lit.corner_radius,
                        lit.weak_bg_fill,
                    );
                }
                if response.clicked() {
                    chosen = Some(window.hwnd);
                }
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
        });
    if let Some(window) = chosen {
        shared.stacks.list_chosen = Some(window);
        ctx.request_repaint();
    }
    shared.stacks.list = Some(Shown {
        hwnd,
        keeps: keep_area(card, list),
        cursor: shared.cursor,
    });
    frame.list_row = row_under;
    frame.covers.push(list);
}

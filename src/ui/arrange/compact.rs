//! The compact Arrange strip: a small popup for an app with one or two
//! windows on this desktop, where the full strip would be mostly empty. It
//! shows those windows in their monitor's shape, and → or the button above
//! them opens the app's windows on the other desktops to the right; Enter on
//! one of those switches to its desktop.

use egui::text::TextWrapping;
use egui::{pos2, vec2, Rect, Response, Sense, StrokeKind, Ui, Vec2};

use super::{
    by_monitor, card_menu, middle_close, name_card, number_chip, only_visible, rows,
    taskbar_numbers, thumbnail, visible_flags, Frame, Pending, Row, Shared,
};
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeMonitor, ArrangeSnapshot,
};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::text;

const PAD: f32 = 10.0;
const GAP: f32 = 8.0;
const HEADER: f32 = 28.0;
const CARD_WIDTH: f32 = 180.0;
const CARD_PAD: f32 = 6.0;
const KEYCAP: f32 = 22.0;
/// A picture in a monitor's shape is never flatter or taller than these.
const THUMB_MIN: f32 = 90.0;
const THUMB_MAX: f32 = 240.0;
/// The shape of a monitor that is not known.
const DEFAULT_SHAPE: Vec2 = Vec2::new(16.0, 9.0);
const BUTTON: f32 = 30.0;
const HINTS: f32 = 16.0;
/// "This desktop" and "Desktop 5 · Left" over the cards, once expanded.
const LABEL: f32 = 18.0;
/// Before another desktop's group: its 1 px rule and the room after it.
const GROUP_LEAD: f32 = 1.0 + 10.0;
/// Another desktop's pictures until one is picked: DWM cannot grey a
/// picture, so it is only faded.
const OTHER_OPACITY: f32 = 0.6;
/// The compact popup shows at most this many windows of this desktop.
pub const MAX_WINDOWS: usize = 2;

/// The rows of the group that the strip shows: this desktop first, then
/// every other desktop with shown windows.
fn shown(group: &ArrangeGroup, desktops: &[ArrangeDesktop], visible: &[bool]) -> Vec<Row> {
    only_visible(rows(group, desktops), visible)
}

/// Whether the strip opens compact: one or two shown windows of the group
/// are on this desktop. A hover strip on a taskbar that lists only its own
/// monitor's windows counts only those (`visible`).
pub fn applies(group: &ArrangeGroup, desktops: &[ArrangeDesktop], visible: &[bool]) -> bool {
    let here = shown(group, desktops, visible)
        .first()
        .map_or(0, |row| row.members.len());
    (1..=MAX_WINDOWS).contains(&here)
}

/// The words on the button over the cards: how many windows wait on the
/// other desktops, given as (desktop name, windows) for each of them. None
/// when there are none.
pub fn more_label(others: &[(String, usize)]) -> Option<String> {
    let total: usize = others.iter().map(|(_, count)| count).sum();
    match others {
        [] => None,
        _ if total == 0 => None,
        [(name, count)] => Some(format!("+{count} on {name}")),
        _ => Some(format!("+{total} on {} other desktops", others.len())),
    }
}

/// How tall a picture 180 wide is in the shape of a monitor `size` big,
/// held between 90 and 240.
pub fn thumb_height(size: Option<Vec2>) -> f32 {
    let shape = size
        .filter(|size| size.x > 0.0 && size.y > 0.0)
        .unwrap_or(DEFAULT_SHAPE);
    (CARD_WIDTH * shape.y / shape.x)
        .round()
        .clamp(THUMB_MIN, THUMB_MAX)
}

/// "Desktop 5 · Left": a desktop's name and the monitors its windows are on.
pub fn group_label(desktop: &str, monitors: &[&str]) -> String {
    let mut names: Vec<&str> = Vec::new();
    for name in monitors.iter().copied().filter(|name| !name.is_empty()) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        desktop.to_string()
    } else {
        format!("{desktop} \u{b7} {}", names.join(", "))
    }
}

/// The popup's left edge once the other desktops open: where it was, unless
/// a popup `width` wide would then leave the work area on the right, in
/// which case it moves left as far as needed.
pub fn keep_left(left: i32, width: i32, work: [i32; 4]) -> i32 {
    left.min(work[2] - width).max(work[0])
}

/// The key hints under the button, over the cards. `switch_to` names the desktop of the
/// picked window when it is on another one.
pub fn hints(expanded: bool, more: bool, switch_to: Option<&str>) -> Vec<(String, String)> {
    let enter = match switch_to {
        Some(desktop) => format!("Switch to {desktop}"),
        None => "Open".to_string(),
    };
    let mut hints = vec![("\u{21b5}".to_string(), enter)];
    if expanded {
        hints.push(("\u{2190}".to_string(), "Back".to_string()));
        hints.push(("Esc".to_string(), "Close".to_string()));
    } else if more {
        hints.push(("\u{2192}".to_string(), "More".to_string()));
    }
    hints
}

/// What Enter on the window at `at` in `order` does: it comes to the front.
/// For a window on another desktop that also switches to that desktop, as
/// Windows does for a window brought to the front there.
pub fn enter(group: &ArrangeGroup, order: &[usize], at: usize) -> Option<ArrangeAction> {
    let member = order.get(at)?;
    Some(ArrangeAction::Activate(group.windows[*member].hwnd))
}

/// What ← or → does in the popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Pick(usize),
    /// → from the last window of this desktop: the other desktops open,
    /// and the first of their windows is picked.
    Expand,
    /// ← from the first window of another desktop: back to this desktop's
    /// last window.
    Collapse,
    Nothing,
}

/// ← (`forward` false) or → from the window at `at` in `order`, the windows
/// on screen, `here` of them on this desktop. `more` says whether other
/// desktops have windows to open.
pub fn arrow(
    order_len: usize,
    here: usize,
    at: usize,
    forward: bool,
    expanded: bool,
    more: bool,
) -> Arrow {
    if forward {
        if !expanded {
            if at + 1 < here {
                Arrow::Pick(at + 1)
            } else if more {
                Arrow::Expand
            } else {
                Arrow::Nothing
            }
        } else if at + 1 < order_len {
            Arrow::Pick(at + 1)
        } else {
            Arrow::Nothing
        }
    } else if expanded && at == here {
        Arrow::Collapse
    } else if at > 0 {
        Arrow::Pick(at - 1)
    } else {
        Arrow::Nothing
    }
}

/// The group on screen and the data it came with.
#[derive(Clone, Copy)]
struct Popup<'a> {
    group: &'a ArrangeGroup,
    snapshot: &'a ArrangeSnapshot,
}

/// Everything the popup is laid out from.
struct Layout {
    /// This desktop's windows, as places in the group.
    here: Vec<usize>,
    /// Each other desktop with shown windows: its name and its windows.
    others: Vec<(String, Vec<usize>)>,
    thumb: f32,
}

impl Layout {
    fn new(group: &ArrangeGroup, snapshot: &ArrangeSnapshot, visible: &[bool]) -> Self {
        let rows = shown(group, &snapshot.desktops, visible);
        let here = rows
            .first()
            .map(|row| row.members.clone())
            .unwrap_or_default();
        let others = rows
            .iter()
            .skip(1)
            .map(|row| (row.title.clone(), row.members.clone()))
            .collect();
        // One height for every card, from this desktop's windows, so a card
        // never moves when the other desktops open.
        let thumb = here
            .iter()
            .map(|member| thumb_height(monitor_size(group, *member, &snapshot.monitors)))
            .fold(THUMB_MIN, f32::max);
        Self {
            here,
            others,
            thumb,
        }
    }

    fn card_height(&self) -> f32 {
        CARD_PAD + self.thumb + CARD_PAD + KEYCAP + CARD_PAD
    }

    fn more(&self) -> bool {
        !self.others.is_empty()
    }

    /// Where the "+N on Desktop X" button starts, from the popup's content top.
    fn button_top(&self) -> f32 {
        HEADER + GAP
    }

    /// Where the key hints start, from the content top.
    fn hints_top(&self) -> f32 {
        let button = if self.more() { BUTTON + GAP } else { 0.0 };
        self.button_top() + button
    }

    /// Where the cards start, from the content top: under the hints, and
    /// under the desktop labels once expanded.
    fn cards_top(&self, expanded: bool) -> f32 {
        let labels = if expanded { LABEL } else { 0.0 };
        self.hints_top() + HINTS + GAP + labels
    }

    /// The windows on screen, in the order ← and → walk them.
    fn order(&self, expanded: bool) -> Vec<usize> {
        let mut order = self.here.clone();
        if expanded {
            order.extend(
                self.others
                    .iter()
                    .flat_map(|(_, members)| members.iter().copied()),
            );
        }
        order
    }

    fn size(&self, expanded: bool) -> Vec2 {
        let cards = |count: usize| count as f32 * CARD_WIDTH + count.saturating_sub(1) as f32 * GAP;
        let mut width = cards(self.here.len());
        let height = PAD + self.cards_top(expanded) + self.card_height() + PAD;
        if expanded {
            for (_, members) in &self.others {
                width += GAP + GROUP_LEAD + cards(members.len());
            }
        }
        vec2(width + 2.0 * PAD, height)
    }
}

/// The monitor shape of the screen a window is on.
fn monitor_size(group: &ArrangeGroup, member: usize, monitors: &[ArrangeMonitor]) -> Option<Vec2> {
    let window = &group.windows[member];
    let monitor = by_monitor::column_of(window, monitors).and_then(|index| monitors.get(index))?;
    let [left, top, right, bottom] = monitor.rect;
    Some(vec2((right - left) as f32, (bottom - top) as f32))
}

/// The popup's size for the group.
pub fn panel_size(shared: &Shared, group: &ArrangeGroup, snapshot: &ArrangeSnapshot) -> Vec2 {
    let visible = visible_flags(group, &snapshot.monitors, shared.only_monitor);
    let layout = Layout::new(group, snapshot, &visible);
    layout.size(expanded(shared, &layout))
}

fn expanded(shared: &Shared, layout: &Layout) -> bool {
    shared.others.shown(layout.others.len())
}

/// What the popup did this frame, and the group picked in its header.
pub fn body(
    ui: &mut Ui,
    shared: &mut Shared,
    group: &ArrangeGroup,
    snapshot: &ArrangeSnapshot,
    panel: Rect,
    menu_open: bool,
) -> (Frame, Option<usize>) {
    let ctx = ui.ctx().clone();
    let tokens = Tokens::get(&ctx);
    let visible = visible_flags(group, &snapshot.monitors, shared.only_monitor);
    let layout = Layout::new(group, snapshot, &visible);
    let mut frame = Frame::default();
    let open = expanded(shared, &layout);
    let order = layout.order(open);
    let member_of = |hwnd: isize| group.windows.iter().position(|w| w.hwnd == hwnd);
    shared.cursor_ghost = false;
    if shared
        .cursor
        .and_then(member_of)
        .is_none_or(|member| !order.contains(&member))
    {
        shared.cursor = order.first().map(|member| group.windows[*member].hwnd);
    }

    ui.painter().rect(
        panel,
        tokens.radius_window,
        tokens.elevated_bg,
        theme::line(&ctx, tokens.border),
        StrokeKind::Inside,
    );
    let content = panel.shrink(PAD);
    let header = Rect::from_min_size(content.min, vec2(content.width(), HEADER));
    let picker_id = ui.id().with("compact-group-picker");
    let chosen = group_picker(ui, header, snapshot, group, picker_id);

    let selected = shared.cursor.and_then(member_of);
    let popup = Popup { group, snapshot };
    // Header, the button and the key hints come first, so a title's tooltip
    // opens downwards over the pictures and not over the button.
    if let Some(text) = more_label(
        &layout
            .others
            .iter()
            .map(|(name, members)| (name.clone(), members.len()))
            .collect::<Vec<_>>(),
    ) {
        let width = layout.size(false).x - 2.0 * PAD;
        let rect = Rect::from_min_size(
            pos2(content.left(), content.top() + layout.button_top()),
            vec2(width, BUTTON),
        );
        let words = if open {
            "Back to this desktop".to_string()
        } else {
            text
        };
        if more_button(ui, rect, &words, open).clicked() {
            toggle(shared, group, &layout, open);
        }
    }
    let switch_to = selected
        .filter(|member| !layout.here.contains(member))
        .and_then(|member| {
            layout
                .others
                .iter()
                .find(|(_, members)| members.contains(&member))
                .map(|(name, _)| name.as_str())
        });
    hint_line(
        ui,
        pos2(content.left(), content.top() + layout.hints_top()),
        &hints(open, layout.more(), switch_to),
    );
    let top = content.top() + layout.cards_top(open);

    let numbers = taskbar_numbers(group, &snapshot.desktops);
    let card_height = layout.card_height();
    let mut x = content.left();
    let mut cards_area = Rect::NOTHING;
    if open {
        label(ui, pos2(x, top - LABEL), "This desktop", tokens.accent);
    }
    let mut draw_cards = |ui: &mut Ui,
                          shared: &mut Shared,
                          frame: &mut Frame,
                          members: &[usize],
                          x: &mut f32,
                          other: bool| {
        for member in members {
            let rect = Rect::from_min_size(pos2(*x, top), vec2(CARD_WIDTH, card_height));
            *x += CARD_WIDTH + GAP;
            cards_area = cards_area.union(rect);
            let picked = selected == Some(*member);
            card(
                ui,
                shared,
                frame,
                popup,
                *member,
                rect,
                CardLook {
                    thumb: layout.thumb,
                    number: numbers[*member],
                    picked,
                    faded: other && !picked,
                    menu_open,
                },
            );
        }
    };
    draw_cards(ui, shared, &mut frame, &layout.here, &mut x, false);
    if open {
        for (name, members) in &layout.others {
            let rule_x = x;
            ui.painter().vline(
                rule_x,
                egui::Rangef::new(top - LABEL, top + card_height),
                theme::line(&ctx, tokens.border),
            );
            x += GROUP_LEAD;
            let monitors: Vec<&str> = members
                .iter()
                .filter_map(|member| {
                    by_monitor::column_of(&group.windows[*member], &snapshot.monitors)
                        .and_then(|index| snapshot.monitors.get(index))
                        .map(|monitor| monitor.label.as_str())
                })
                .collect();
            label(
                ui,
                pos2(x, top - LABEL),
                &group_label(name, &monitors),
                tokens.text_disabled,
            );
            draw_cards(ui, shared, &mut frame, members, &mut x, true);
        }
    }
    frame.area = cards_area;

    if !menu_open {
        keys(&ctx, shared, popup, &layout, &order, picker_id, &mut frame);
    }
    (frame, chosen)
}

/// The group picked in the header's list, as in the full strip's app
/// picker. A click or Tab opens it.
fn group_picker(
    ui: &mut Ui,
    header: Rect,
    snapshot: &ArrangeSnapshot,
    group: &ArrangeGroup,
    popup_id: egui::Id,
) -> Option<usize> {
    let tokens = Tokens::get(ui.ctx());
    let response = ui
        .interact(header, popup_id.with("header"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let chevron = Rect::from_center_size(
        pos2(header.right() - 7.0, header.center().y),
        Vec2::splat(14.0),
    );
    let mut job = text::job(
        &group.label,
        theme::semibold(13.0),
        tokens.text_primary,
        None,
    );
    job.wrap = TextWrapping {
        max_width: (chevron.left() - 6.0 - header.left()).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(header.left(), header.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_primary,
    );
    icons::paint(
        ui.painter(),
        chevron,
        Icon::ChevronDown,
        tokens.text_primary,
    );
    let mut chosen = None;
    egui::Popup::menu(&response)
        .id(popup_id)
        .width(header.width().max(200.0))
        .show(|ui| {
            for (index, other) in snapshot.groups.iter().enumerate() {
                let (row, row_response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                if row_response.hovered() {
                    ui.painter()
                        .rect_filled(row, tokens.radius_row, tokens.hover_bg);
                }
                let colour = if other.key == group.key {
                    tokens.accent
                } else {
                    tokens.text_primary
                };
                ui.painter().text(
                    pos2(row.left() + 10.0, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    &other.label,
                    theme::regular(13.0),
                    colour,
                );
                if row_response.clicked() && other.key != group.key {
                    chosen = Some(index);
                }
            }
        });
    chosen
}

fn label(ui: &Ui, at: egui::Pos2, words: &str, colour: egui::Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(words.to_string(), theme::regular(12.0), colour);
    ui.painter().galley(
        pos2(at.x, at.y + (LABEL - 4.0 - galley.size().y) / 2.0),
        galley,
        colour,
    );
}

/// How one card is drawn.
struct CardLook {
    thumb: f32,
    number: usize,
    picked: bool,
    /// Another desktop's window that is not picked.
    faded: bool,
    menu_open: bool,
}

fn card(
    ui: &mut Ui,
    shared: &mut Shared,
    frame: &mut Frame,
    popup: Popup,
    member: usize,
    rect: Rect,
    look: CardLook,
) {
    let Popup { group, snapshot } = popup;
    let tokens = Tokens::get(ui.ctx());
    let window = &group.windows[member];
    let response = ui.interact(
        rect,
        egui::Id::new(("arrange-compact-card", window.hwnd)),
        Sense::click(),
    );
    let painter = ui.painter();
    if look.picked {
        painter.rect(
            rect,
            tokens.radius_tile,
            tokens.selection_bg,
            theme::stroke(ui.ctx(), 2.0, tokens.accent),
            StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, tokens.radius_tile, tokens.hover_bg);
    }
    // The title row is first and the picture last, so the pictures of all
    // cards sit on one line at the bottom of the popup.
    let (title_top, thumb_top) = card_rows(rect.top());
    let thumb = Rect::from_min_size(
        pos2(rect.left() + CARD_PAD, thumb_top),
        vec2(rect.width() - 2.0 * CARD_PAD, look.thumb),
    );
    painter.rect(
        thumb,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let picture = thumb.shrink(1.0);
    let opacity = if look.faded { OTHER_OPACITY } else { 1.0 };
    let has_picture = !look.menu_open
        && thumbnail(shared, window.hwnd).is_some_and(|thumb| thumb.source_size().is_some());
    if has_picture {
        frame.pending.push(Pending {
            hwnd: window.hwnd,
            dest: picture,
            clip: ui.clip_rect(),
            opacity: (255.0 * opacity).round() as u8,
            clear: Rect::NOTHING,
            ghost: false,
        });
    } else {
        let mut faded = painter.clone();
        faded.multiply_opacity(opacity);
        name_card(&faded, picture, &window.label, "");
    }
    let chip = number_chip(
        painter,
        pos2(rect.left() + CARD_PAD, title_top),
        KEYCAP,
        look.number,
    );
    let colour = if look.faded {
        tokens.text_secondary
    } else {
        tokens.text_primary
    };
    let left = chip.right() + 6.0;
    let mut job = text::job(&window.label, theme::regular(13.0), colour, None);
    job.wrap = TextWrapping {
        max_width: (rect.right() - CARD_PAD - left).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    let elided = galley.elided;
    painter.galley(
        pos2(left, title_top + (KEYCAP - galley.size().y) / 2.0),
        galley,
        colour,
    );
    if elided {
        title_tooltip(&response, title_top, &window.label);
    }
    interact(shared, frame, group, snapshot, member, &response);
}

/// Where a card whose top is `top` starts its title row and its picture.
fn card_rows(top: f32) -> (f32, f32) {
    let title = top + CARD_PAD;
    (title, title + KEYCAP + CARD_PAD)
}

/// The full title as a tooltip under the title row. Anchored to the row and
/// not to the whole card: the card ends at the bottom of the popup, so a
/// tooltip under it would not fit and would open upwards over the button.
fn title_tooltip(card: &Response, title_top: f32, title: &str) {
    let mut row = card.clone();
    row.rect = Rect::from_min_size(
        pos2(card.rect.left(), title_top),
        vec2(card.rect.width(), KEYCAP),
    );
    egui::Tooltip::for_enabled(&row).show(|ui| {
        ui.set_max_width(ui.spacing().tooltip_width);
        ui.add(egui::Label::new(title));
    });
}

/// Clicks, the menu and the hover peek of one card, as in the full strip:
/// a click switches to the window, a middle-click closes it.
fn interact(
    shared: &mut Shared,
    frame: &mut Frame,
    group: &ArrangeGroup,
    snapshot: &ArrangeSnapshot,
    member: usize,
    response: &Response,
) {
    let window = &group.windows[member];
    frame.cards.push((window.hwnd, window.monitor.is_some()));
    if response.hovered() {
        frame.hovered = Some(window.hwnd);
    }
    if response.clicked() {
        shared.cursor = Some(window.hwnd);
        frame.activate = Some(window.hwnd);
    }
    if response.middle_clicked() {
        frame.close.extend(middle_close(window.hwnd, false));
    }
    let names = snapshot.can_rename.then_some(window.renamed);
    response.context_menu(|ui| {
        if let Some(action) = card_menu(ui, group, member, &snapshot.desktops, names) {
            frame.actions.push(action);
        }
    });
}

fn more_button(ui: &mut Ui, rect: Rect, words: &str, open: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let response = ui
        .interact(rect, egui::Id::new("arrange-compact-more"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    let fill = if response.hovered() {
        tokens.hover_bg
    } else {
        egui::Color32::TRANSPARENT
    };
    painter.rect(
        rect,
        tokens.radius_row,
        fill,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let arrow = Rect::from_center_size(
        pos2(rect.right() - 10.0 - 7.0, rect.center().y),
        Vec2::splat(14.0),
    );
    let icon = if open {
        Icon::ArrowLeft
    } else {
        Icon::ArrowRight
    };
    icons::paint(painter, arrow, icon, tokens.text_secondary);
    let mut job = text::job(words, theme::regular(13.0), tokens.text_secondary, None);
    job.wrap = TextWrapping {
        max_width: (arrow.left() - 6.0 - rect.left() - 10.0).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_secondary,
    );
    response
}

fn hint_line(ui: &mut Ui, at: egui::Pos2, hints: &[(String, String)]) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let mut x = at.x;
    let centre = at.y + HINTS / 2.0;
    for (index, (key, words)) in hints.iter().enumerate() {
        if index > 0 {
            let dot = painter.layout_no_wrap(
                "\u{b7}".to_string(),
                theme::regular(12.0),
                tokens.text_disabled,
            );
            let width = dot.size().x;
            painter.galley(
                pos2(x + 6.0, centre - dot.size().y / 2.0),
                dot,
                tokens.text_disabled,
            );
            x += width + 12.0;
        }
        let font = if key.is_ascii() {
            theme::regular(12.0)
        } else {
            theme::mono(12.0)
        };
        let key = painter.layout_no_wrap(key.clone(), font, tokens.text_secondary);
        let chip = Rect::from_min_size(
            pos2(x, centre - key.size().y / 2.0 - 1.0),
            vec2(key.size().x + 10.0, key.size().y + 2.0),
        );
        painter.rect(
            chip,
            tokens.radius_row,
            tokens.panel_bg,
            theme::line(ui.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        painter.galley(chip.center() - key.size() / 2.0, key, tokens.text_secondary);
        x = chip.right() + 6.0;
        let words =
            painter.layout_no_wrap(words.clone(), theme::regular(12.0), tokens.text_secondary);
        let width = words.size().x;
        painter.galley(
            pos2(x, centre - words.size().y / 2.0),
            words,
            tokens.text_secondary,
        );
        x += width;
    }
}

/// Opens the other desktops and picks their first window, or closes them
/// and picks this desktop's last.
fn toggle(shared: &mut Shared, group: &ArrangeGroup, layout: &Layout, open: bool) {
    if open {
        shared.others.reset();
        shared.cursor = layout.here.last().map(|member| group.windows[*member].hwnd);
    } else {
        shared.others.expand();
        shared.cursor = layout
            .others
            .first()
            .and_then(|(_, members)| members.first())
            .map(|member| group.windows[*member].hwnd);
    }
}

/// ← and → pick, and open or close the other desktops; Enter switches to
/// the picked window, on its own desktop; Tab opens the group list. The
/// keys that move a window or change its number work as in the full strip.
fn keys(
    ctx: &egui::Context,
    shared: &mut Shared,
    popup: Popup,
    layout: &Layout,
    order: &[usize],
    picker_id: egui::Id,
    frame: &mut Frame,
) {
    let Popup { group, snapshot } = popup;
    let (left, right, enter_pressed, tab) = ctx.input_mut(|i| {
        // One line of cards: ↑ and ↓ do nothing here. Taken anyway, or the
        // full strip's keys would pick by places this popup does not have.
        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
        i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
        )
    });
    if tab {
        egui::Popup::open_id(ctx, picker_id);
        return;
    }
    let Some(at) = shared
        .cursor
        .and_then(|hwnd| order.iter().position(|m| group.windows[*m].hwnd == hwnd))
    else {
        return;
    };
    if enter_pressed {
        if let Some(ArrangeAction::Activate(hwnd)) = enter(group, order, at) {
            frame.activate = Some(hwnd);
        }
        return;
    }
    if left || right {
        let open = shared.others.shown(layout.others.len());
        match arrow(
            order.len(),
            layout.here.len(),
            at,
            right,
            open,
            layout.more(),
        ) {
            Arrow::Pick(next) => shared.cursor = Some(group.windows[order[next]].hwnd),
            Arrow::Expand => toggle(shared, group, layout, false),
            Arrow::Collapse => toggle(shared, group, layout, true),
            Arrow::Nothing => {}
        }
        ctx.request_repaint();
        return;
    }
    by_monitor::move_keys(ctx, shared, group, snapshot, frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::ArrangeWindow;

    fn desktops() -> Vec<ArrangeDesktop> {
        ["Desktop 1", "Desktop 2", "Desktop 5"]
            .iter()
            .enumerate()
            .map(|(i, name)| ArrangeDesktop {
                id: format!("d{i}"),
                name: name.to_string(),
                current: i == 0,
            })
            .collect()
    }

    fn group(on: &[&str]) -> ArrangeGroup {
        ArrangeGroup {
            key: "code.exe".to_string(),
            label: "Code".to_string(),
            windows: on
                .iter()
                .enumerate()
                .map(|(i, desktop)| ArrangeWindow {
                    hwnd: 100 + i as isize,
                    label: format!("w{i}"),
                    desktop: desktop.to_string(),
                    monitor: None,
                    on_monitor: Some(0),
                    saved_monitor: Some(0),
                    renamed: false,
                    same_title: false,
                    attention: false,
                })
                .collect(),
        }
    }

    #[test]
    fn one_or_two_windows_on_this_desktop_open_the_compact_popup() {
        let desktops = desktops();
        let all = |n: usize| vec![true; n];
        assert!(!applies(&group(&["d1"]), &desktops, &all(1)), "none here");
        assert!(applies(&group(&["d0"]), &desktops, &all(1)));
        assert!(applies(&group(&["d0", "d2", "d0"]), &desktops, &all(3)));
        assert!(!applies(&group(&["d0", "d0", "d0"]), &desktops, &all(3)));
        // A secondary taskbar's hover shows only its monitor's windows: two
        // of the three are on it.
        assert!(applies(
            &group(&["d0", "d0", "d0"]),
            &desktops,
            &[true, false, true]
        ));
    }

    #[test]
    fn the_button_counts_the_other_desktops_windows() {
        assert_eq!(more_label(&[]), None);
        assert_eq!(
            more_label(&[("Desktop 5".to_string(), 2)]).as_deref(),
            Some("+2 on Desktop 5")
        );
        assert_eq!(
            more_label(&[("Desktop 2".to_string(), 1), ("Desktop 5".to_string(), 3)]).as_deref(),
            Some("+4 on 2 other desktops")
        );
    }

    #[test]
    fn a_picture_takes_its_monitors_shape_within_limits() {
        // 2560 x 2880 at 180 wide is 202.5 high.
        assert_eq!(thumb_height(Some(vec2(2560.0, 2880.0))), 203.0);
        assert_eq!(thumb_height(Some(vec2(1920.0, 1080.0))), 101.0);
        assert_eq!(thumb_height(Some(vec2(5120.0, 1440.0))), THUMB_MIN);
        assert_eq!(thumb_height(Some(vec2(1080.0, 3840.0))), THUMB_MAX);
        assert_eq!(thumb_height(None), 101.0);
    }

    #[test]
    fn another_desktops_group_names_its_desktop_and_monitors() {
        assert_eq!(group_label("Desktop 5", &["Left"]), "Desktop 5 \u{b7} Left");
        assert_eq!(
            group_label("Desktop 5", &["Left", "Left", "Right"]),
            "Desktop 5 \u{b7} Left, Right"
        );
        assert_eq!(group_label("Desktop 5", &[""]), "Desktop 5");
    }

    #[test]
    fn right_from_the_last_card_expands_and_left_from_the_first_other_collapses() {
        // Two windows here, three on other desktops.
        assert_eq!(arrow(2, 2, 0, true, false, true), Arrow::Pick(1));
        assert_eq!(arrow(2, 2, 1, true, false, true), Arrow::Expand);
        assert_eq!(arrow(2, 2, 1, true, false, false), Arrow::Nothing);
        assert_eq!(arrow(5, 2, 2, false, true, true), Arrow::Collapse);
        assert_eq!(arrow(5, 2, 3, false, true, true), Arrow::Pick(2));
        assert_eq!(arrow(5, 2, 4, true, true, true), Arrow::Nothing);
        assert_eq!(arrow(5, 2, 1, true, true, true), Arrow::Pick(2));
        assert_eq!(arrow(2, 2, 0, false, false, true), Arrow::Nothing);
    }

    #[test]
    fn the_popup_keeps_its_left_edge_unless_it_would_leave_the_screen() {
        let work = [0, 0, 1920, 1040];
        assert_eq!(keep_left(800, 600, work), 800);
        assert_eq!(keep_left(1500, 600, work), 1320);
        assert_eq!(keep_left(100, 3000, work), 0);
    }

    #[test]
    fn enter_on_another_desktops_window_brings_that_window_to_the_front() {
        let group = group(&["d0", "d2"]);
        // Expanded: this desktop's window, then Desktop 5's.
        let order = [0, 1];
        assert_eq!(enter(&group, &order, 1), Some(ArrangeAction::Activate(101)));
        assert_eq!(enter(&group, &order, 0), Some(ArrangeAction::Activate(100)));
        assert_eq!(enter(&group, &order, 2), None);
    }

    #[test]
    fn the_hints_say_what_enter_does() {
        let words = |hints: Vec<(String, String)>| -> Vec<String> {
            hints.into_iter().map(|(_, words)| words).collect()
        };
        assert_eq!(words(hints(false, true, None)), ["Open", "More"]);
        assert_eq!(words(hints(false, false, None)), ["Open"]);
        assert_eq!(
            words(hints(true, true, Some("Desktop 5"))),
            ["Switch to Desktop 5", "Back", "Close"]
        );
    }

    #[test]
    fn the_button_and_hints_sit_above_the_cards_and_each_picture_under_its_title() {
        let desktops = desktops();
        let group = group(&["d0", "d2"]);
        let snapshot = ArrangeSnapshot {
            plugin: "layout_keeper".to_string(),
            look_plugin: "window_deck".to_string(),
            groups: vec![group.clone()],
            desktops,
            focus: 0,
            watched: Vec::new(),
            preview: true,
            can_rename: false,
            monitors: Vec::new(),
            view: crate::core::ui_bridge::ArrangeView::ByMonitor,
            anchor: None,
        };
        let layout = Layout::new(&group, &snapshot, &[true; 2]);
        assert!(layout.more());
        for expanded in [false, true] {
            let cards = layout.cards_top(expanded);
            assert!(layout.button_top() >= HEADER);
            assert!(layout.hints_top() >= layout.button_top() + BUTTON);
            assert!(cards >= layout.hints_top() + HINTS);
            let (title, thumb) = card_rows(cards);
            assert!(
                thumb >= title + KEYCAP,
                "the picture is under the title row"
            );
            // The picture is the last thing in the card, so the pictures of
            // all cards end on one line at the bottom of the popup.
            assert_eq!(
                thumb + layout.thumb + CARD_PAD,
                cards + layout.card_height()
            );
            assert_eq!(
                layout.size(expanded).y,
                PAD + cards + layout.card_height() + PAD
            );
        }
    }

    #[test]
    fn the_popup_grows_to_the_right_and_keeps_its_height() {
        let desktops = desktops();
        let group = group(&["d0", "d2", "d2", "d1"]);
        let snapshot = ArrangeSnapshot {
            plugin: "layout_keeper".to_string(),
            look_plugin: "window_deck".to_string(),
            groups: vec![group.clone()],
            desktops,
            focus: 0,
            watched: Vec::new(),
            preview: true,
            can_rename: false,
            monitors: Vec::new(),
            view: crate::core::ui_bridge::ArrangeView::ByMonitor,
            anchor: None,
        };
        let layout = Layout::new(&group, &snapshot, &[true; 4]);
        assert_eq!(layout.here, [0]);
        assert_eq!(layout.order(true), [0, 3, 1, 2]);
        let rest = layout.size(false);
        let open = layout.size(true);
        assert_eq!(rest.x, CARD_WIDTH + 2.0 * PAD);
        // Desktop 2 with one window and Desktop 5 with two.
        let grown = 2.0 * (GAP + GROUP_LEAD) + 3.0 * CARD_WIDTH + GAP;
        assert_eq!(open.x, rest.x + grown);
        // Only the labels over the cards are added, above them.
        assert_eq!(open.y, rest.y + LABEL);
    }
}

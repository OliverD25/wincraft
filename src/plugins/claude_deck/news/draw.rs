//! Drawing the tabs strip under the header, the News tab, and the unseen
//! count in mini mode. Sizes are the design page's (section 13): a 30 px
//! strip, flat items with a thin line between them, 13 px names and 12 px
//! text on 16 px lines.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Color32, CursorIcon, Galley, Painter, Rect, Sense, Shape, Stroke, Ui};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::HostChannel;
use crate::ui::widgets::text;

use super::super::labels::time_label;
use super::super::list;
use super::super::marks;
use super::super::model::Line as ChatLine;
use super::super::open_app::open_chat;
use super::super::rows::Look;
use super::super::servers;
use super::super::window::{empty, DeckUi};
use super::super::MarkStyle;
use super::collect::Item;
use super::view::{self, Head, Line};
use super::Tab;

pub const TABS: f32 = 30.0;
const TAB_PAD: f32 = 10.0;
const TAB_GAP: f32 = 2.0;
const STRIP_PAD: f32 = 6.0;
const TAB_SIZE: f32 = 13.0;
const SMALL: f32 = 12.0;
const BADGE_H: f32 = 16.0;
const BADGE_MIN: f32 = 18.0;
const BADGE_PAD: f32 = 5.0;

const HEAD: f32 = 24.0;
const HEAD_GAP: f32 = 4.0;
const PAD: f32 = 10.0;
const PAD_TOP: f32 = 6.0;
const PAD_BOTTOM: f32 = 7.0;
const LEAD: f32 = 14.0;
const LEAD_GAP: f32 = 7.0;
const L1: f32 = 15.0;
const TEXT_GAP: f32 = 3.0;
const LINE: f32 = 16.0;
const NAME: f32 = 13.0;
const DOT: f32 = 8.0;
const PART_GAP: f32 = 6.0;
const SMALLEST_TAG: f32 = 24.0;
const LINK_GAP: f32 = 4.0;
const ACT_GAP: f32 = 8.0;
const BUTTON_H: f32 = 24.0;
const BUTTON_PAD: f32 = 10.0;
const SWITCH: egui::Vec2 = egui::Vec2::new(24.0, 13.0);
const LIST_BOTTOM: f32 = 6.0;
/// The count's height in mini mode.
pub const MINI_COUNT: f32 = 20.0;

/// Draws the tabs strip and, under it, the tab that is on.
pub(in crate::plugins::claude_deck) fn body(
    ui: &mut Ui,
    rest: Rect,
    lines: &[ChatLine],
    look: &Look,
    state: &mut DeckUi,
    to_host: &HostChannel,
) {
    let strip = Rect::from_min_size(rest.min, vec2(rest.width(), TABS));
    tabs(ui, strip, state, look.tokens);
    let below = Rect::from_min_max(pos2(rest.left(), strip.bottom()), rest.max);
    let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(below));
    match state.news.tab {
        Tab::News => show(&mut list_ui, look, state, to_host),
        Tab::Chats if !servers::draw::has_lines(lines, state) => {
            empty(&mut list_ui, look.tokens, !state.chats.is_empty());
        }
        Tab::Chats => list::show(&mut list_ui, lines, look, state, to_host),
    }
}

fn galley(painter: &Painter, words: &str, font: egui::FontId, colour: Color32) -> Arc<Galley> {
    painter.layout_job(text::job(words, font, colour, None))
}

fn cut_job(mut job: LayoutJob, width: f32, rows: usize) -> LayoutJob {
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        max_rows: rows,
        break_anywhere: rows == 1,
        overflow_character: Some('\u{2026}'),
    };
    job
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

/// The strip: "Chats" and "News" with its count of unseen replies.
fn tabs(ui: &mut Ui, strip: Rect, state: &mut DeckUi, tokens: &Tokens) {
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, tokens.window_bg);
    painter.hline(
        strip.x_range(),
        strip.bottom() - 0.5,
        Stroke::new(1.0, tokens.border),
    );
    let mut x = strip.left() + STRIP_PAD;
    for (tab, name) in [(Tab::Chats, view::CHATS), (Tab::News, view::NEWS)] {
        let on = state.news.tab == tab;
        let (font, colour) = if on {
            (theme::semibold(TAB_SIZE), tokens.text_primary)
        } else {
            (theme::regular(TAB_SIZE), tokens.text_secondary)
        };
        let words = galley(&painter, name, font, colour);
        let count = (tab == Tab::News && state.news.count > 0).then(|| {
            galley(
                &painter,
                &state.news.count.to_string(),
                theme::semibold(SMALL),
                tokens.window_bg,
            )
        });
        let badge_w = count.as_ref().map_or(0.0, |c| {
            (c.size().x + 2.0 * BADGE_PAD).max(BADGE_MIN) + PART_GAP
        });
        let width = TAB_PAD * 2.0 + words.size().x + badge_w;
        let rect = Rect::from_min_size(pos2(x, strip.bottom() - 29.0), vec2(width, 29.0));
        let response = ui
            .interact(rect, ui.id().with(("cd-tab", name)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        if response.hovered() && !on {
            painter.rect_filled(rect, 4.0, tokens.hover_bg);
        }
        let middle = rect.center().y;
        let mut at = rect.left() + TAB_PAD;
        at += put(&painter, at, middle, words) + PART_GAP;
        if let Some(count) = count {
            let w = badge_w - PART_GAP;
            let badge = Rect::from_min_size(pos2(at, middle - BADGE_H / 2.0), vec2(w, BADGE_H));
            painter.rect_filled(badge, BADGE_H / 2.0, tokens.accent);
            let size = count.size();
            painter.galley(badge.center() - size / 2.0, count, Color32::PLACEHOLDER);
        }
        if on {
            let line = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.max);
            painter.rect_filled(line, 0.0, tokens.accent);
        }
        if response.clicked() {
            state.news.tab = tab;
        }
        x += width + TAB_GAP;
    }
}

/// The text of an item, laid out: its first lines when closed, its whole
/// reply cut at `LONG_LINES` when open. With the link's words when cut.
fn text_of(
    painter: &Painter,
    item: &Item,
    open: bool,
    whole: bool,
    width: f32,
    tokens: &Tokens,
) -> (Arc<Galley>, Option<String>) {
    let colour = if item.unseen || open {
        tokens.text_primary
    } else {
        tokens.text_secondary
    };
    if !open {
        let job = text::job(
            &view::short_text(&item.text),
            theme::regular(SMALL),
            colour,
            Some(LINE),
        );
        return (
            painter.layout_job(cut_job(job, width, view::SHORT_LINES)),
            None,
        );
    }
    let job = text::job(item.text.trim(), theme::regular(SMALL), colour, Some(LINE));
    let all = painter.layout_job(cut_job(job.clone(), width, usize::MAX));
    let (rows, link) = view::cut(all.rows.len(), whole);
    if link.is_none() {
        return (all, None);
    }
    (painter.layout_job(cut_job(job, width, rows)), link)
}

fn text_width(width: f32) -> f32 {
    width - PAD * 2.0 - LEAD - LEAD_GAP
}

fn item_height(text: &Galley, open: bool, link: bool) -> f32 {
    let mut height = PAD_TOP + L1 + TEXT_GAP + text.size().y + PAD_BOTTOM;
    if open {
        if link {
            height += LINK_GAP + LINE;
        }
        height += ACT_GAP + BUTTON_H;
    }
    height
}

fn head_height(first: bool) -> f32 {
    if first {
        HEAD
    } else {
        HEAD + HEAD_GAP
    }
}

/// An item's text, laid out, and the words of its "Show all" link.
type Laid = (Arc<Galley>, Option<String>);

/// What a frame of the tab asks for, applied after drawing.
#[derive(Default)]
struct Asks {
    toggle: Option<String>,
    open_chat: Option<Item>,
    whole: Option<String>,
    fold: Option<Head>,
    only_unseen: bool,
    mark_all: bool,
    select: Option<String>,
}

/// One frame of the News tab.
fn show(ui: &mut Ui, look: &Look, state: &mut DeckUi, to_host: &HostChannel) {
    let tokens = look.tokens;
    let news = Arc::clone(&state.news.news);
    let lines = view::lines(&news, state.news.only_unseen, |head| {
        state.news.is_folded(head)
    });
    state.news.picks = view::picks(&lines);
    if lines == [Line::Empty] {
        empty_tab(ui, tokens);
        return;
    }
    let mut asks = Asks::default();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let width = ui.available_width();
            let painter = ui.painter().clone();
            let laid: Vec<(f32, Option<Laid>)> = lines
                .iter()
                .enumerate()
                .map(|(at, line)| match line {
                    Line::Head { .. } => (head_height(at == 0), None),
                    Line::Item(item) => {
                        let open = state.news.open.as_deref() == Some(item.key.as_str());
                        let whole = state.news.whole.contains(&item.key);
                        let (galley, link) =
                            text_of(&painter, item, open, whole, text_width(width), tokens);
                        (
                            item_height(&galley, open, link.is_some()),
                            Some((galley, link)),
                        )
                    }
                    Line::Empty => (0.0, None),
                })
                .collect();
            let total: f32 = laid.iter().map(|(h, _)| h).sum::<f32>() + LIST_BOTTOM;
            let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
            let painter = ui.painter_at(area);
            let mut top = area.top();
            let mut item_before = false;
            for ((line, (height, laid)), at) in lines.iter().zip(&laid).zip(0..) {
                let whole = Rect::from_min_size(pos2(area.left(), top), vec2(width, *height));
                top += height;
                match line {
                    Line::Head {
                        head,
                        count,
                        folded,
                        dot,
                    } => {
                        item_before = false;
                        let rect = Rect::from_min_max(
                            pos2(whole.left(), whole.bottom() - HEAD),
                            whole.max,
                        );
                        let key = head.key();
                        let selected = state.news.selected.as_deref() == Some(key.as_str());
                        if selected && std::mem::take(&mut state.news.scroll_to_selected) {
                            ui.scroll_to_rect(rect, None);
                        }
                        if !ui.is_rect_visible(rect) {
                            continue;
                        }
                        let parts = HeadParts {
                            head: *head,
                            count: *count,
                            folded: *folded,
                            dot: *dot,
                            selected,
                            only_unseen: state.news.only_unseen,
                        };
                        head_line(ui, &painter, rect, &parts, tokens, &mut asks, at);
                    }
                    Line::Item(item) => {
                        if item_before {
                            painter.hline(
                                whole.x_range(),
                                whole.top() + 0.5,
                                Stroke::new(1.0, tokens.hover_bg),
                            );
                        }
                        item_before = true;
                        let selected = state.news.selected.as_deref() == Some(item.key.as_str());
                        if selected && std::mem::take(&mut state.news.scroll_to_selected) {
                            ui.scroll_to_rect(whole, None);
                        }
                        if !ui.is_rect_visible(whole) {
                            continue;
                        }
                        let Some((galley, link)) = laid else {
                            continue;
                        };
                        let open = state.news.open.as_deref() == Some(item.key.as_str());
                        let shown = Shown {
                            open,
                            selected,
                            galley: galley.clone(),
                            link: link.clone(),
                        };
                        item_line(ui, &painter, whole, item, &shown, look, &mut asks);
                    }
                    Line::Empty => {}
                }
            }
        });
    apply(asks, state, to_host, look.now);
}

/// Applies what the clicks asked for.
fn apply(asks: Asks, state: &mut DeckUi, to_host: &HostChannel, now: u64) {
    let news = &mut state.news;
    if let Some(key) = asks.select {
        news.selected = Some(key);
    }
    if let Some(key) = asks.toggle {
        news.toggle(&key, now);
    }
    if let Some(key) = asks.whole {
        news.whole.insert(key);
    }
    if let Some(head) = asks.fold {
        let folded = news.is_folded(head);
        news.fold(head, !folded);
    }
    if asks.only_unseen {
        news.only_unseen = !news.only_unseen;
    }
    if asks.mark_all {
        news.mark_all(now);
    }
    if let Some(item) = asks.open_chat {
        open_item(state, &item, to_host, now);
    }
}

/// Opens an item's chat the way the Chats tab does: its window, the
/// Claude app, or a terminal on its server. The reply counts as seen.
pub fn open_item(state: &mut DeckUi, item: &Item, to_host: &HostChannel, now: u64) {
    state.news.read(&item.key, now);
    match &item.server {
        Some(server) => servers::open_terminal(server, item.folder.as_deref()),
        None => open_chat(item.window, &item.title, to_host),
    }
}

struct HeadParts {
    head: Head,
    count: usize,
    folded: bool,
    dot: bool,
    selected: bool,
    only_unseen: bool,
}

/// A group's line: the fold arrow, the name and count; the line of the
/// last 24 hours also holds "Only unseen" and "Mark all seen".
fn head_line(
    ui: &mut Ui,
    painter: &Painter,
    rect: Rect,
    parts: &HeadParts,
    tokens: &Tokens,
    asks: &mut Asks,
    at: usize,
) {
    let response = ui
        .interact(rect, ui.id().with(("cd-news-head", at)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if parts.selected {
        painter.rect_filled(rect, 0.0, tokens.selection_bg);
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
    let middle = rect.center().y;
    let mut x = rect.left() + PAD;
    chevron(
        painter,
        pos2(x, middle - 5.0),
        parts.folded,
        tokens.text_secondary,
    );
    x += 10.0 + PART_GAP;
    let name = galley(
        painter,
        parts.head.name(),
        theme::semibold(SMALL),
        tokens.text_secondary,
    );
    x += put(painter, x, middle, name) + PART_GAP;
    let count = galley(
        painter,
        &parts.count.to_string(),
        theme::regular(SMALL),
        tokens.text_secondary,
    );
    if parts.head == Head::Recent {
        put(painter, x, middle, count);
        controls(ui, painter, rect, parts.only_unseen, tokens, asks);
    } else {
        let mut right = rect.right() - PAD - count.size().x;
        put(painter, right, middle, count);
        if parts.dot {
            right -= PART_GAP + DOT / 2.0;
            painter.circle_filled(pos2(right, middle), DOT / 2.0, tokens.accent);
        }
    }
    if response.clicked() {
        asks.fold = Some(parts.head);
        asks.select = Some(parts.head.key());
    }
}

/// "Only unseen" with its switch, and "Mark all seen", at the right.
fn controls(
    ui: &mut Ui,
    painter: &Painter,
    rect: Rect,
    only_unseen: bool,
    tokens: &Tokens,
    asks: &mut Asks,
) {
    let middle = rect.center().y;
    let mark = galley(
        painter,
        view::MARK_ALL,
        theme::regular(SMALL),
        tokens.accent,
    );
    let mut right = rect.right() - PAD;
    let mark_rect = Rect::from_min_max(
        pos2(right - mark.size().x, rect.top()),
        pos2(right, rect.bottom()),
    );
    let mark_click = ui
        .interact(mark_rect, ui.id().with("cd-news-mark-all"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    put(painter, mark_rect.left(), middle, mark);
    if mark_click.clicked() {
        asks.mark_all = true;
    }
    right = mark_rect.left() - 8.0;
    let label = galley(
        painter,
        view::ONLY_UNSEEN,
        theme::regular(SMALL),
        tokens.text_secondary,
    );
    let label_left = right - label.size().x;
    let switch = Rect::from_min_size(
        pos2(label_left - PART_GAP - SWITCH.x, middle - SWITCH.y / 2.0),
        SWITCH,
    );
    let switch_rect =
        Rect::from_min_max(pos2(switch.left(), rect.top()), pos2(right, rect.bottom()));
    let switch_click = ui
        .interact(
            switch_rect,
            ui.id().with("cd-news-only-unseen"),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    super::super::header::paint_switch(painter, switch, only_unseen, tokens);
    put(painter, label_left, middle, label);
    if switch_click.clicked() {
        asks.only_unseen = true;
    }
}

/// The fold arrow: pointing down when open, right when folded.
fn chevron(painter: &Painter, at: egui::Pos2, folded: bool, colour: Color32) {
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

struct Shown {
    open: bool,
    selected: bool,
    galley: Arc<Galley>,
    link: Option<String>,
}

/// One item: the lead (a dot while unseen, or a pinned chat's mark), the
/// name, project, server and time, and the reply's text. Open, it shows
/// "Show all" when cut and the "Open the chat" button.
fn item_line(
    ui: &mut Ui,
    painter: &Painter,
    rect: Rect,
    item: &Item,
    shown: &Shown,
    look: &Look,
    asks: &mut Asks,
) {
    let tokens = look.tokens;
    let response = ui.interact(rect, ui.id().with(("cd-news", &item.key)), Sense::click());
    if shown.selected {
        painter.rect_filled(rect, 0.0, tokens.selection_bg);
        painter.rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
    } else if shown.open {
        painter.rect_filled(rect, 0.0, tokens.selection_bg);
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
    let left = rect.left() + PAD;
    let l1_middle = rect.top() + PAD_TOP + L1 / 2.0;
    match item.mark {
        Some(mark) => {
            let at = pos2(left, l1_middle - marks::SIZE / 2.0);
            marks::paint(painter, at, mark, MarkStyle::Shapes, tokens, look.turn);
        }
        None if item.unseen => {
            painter.circle_filled(pos2(left + LEAD / 2.0, l1_middle), DOT / 2.0, tokens.accent);
        }
        None => {}
    }
    let body = left + LEAD + LEAD_GAP;
    first_line(painter, rect, body, item, look);
    let text_top = rect.top() + PAD_TOP + L1 + TEXT_GAP;
    let text_h = shown.galley.size().y;
    painter.galley(
        pos2(body, text_top),
        shown.galley.clone(),
        Color32::PLACEHOLDER,
    );
    let mut y = text_top + text_h;
    if !shown.open {
        if response.double_clicked() {
            asks.open_chat = Some(item.clone());
        } else if response.clicked() {
            asks.toggle = Some(item.key.clone());
            asks.select = Some(item.key.clone());
        }
        if item.unseen && item.mark.is_none() {
            response.on_hover_text(view::NOT_SEEN);
        }
        return;
    }
    if let Some(link) = &shown.link {
        y += LINK_GAP;
        let words = galley(painter, link, theme::regular(SMALL), tokens.accent);
        let link_rect = Rect::from_min_size(pos2(body, y), vec2(words.size().x, LINE));
        put(painter, body, link_rect.center().y, words);
        let click = ui
            .interact(
                link_rect,
                ui.id().with(("cd-news-all", &item.key)),
                Sense::click(),
            )
            .on_hover_cursor(CursorIcon::PointingHand);
        if click.clicked() {
            asks.whole = Some(item.key.clone());
        }
        y += LINE;
    }
    y += ACT_GAP;
    let words = galley(
        painter,
        view::OPEN_CHAT,
        theme::semibold(SMALL),
        tokens.window_bg,
    );
    let button = Rect::from_min_size(
        pos2(body, y),
        vec2(words.size().x + 2.0 * BUTTON_PAD, BUTTON_H),
    );
    let pressed = ui
        .interact(
            button,
            ui.id().with(("cd-news-open", &item.key)),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    let fill = if pressed.hovered() {
        tokens.accent_hover
    } else {
        tokens.accent
    };
    painter.rect_filled(button, 4.0, fill);
    put(painter, body + BUTTON_PAD, button.center().y, words);
    let hint = galley(
        painter,
        view::OPEN_HINT,
        theme::regular(SMALL),
        tokens.text_secondary,
    );
    put(painter, button.right() + 10.0, button.center().y, hint);
    if pressed.clicked() || response.double_clicked() {
        asks.open_chat = Some(item.clone());
    } else if response.clicked() {
        asks.toggle = Some(item.key.clone());
        asks.select = Some(item.key.clone());
    }
}

/// The name (bold while unseen), the project, the server and the time.
fn first_line(painter: &Painter, rect: Rect, left: f32, item: &Item, look: &Look) {
    let tokens = look.tokens;
    let middle = rect.top() + PAD_TOP + L1 / 2.0;
    let time = galley(
        painter,
        &time_label(Some(item.at), look.now),
        theme::regular(SMALL),
        tokens.text_secondary,
    );
    let mut right = rect.right() - PAD - time.size().x;
    put(painter, right, middle, time);
    right -= PART_GAP;
    let server = item.server.as_deref().map(|server| {
        galley(
            painter,
            &servers::group_name(server),
            theme::regular(SMALL),
            tokens.text_secondary,
        )
    });
    let server_w = server
        .as_ref()
        .map_or(0.0, |g| 12.0 + 3.0 + g.size().x + PART_GAP);
    let name_font = if item.unseen {
        theme::semibold(NAME)
    } else {
        theme::regular(NAME)
    };
    let name_job = text::job(&item.title, name_font, tokens.text_primary, None);
    let room = (right - left - server_w).max(0.0);
    let name_natural = painter.layout_job(name_job.clone()).size().x;
    let name_w = name_natural.min(room);
    let mut x =
        left + put(
            painter,
            left,
            middle,
            painter.layout_job(cut_job(name_job, name_w + 1.0, 1)),
        ) + PART_GAP;
    let tag_room = room - name_w - PART_GAP;
    if let Some(project) = item.project.as_deref().filter(|_| tag_room >= SMALLEST_TAG) {
        let job = text::job(project, theme::regular(SMALL), tokens.text_disabled, None);
        x += put(
            painter,
            x,
            middle,
            painter.layout_job(cut_job(job, tag_room, 1)),
        ) + PART_GAP;
    }
    if let Some(server) = server {
        servers::draw::server_icon(painter, pos2(x, middle - 6.0), tokens.text_secondary);
        put(painter, x + 15.0, middle, server);
    }
}

/// The words of an empty tab.
fn empty_tab(ui: &mut Ui, tokens: &Tokens) {
    let mut job = text::job(
        view::EMPTY,
        theme::regular(13.0),
        tokens.text_secondary,
        None,
    );
    job.halign = egui::Align::Center;
    job.wrap.max_width = (ui.max_rect().width() - 36.0).max(0.0);
    let galley = ui.painter().layout_job(job);
    let rect = ui.max_rect();
    ui.painter().galley(
        pos2(rect.center().x, rect.top() + 28.0),
        galley,
        Color32::PLACEHOLDER,
    );
}

/// The count of unseen replies under the Full list button in mini mode.
/// True when it was clicked: the full window then opens on the News tab.
pub fn mini_count(ui: &mut Ui, rect: Rect, count: usize, tokens: &Tokens) -> bool {
    if count == 0 {
        return false;
    }
    let painter = ui.painter_at(rect);
    let words = galley(
        &painter,
        &count.to_string(),
        theme::semibold(SMALL),
        tokens.window_bg,
    );
    let width = (words.size().x + 2.0 * BADGE_PAD).max(BADGE_MIN);
    let badge = Rect::from_center_size(rect.center(), vec2(width, BADGE_H));
    let response = ui
        .interact(badge, ui.id().with("cd-mini-news"), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(view::mini_tip(count));
    painter.rect_filled(badge, BADGE_H / 2.0, tokens.accent);
    let size = words.size();
    painter.galley(badge.center() - size / 2.0, words, Color32::PLACEHOLDER);
    response.clicked()
}

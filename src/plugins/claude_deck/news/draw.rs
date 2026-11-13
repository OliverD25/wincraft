//! Drawing the tabs strip under the header, the News tab, and the unseen
//! count in mini mode. Sizes are the design page's (sections 13 and 14):
//! a 30 px strip, 24 px group lines, and one block per chat drawn by
//! `groups`, with a thin line between the blocks.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Color32, CursorIcon, Galley, Painter, Rect, Sense, Shape, Stroke, Ui};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::HostChannel;
use crate::ui::widgets::text;

use super::super::list;
use super::super::model::Line as ChatLine;
use super::super::open_app::open_chat;
use super::super::rows::Look;
use super::super::servers;
use super::super::window::{empty, DeckUi};
use super::collect::Item;
use super::groups::{self, BlockParts, Shown};
use super::view::{self, ChatFold, Head, Line};
use super::Tab;

pub const TABS: f32 = 30.0;
const TAB_PAD: f32 = 10.0;
const TAB_GAP: f32 = 2.0;
const STRIP_PAD: f32 = 6.0;
const TAB_SIZE: f32 = 13.0;
pub(super) const SMALL: f32 = 12.0;
const BADGE_H: f32 = 16.0;
const BADGE_MIN: f32 = 18.0;
const BADGE_PAD: f32 = 5.0;

const HEAD: f32 = 24.0;
const HEAD_GAP: f32 = 4.0;
pub(super) const PAD: f32 = 10.0;
const LINE: f32 = 16.0;
const DOT: f32 = 8.0;
pub(super) const PART_GAP: f32 = 6.0;
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
    if state.news.tab == Tab::Servers && !state.servers.is_on() {
        state.news.tab = Tab::Chats;
    }
    match state.news.tab {
        Tab::News => show(&mut list_ui, look, state, to_host),
        Tab::Servers => servers::tab::draw::show(&mut list_ui, look, state),
        Tab::Chats if !servers::draw::has_lines(lines, state) => {
            empty(&mut list_ui, look.tokens, !state.chats.is_empty());
        }
        Tab::Chats => list::show(&mut list_ui, lines, look, state, to_host),
    }
}

pub(super) fn galley(
    painter: &Painter,
    words: &str,
    font: egui::FontId,
    colour: Color32,
) -> Arc<Galley> {
    painter.layout_job(text::job(words, font, colour, None))
}

pub(super) fn cut_job(mut job: LayoutJob, width: f32, rows: usize) -> LayoutJob {
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        max_rows: rows,
        break_anywhere: rows == 1,
        overflow_character: Some('\u{2026}'),
    };
    job
}

pub(super) fn put(painter: &Painter, left: f32, middle: f32, galley: Arc<Galley>) -> f32 {
    let size = galley.size();
    painter.galley(
        pos2(left, middle - size.y / 2.0),
        galley,
        Color32::PLACEHOLDER,
    );
    size.x
}

/// The strip: "Chats", "News" with its amber count of unseen replies, and
/// "Servers" with its red count of problems while a server is listed.
fn tabs(ui: &mut Ui, strip: Rect, state: &mut DeckUi, tokens: &Tokens) {
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, tokens.window_bg);
    painter.hline(
        strip.x_range(),
        strip.bottom() - 0.5,
        Stroke::new(1.0, tokens.border),
    );
    let mut x = strip.left() + STRIP_PAD;
    let problems = if state.servers.is_on() {
        servers::tab::view::problems(&state.servers.views())
    } else {
        Vec::new()
    };
    for &tab in super::keys::tabs(state.servers.is_on()) {
        let (name, number, badge_colour) = match tab {
            Tab::Chats => (view::CHATS, 0, tokens.accent),
            Tab::News => (view::NEWS, state.news.count, tokens.accent),
            Tab::Servers => (servers::tab::view::TAB, problems.len(), tokens.danger),
        };
        let on = state.news.tab == tab;
        let (font, colour) = if on {
            (theme::semibold(TAB_SIZE), tokens.text_primary)
        } else {
            (theme::regular(TAB_SIZE), tokens.text_secondary)
        };
        let words = galley(&painter, name, font, colour);
        let count = (number > 0).then(|| {
            galley(
                &painter,
                &number.to_string(),
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
            painter.rect_filled(badge, BADGE_H / 2.0, badge_colour);
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
        if tab == Tab::Servers && !problems.is_empty() {
            response.on_hover_text(servers::tab::view::problems_text(&problems));
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

fn head_height(first: bool) -> f32 {
    if first {
        HEAD
    } else {
        HEAD + HEAD_GAP
    }
}

/// A reply's text, laid out, the words of its "Show all" link, its time.
type Laid = (Arc<Galley>, Option<String>, Arc<Galley>);

/// What a frame of the tab asks for, applied after drawing.
#[derive(Default)]
pub(super) struct Asks {
    pub toggle: Option<String>,
    pub open_chat: Option<Item>,
    /// A block line's chat to open.
    pub open_block: Option<Item>,
    pub whole: Option<String>,
    pub fold: Option<Head>,
    /// A chat and how it is to fold.
    pub fold_chat: Option<(String, ChatFold)>,
    pub only_unseen: bool,
    pub mark_all: bool,
    pub select: Option<String>,
}

/// Whether the line at `at` is the last of a chat's block.
fn ends_block(lines: &[Line], at: usize) -> bool {
    matches!(
        lines[at],
        Line::Block { .. } | Line::Item(_) | Line::Earlier { .. }
    ) && !matches!(
        lines.get(at + 1),
        Some(Line::Item(_) | Line::Earlier { .. })
    )
}

/// One frame of the News tab.
fn show(ui: &mut Ui, look: &Look, state: &mut DeckUi, to_host: &HostChannel) {
    let tokens = look.tokens;
    let news = Arc::clone(&state.news.news);
    let lines = view::lines(&news, &state.news);
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
            let laid: Vec<(f32, f32, Option<Laid>)> = lines
                .iter()
                .enumerate()
                .map(|(at, line)| {
                    let bottom = if ends_block(&lines, at) {
                        groups::BLOCK_BOTTOM
                    } else {
                        0.0
                    };
                    match line {
                        Line::Head { .. } => (head_height(at == 0), 0.0, None),
                        Line::Block { .. } => (
                            groups::BLOCK_TOP + groups::BLOCK_LINE + bottom,
                            bottom,
                            None,
                        ),
                        Line::Item(item) => {
                            let open = state.news.open.as_deref() == Some(item.key.as_str());
                            let whole = state.news.whole.contains(&item.key);
                            let time = groups::time(&painter, item, look);
                            let room = groups::text_width(width, &time);
                            let (galley, link) = text_of(&painter, item, open, whole, room, tokens);
                            let height = groups::reply_height(&galley, open, link.is_some());
                            (height + bottom, bottom, Some((galley, link, time)))
                        }
                        Line::Earlier { .. } => (groups::EARLIER + bottom, bottom, None),
                        Line::Empty => (0.0, 0.0, None),
                    }
                })
                .collect();
            let total: f32 = laid.iter().map(|(h, _, _)| h).sum::<f32>() + LIST_BOTTOM;
            let (area, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
            let painter = ui.painter_at(area);
            let mut top = area.top();
            for ((line, (height, bottom, laid)), at) in lines.iter().zip(&laid).zip(0..) {
                let whole = Rect::from_min_size(pos2(area.left(), top), vec2(width, *height));
                let rect =
                    Rect::from_min_max(whole.min, pos2(whole.right(), whole.bottom() - bottom));
                top += height;
                let key = match line {
                    Line::Head { head, .. } => Some(head.key()),
                    Line::Block { head, item, .. } => Some(view::block_key(*head, &item.chat)),
                    Line::Item(item) => Some(item.key.clone()),
                    Line::Earlier { .. } | Line::Empty => None,
                };
                let selected = key.is_some() && state.news.selected == key;
                if selected && std::mem::take(&mut state.news.scroll_to_selected) {
                    ui.scroll_to_rect(rect, None);
                }
                if !ui.is_rect_visible(whole) {
                    continue;
                }
                match line {
                    Line::Head {
                        head,
                        count,
                        folded,
                        dot,
                    } => {
                        let rect = Rect::from_min_max(
                            pos2(whole.left(), whole.bottom() - HEAD),
                            whole.max,
                        );
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
                    Line::Block {
                        head,
                        item,
                        unseen,
                        open,
                        fold,
                    } => {
                        let parts = BlockParts {
                            head: *head,
                            fold: *fold,
                            item,
                            unseen: *unseen,
                            open: *open,
                            selected,
                            after_block: at > 0 && !matches!(lines[at - 1], Line::Head { .. }),
                        };
                        groups::block_line(ui, &painter, rect, &parts, look, &mut asks);
                    }
                    Line::Item(item) => {
                        let Some((galley, link, time)) = laid else {
                            continue;
                        };
                        let shown = Shown {
                            open: state.news.open.as_deref() == Some(item.key.as_str()),
                            selected,
                            galley: galley.clone(),
                            link: link.clone(),
                            time: time.clone(),
                        };
                        groups::reply(ui, &painter, rect, item, &shown, look, &mut asks);
                    }
                    Line::Earlier { chat, count } => {
                        groups::earlier(ui, &painter, rect, chat, *count, tokens, &mut asks);
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
    if let Some((chat, folded)) = asks.fold_chat {
        news.fold_chat(&chat, folded);
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
    if let Some(item) = asks.open_block {
        open_block(state, &item, to_host, now);
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

/// Opens a block line's chat; every reply of the chat counts as seen,
/// since the user goes to the chat itself.
pub fn open_block(state: &mut DeckUi, item: &Item, to_host: &HostChannel, now: u64) {
    state.news.read_chat(&item.chat, now);
    open_item(state, item, to_host, now);
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
pub(in crate::plugins::claude_deck) fn chevron(
    painter: &Painter,
    at: egui::Pos2,
    folded: bool,
    colour: Color32,
) {
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

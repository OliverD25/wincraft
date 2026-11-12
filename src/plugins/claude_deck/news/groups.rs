//! Drawing one chat's block on the News tab (design section 14, option
//! B): the block line with the fold arrow, the status mark, the name,
//! the folder, the server and the unseen count; under it the replies, one
//! or two lines each with the time at the right; and the amber "N earlier
//! replies" line of a folded chat.

use std::sync::Arc;

use egui::{pos2, vec2, Color32, CursorIcon, Galley, Painter, Rect, Sense, Stroke, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::text;

use super::super::labels::time_label;
use super::super::marks;
use super::super::rows::Look;
use super::super::servers;
use super::super::MarkStyle;
use super::collect::Item;
use super::draw::{chevron, cut_job, galley, put, Asks, PAD, PART_GAP, SMALL};
use super::view::{self, Head};

/// Space above a block line and under a block's last line.
pub const BLOCK_TOP: f32 = 5.0;
pub const BLOCK_BOTTOM: f32 = 6.0;
pub const BLOCK_LINE: f32 = 24.0;
/// The "earlier replies" line.
pub const EARLIER: f32 = 2.0 + LINE;
const LINE: f32 = 16.0;
const NAME: f32 = 13.0;
const GAP: f32 = 7.0;
const ARROW: f32 = 10.0;
const DOT: f32 = 8.0;
const TAG_MAX: f32 = 130.0;
const SMALLEST_TAG: f32 = 24.0;
const PILL_H: f32 = 16.0;
const PILL_MIN: f32 = 18.0;
const PILL_PAD: f32 = 5.0;
const REPLY_PAD: f32 = 3.0;
/// A reply's dot sits under the block line's status mark, and its text
/// under the chat's name.
const REPLY_LEFT: f32 = PAD + ARROW + GAP;
const REPLY_TEXT: f32 = REPLY_LEFT + DOT + GAP;
const TIME_GAP: f32 = 6.0;
const LINK_GAP: f32 = 4.0;
const ACT_GAP: f32 = 8.0;
const BUTTON_H: f32 = 24.0;
const BUTTON_PAD: f32 = 10.0;

/// A reply's time, laid out.
pub fn time(painter: &Painter, item: &Item, look: &Look) -> Arc<Galley> {
    galley(
        painter,
        &time_label(Some(item.at), look.now),
        theme::regular(SMALL),
        look.tokens.text_secondary,
    )
}

/// The room for a reply's text in a list `width` wide.
pub fn text_width(width: f32, time: &Galley) -> f32 {
    width - REPLY_TEXT - TIME_GAP - time.size().x - PAD
}

pub fn reply_height(text: &Galley, open: bool, link: bool) -> f32 {
    let mut height = REPLY_PAD + text.size().y + REPLY_PAD;
    if open {
        if link {
            height += LINK_GAP + LINE;
        }
        height += ACT_GAP + BUTTON_H + REPLY_PAD;
    }
    height
}

/// A block line as the lines give it.
pub struct BlockParts<'a> {
    pub head: Head,
    pub item: &'a Item,
    pub unseen: usize,
    pub open: bool,
    pub selected: bool,
    /// Another block is just above, so a thin line parts them.
    pub after_block: bool,
}

/// A chat's block line. A click on the arrow folds or unfolds the chat;
/// a click anywhere else opens the chat.
pub fn block_line(
    ui: &mut Ui,
    painter: &Painter,
    whole: Rect,
    parts: &BlockParts,
    look: &Look,
    asks: &mut Asks,
) {
    let tokens = look.tokens;
    if parts.after_block {
        painter.hline(
            whole.x_range(),
            whole.top() + 0.5,
            Stroke::new(1.0, tokens.hover_bg),
        );
    }
    let rect = Rect::from_min_size(
        pos2(whole.left(), whole.top() + BLOCK_TOP),
        vec2(whole.width(), BLOCK_LINE),
    );
    let item = parts.item;
    let key = view::block_key(parts.head, &item.chat);
    let response = ui
        .interact(rect, ui.id().with(("cd-news-block", &key)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let arrow_rect = Rect::from_min_max(
        rect.min,
        pos2(rect.left() + PAD + ARROW + GAP / 2.0, rect.bottom()),
    );
    let arrow = ui
        .interact(
            arrow_rect,
            ui.id().with(("cd-news-arrow", &key)),
            Sense::click(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    if parts.selected {
        painter.rect_filled(rect, 4.0, tokens.selection_bg);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() || arrow.hovered() {
        painter.rect_filled(rect, 0.0, tokens.hover_bg);
    }
    let middle = rect.center().y;
    let mut x = rect.left() + PAD;
    chevron(
        painter,
        pos2(x, middle - ARROW / 2.0),
        !parts.open,
        tokens.text_secondary,
    );
    x += ARROW + GAP;
    let mark_at = pos2(x, middle - marks::SIZE / 2.0);
    marks::paint(
        painter,
        mark_at,
        item.status,
        MarkStyle::Shapes,
        tokens,
        look.turn,
    );
    x += marks::SIZE + GAP;
    let mut right = rect.right() - PAD;
    if parts.unseen > 0 {
        right -= pill(painter, right, middle, parts.unseen, tokens) + GAP;
    }
    names(painter, x, right, middle, item, tokens);
    if arrow.clicked() {
        asks.fold_chat = Some((item.chat.clone(), parts.open));
        asks.select = Some(key);
    } else if response.clicked() {
        asks.open_block = Some(item.clone());
        asks.select = Some(key);
    }
}

/// The amber count of unseen replies, its right edge at `right`. Returns
/// its width.
fn pill(painter: &Painter, right: f32, middle: f32, count: usize, tokens: &Tokens) -> f32 {
    let words = galley(
        painter,
        &count.to_string(),
        theme::semibold(SMALL),
        tokens.window_bg,
    );
    let width = (words.size().x + 2.0 * PILL_PAD).max(PILL_MIN);
    let rect = Rect::from_min_size(
        pos2(right - width, middle - PILL_H / 2.0),
        vec2(width, PILL_H),
    );
    painter.rect_filled(rect, PILL_H / 2.0, tokens.accent);
    let size = words.size();
    painter.galley(rect.center() - size / 2.0, words, Color32::PLACEHOLDER);
    width
}

/// The chat's name in bold, its folder and its server, from `left` to no
/// further than `right`. The name keeps its room first.
fn names(painter: &Painter, left: f32, right: f32, middle: f32, item: &Item, tokens: &Tokens) {
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
    let name_job = text::job(
        &item.title,
        theme::semibold(NAME),
        tokens.text_primary,
        None,
    );
    let room = (right - left - server_w).max(0.0);
    let name_w = painter.layout_job(name_job.clone()).size().x.min(room);
    let mut x =
        left + put(
            painter,
            left,
            middle,
            painter.layout_job(cut_job(name_job, name_w + 1.0, 1)),
        ) + PART_GAP;
    let tag_room = (room - name_w - PART_GAP).min(TAG_MAX);
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

/// A reply as it is drawn this frame.
pub struct Shown {
    pub open: bool,
    pub selected: bool,
    pub galley: Arc<Galley>,
    pub link: Option<String>,
    pub time: Arc<Galley>,
}

/// One reply: the dot while unseen, one or two lines of text, the time.
/// Open, the whole text, "Show all" when cut and "Open the chat".
pub fn reply(
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
    let top = rect.top() + REPLY_PAD;
    let first = top + LINE / 2.0;
    let is_reply = item.mark.is_none();
    if item.unseen && is_reply {
        let at = pos2(rect.left() + REPLY_LEFT + DOT / 2.0, first);
        painter.circle_filled(at, DOT / 2.0, tokens.accent);
    }
    let body = rect.left() + REPLY_TEXT;
    painter.galley(pos2(body, top), shown.galley.clone(), Color32::PLACEHOLDER);
    let time_w = shown.time.size().x;
    put(
        painter,
        rect.right() - PAD - time_w,
        first,
        shown.time.clone(),
    );
    let mut y = top + shown.galley.size().y;
    if !shown.open {
        if response.double_clicked() {
            asks.open_chat = Some(item.clone());
        } else if response.clicked() {
            asks.toggle = Some(item.key.clone());
            asks.select = Some(item.key.clone());
        }
        if item.unseen && is_reply {
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

/// "N earlier replies" in amber, under the text of the replies; a click
/// unfolds the chat.
pub fn earlier(
    ui: &mut Ui,
    painter: &Painter,
    rect: Rect,
    chat: &str,
    count: usize,
    tokens: &Tokens,
    asks: &mut Asks,
) {
    let words = galley(
        painter,
        &view::earlier(count),
        theme::regular(SMALL),
        tokens.accent,
    );
    let middle = rect.bottom() - LINE / 2.0;
    let left = rect.left() + REPLY_TEXT;
    let hit = Rect::from_min_size(pos2(left, rect.bottom() - LINE), vec2(words.size().x, LINE));
    put(painter, left, middle, words);
    let click = ui
        .interact(hit, ui.id().with(("cd-news-earlier", chat)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if click.clicked() {
        asks.fold_chat = Some((chat.to_string(), false));
    }
}

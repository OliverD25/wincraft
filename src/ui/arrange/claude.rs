//! Custom WindowDeck, release 1 (Classic): the cards of the Claude desktop
//! app show what each chat is doing (a status chip on the picture) and how
//! old it is (the age at the end of the title line, and a picture that fades
//! with age). The toolbar counts the chats by status; a counter, or S,
//! lights up the matching cards in place, and B opens the Board, a panel
//! that lists the same chats by status. Nothing here moves a card: the
//! cards stay in the user's order (Principle 1). The host fills in each
//! window's `claude` card; other apps are not touched.

use super::*;
use crate::core::claude_sessions::{self, Age, Card, Status};

mod board;
mod paint;
pub(super) use board::*;
pub(super) use paint::*;

/// A lit status: the counters and S step through them in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Waiting,
    Working,
    Idle,
    /// Idle for 1 day or more.
    Cold,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::Waiting, Filter::Working, Filter::Idle, Filter::Cold];

    pub fn label(self) -> &'static str {
        match self {
            Filter::Waiting => "Waiting",
            Filter::Working => "Working",
            Filter::Idle => "Idle",
            Filter::Cold => "Cold",
        }
    }

    /// What S lights after `current`: Waiting, Working, Idle, Cold, then
    /// nothing, and round again.
    pub fn next(current: Option<Filter>) -> Option<Filter> {
        match current {
            None => Some(Filter::Waiting),
            Some(Filter::Waiting) => Some(Filter::Working),
            Some(Filter::Working) => Some(Filter::Idle),
            Some(Filter::Idle) => Some(Filter::Cold),
            Some(Filter::Cold) => None,
        }
    }

    /// Whether a card belongs here. Idle counts every idle chat, Cold only
    /// those idle for a day or more, as the counters show them.
    pub fn matches(self, card: &Card, now: u64) -> bool {
        match self {
            Filter::Waiting => card.status == Some(Status::Waiting),
            Filter::Working => card.status == Some(Status::Working),
            Filter::Idle => card.status == Some(Status::Idle),
            Filter::Cold => {
                card.status == Some(Status::Idle) && age(card, now).is_some_and(Age::is_cold)
            }
        }
    }
}

/// How old the card's chat is, when its last activity is known.
pub fn age(card: &Card, now: u64) -> Option<Age> {
    card.last_activity.map(|at| Age::of(now.saturating_sub(at)))
}

/// How many cards each counter shows, in `Filter::ALL` order.
pub fn counts<'a>(cards: impl Iterator<Item = &'a Card>, now: u64) -> [usize; 4] {
    let mut counts = [0; 4];
    for card in cards {
        for (count, filter) in counts.iter_mut().zip(Filter::ALL) {
            if filter.matches(card, now) {
                *count += 1;
            }
        }
    }
    counts
}

/// The chip's words: "Waiting 4 min", "Working 40 s", "Idle". None for a
/// card without a status.
pub fn chip_text(card: &Card, now: u64) -> Option<String> {
    let status = card.status?;
    let lasted = card
        .since
        .map(|since| claude_sessions::duration_text(now.saturating_sub(since)));
    Some(match (status, lasted) {
        (Status::Waiting, Some(time)) => format!("Waiting {time}"),
        (Status::Waiting, None) => "Waiting".to_string(),
        (Status::Working, Some(time)) => format!("Working {time}"),
        (Status::Working, None) => "Working".to_string(),
        (Status::Idle, _) => "Idle".to_string(),
    })
}

/// The arrow keys while a status is lit: the next (or previous) lit card in
/// the order the cards are on screen, skipping the others. From a card that
/// is not lit, the first lit one after it (or before it). None when no card
/// is lit or the end is reached.
pub fn step_lit(
    order: &[isize],
    lit: &dyn Fn(isize) -> bool,
    cursor: Option<isize>,
    forward: bool,
) -> Option<isize> {
    let at = cursor.and_then(|hwnd| order.iter().position(|card| *card == hwnd));
    let mut candidates: Box<dyn Iterator<Item = &isize>> = match (at, forward) {
        (Some(at), true) => Box::new(order[at + 1..].iter()),
        (Some(at), false) => Box::new(order[..at].iter().rev()),
        (None, true) => Box::new(order.iter()),
        (None, false) => Box::new(order.iter().rev()),
    };
    candidates.find(|hwnd| lit(**hwnd)).copied()
}

/// Custom WindowDeck's part of the strip's state.
#[derive(Debug, Default)]
pub(super) struct Deck {
    /// The status whose cards stay bright; the others dim.
    pub(super) highlight: Option<Filter>,
    /// The Board is open, with its selected (column, row).
    pub(super) board: Option<(usize, usize)>,
    /// The cards in their order on screen last frame, for the arrows while
    /// a status is lit.
    pub(super) order: Vec<isize>,
    /// When fresh data was last asked for.
    pub(super) asked: Option<Instant>,
}

impl Deck {
    /// Esc belongs to the Board or the light first, and closes WindowDeck
    /// only after them.
    pub(super) fn takes_escape(&self) -> bool {
        self.board.is_some() || self.highlight.is_some()
    }

    pub(super) fn escape(&mut self) {
        if self.board.is_some() {
            self.board = None;
        } else {
            self.highlight = None;
        }
    }
}

/// How the fresh data is asked for while Claude cards are on screen.
const ASK_EVERY: Duration = Duration::from_millis(1500);

/// How one Claude card is drawn this frame.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Mark {
    pub(super) status: Option<Status>,
    pub(super) chip: Option<String>,
    pub(super) age: Option<Age>,
    pub(super) age_text: Option<String>,
    /// A status is lit and this card is not one of it: drawn at 30 %.
    pub(super) dimmed: bool,
    /// The working ring turns.
    pub(super) animate: bool,
}

/// The opacity of a card that is not lit while a status is.
pub const DIMMED: f32 = 0.3;

impl Mark {
    pub(super) fn new(card: &Card, now: u64, highlight: Option<Filter>, animate: bool) -> Self {
        let age = age(card, now);
        Mark {
            status: card.status,
            chip: chip_text(card, now),
            age,
            age_text: card
                .last_activity
                .map(|at| claude_sessions::age_text(now.saturating_sub(at))),
            dimmed: highlight.is_some_and(|filter| !filter.matches(card, now)),
            animate,
        }
    }

    /// The whole card's opacity.
    pub(super) fn card_alpha(&self) -> f32 {
        if self.dimmed {
            DIMMED
        } else {
            1.0
        }
    }

    /// The picture's own opacity, by age.
    pub(super) fn picture_alpha(&self) -> f32 {
        self.age.map_or(1.0, Age::picture_opacity)
    }

    /// The cold age gets a light blue tint behind its faded picture.
    pub(super) fn tinted(&self) -> bool {
        self.age == Some(Age::Cold)
    }

    /// Cold and frozen ages are written in blue.
    pub(super) fn age_colour(&self, tokens: &Tokens) -> Color32 {
        if self.age.is_some_and(Age::is_cold) {
            tokens.info
        } else {
            tokens.text_secondary
        }
    }
}

impl Shared {
    /// How the card of `window` is drawn, when it is a Claude card.
    pub(super) fn claude_mark(&self, window: &ArrangeWindow) -> Option<Mark> {
        let card = window.claude.as_ref()?;
        let animate = self.snapshot.as_ref().is_some_and(|s| s.claude_animate);
        Some(Mark::new(
            card,
            claude_sessions::now_ms(),
            self.claude.highlight,
            animate,
        ))
    }
}

/// The rectangle round both the badges and the status chip, for the live
/// picture to keep clear of.
pub(super) fn clear_union(badges: Rect, chip: Rect) -> Rect {
    match (badges.is_positive(), chip.is_positive()) {
        (true, true) => badges.union(chip),
        (false, true) => chip,
        _ => badges,
    }
}

/// Whether the group shows Claude cards.
pub(super) fn has_cards(group: &ArrangeGroup) -> bool {
    group.windows.iter().any(|window| window.claude.is_some())
}

/// Keeps the times and statuses fresh while Claude cards are on screen: a
/// repaint each second (the turning ring asks for more), and fresh data
/// from the host every `ASK_EVERY`.
pub(super) fn tick(ctx: &egui::Context, shared: &mut Shared, group: &ArrangeGroup) {
    let now = Instant::now();
    if shared
        .claude
        .asked
        .is_none_or(|asked| now.duration_since(asked) >= ASK_EVERY)
    {
        shared.claude.asked = Some(now);
        shared.to_host.send(HostRequest::ClaudeRefresh);
    }
    let animate = shared.snapshot.as_ref().is_some_and(|s| s.claude_animate);
    let working = group.windows.iter().any(|w| {
        w.claude
            .as_ref()
            .is_some_and(|c| c.status == Some(Status::Working))
    });
    if animate && working && shared.claude.board.is_none() {
        ctx.request_repaint_after(Duration::from_millis(40));
    } else {
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

/// S, B and the arrows while a status is lit. Runs before the view's own
/// keys, so the arrows it takes never reach them; Ctrl and the arrows still
/// move cards as always.
pub(super) fn keys(ctx: &egui::Context, shared: &mut Shared) {
    let (step, board) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::S),
            i.consume_key(egui::Modifiers::NONE, egui::Key::B),
        )
    });
    if step {
        shared.claude.highlight = Filter::next(shared.claude.highlight);
        log::info!(
            "Custom WindowDeck: S lights {}",
            shared.claude.highlight.map_or("nothing", Filter::label)
        );
        ctx.request_repaint();
    }
    if board {
        shared.claude.board = Some((0, 0));
        shared.hide_peek();
        ctx.request_repaint();
        return;
    }
    let Some(filter) = shared.claude.highlight else {
        return;
    };
    let now = claude_sessions::now_ms();
    let (forward, back) = ctx.input_mut(|i| {
        let mut forward = false;
        let mut back = false;
        for key in [egui::Key::ArrowRight, egui::Key::ArrowDown] {
            forward |= i.consume_key(egui::Modifiers::NONE, key);
        }
        for key in [egui::Key::ArrowLeft, egui::Key::ArrowUp] {
            back |= i.consume_key(egui::Modifiers::NONE, key);
        }
        (forward, back)
    });
    if !(forward || back) {
        return;
    }
    let cards: HashMap<isize, Card> = shared
        .group()
        .map(|group| {
            group
                .windows
                .iter()
                .filter_map(|w| w.claude.clone().map(|card| (w.hwnd, card)))
                .collect()
        })
        .unwrap_or_default();
    let lit = |hwnd: isize| cards.get(&hwnd).is_some_and(|c| filter.matches(c, now));
    if let Some(next) = step_lit(&shared.claude.order, &lit, shared.cursor, forward) {
        shared.cursor = Some(next);
        shared.cursor_ghost = false;
        ctx.request_repaint();
    }
}

/// The toolbar's line while a status is lit, in place of the counts.
pub(super) fn lit_line(filter: Filter) -> String {
    format!(
        "Showing: {} \u{b7} S next \u{b7} Esc clears",
        filter.label()
    )
}

/// What a click on a counter or the Board button does.
pub(super) fn apply_click(shared: &mut Shared, click: Click) {
    match click {
        Click::Counter(filter) => {
            shared.claude.highlight = if shared.claude.highlight == Some(filter) {
                None
            } else {
                Some(filter)
            };
        }
        Click::Board => {
            shared.claude.board = match shared.claude.board {
                Some(_) => None,
                None => Some((0, 0)),
            };
            shared.hide_peek();
        }
    }
}

/// The cards on this desktop for the counters and the Board: (window,
/// taskbar number, title, card), in the cards' order.
pub(super) fn this_desktop<'a>(
    shared: &Shared,
    group: &'a ArrangeGroup,
    desktops: &[ArrangeDesktop],
) -> Vec<(isize, usize, &'a str, &'a Card)> {
    let visible = shared.visible(group);
    let numbers = taskbar_numbers(group, desktops);
    let Some(row) = rows(group, desktops).into_iter().next() else {
        return Vec::new();
    };
    row.members
        .iter()
        .filter(|member| visible.get(**member).copied().unwrap_or(true))
        .filter_map(|member| {
            let window = &group.windows[*member];
            let card = window.claude.as_ref()?;
            Some((window.hwnd, numbers[*member], window.label.as_str(), card))
        })
        .collect()
}

#[cfg(test)]
mod tests;

//! How Quick access cards move: a dragged card follows the pointer while
//! the others slide aside to open the gap it will land in, a change sent to
//! the engine shows at once (the cards stand in their new order until the
//! view catches up), and an unpinned card shrinks and fades out. The
//! timing and easing are WindowDeck's (`ui::slide`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use egui::{Pos2, Rect, Vec2};

use crate::ui::slide::{self, SlideKey, Slides, CLOSE_TIME};

use super::super::matching::SlotView;
use super::super::shared::{Request, View};
use super::grid::card_rect;

/// A change the engine has not shown yet is given up after this long, and
/// the cards slide back to the view's order.
const PENDING_FOR: Duration = Duration::from_millis(1500);

/// A card being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    /// The card's place in the list when the drag started.
    pub from: usize,
    /// Where the pointer holds the card, from its top left.
    pub grab: Vec2,
    /// The place it lands in when it is dropped now.
    pub gap: usize,
}

/// A change sent to the engine: the order the cards stand in until the
/// view shows it.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    keys: Vec<isize>,
    order: Vec<usize>,
    since: Instant,
}

/// An unpinned card, fading out where it was.
#[derive(Clone, Debug)]
pub struct Leaving {
    pub card: SlotView,
    pub rect: Rect,
    pub since: Instant,
}

pub struct Motion {
    pub slides: Slides,
    pub drag: Option<Drag>,
    pending: Option<Pending>,
    pub leaving: Option<Leaving>,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            slides: Slides::following(),
            drag: None,
            pending: None,
            leaving: None,
        }
    }
}

/// The slide key of the card with `key`.
pub fn slide_key(key: isize) -> SlideKey {
    (key, false)
}

/// One key per card that stays with its pin when the pins change places:
/// made from what the pin finds, with a count for pins that are alike.
pub fn card_keys(view: &View) -> Vec<isize> {
    if view.pins.len() != view.slots.len() {
        return (0..view.slots.len() as isize).collect();
    }
    let mut keys: Vec<isize> = Vec::with_capacity(view.pins.len());
    for pin in &view.pins {
        let mut hasher = DefaultHasher::new();
        (&pin.exe, &pin.app_id, &pin.title, &pin.label).hash(&mut hasher);
        let mut key = hasher.finish() as isize;
        while keys.contains(&key) {
            key = key.wrapping_add(1);
        }
        keys.push(key);
    }
    keys
}

/// The order after the card at place `from` moves to place `to`, the
/// others closing up around it, as the engine's move does.
pub fn moved_order(order: &[usize], from: usize, to: usize) -> Vec<usize> {
    let mut order = order.to_vec();
    if from < order.len() {
        let to = to.min(order.len() - 1);
        let card = order.remove(from);
        order.insert(to, card);
    }
    order
}

/// The order after the card at place `slot` is unpinned.
pub fn removed_order(order: &[usize], slot: usize) -> Vec<usize> {
    let mut order = order.to_vec();
    if slot < order.len() {
        order.remove(slot);
    }
    order
}

/// The place a card whose centre is at `centre` lands in: the nearest of
/// the `count` places.
pub fn gap_at(origin: Pos2, centre: Pos2, count: usize) -> usize {
    (0..count)
        .min_by(|a, b| {
            let da = card_rect(origin, *a).center().distance_sq(centre);
            let db = card_rect(origin, *b).center().distance_sq(centre);
            da.total_cmp(&db)
        })
        .unwrap_or(0)
}

impl Motion {
    /// The start of a frame: forget slides of cards that are gone, a change
    /// the view now shows (or that took too long) and a card that faded.
    pub fn begin(&mut self, keys: &[isize], now: Instant) {
        self.slides.next_frame();
        if self.drag.is_some_and(|drag| drag.from >= keys.len()) {
            self.drag = None;
        }
        if self.pending.as_ref().is_some_and(|pending| {
            pending.keys != keys || now.saturating_duration_since(pending.since) >= PENDING_FOR
        }) {
            self.pending = None;
        }
        if self
            .leaving
            .as_ref()
            .is_some_and(|gone| now.saturating_duration_since(gone.since) >= CLOSE_TIME)
        {
            self.leaving = None;
        }
    }

    /// Which card of the view stands at each place: the view's order, with
    /// a change that is on its way, and with the gap of a drag.
    pub fn order(&self, count: usize) -> Vec<usize> {
        let order = self.settled(count);
        match self.drag {
            Some(drag) => moved_order(&order, drag.from, drag.gap),
            None => order,
        }
    }

    fn settled(&self, count: usize) -> Vec<usize> {
        match &self.pending {
            Some(pending) => pending.order.clone(),
            None => (0..count).collect(),
        }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn start(&mut self, from: usize, grab: Vec2) {
        self.drag = Some(Drag {
            from,
            grab,
            gap: from,
        });
    }

    pub fn drag_to(&mut self, gap: usize) {
        if let Some(drag) = self.drag.as_mut() {
            drag.gap = gap;
        }
    }

    /// Esc: the card goes back, and nothing is sent.
    pub fn cancel(&mut self) {
        self.drag = None;
    }

    /// The button came up: the move to send, when the card landed in
    /// another place.
    pub fn release(&mut self, keys: &[isize], now: Instant) -> Option<Request> {
        let drag = self.drag.take()?;
        (drag.gap != drag.from).then(|| {
            self.moved(keys, drag.from, drag.gap, now);
            Request::Move {
                from: drag.from,
                to: drag.gap,
            }
        })
    }

    /// A move was sent: the cards stand in the new order at once.
    pub fn moved(&mut self, keys: &[isize], from: usize, to: usize, now: Instant) {
        let order = moved_order(&self.settled(keys.len()), from, to);
        self.pending = Some(Pending {
            keys: keys.to_vec(),
            order,
            since: now,
        });
    }

    /// A card was unpinned: it fades out at `rect`, and the cards after it
    /// close the gap at once.
    pub fn removed(
        &mut self,
        keys: &[isize],
        slot: usize,
        card: Option<(SlotView, Rect)>,
        now: Instant,
    ) {
        let order = removed_order(&self.settled(keys.len()), slot);
        self.pending = Some(Pending {
            keys: keys.to_vec(),
            order,
            since: now,
        });
        self.leaving = card.map(|(card, rect)| Leaving {
            card,
            rect,
            since: now,
        });
    }

    /// Whether something still moves, so the window keeps drawing.
    pub fn moving(&self) -> bool {
        self.drag.is_some() || self.leaving.is_some() || self.slides.running()
    }
}

impl Leaving {
    /// Where the fading card is drawn and how much of it shows.
    pub fn look(&self, now: Instant) -> (Rect, f32) {
        let (scale, fade) = slide::close_curve(now.saturating_duration_since(self.since));
        (
            Rect::from_center_size(self.rect.center(), self.rect.size() * scale),
            fade,
        )
    }
}

#[cfg(test)]
mod tests;

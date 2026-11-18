//! How Quick access cards move: a dragged card follows the pointer while
//! the others slide aside to open the gap it will land in, a change sent to
//! the engine shows at once (the cards stand at their new numbers until the
//! view catches up), and an unpinned card shrinks and fades out. The
//! timing and easing are WindowDeck's (`ui::slide`). The new numbers come
//! from the same rules the engine uses (`pins::reorder_at`, `pins::put_at`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use egui::{Pos2, Rect, Vec2};

use crate::ui::slide::{self, SlideKey, Slides, CLOSE_TIME};

use super::super::matching::SlotView;
use super::super::pins::{put_at, reorder_at, SLOTS};
use super::super::shared::{Request, View};
use super::grid::card_rect;

/// A change the engine has not shown yet is given up after this long, and
/// the cards slide back to the view's order.
const PENDING_FOR: Duration = Duration::from_millis(1500);

/// A card being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    /// The dragged card, as its index in the view.
    pub card: usize,
    /// Where the pointer holds the card, from its top left.
    pub grab: Vec2,
    /// The place the pointer is over: the card there gives its number.
    pub gap: usize,
}

/// A change sent to the engine: each card's number (None for an unpinned
/// one) until the view shows it.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    keys: Vec<isize>,
    numbers: Vec<Option<usize>>,
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

/// The cards in number order, the unpinned ones left out.
pub fn order_of(numbers: &[Option<usize>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..numbers.len())
        .filter(|&card| numbers[card].is_some())
        .collect();
    order.sort_by_key(|&card| numbers[card]);
    order
}

/// Each card's number after `card` goes to number `to`: as a drag moves
/// it, or with `push` as Put at number does.
pub fn moved_numbers(
    numbers: &[Option<usize>],
    card: usize,
    to: usize,
    push: bool,
) -> Vec<Option<usize>> {
    let mut places: Vec<Option<usize>> = vec![None; SLOTS];
    for (index, number) in numbers.iter().enumerate() {
        if let Some(place) = number.and_then(|number| places.get_mut(number)) {
            *place = Some(index);
        }
    }
    let Some(from) = numbers.get(card).copied().flatten().filter(|&n| n < SLOTS) else {
        return numbers.to_vec();
    };
    if push {
        places[from] = None;
        put_at(&mut places, to, card);
    } else {
        reorder_at(&mut places, from, to);
    }
    let mut moved = vec![None; numbers.len()];
    for (number, index) in places.into_iter().enumerate() {
        if let Some(index) = index {
            moved[index] = Some(number);
        }
    }
    moved
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
        if self.drag.is_some_and(|drag| drag.card >= keys.len()) {
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

    /// Which card of the view stands at each place, for the view's
    /// `numbers`: with a change that is on its way, and with the gap of a
    /// drag.
    pub fn order(&self, numbers: &[usize]) -> Vec<usize> {
        let settled = self.settled(numbers);
        match (self.drag, self.drop_on(&settled)) {
            (Some(drag), Some(to)) => order_of(&moved_numbers(&settled, drag.card, to, false)),
            _ => order_of(&settled),
        }
    }

    fn settled(&self, numbers: &[usize]) -> Vec<Option<usize>> {
        match &self.pending {
            Some(pending) => pending.numbers.clone(),
            None => numbers.iter().copied().map(Some).collect(),
        }
    }

    /// The number of the card the dragged one is over, when that is
    /// another card.
    fn drop_on(&self, settled: &[Option<usize>]) -> Option<usize> {
        let drag = self.drag?;
        let on = *order_of(settled).get(drag.gap)?;
        (on != drag.card).then_some(settled[on]).flatten()
    }

    /// Where `card` stands now, a change on its way counted.
    pub fn place_of(&self, numbers: &[usize], card: usize) -> Option<usize> {
        order_of(&self.settled(numbers))
            .iter()
            .position(|&at| at == card)
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// `card`, standing at `place`, is picked up.
    pub fn start(&mut self, card: usize, place: usize, grab: Vec2) {
        self.drag = Some(Drag {
            card,
            grab,
            gap: place,
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

    /// The button came up: the move to send, when the card was dropped
    /// on another card.
    pub fn release(&mut self, keys: &[isize], numbers: &[usize], now: Instant) -> Option<Request> {
        let to = self.drop_on(&self.settled(numbers));
        let drag = self.drag.take()?;
        Some(self.moved(keys, numbers, (drag.card, to?), false, now))
    }

    /// `card` goes to number `to`, as a drag moves it or with `push` as
    /// Put at number does: the cards stand at their new numbers at once.
    /// The request to send.
    pub fn moved(
        &mut self,
        keys: &[isize],
        numbers: &[usize],
        (card, to): (usize, usize),
        push: bool,
        now: Instant,
    ) -> Request {
        let settled = self.settled(numbers);
        let from = settled.get(card).copied().flatten().unwrap_or(to);
        self.pending = Some(Pending {
            keys: keys.to_vec(),
            numbers: moved_numbers(&settled, card, to, push),
            since: now,
        });
        if push {
            Request::PutAt { from, to }
        } else {
            Request::Move { from, to }
        }
    }

    /// `card` was unpinned: it fades out where it was drawn, and the
    /// others keep their numbers. The request to send; None when the card
    /// has no number.
    pub fn removed(
        &mut self,
        keys: &[isize],
        numbers: &[usize],
        (card, leaving): (usize, Option<(SlotView, Rect)>),
        now: Instant,
    ) -> Option<Request> {
        let mut settled = self.settled(numbers);
        let from = settled.get_mut(card)?.take()?;
        self.pending = Some(Pending {
            keys: keys.to_vec(),
            numbers: settled,
            since: now,
        });
        self.leaving = leaving.map(|(card, rect)| Leaving {
            card,
            rect,
            since: now,
        });
        Some(Request::Remove(from))
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

//! The mouse counts only once it really moves after the strip opened, as in
//! Windows' menus and Alt+Tab: a strip that opens under a resting cursor
//! must not pick, light or peek at the card that happens to be under it.
//! Once it has moved, the card under it is the one selected card, so the
//! keyboard and the mouse never point at two cards at once.

/// A move this far, in pixels at 100 % scale, wakes the mouse.
const MOVE_PX: f32 = 3.0;

/// The threshold in screen pixels at `scale` (egui's pixels per point).
pub fn threshold(scale: f32) -> i32 {
    (MOVE_PX * scale).round().max(1.0) as i32
}

/// Where the mouse was when it last had to rest, and whether it has moved
/// since.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pointer {
    anchor: Option<(i32, i32)>,
    moved: bool,
}

impl Pointer {
    /// The mouse rests at `at` (screen pixels): hover does not count until
    /// it moves from there. For every open of the strip, and when the
    /// keyboard moves the selection away from the card under the mouse.
    pub fn rest(&mut self, at: Option<(i32, i32)>) {
        self.anchor = at;
        self.moved = false;
    }

    /// One look at the mouse; true once it has moved `threshold` pixels or
    /// more from where it rests.
    pub fn see(&mut self, at: Option<(i32, i32)>, threshold: i32) -> bool {
        if self.moved {
            return true;
        }
        match (self.anchor, at) {
            (Some(from), Some(to)) => {
                let far = (to.0 - from.0).abs().max((to.1 - from.1).abs());
                self.moved = far >= threshold;
            }
            (None, Some(to)) => self.anchor = Some(to),
            _ => {}
        }
        self.moved
    }

    pub fn moved(&self) -> bool {
        self.moved
    }
}

/// The selected card: its window, and whether it is the ghost of it.
pub type Selection = (Option<isize>, bool);

/// The one selection after a frame. `before` is the selection when the
/// frame began, `now` after the keys had their turn, `hovered` the card
/// under a mouse that has moved. Returns the selection and whether the
/// mouse must rest again: the keyboard moved the selection off the card
/// under it, and the keyboard wins until the mouse moves once more.
pub fn settle(before: Selection, now: Selection, hovered: Option<Selection>) -> (Selection, bool) {
    let changed = now != before;
    match hovered {
        Some(card) if changed && card != now => (now, true),
        Some(card) => (card, false),
        None => (now, changed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_still_mouse_never_counts() {
        let mut pointer = Pointer::default();
        pointer.rest(Some((500, 900)));
        assert!(!pointer.see(Some((500, 900)), 3));
        assert!(!pointer.see(Some((502, 901)), 3), "below the threshold");
        assert!(!pointer.see(None, 3), "no cursor position");
        assert!(!pointer.moved());
    }

    #[test]
    fn a_move_of_the_threshold_or_more_counts_and_stays_counted() {
        let mut pointer = Pointer::default();
        pointer.rest(Some((500, 900)));
        assert!(pointer.see(Some((500, 897)), 3), "3 px up");
        assert!(pointer.see(Some((500, 900)), 3), "back where it was");
        let mut sideways = Pointer::default();
        sideways.rest(Some((10, 10)));
        assert!(sideways.see(Some((6, 10)), 3));
    }

    #[test]
    fn the_threshold_grows_with_the_scale() {
        assert_eq!(threshold(1.0), 3);
        assert_eq!(threshold(1.5), 5);
        assert_eq!(threshold(2.0), 6);
        assert_eq!(threshold(0.1), 1);
        let mut pointer = Pointer::default();
        pointer.rest(Some((0, 0)));
        assert!(!pointer.see(Some((4, 0)), threshold(2.0)));
        assert!(pointer.see(Some((6, 0)), threshold(2.0)));
    }

    #[test]
    fn every_open_makes_the_mouse_rest_again() {
        let mut pointer = Pointer::default();
        pointer.rest(Some((0, 0)));
        assert!(pointer.see(Some((50, 0)), 3));
        pointer.rest(Some((50, 0)));
        assert!(!pointer.moved());
        assert!(!pointer.see(Some((51, 0)), 3));
        // No position known at the open: the first one seen is the rest.
        pointer.rest(None);
        assert!(!pointer.see(Some((80, 80)), 3));
        assert!(pointer.see(Some((90, 80)), 3));
    }

    #[test]
    fn hover_after_a_move_is_the_selection() {
        let keys = (Some(1), false);
        assert_eq!(
            settle(keys, keys, Some((Some(2), false))),
            ((Some(2), false), false)
        );
        assert_eq!(
            settle(keys, keys, Some((Some(2), true))),
            ((Some(2), true), false),
            "a ghost is picked like a card"
        );
        assert_eq!(settle(keys, keys, None), (keys, false), "nothing hovered");
    }

    #[test]
    fn the_keyboard_after_hover_goes_on_from_the_hovered_card() {
        // The mouse picked card 2; the right arrow then moved to card 3.
        let hovered = (Some(2), false);
        let (selection, rest) = settle(hovered, (Some(3), false), Some(hovered));
        assert_eq!(selection, (Some(3), false));
        assert!(rest, "the mouse must move again to take over");
        // A click or drag that picks the hovered card is no keyboard move.
        assert_eq!(
            settle((Some(1), false), hovered, Some(hovered)),
            (hovered, false)
        );
        // The keyboard moved with no card under the mouse.
        assert_eq!(
            settle((Some(1), false), (Some(3), false), None),
            ((Some(3), false), true)
        );
    }

    #[test]
    fn a_click_picks_without_a_move_first() {
        // The mouse has not moved, so nothing counts as hovered; the click
        // itself picked card 4.
        let (selection, _) = settle((Some(1), false), (Some(4), false), None);
        assert_eq!(selection, (Some(4), false));
    }
}

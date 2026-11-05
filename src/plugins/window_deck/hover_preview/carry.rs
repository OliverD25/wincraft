//! Moving on from one taskbar button to the next while the hover strip is
//! engaged. Windows is then in its quick preview mode and shows the next
//! button's popup at once, but which button that is becomes known only after
//! the mouse rests and UI Automation answers. Until it does, the popup of
//! any button on the same taskbar may stay hidden ("carried"), so it does
//! not flash up for a moment before the strip takes its place.
//!
//! The buttons answered during this visit to the taskbar are also kept, so
//! a button already known to have two or more windows is covered at once,
//! and one known to have a single window is never hidden.

use super::machine::{contains, Button, Look, Point, Rect};

/// Where the hover stands after a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// On an eligible button, or the open strip's button, or in the strip.
    Holds,
    /// On a taskbar, at a point whose button is not known yet.
    Waiting(Point),
    /// Anything else: off the taskbar, a one-window button, done.
    Other,
}

#[derive(Clone, Debug, Default)]
pub struct Carry {
    /// The taskbar of the last hover that held a button.
    home: Option<Rect>,
    active: bool,
    /// A lookup at this point said: no app button here.
    refused: Option<Point>,
    /// The point whose answer the active carry waits for.
    waiting: Option<Point>,
    /// The carry just ended on an answer that leaves the popup to Windows.
    undo: bool,
    /// The taskbar the known buttons are on.
    taskbar: Option<Rect>,
    changes: u64,
    known: Vec<Button>,
}

impl Carry {
    /// `held` says whether the hover held a button, or carried, before
    /// this step.
    pub fn step(&mut self, held: bool, spot: Spot, look: &Look) {
        let waited = self.waiting.take().filter(|_| self.active);
        self.undo = waited.is_some_and(|at| leaves_it_to_windows(look, at));
        if look.taskbar != self.taskbar || look.button_changes != self.changes {
            self.known.clear();
            self.taskbar = look.taskbar;
            self.changes = look.button_changes;
        }
        if let Some(button) = look.answer.as_ref().and_then(|a| a.button.as_ref()) {
            self.remember(button);
        }
        let at = match spot {
            Spot::Holds => {
                self.home = look.taskbar;
                self.active = false;
                self.refused = None;
                return;
            }
            Spot::Waiting(at) => at,
            Spot::Other => {
                self.active = false;
                return;
            }
        };
        let no_button = look.answer.as_ref().is_some_and(|answer| {
            answer.at == at
                && !answer
                    .button
                    .as_ref()
                    .is_some_and(|button| contains(button.rect, look.cursor))
        });
        if no_button {
            self.refused = Some(at);
        }
        self.active =
            held && look.taskbar.is_some() && look.taskbar == self.home && self.refused != Some(at);
        self.waiting = self.active.then_some(at);
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// True once, right after the step in which the answer for the carried
    /// point said one window or no app button: a popup the carry hid
    /// belongs to Windows again.
    pub fn take_undo(&mut self) -> bool {
        std::mem::take(&mut self.undo)
    }

    /// The taskbar on which any popup may be hidden now.
    pub fn area(&self) -> Option<Rect> {
        self.home.filter(|_| self.active)
    }

    /// The known buttons with two or more windows, or with fewer.
    pub fn known(&self, eligible: bool) -> Vec<Rect> {
        self.known
            .iter()
            .filter(|button| button.eligible() == eligible)
            .map(|button| button.rect)
            .collect()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// A newer answer replaces any known button it overlaps: buttons move
    /// when a window comes or goes.
    fn remember(&mut self, button: &Button) {
        self.known.retain(|known| !overlap(known.rect, button.rect));
        self.known.push(button.clone());
    }
}

/// The answer for `at` came, and it is not an eligible button under the
/// mouse.
fn leaves_it_to_windows(look: &Look, at: Point) -> bool {
    look.answer.as_ref().is_some_and(|answer| {
        answer.at == at
            && !answer
                .button
                .as_ref()
                .is_some_and(|button| button.eligible() && contains(button.rect, look.cursor))
    })
}

/// Whether the popups the carry hid are shown again: only when the carry
/// ended on such an answer, it hid something, the mouse is still on the
/// taskbar, and this WinCraft may hide (and so undo) at all.
pub fn reshow(undo: bool, hid: bool, on_taskbar: bool, may_hide: bool) -> bool {
    undo && hid && on_taskbar && may_hide
}

fn overlap(a: Rect, b: Rect) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

#[cfg(test)]
mod tests {
    use super::super::machine::Answer;
    use super::*;

    const TASKBAR: Rect = [0, 2040, 3840, 2088];

    fn button(x: i32, windows: usize) -> Button {
        Button {
            app: format!("app{x}"),
            rect: [x, 2040, x + 48, 2088],
            windows,
        }
    }

    fn look(cursor: Point, answer: Option<Answer>) -> Look {
        Look {
            cursor,
            on_taskbar: true,
            taskbar: Some(TASKBAR),
            answer,
            ..Look::default()
        }
    }

    #[test]
    fn a_newer_answer_replaces_the_buttons_it_overlaps() {
        let mut carry = Carry::default();
        let first = Answer {
            at: (10, 2050),
            button: Some(button(0, 2)),
        };
        carry.step(false, Spot::Other, &look((10, 2050), Some(first)));
        let moved = Answer {
            at: (30, 2050),
            button: Some(button(24, 1)),
        };
        carry.step(false, Spot::Other, &look((30, 2050), Some(moved)));
        assert!(carry.known(true).is_empty());
        assert_eq!(carry.known(false), [[24, 2040, 72, 2088]]);
    }

    #[test]
    fn known_buttons_are_forgotten_off_the_taskbar_and_after_a_change() {
        let mut carry = Carry::default();
        let answer = Answer {
            at: (10, 2050),
            button: Some(button(0, 2)),
        };
        carry.step(false, Spot::Other, &look((10, 2050), Some(answer.clone())));
        assert_eq!(carry.known(true).len(), 1);
        let changed = Look {
            button_changes: 1,
            ..look((10, 2050), None)
        };
        carry.step(false, Spot::Other, &changed);
        assert!(carry.known(true).is_empty(), "a window came or went");

        carry.step(false, Spot::Other, &look((10, 2050), Some(answer)));
        carry.step(false, Spot::Other, &Look::default());
        assert!(carry.known(true).is_empty(), "the mouse left the taskbar");
    }

    fn carried_to(carry: &mut Carry, at: Point) {
        carry.step(true, Spot::Holds, &look((10, 2050), None));
        carry.step(true, Spot::Waiting(at), &look(at, None));
        assert!(carry.active());
        assert!(!carry.take_undo());
    }

    #[test]
    fn an_answer_of_one_window_or_no_button_ends_the_carry_with_an_undo() {
        let at = (200, 2050);
        for button in [Some(button(190, 1)), None] {
            let mut carry = Carry::default();
            carried_to(&mut carry, at);
            let answer = Answer { at, button };
            let spot = if answer.button.is_some() {
                Spot::Other
            } else {
                Spot::Waiting(at)
            };
            carry.step(true, spot, &look(at, Some(answer)));
            assert!(!carry.active());
            assert!(carry.take_undo());
            assert!(!carry.take_undo(), "only once");
        }
    }

    #[test]
    fn an_eligible_answer_or_leaving_undoes_nothing() {
        let at = (200, 2050);
        let mut carry = Carry::default();
        carried_to(&mut carry, at);
        let answer = Answer {
            at,
            button: Some(button(190, 2)),
        };
        carry.step(true, Spot::Holds, &look(at, Some(answer)));
        assert!(!carry.take_undo());

        let mut left = Carry::default();
        carried_to(&mut left, at);
        left.step(true, Spot::Other, &Look::default());
        assert!(!left.take_undo());

        let mut never = Carry::default();
        let answer = Answer {
            at,
            button: Some(button(190, 1)),
        };
        never.step(false, Spot::Other, &look(at, Some(answer)));
        assert!(!never.take_undo(), "no carry, nothing to undo");
    }

    #[test]
    fn hidden_popups_are_shown_again_only_when_all_holds() {
        assert!(reshow(true, true, true, true));
        assert!(!reshow(false, true, true, true), "eligible, or no answer");
        assert!(!reshow(true, false, true, true), "nothing was hidden");
        assert!(!reshow(true, true, false, true), "left the taskbar");
        assert!(!reshow(true, true, true, false), "read-only test instance");
    }

    #[test]
    fn carrying_needs_the_same_taskbar() {
        let mut carry = Carry::default();
        carry.step(true, Spot::Holds, &look((10, 2050), None));
        carry.step(true, Spot::Waiting((200, 2050)), &look((200, 2050), None));
        assert_eq!(carry.area(), Some(TASKBAR));
        let other = Look {
            taskbar: Some([3840, 2040, 7680, 2088]),
            ..look((4000, 2050), None)
        };
        carry.step(true, Spot::Waiting((4000, 2050)), &other);
        assert_eq!(carry.area(), None, "another monitor's taskbar");
    }
}

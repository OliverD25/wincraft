//! The strip's window from one opening to the next. It is made at the first
//! opening, hidden (not destroyed) when the strip closes, used again at the
//! next opening, and freed only when WindowDeck is switched off: a new
//! window and GL context took about 200 ms of every opening.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Life {
    /// No window: never opened, or WindowDeck was switched off.
    #[default]
    None,
    Open,
    /// Closed, with its window kept hidden.
    Parked,
}

impl Life {
    pub fn open(&mut self) {
        *self = Life::Open;
    }

    /// The strip closed. True when it was open, so its pictures and window
    /// are to be put away.
    pub fn close(&mut self) -> bool {
        let was_open = self.is_open();
        if was_open {
            *self = Life::Parked;
        }
        was_open
    }

    /// WindowDeck was switched off: the window goes. True when the strip
    /// was open, so it is to be put away first.
    pub fn free(&mut self) -> bool {
        let was_open = self.is_open();
        *self = Life::None;
        was_open
    }

    pub fn is_open(self) -> bool {
        self == Life::Open
    }

    /// Whether egui is to keep the window this frame.
    pub fn keeps_window(self) -> bool {
        self != Life::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_kept_from_the_first_opening_until_switched_off() {
        let mut life = Life::default();
        assert!(!life.keeps_window(), "nothing is made at start");
        assert!(!life.close(), "closing what never opened does nothing");
        life.open();
        assert!(life.is_open() && life.keeps_window());
        assert!(life.close());
        assert_eq!(life, Life::Parked);
        assert!(life.keeps_window(), "hidden, not destroyed");
        assert!(!life.close(), "closed once only");
        life.open();
        assert!(life.is_open(), "the same window again");
        assert!(life.free(), "switched off while open: put away first");
        assert!(!life.keeps_window());
        life.open();
        life.close();
        assert!(!life.free(), "switched off while closed");
        assert_eq!(life, Life::None);
    }
}

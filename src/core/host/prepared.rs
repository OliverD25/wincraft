//! WindowDeck's windows, gathered while the user still holds Win, so the
//! digit that follows opens WindowDeck without waiting for them.
//!
//! The hook tells the host when a Windows key goes down (only while the
//! Win+1…9 switch is on). The host asks the plugin for its window groups
//! then, and a Win+digit that comes within `FRESH` takes them instead of
//! asking again. A window that comes or goes in between throws them away.

use std::time::{Duration, Instant};

/// Gathered windows older than this are asked for again.
pub const FRESH: Duration = Duration::from_millis(1500);

#[derive(Debug)]
pub struct Prepared<T> {
    item: Option<(Instant, String, T)>,
}

impl<T> Default for Prepared<T> {
    fn default() -> Self {
        Self { item: None }
    }
}

impl<T> Prepared<T> {
    /// Whether gathered windows are waiting and still fresh.
    pub fn is_fresh(&self, now: Instant) -> bool {
        self.item
            .as_ref()
            .is_some_and(|(at, _, _)| now.saturating_duration_since(*at) < FRESH)
    }

    /// Keeps what the plugin `look` gave at `now`.
    pub fn put(&mut self, look: &str, item: T, now: Instant) {
        self.item = Some((now, look.to_string(), item));
    }

    /// What was gathered for the plugin `look`, if still fresh; used once.
    pub fn take(&mut self, look: &str, now: Instant) -> Option<T> {
        let fresh = self.is_fresh(now);
        let (_, for_look, item) = self.item.take()?;
        (fresh && for_look == look).then_some(item)
    }

    /// A window came or went: what was gathered is out of date.
    pub fn forget(&mut self) {
        self.item = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gathered_windows_are_used_once_while_fresh() {
        let now = Instant::now();
        let mut prepared = Prepared::default();
        assert!(!prepared.is_fresh(now));
        assert_eq!(prepared.take("deck", now), None::<u8>);
        prepared.put("deck", 7, now);
        assert!(prepared.is_fresh(now));
        assert_eq!(prepared.take("deck", now + FRESH / 2), Some(7));
        assert_eq!(prepared.take("deck", now), None, "once only");
    }

    #[test]
    fn old_or_foreign_or_forgotten_windows_are_not_used() {
        let now = Instant::now();
        let mut prepared = Prepared::default();
        prepared.put("deck", 1, now);
        assert_eq!(prepared.take("deck", now + FRESH), None, "too old");
        prepared.put("deck", 2, now);
        assert_eq!(prepared.take("other", now), None, "another look plugin");
        assert_eq!(prepared.take("deck", now), None, "dropped by that take");
        prepared.put("deck", 3, now);
        prepared.forget();
        assert_eq!(prepared.take("deck", now), None, "a window came or went");
    }
}

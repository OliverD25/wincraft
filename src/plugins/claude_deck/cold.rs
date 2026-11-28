//! The cold time: how long a chat stays idle before it counts as cold. One
//! setting ("Cold after", in hours) for the whole deck: the Chats tab's cold
//! area, the Servers tab, the fading look of the status marks and how long
//! a chat dragged out of the cold area stays in its group.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;

pub const KEY: &str = "cold_after_hours";
pub const LABEL: &str = "Cold after (hours)";
pub const DEFAULT_HOURS: u32 = 48;
pub const MIN_HOURS: u32 = 1;
/// 30 days.
pub const MAX_HOURS: u32 = 720;

const HOUR_MS: u64 = 60 * 60 * 1000;

/// The saved hours, rounded and kept between the smallest and the largest;
/// a missing or broken value is the default.
pub fn read(settings: &Value) -> u32 {
    settings
        .get(KEY)
        .and_then(Value::as_f64)
        .filter(|hours| hours.is_finite())
        .map_or(DEFAULT_HOURS, |hours| {
            (hours.round().max(0.0) as u32).clamp(MIN_HOURS, MAX_HOURS)
        })
}

pub fn ms(hours: u32) -> u64 {
    u64::from(hours) * HOUR_MS
}

/// The cold time in words: whole days when the hours make whole days,
/// else hours ("1 day", "2 days", "36 h").
pub fn span(hours: u32) -> String {
    match hours {
        24 => "1 day".to_string(),
        h if h % 24 == 0 => format!("{} days", h / 24),
        h => format!("{h} h"),
    }
}

/// The quick settings' label of the fold toggle, with the cold time in it.
pub fn fold_label(hours: u32) -> String {
    format!("Fold chats idle for over {}", span(hours))
}

/// The cold time the server threads read on each call, so a change of the
/// setting reaches them without starting them again.
#[derive(Clone, Debug)]
pub struct Shared(Arc<AtomicU64>);

impl Default for Shared {
    fn default() -> Self {
        Shared(Arc::new(AtomicU64::new(ms(DEFAULT_HOURS))))
    }
}

impl Shared {
    pub fn set(&self, cold_ms: u64) {
        self.0.store(cold_ms, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_span_says_days_when_it_can_else_hours() {
        assert_eq!(span(24), "1 day");
        assert_eq!(span(48), "2 days");
        assert_eq!(span(36), "36 h");
        assert_eq!(span(1), "1 h");
        assert_eq!(span(720), "30 days");
        assert_eq!(fold_label(48), "Fold chats idle for over 2 days");
        assert_eq!(fold_label(24), "Fold chats idle for over 1 day");
        assert_eq!(fold_label(36), "Fold chats idle for over 36 h");
    }

    #[test]
    fn a_missing_value_is_48_hours_and_others_are_kept_in_range() {
        assert_eq!(read(&serde_json::json!({})), 48);
        assert_eq!(read(&serde_json::json!({ KEY: 36 })), 36);
        assert_eq!(read(&serde_json::json!({ KEY: 12.4 })), 12);
        assert_eq!(read(&serde_json::json!({ KEY: 0 })), 1);
        assert_eq!(read(&serde_json::json!({ KEY: -5 })), 1);
        assert_eq!(read(&serde_json::json!({ KEY: 5000 })), 720);
        assert_eq!(read(&serde_json::json!({ KEY: "soon" })), 48);
        assert_eq!(ms(48), 48 * 60 * 60 * 1000);
    }

    #[test]
    fn the_shared_time_starts_at_48_hours_and_follows_a_change() {
        let shared = Shared::default();
        let thread_copy = shared.clone();
        assert_eq!(thread_copy.get(), ms(48));
        shared.set(ms(24));
        assert_eq!(thread_copy.get(), ms(24));
    }
}

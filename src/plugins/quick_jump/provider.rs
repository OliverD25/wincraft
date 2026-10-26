//! The palette's rows for the pinned slots: "Jump to 3 · Claude", with the
//! keys that do the same from anywhere.

use std::path::PathBuf;

use crate::core::shortcuts;
use crate::search::{Action, Choice, Context, IconRef, Query, ResultItem, SearchProvider};
use crate::ui::fuzzy;

use super::matching::{SlotView, State};
use super::shared::{Handle, Request};

const ID: &str = "quick_jump";
pub const GROUP: &str = "QuickJump";

pub struct Slots {
    pub shared: Handle,
}

/// What a row of a pinned slot says.
pub fn row(view: &SlotView, jump_keys: &str) -> ResultItem {
    let digit = view.digit;
    let subtitle = match view.state {
        State::NotOpen => "not open".to_string(),
        State::InFront => "in front".to_string(),
        _ if view.place.is_empty() => view.title.clone(),
        _ => view.place.clone(),
    };
    // A single press with the digit after it reads as two steps; a chord
    // with a digit after it would need three, which keycaps cannot show.
    let keys = shortcuts::parse(jump_keys)
        .filter(|keys| !keys.is_chord())
        .map(|_| format!("{jump_keys}, {digit}"))
        .unwrap_or_default();
    let icon = match view.hwnd {
        Some(hwnd) => IconRef::Window {
            hwnd,
            exe: PathBuf::from(&view.exe),
        },
        None => IconRef::Path(PathBuf::from(&view.exe)),
    };
    ResultItem {
        group: GROUP.to_string(),
        title: format!("Jump to {digit} \u{b7} {}", view.name),
        subtitle,
        icon,
        keys,
        enter: Some(Choice {
            label: "Jump".to_string(),
            action: Action::Provider {
                provider: ID.to_string(),
                command: format!("jump:{}", view.slot),
            },
        }),
        usage_key: Some(format!("slot/{}", view.slot)),
        ..Default::default()
    }
}

impl SearchProvider for Slots {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        GROUP
    }

    fn description(&self) -> &'static str {
        "Jump to a pinned window"
    }

    fn blended(&self) -> bool {
        true
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        if query.text.trim().is_empty() {
            return Vec::new();
        }
        let Ok(shared) = self.shared.lock() else {
            return Vec::new();
        };
        let mut rows: Vec<ResultItem> = shared
            .view
            .slots
            .iter()
            .filter(|view| view.is_pinned())
            .filter_map(|view| {
                let mut row = row(view, &shared.view.jump_keys);
                row.score = fuzzy::score_command(&query.text, GROUP, &row.title)?;
                Some(row)
            })
            .collect();
        rows.truncate(query.limit.max(1));
        rows
    }

    fn act(&mut self, command: &str, _context: &Context) -> crate::search::Reply {
        let slot = command
            .strip_prefix("jump:")
            .and_then(|slot| slot.parse::<usize>().ok());
        if let (Some(slot), Ok(mut shared)) = (slot, self.shared.lock()) {
            shared.send(Request::Jump(slot));
        }
        crate::search::Reply::Hide
    }
}

#[cfg(test)]
mod tests {
    use super::super::matching::slot_views;
    use super::super::matching::tests::window;
    use super::super::pins::tests::pin;
    use super::super::pins::Slots as Pins;
    use super::super::shared;
    use super::*;

    fn provider() -> Slots {
        let mut pins = Pins::default();
        pins.pin(2, pin("Claude", None));
        pins.pin(4, pin("Code", None));
        let views = slot_views(&pins, &[window(1, "Claude", "Claude")], 0);
        let shared = shared::new();
        if let Ok(mut state) = shared.lock() {
            state.view.slots = views;
            state.view.jump_keys = "Win+Alt+Q".to_string();
        }
        Slots { shared }
    }

    #[test]
    fn each_pinned_slot_is_a_row_with_its_keys() {
        let mut slots = provider();
        let query = Query {
            text: "jump".to_string(),
            prefix: None,
            limit: 8,
        };
        let rows = slots.query(&query, &crate::search::test_context());
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(titles, ["Jump to 3 \u{b7} Claude", "Jump to 5 \u{b7} Code"]);
        assert_eq!(rows[0].keys, "Win+Alt+Q, 3");
        assert_eq!(rows[1].subtitle, "not open");
        assert!(matches!(rows[0].icon, IconRef::Window { hwnd: 1, .. }));
        let empty = Query {
            text: " ".to_string(),
            prefix: None,
            limit: 8,
        };
        assert!(slots
            .query(&empty, &crate::search::test_context())
            .is_empty());
        assert_eq!(
            slots.act("jump:2", &crate::search::test_context()),
            crate::search::Reply::Hide
        );
    }

    #[test]
    fn a_chord_or_no_shortcut_leaves_the_keys_out() {
        let view = SlotView {
            digit: 3,
            name: "Claude".to_string(),
            state: State::Open,
            ..SlotView::default()
        };
        assert_eq!(row(&view, "Win+Alt+K, J").keys, "");
        assert_eq!(row(&view, "").keys, "");
        assert_eq!(row(&view, "Win+Alt+Q").keys, "Win+Alt+Q, 3");
    }
}

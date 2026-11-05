//! After a jump to a window on another monitor, QuickJump points to it with
//! the host's window hint (`core::window_hint`): a glow around the window
//! and an arrow on the monitor the user was using. This file holds the
//! switch for it and what the hint calls the window.

use serde_json::Value;

use crate::core::monitors::Rect;
use crate::core::window_hint::{self, Before, Hint};

use super::matching::Live;
use super::pins::Pin;

/// The setting's key; on (true) when missing.
pub const KEY: &str = "point_to_other_monitor";

pub fn on(settings: &Value) -> bool {
    settings.get(KEY).and_then(Value::as_bool).unwrap_or(true)
}

/// The pin's label, else the window's title, else the app's name.
pub fn name(pin: &Pin, title: Option<&str>) -> String {
    let label = pin.label.as_deref().map(str::trim).unwrap_or("");
    let title = title.map(str::trim).unwrap_or("");
    [label, title, pin.app_name.trim()]
        .into_iter()
        .find(|text| !text.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Where the user looked: where Quick access was when the jump came from
/// it, else the window that was in front.
pub fn from(front: isize, quick_access: Option<Rect>) -> Before {
    quick_access.map_or(Before::Window(front), Before::Area)
}

/// Points to `hwnd` when it came up on another monitor.
pub fn point(pin: &Pin, live: &[Live], hwnd: isize, from: Before) {
    let title = live
        .iter()
        .find(|window| window.hwnd == hwnd)
        .map(|window| window.shown.as_str());
    window_hint::show(&Hint {
        window: hwnd,
        name: name(pin, title),
        from,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(label: Option<&str>) -> Pin {
        Pin {
            exe: r"C:\Apps\Telegram.exe".to_string(),
            app_id: None,
            app_name: "Telegram".to_string(),
            title: String::new(),
            label: label.map(str::to_string),
            start: true,
            hwnd: None,
        }
    }

    #[test]
    fn the_switch_is_on_by_default_and_off_is_kept() {
        assert!(on(&serde_json::json!({})));
        assert!(on(&serde_json::json!({ KEY: true })));
        assert!(!on(&serde_json::json!({ KEY: false })));
    }

    #[test]
    fn the_name_is_the_label_then_the_title_then_the_app() {
        assert_eq!(name(&pin(Some("Chats")), Some("Saved")), "Chats");
        assert_eq!(
            name(&pin(Some("  ")), Some("Saved messages")),
            "Saved messages"
        );
        assert_eq!(name(&pin(None), Some(" ")), "Telegram");
        assert_eq!(name(&pin(None), None), "Telegram");
    }

    #[test]
    fn a_jump_from_quick_access_looks_where_it_was() {
        assert_eq!(from(0x42, None), Before::Window(0x42));
        let spot = [100, 100, 900, 700];
        assert_eq!(from(0x42, Some(spot)), Before::Area(spot));
    }
}

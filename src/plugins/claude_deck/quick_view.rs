//! The **View** submenu of the quick settings menu: what each chat row
//! shows, with the labels and the order of the Settings page's **View**
//! heading. In the full list it opens to the side, as egui submenus do;
//! the mini mode's menu window is too small for that, so there the items
//! open below the **View** row. A click or `Enter` changes a setting and
//! saves it; the menu stays open. When the keyboard opens the submenu, its
//! first item takes the keyboard. Its last part is the token mark: on or
//! off, its three styles, and what it counts.

use egui::containers::menu::{MenuConfig, SubMenuButton};
use egui::{Id, PopupCloseBehavior, Ui};
use serde_json::Value;

use crate::core::ui_bridge::{HostChannel, HostRequest};

use super::quick::{Pick, Toggle};
use super::token_mark::{self, Counts, Style};
use super::{MarkStyle, Settings, ID};

pub const VIEW: &str = "View";
/// The heading over the three ways to draw the status mark.
const MARK: &str = "Status mark";

/// The status mark's choices, with the Settings page's words.
pub const MARKS: [(&str, MarkStyle); 3] = [
    (super::MARK_SHAPES, MarkStyle::Shapes),
    (super::MARK_WORDS, MarkStyle::ShapesAndWords),
    (super::MARK_OFF, MarkStyle::Off),
];

/// The on/off row settings, in the Settings page's order.
pub const TOGGLES: [Toggle; 6] = [
    Toggle {
        label: "A line of text (the question, or what it does)",
        key: super::TEXT_KEY,
        get: |s| s.text,
        set: |s, on| s.text = on,
    },
    Toggle {
        label: "Time in the status",
        key: super::TIME_KEY,
        get: |s| s.time,
        set: |s, on| s.time = on,
    },
    Toggle {
        label: "Project and branch",
        key: super::PROJECT_KEY,
        get: |s| s.project,
        set: |s, on| s.project = on,
    },
    Toggle {
        label: "Job count",
        key: super::JOBS_KEY,
        get: |s| s.jobs,
        set: |s, on| s.jobs = on,
    },
    Toggle {
        label: "Model name",
        key: super::MODEL_KEY,
        get: |s| s.model,
        set: |s, on| s.model = on,
    },
    Toggle {
        label: "Quick access numbers",
        key: super::QUICK_NUMBERS_KEY,
        get: |s| s.quick_numbers,
        set: |s, on| s.quick_numbers = on,
    },
];

fn focus_id() -> Id {
    Id::new("claude-deck-quick-view-focus")
}

/// The **View** row. `below` is the mini mode's open state: Some draws
/// the items under the row instead of in a submenu.
pub fn entry(ui: &mut Ui, settings: &Settings, below: Option<&mut bool>) -> Option<Pick> {
    let (response, picked) = match below {
        Some(open) => {
            let arrow = if *open { "\u{23f7}" } else { "\u{23f5}" };
            let response = ui.add(egui::Button::new(VIEW).right_text(arrow));
            if response.clicked() {
                *open = !*open;
            }
            let picked = if *open {
                ui.indent("claude-deck-quick-view", |ui| items(ui, settings))
                    .inner
            } else {
                None
            };
            (response, picked)
        }
        None => {
            let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
            let (response, shown) = SubMenuButton::new(VIEW)
                .config(config)
                .ui(ui, |ui| items(ui, settings));
            (response, shown.and_then(|shown| shown.inner))
        }
    };
    // `clicked` also counts Enter and Space; a mouse click leaves the
    // keyboard where it is.
    if response.clicked() && !response.clicked_by(egui::PointerButton::Primary) {
        ui.ctx().data_mut(|data| data.insert_temp(focus_id(), true));
    }
    picked
}

fn items(ui: &mut Ui, settings: &Settings) -> Option<Pick> {
    let focus_first = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<bool>(focus_id()))
        .unwrap_or(false);
    let mut picked = None;
    ui.weak(MARK);
    for (at, (label, style)) in MARKS.iter().enumerate() {
        let response = ui.radio(settings.mark == *style, *label);
        if at == 0 && focus_first {
            response.request_focus();
        }
        if response.clicked() {
            picked = Some(Pick::Mark(*style));
        }
    }
    ui.separator();
    for (at, toggle) in TOGGLES.iter().enumerate() {
        let mut on = (toggle.get)(settings);
        if ui.checkbox(&mut on, toggle.label).clicked() {
            picked = Some(Pick::View(at));
        }
    }
    token_items(ui, settings).or(picked)
}

/// **Token mark** with its keys, the three styles under it, a line, and
/// the two counts, and the note on the counts hotkey when it has keys.
fn token_items(ui: &mut Ui, settings: &Settings) -> Option<Pick> {
    let mut picked = None;
    let mut on = settings.token_mark != Style::Off;
    let label = (
        token_mark::LABEL,
        egui::Atom::grow(),
        egui::RichText::new(token_mark::flip_keys()).weak(),
    );
    if ui.add(egui::Checkbox::new(&mut on, label)).clicked() {
        picked = Some(Pick::TokenFlip);
    }
    ui.indent("claude-deck-token-styles", |ui| {
        for (label, style) in token_mark::STYLES {
            if ui.radio(settings.token_mark == style, label).clicked() {
                picked = Some(Pick::TokenStyle(style));
            }
        }
    });
    ui.separator();
    for (label, counts) in [
        (token_mark::COUNT_TODAY, Counts::Today),
        (token_mark::COUNT_WEEK, Counts::Week),
    ] {
        if ui.radio(settings.token_counts == counts, label).clicked() {
            picked = Some(Pick::TokenCounts(counts));
        }
    }
    if let Some(note) = token_mark::switch_note() {
        ui.weak(note);
    }
    picked
}

/// Changes and saves a View pick. False for a pick of another kind.
pub fn apply(settings: &mut Settings, pick: Pick, to_host: &HostChannel) -> bool {
    let (key, value) = match pick {
        Pick::Mark(style) => {
            settings.mark = style;
            let name = MARKS
                .iter()
                .find(|(_, s)| *s == style)
                .map_or(super::MARK_SHAPES, |(name, _)| *name);
            (super::MARK_KEY, Value::String(name.to_string()))
        }
        Pick::View(at) => {
            let Some(toggle) = TOGGLES.get(at) else {
                return true;
            };
            let on = !(toggle.get)(settings);
            (toggle.set)(settings, on);
            (toggle.key, Value::Bool(on))
        }
        Pick::TokenFlip => return save(token_mark::flip(settings), to_host),
        Pick::TokenStyle(style) => return save(token_mark::set_style(settings, style), to_host),
        Pick::TokenCounts(counts) => {
            return save(token_mark::set_counts(settings, counts), to_host)
        }
        Pick::Flip(_) | Pick::All => return false,
    };
    save(vec![(key, value)], to_host)
}

/// Sends each change to the host, which saves it.
pub fn save(saves: token_mark::Saves, to_host: &HostChannel) -> bool {
    for (key, value) in saves {
        to_host.send(HostRequest::SetSetting {
            plugin: ID.to_string(),
            key: key.to_string(),
            value,
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_items_follow_the_settings_page_in_its_order() {
        use crate::core::traits::WinCraftPlugin;
        let deck = super::super::ClaudeDeck::new();
        let fields = deck.settings_fields();
        let view: Vec<&str> = fields
            .iter()
            .filter(|field| deck.settings_group(field.key) == Some(VIEW))
            .map(|field| field.label)
            .collect();
        let mut menu = vec![MARK];
        menu.extend(TOGGLES.iter().map(|toggle| toggle.label));
        menu.extend([token_mark::LABEL, token_mark::COUNTS_LABEL]);
        assert_eq!(view, menu);
        for toggle in &TOGGLES {
            let field = fields.iter().find(|field| field.key == toggle.key);
            assert_eq!(field.map(|field| field.label), Some(toggle.label));
        }
    }

    #[test]
    fn each_view_item_changes_its_own_setting_and_saves_it() {
        let bridge = crate::core::ui_bridge::create();
        let (to_host, from_ui) = (&bridge.to_host, &bridge.host_rx);
        for (at, toggle) in TOGGLES.iter().enumerate() {
            let mut settings = Settings::default();
            let before = settings.clone();
            assert!(apply(&mut settings, Pick::View(at), to_host));
            assert_ne!(settings, before, "row {at}");
            assert!(matches!(
                from_ui.try_recv(),
                Ok(HostRequest::SetSetting { key, .. }) if key == toggle.key
            ));
            assert!(apply(&mut settings, Pick::View(at), to_host));
            assert_eq!(settings, before, "row {at} touched another setting");
            let _ = from_ui.try_recv();
        }
        let mut settings = Settings::default();
        assert!(apply(&mut settings, Pick::Mark(MarkStyle::Off), to_host));
        assert_eq!(settings.mark, MarkStyle::Off);
        assert!(matches!(
            from_ui.try_recv(),
            Ok(HostRequest::SetSetting { key, value: Value::String(word), .. })
                if key == "status_mark" && word == "Off"
        ));
        assert!(!apply(&mut settings, Pick::All, to_host));
    }
}

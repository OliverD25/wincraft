//! Quick settings: the gear in the header (or `Ctrl+,`) opens a small menu
//! of ClaudeDeck's on/off settings, with the labels of the Settings page,
//! and "All settings…", which opens ClaudeDeck's page in WinCraft's
//! Settings window. A click flips a setting and saves it at once; the menu
//! stays open, so several can be changed. In mini mode the column is too
//! narrow for the menu, so it opens in a small window of its own beside it.

use egui::{pos2, vec2, Id, Pos2, Rect, Ui};
use serde_json::Value;

use crate::core::theme::Tokens;
use crate::core::ui_bridge::{HostChannel, HostRequest};

use super::header::Click;
use super::window::DeckUi;
use super::{Settings, ID, MINI_KEY, ONLY_OPEN_KEY, STAY_ON_TOP_KEY};

pub const TIP: &str = "Quick settings";
pub const ALL: &str = "All settings\u{2026}";
/// The key that opens the menu, as the docs and the tooltip name it.
pub const KEY: &str = "Ctrl+,";

/// One on/off setting in the menu.
pub struct Toggle {
    pub label: &'static str,
    pub key: &'static str,
    get: fn(&Settings) -> bool,
    set: fn(&mut Settings, bool),
}

/// The settings the menu lists, in its order. A separator comes before
/// the row settings, as on the Settings page under "On each row".
pub const TOGGLES: [Toggle; 11] = [
    Toggle {
        label: "Fade when the mouse is away",
        key: super::FADE_KEY,
        get: |s| s.fade,
        set: |s, on| s.fade = on,
    },
    Toggle {
        label: "Only open windows",
        key: ONLY_OPEN_KEY,
        get: |s| s.only_open,
        set: |s, on| s.only_open = on,
    },
    Toggle {
        label: "Stay on top",
        key: STAY_ON_TOP_KEY,
        get: |s| s.stay_on_top,
        set: |s, on| s.stay_on_top = on,
    },
    Toggle {
        label: "Mini mode",
        key: MINI_KEY,
        get: |s| s.mini,
        set: |s, on| s.mini = on,
    },
    Toggle {
        label: "Fold chats idle for over 1 day",
        key: super::FOLD_COLD_KEY,
        get: |s| s.fold_cold,
        set: |s, on| s.fold_cold = on,
    },
    Toggle {
        label: "Show a count on the tray icon",
        key: super::TRAY_KEY,
        get: |s| s.tray_count,
        set: |s, on| s.tray_count = on,
    },
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
];
/// The height of a row, smaller than the theme's 32 px.
const ROW: f32 = 24.0;
/// The first row setting: a separator goes before it.
const ROW_SETTINGS_FROM: usize = 6;

/// What the user picked in the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Flip(usize),
    All,
}

/// The menu's state in the window.
#[derive(Default)]
pub struct Quick {
    /// The gear's place in the header this frame.
    pub gear: Option<Rect>,
    /// The gear was clicked: open or close the menu.
    pub toggle: bool,
    /// The key (or the palette) asked for the menu.
    pub by_key: bool,
    /// The first row takes the keyboard on the next frame.
    focus_first: bool,
    /// Mini mode: the menu's own window is open, and had the keyboard.
    mini_open: bool,
    mini_focused: bool,
    mini_size: Option<egui::Vec2>,
}

impl Quick {
    /// Whether the menu shows now, in either mode.
    pub fn is_open(&self, ctx: &egui::Context) -> bool {
        self.mini_open || egui::Popup::is_id_open(ctx, popup_id())
    }
}

fn popup_id() -> Id {
    Id::new("claude-deck-quick-settings")
}

/// The rows. A click flips a setting; the menu stays open.
fn rows(ui: &mut Ui, settings: &Settings, focus_first: bool) -> Option<Pick> {
    // Thirteen rows of the theme's 32 px would not fit the window's 480 px.
    ui.spacing_mut().interact_size.y = ROW;
    ui.spacing_mut().item_spacing.y = 2.0;
    let mut picked = None;
    for (at, toggle) in TOGGLES.iter().enumerate() {
        if at == ROW_SETTINGS_FROM {
            ui.separator();
        }
        let mut on = (toggle.get)(settings);
        let response = ui.checkbox(&mut on, toggle.label);
        if at == 0 && focus_first {
            response.request_focus();
        }
        if response.clicked() {
            picked = Some(Pick::Flip(at));
        }
    }
    ui.separator();
    if ui.button(ALL).clicked() {
        picked = Some(Pick::All);
    }
    picked
}

/// One frame of the menu. In the full list it hangs under the gear, as
/// the rows' menus do; in mini mode it has a window of its own.
pub fn show(ui: &mut Ui, state: &mut DeckUi, settings: &Settings, tokens: &Tokens) -> Option<Pick> {
    let quick = &mut state.quick;
    if settings.mini {
        egui::Popup::close_id(ui.ctx(), popup_id());
        quick.gear = None;
        if std::mem::take(&mut quick.by_key) {
            quick.mini_open = true;
            quick.mini_focused = false;
            quick.focus_first = true;
        }
        quick.toggle = false;
        return beside(ui.ctx(), quick, settings, tokens);
    }
    quick.mini_open = false;
    let by_key = std::mem::take(&mut quick.by_key);
    let command = if std::mem::take(&mut quick.toggle) {
        Some(egui::SetOpenCommand::Toggle)
    } else {
        by_key.then_some(egui::SetOpenCommand::Bool(true))
    };
    if by_key {
        quick.focus_first = true;
    }
    let full = ui.ctx().content_rect();
    let anchor = quick
        .gear
        .unwrap_or_else(|| Rect::from_min_size(full.min + vec2(8.0, 4.0), vec2(1.0, 20.0)));
    let focus_first = std::mem::take(&mut quick.focus_first);
    let shown = egui::Popup::new(popup_id(), ui.ctx().clone(), anchor, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .open_memory(command)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| rows(ui, settings, focus_first));
    let picked = shown.and_then(|shown| shown.inner);
    if picked == Some(Pick::All) {
        egui::Popup::close_id(ui.ctx(), popup_id());
    }
    picked
}

/// Where the menu's own window goes in mini mode: right of the column, or
/// left of it when the monitor has no room on the right.
pub fn beside_at(column: Rect, size: egui::Vec2, monitor_width: Option<f32>, gap: f32) -> Pos2 {
    let right_room = monitor_width.is_none_or(|width| column.right() + gap + size.x <= width);
    let left = if right_room {
        column.right() + gap
    } else {
        column.left() - gap - size.x
    };
    pos2(left, column.top())
}

fn beside(
    ctx: &egui::Context,
    quick: &mut Quick,
    settings: &Settings,
    tokens: &Tokens,
) -> Option<Pick> {
    if !quick.mini_open {
        return None;
    }
    let outer = ctx.input(|i| i.viewport().outer_rect)?;
    let monitor = ctx.input(|i| i.viewport().monitor_size).map(|m| m.x);
    let size = quick.mini_size.unwrap_or(vec2(320.0, 330.0));
    let builder = egui::ViewportBuilder::default()
        .with_title("WinCraft \u{b7} ClaudeDeck quick settings")
        .with_decorations(false)
        .with_taskbar(false)
        .with_resizable(false)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_position(beside_at(outer, size, monitor, 6.0))
        .with_inner_size(size);
    let focus_first = std::mem::take(&mut quick.focus_first);
    let fill = tokens.panel_bg;
    let (picked, close, focused, used) = ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("wincraft-claude-deck-quick"),
        builder,
        |ui, _class| {
            let shown = egui::Frame::menu(ui.style()).fill(fill).show(ui, |ui| {
                ui.set_width(300.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    egui::containers::menu::menu_style(ui.style_mut());
                    rows(ui, settings, focus_first)
                })
                .inner
            });
            let (esc, focused) = ui
                .ctx()
                .input(|i| (i.key_pressed(egui::Key::Escape), i.focused));
            let used = shown.response.rect.size();
            (shown.inner, esc, focused, used)
        },
    );
    quick.mini_size = Some(used.ceil());
    if focused {
        quick.mini_focused = true;
    }
    let lost = quick.mini_focused && !focused;
    if close || lost || picked == Some(Pick::All) {
        quick.mini_open = false;
    }
    picked
}

/// Carries out a pick: flips the setting in `settings` and saves it, the
/// way the header's switch and pin do. The pin, "Only open windows" and
/// mini mode go through the header's own clicks, so they act the same.
pub fn apply(settings: &mut Settings, pick: Pick, to_host: &HostChannel) -> Option<Click> {
    let toggle = match pick {
        Pick::All => {
            to_host.send(HostRequest::ShowPluginPage(ID.to_string()));
            return None;
        }
        Pick::Flip(at) => TOGGLES.get(at)?,
    };
    match toggle.key {
        STAY_ON_TOP_KEY => return Some(Click::Pin),
        ONLY_OPEN_KEY => return Some(Click::OnlyOpen),
        MINI_KEY => return Some(Click::Mini),
        _ => {}
    }
    let on = !(toggle.get)(settings);
    (toggle.set)(settings, on);
    to_host.send(HostRequest::SetSetting {
        plugin: ID.to_string(),
        key: toggle.key.to_string(),
        value: Value::Bool(on),
    });
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_row_flips_its_own_setting_and_nothing_else() {
        for (at, toggle) in TOGGLES.iter().enumerate() {
            let mut settings = Settings::default();
            let before = settings.clone();
            let on = !(toggle.get)(&settings);
            (toggle.set)(&mut settings, on);
            assert_eq!((toggle.get)(&settings), on, "row {at}");
            (toggle.set)(&mut settings, !on);
            assert_eq!(settings, before, "row {at} touched another setting");
        }
    }

    #[test]
    fn the_rows_use_the_settings_page_labels() {
        let deck = super::super::ClaudeDeck::new();
        use crate::core::traits::WinCraftPlugin;
        let fields = deck.settings_fields();
        for toggle in TOGGLES.iter().filter(|t| t.key != MINI_KEY) {
            let field = fields.iter().find(|field| field.key == toggle.key);
            assert_eq!(field.map(|field| field.label), Some(toggle.label));
        }
        assert_eq!(TOGGLES[3].label, super::super::header::MINI_TIP);
    }

    #[test]
    fn a_flip_saves_the_setting_and_the_header_ones_go_through_the_header() {
        let bridge = crate::core::ui_bridge::create();
        let (to_host, from_ui) = (&bridge.to_host, &bridge.host_rx);
        let mut settings = Settings::default();
        assert_eq!(apply(&mut settings, Pick::Flip(0), to_host), None);
        assert!(!settings.fade, "fade was on by default and is off now");
        assert!(matches!(
            from_ui.try_recv(),
            Ok(HostRequest::SetSetting { key, value: Value::Bool(false), .. }) if key == "fade_when_away"
        ));
        assert_eq!(
            apply(&mut settings, Pick::Flip(1), to_host),
            Some(Click::OnlyOpen)
        );
        assert_eq!(
            apply(&mut settings, Pick::Flip(2), to_host),
            Some(Click::Pin)
        );
        assert_eq!(
            apply(&mut settings, Pick::Flip(3), to_host),
            Some(Click::Mini)
        );
        assert_eq!(apply(&mut settings, Pick::All, to_host), None);
        assert!(matches!(
            from_ui.try_recv(),
            Ok(HostRequest::ShowPluginPage(id)) if id == "claude_deck"
        ));
        assert_eq!(apply(&mut settings, Pick::Flip(99), to_host), None);
    }

    #[test]
    fn the_mini_menu_goes_right_of_the_column_or_left_when_there_is_no_room() {
        let column = Rect::from_min_size(pos2(100.0, 50.0), vec2(30.0, 300.0));
        let size = vec2(300.0, 320.0);
        assert_eq!(
            beside_at(column, size, Some(1920.0), 6.0),
            pos2(136.0, 50.0)
        );
        let far = Rect::from_min_size(pos2(1880.0, 50.0), vec2(30.0, 300.0));
        assert_eq!(beside_at(far, size, Some(1920.0), 6.0), pos2(1574.0, 50.0));
    }
}

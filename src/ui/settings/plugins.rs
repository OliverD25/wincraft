use std::sync::Arc;

use crate::core::hotkeys;
use crate::core::theme::Tokens;
use crate::core::ui_bridge::{HostChannel, HostRequest, HotkeyInfo, PluginInfo, UiSnapshot};
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::capture::{self, Outcome};
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::{field, keycap, text, toggle};

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.open_plugin.clone() {
            Some(id) => match snapshot.plugins.iter().find(|plugin| plugin.id == id) {
                Some(plugin) => detail(ui, plugin, snapshot, state, to_host),
                None => state.open_plugin = None,
            },
            None => list(ui, snapshot, state, to_host),
        });
}

fn count_in_words(count: usize) -> String {
    const WORDS: [&str; 11] = [
        "No plugins",
        "One",
        "Two",
        "Three",
        "Four",
        "Five",
        "Six",
        "Seven",
        "Eight",
        "Nine",
        "Ten",
    ];
    WORDS
        .get(count)
        .map(|word| word.to_string())
        .unwrap_or_else(|| count.to_string())
}

fn meta(version: &str, author: &str) -> String {
    format!("{version} \u{00B7} {author}")
}

fn list(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let description = format!(
        "{} installed. Click a plugin for its settings, hotkeys and files.",
        count_in_words(snapshot.plugins.len())
    );
    page::page_header(ui, "Plugins", None, &description, |_| {});
    ui.add_space(16.0);
    ui.spacing_mut().item_spacing.y = 0.0;

    if snapshot.plugins.is_empty() {
        empty_state(
            ui,
            "No plugins installed",
            "Open the Plugin Store to add one.",
        );
        return;
    }

    for plugin in &snapshot.plugins {
        let meta = meta(&plugin.version, &plugin.author);
        let text = RowText::new(&plugin.name)
            .meta(&meta)
            .desc(&plugin.description, tokens.text_secondary);
        let mut toggled = false;
        let row = settings_row(ui, &plugin.id, text, true, |ui| {
            let mut enabled = plugin.enabled;
            if toggle::toggle(ui, &mut enabled).changed() {
                toggled = true;
                to_host.send(HostRequest::SetPluginEnabled {
                    id: plugin.id.clone(),
                    enabled,
                });
            }
            ui.add_space(4.0);
            icons::show(ui, Icon::ChevronRight, 14.0, tokens.text_disabled);
        });
        if row.clicked() && !toggled {
            state.open_plugin = Some(plugin.id.clone());
        }
    }
}

fn detail(
    ui: &mut egui::Ui,
    plugin: &PluginInfo,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    if text::link(ui, "\u{2039} All plugins").clicked() {
        state.open_plugin = None;
        state.end_capture();
        return;
    }
    ui.add_space(8.0);

    let meta = meta(&plugin.version, &plugin.author);
    page::page_header(ui, &plugin.name, Some(&meta), &plugin.description, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let mut enabled = plugin.enabled;
        if toggle::toggle(ui, &mut enabled).changed() {
            to_host.send(HostRequest::SetPluginEnabled {
                id: plugin.id.clone(),
                enabled,
            });
        }
        text::secondary(ui, "Enabled", tokens.text_secondary);
    });
    if let Some(status) = &plugin.status {
        text::secondary(ui, status, tokens.text_secondary);
        ui.add_space(8.0);
    }
    if let Some((label, command)) = &plugin.page_action {
        if button::button(ui, label, Kind::Secondary).clicked() {
            to_host.send(HostRequest::RunCommand(*command));
        }
        ui.add_space(8.0);
    }

    if !plugin.fields.is_empty() {
        page::section_header(ui, "Settings");
        ui.spacing_mut().item_spacing.y = 0.0;
        for info in &plugin.fields {
            if let Some(value) = field::show(ui, info) {
                to_host.send(HostRequest::SetSetting {
                    plugin: plugin.id.clone(),
                    key: info.key.clone(),
                    value,
                });
            }
        }
    }

    if !plugin.hotkeys.is_empty() {
        page::section_header(ui, "Hotkeys");
        ui.spacing_mut().item_spacing.y = 0.0;
        for info in &plugin.hotkeys {
            hotkey_row(ui, plugin, info, snapshot, state, to_host);
        }
    }

    page::section_header(ui, "Files");
    ui.spacing_mut().item_spacing.y = 0.0;
    let relative = format!("plugins\\{}.json", plugin.id);
    settings_row(ui, "files", RowText::new("Plugin settings"), false, |ui| {
        text::mono(ui, &relative, 12.0, tokens.text_secondary);
        if button::button(ui, "Open file", Kind::Secondary).clicked() {
            to_host.send(HostRequest::OpenPath(plugin.config_path.clone()));
        }
        if button::button(ui, "Reset to defaults", Kind::Danger).clicked() {
            to_host.send(HostRequest::ResetPlugin(plugin.id.clone()));
        }
    });

    ui.add_space(20.0);
    text::group_header(ui, "About this plugin");
    ui.add_space(8.0);
    state.readme.boxed(ui, &plugin.readme);
}

fn hotkey_row(
    ui: &mut egui::Ui,
    plugin: &PluginInfo,
    info: &HotkeyInfo,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let owner = format!("{}/{}", plugin.id, info.action);
    let (status, colour) = if info.registered {
        ("registered", tokens.success)
    } else {
        ("taken by another app", tokens.warning)
    };

    let mut change = false;
    settings_row(ui, &owner, RowText::new(&info.label), false, |ui| {
        let chips = keycap::chips(ui, &info.binding, 12.0, false);
        let chips = ui
            .interact(
                chips.rect,
                ui.id().with(("rebind", &owner)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Click to change");
        change = chips.clicked();
        text::secondary(ui, status, colour);
        let is_default = info.binding == info.default_binding;
        let reset = ui.add_enabled_ui(!is_default, |ui| {
            button::button(ui, "Reset", Kind::Secondary)
        });
        if reset.inner.clicked() {
            to_host.send(HostRequest::SetHotkey {
                plugin: plugin.id.clone(),
                action: info.action.clone(),
                binding: info.default_binding.clone(),
            });
        }
    });
    if change {
        state.begin_capture(owner.clone());
    }

    if let Some(session) = state
        .capture
        .as_mut()
        .filter(|session| session.owner == owner)
    {
        match capture::show(ui, session, |hotkey| {
            page::verdict(hotkey, snapshot, &tokens)
        }) {
            Some(Outcome::Apply(hotkey)) => {
                to_host.send(HostRequest::SetHotkey {
                    plugin: plugin.id.clone(),
                    action: info.action.clone(),
                    binding: hotkeys::format(hotkey),
                });
                state.end_capture();
            }
            Some(Outcome::Cancel) => state.end_capture(),
            None => {}
        }
    }
}

use std::sync::Arc;

use crate::core::hotkeys;
use crate::core::traits::Hotkey;
use crate::core::ui_bridge::{HostChannel, HostRequest, PluginInfo, UiSnapshot};
use crate::plugins::shortcut_detector::probe;
use crate::ui::settings::SettingsState;
use crate::ui::widgets::{field, hotkey_capture};

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    match state.open_plugin.clone() {
        Some(id) => match snapshot.plugins.iter().find(|plugin| plugin.id == id) {
            Some(plugin) => plugin_page(ui, plugin, snapshot, state, to_host),
            None => state.open_plugin = None,
        },
        None => plugin_list(ui, snapshot, state, to_host),
    }
}

fn plugin_list(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    ui.heading("Plugins");
    ui.add_space(12.0);

    for plugin in &snapshot.plugins {
        let frame = egui::Frame::NONE
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(12));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .link(egui::RichText::new(&plugin.name).strong())
                            .clicked()
                        {
                            state.open_plugin = Some(plugin.id.clone());
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "{} \u{00B7} {}",
                                plugin.version, plugin.author
                            ))
                            .weak()
                            .small(),
                        );
                    });
                    ui.label(egui::RichText::new(&plugin.description).small());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut enabled = plugin.enabled;
                    if ui.checkbox(&mut enabled, "Enabled").changed() {
                        to_host.send(HostRequest::SetPluginEnabled {
                            id: plugin.id.clone(),
                            enabled,
                        });
                    }
                });
            });
        });
        ui.add_space(8.0);
    }
}

fn plugin_page(
    ui: &mut egui::Ui,
    plugin: &PluginInfo,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    ui.horizontal(|ui| {
        if ui.button("\u{2190} All plugins").clicked() {
            state.open_plugin = None;
            state.end_capture();
        }
    });
    ui.add_space(8.0);

    ui.heading(&plugin.name);
    ui.label(
        egui::RichText::new(format!("{} \u{00B7} {}", plugin.version, plugin.author))
            .weak()
            .small(),
    );
    ui.label(&plugin.description);
    ui.add_space(12.0);

    let mut enabled = plugin.enabled;
    if ui.checkbox(&mut enabled, "Enabled").changed() {
        to_host.send(HostRequest::SetPluginEnabled {
            id: plugin.id.clone(),
            enabled,
        });
    }

    if !plugin.fields.is_empty() {
        ui.add_space(16.0);
        ui.label(egui::RichText::new("Settings").strong());
        ui.add_space(6.0);
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
        ui.add_space(16.0);
        ui.label(egui::RichText::new("Hotkeys").strong());
        ui.add_space(6.0);
        for info in &plugin.hotkeys {
            hotkey_row(ui, plugin, info, snapshot, state, to_host);
        }
    }

    ui.add_space(16.0);
    ui.label(egui::RichText::new("Files").strong());
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(plugin.config_path.display().to_string())
                .monospace()
                .small(),
        );
        if ui.button("Open file").clicked() {
            to_host.send(HostRequest::OpenPath(plugin.config_path.clone()));
        }
        if ui.button("Reset to defaults").clicked() {
            to_host.send(HostRequest::ResetPlugin(plugin.id.clone()));
        }
    });

    ui.add_space(16.0);
    ui.label(egui::RichText::new("About this plugin").strong());
    ui.add_space(6.0);
    state.readme.show(ui, &plugin.readme);
}

fn hotkey_row(
    ui: &mut egui::Ui,
    plugin: &PluginInfo,
    info: &crate::core::ui_bridge::HotkeyInfo,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let owner = format!("{}/{}", plugin.id, info.action);
    let capturing = state.capture_owner.as_deref() == Some(owner.as_str());

    ui.horizontal(|ui| {
        ui.label(&info.label);
        ui.add_space(8.0);
        let label = if capturing {
            hotkey_capture::describe(hotkey_capture::live_modifiers())
        } else {
            info.binding.clone()
        };
        if ui.selectable_label(capturing, label).clicked() {
            state.begin_capture(owner.clone());
        }
        if info.registered {
            ui.label(egui::RichText::new("registered").weak().small());
        } else {
            ui.colored_label(ui.visuals().error_fg_color, "taken by another app");
        }
    });

    if !capturing {
        return;
    }
    ui.label(
        egui::RichText::new("Press the combination you want, or Esc to cancel.")
            .weak()
            .small(),
    );
    if let Some(hotkey) = hotkey_capture::take_result() {
        state.pending = Some((owner.clone(), hotkey, verdict(hotkey, snapshot)));
    }
    if hotkey_capture::take_cancelled() {
        state.end_capture();
        return;
    }

    let pending = state.pending.clone();
    if let Some((pending_owner, hotkey, verdict_text)) = pending {
        if pending_owner != owner {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(hotkeys::format(hotkey)).strong());
            ui.label(egui::RichText::new(&verdict_text).weak().small());
        });
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() {
                to_host.send(HostRequest::SetHotkey {
                    plugin: plugin.id.clone(),
                    action: info.action.clone(),
                    binding: hotkeys::format(hotkey),
                });
                state.end_capture();
            }
            if ui.button("Cancel").clicked() {
                state.end_capture();
            }
        });
    }
}

/// The plugin list in the snapshot is what says a combination is already ours;
/// everything else comes from a live probe, which is pure Win32 and so is safe
/// to run from this thread.
fn verdict(hotkey: Hotkey, snapshot: &UiSnapshot) -> String {
    let mine: Vec<(String, String, Hotkey)> = snapshot
        .plugins
        .iter()
        .flat_map(|plugin| {
            plugin.hotkeys.iter().filter_map(move |info| {
                hotkeys::parse(&info.binding)
                    .ok()
                    .map(|parsed| (plugin.name.clone(), info.label.clone(), parsed))
            })
        })
        .collect();
    let entry = probe::verdict(std::ptr::null_mut(), hotkey, &mine);
    probe::verdict_text(&entry)
}

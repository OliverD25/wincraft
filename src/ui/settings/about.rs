use std::sync::Arc;

use crate::core::theme::{self, Tokens};
use crate::core::traits::FieldKind;
use crate::core::ui_bridge::{
    HostChannel, HostRequest, HostSetting, NewRelease, UiSnapshot, UpdateStatus, UpdateView,
};
use crate::store::github;
use crate::ui::settings::{self as page, Readme, SettingsState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::{keycap, text, toggle};

/// The release notes of a newer version, drawn like a plugin's README.
#[derive(Default)]
pub struct Update {
    notes: Readme,
}

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page::page_header(
                ui,
                "About",
                None,
                "WinCraft is open source under the MIT licence.",
                |_| {},
            );
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.y = 0.0;

            let version = format!(
                "{} \u{00B7} built {}",
                snapshot.version,
                env!("WINCRAFT_BUILD_DATE")
            );
            settings_row(ui, "version", RowText::new("Version"), false, |ui| {
                text::mono(ui, &version, 12.0, tokens.text_secondary);
            });

            settings_row(ui, "links", RowText::new("Links"), false, |ui| {
                if button::button(ui, "Repository", Kind::Secondary).clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(github::repo_url()));
                }
                if button::button(ui, "Report an issue", Kind::Secondary).clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(github::issues_url()));
                }
            });

            let update = &snapshot.update;
            settings_row(
                ui,
                "updates",
                RowText::new("Updates").desc(
                    "Checks GitHub releases once a day, and when you ask.",
                    tokens.text_secondary,
                ),
                false,
                |ui| {
                    update_status(ui, update);
                    if button::button(ui, "Check for updates", Kind::Primary).clicked() {
                        to_host.send(HostRequest::CheckForUpdates);
                    }
                },
            );
            settings_row(
                ui,
                "check-daily",
                RowText::new("Check for updates daily").desc(
                    "Only tells you. Nothing changes until you click Update now.",
                    tokens.text_secondary,
                ),
                false,
                |ui| {
                    let mut on = update.check_daily;
                    if toggle::toggle(ui, &mut on).changed() {
                        to_host.send(HostRequest::SetHostSetting(HostSetting::CheckUpdatesDaily(
                            on,
                        )));
                    }
                },
            );
            if let Some(failed) = &update.failed {
                settings_row(
                    ui,
                    "update-failed",
                    RowText::new("Last update").desc(failed, tokens.warning),
                    false,
                    |_| {},
                );
            }

            settings_row(
                ui,
                "fonts",
                RowText::new("Fonts").desc(
                    "Segoe UI from Windows. Bundled: Selawik and JetBrains Mono, \
                     both under the SIL Open Font License 1.1.",
                    tokens.text_secondary,
                ),
                false,
                |_| {},
            );

            if let Some(release) = &update.available {
                new_release(ui, release, update, &mut state.update, to_host);
                ui.spacing_mut().item_spacing.y = 0.0;
            }

            page::section_header(ui, "Hotkeys");
            ui.spacing_mut().item_spacing.y = 0.0;
            for (index, row) in hotkey_rows(snapshot).iter().enumerate() {
                let owner = if row.state == HotkeyState::PluginOff {
                    format!("{} \u{00B7} plugin off", row.owner)
                } else {
                    row.owner.clone()
                };
                settings_row(
                    ui,
                    &format!("hotkey-{index}"),
                    RowText::new(&row.label).desc(&owner, tokens.text_secondary),
                    false,
                    |ui| {
                        if row.state == HotkeyState::Taken {
                            text::secondary(ui, "taken by another app", tokens.warning);
                        }
                        keycap::binding(ui, &keycap::spaced(&row.binding), false);
                    },
                );
            }

            ui.add_space(20.0);
            text::group_header(ui, "For plugin authors");
            ui.add_space(6.0);
            authors_note(ui);
        });
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HotkeyState {
    Registered,
    Taken,
    /// A disabled plugin registers nothing, so its keys are neither live nor
    /// taken.
    PluginOff,
}

#[derive(Debug, PartialEq)]
struct HotkeyRow {
    owner: String,
    label: String,
    binding: String,
    state: HotkeyState,
}

/// Every hotkey WinCraft knows about, the palette's first, then each plugin's
/// in plugin order, as the v0.1 About listed them.
fn hotkey_rows(snapshot: &UiSnapshot) -> Vec<HotkeyRow> {
    let state = |plugin_on: bool, registered: bool| match (plugin_on, registered) {
        (false, _) => HotkeyState::PluginOff,
        (true, true) => HotkeyState::Registered,
        (true, false) => HotkeyState::Taken,
    };
    let mut rows = vec![HotkeyRow {
        owner: "WinCraft".to_string(),
        label: "Open the palette".to_string(),
        binding: snapshot.palette_hotkey.clone(),
        state: state(true, snapshot.palette_hotkey_registered),
    }];
    for plugin in &snapshot.plugins {
        for info in &plugin.hotkeys {
            rows.push(HotkeyRow {
                owner: plugin.name.clone(),
                label: info.label.clone(),
                binding: info.binding.clone(),
                state: state(plugin.enabled, info.registered),
            });
        }
    }
    rows
}

/// The result sits in the status text beside the button; the button itself
/// never changes, so it is always where the user left it.
fn update_status(ui: &mut egui::Ui, update: &UpdateView) {
    let tokens = Tokens::get(ui.ctx());
    match &update.status {
        UpdateStatus::Idle => {
            if let Some(release) = &update.available {
                text::secondary(ui, &format!("{} available", release.tag), tokens.accent);
            }
        }
        UpdateStatus::Checking => {
            text::secondary(ui, "Checking\u{2026}", tokens.text_disabled);
        }
        UpdateStatus::UpToDate => {
            text::secondary(ui, "Up to date", tokens.success);
        }
        UpdateStatus::NoRelease => {
            text::secondary(ui, "No release published yet", tokens.text_secondary);
        }
        UpdateStatus::Offline => {
            text::secondary(ui, "Could not check \u{2014} offline", tokens.warning);
        }
        UpdateStatus::Downloading(Some(percent)) => {
            text::secondary(
                ui,
                &format!("Downloading\u{2026} {percent}%"),
                tokens.text_secondary,
            );
        }
        UpdateStatus::Downloading(None) => {
            text::secondary(ui, "Downloading\u{2026}", tokens.text_secondary);
        }
        UpdateStatus::Installing => {
            text::secondary(
                ui,
                "Installing \u{2014} WinCraft restarts",
                tokens.text_secondary,
            );
        }
        UpdateStatus::Error(_) => {
            text::secondary(ui, "Update failed", tokens.warning);
        }
    }
}

/// Whether Update now has something to do: not while a download or the
/// restart is under way.
fn can_update(update: &UpdateView) -> bool {
    update.blocked.is_none()
        && !matches!(
            update.status,
            UpdateStatus::Downloading(_) | UpdateStatus::Installing
        )
}

/// The newer version: its name, Update now, a link to its page and its
/// release notes. A copy that cannot update itself says why instead of
/// offering the button.
fn new_release(
    ui: &mut egui::Ui,
    release: &NewRelease,
    view: &UpdateView,
    update: &mut Update,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let version = release.tag.trim_start_matches(['v', 'V']);
    page::section_header(ui, &format!("WinCraft {version} is available"));
    ui.spacing_mut().item_spacing.y = 0.0;
    let (desc, colour) = match &view.blocked {
        Some(reason) => (reason.as_str(), tokens.warning),
        None => (
            "Downloads the new version, checks it and restarts WinCraft. \
             If it does not start properly, this version comes back.",
            tokens.text_secondary,
        ),
    };
    settings_row(
        ui,
        "new-release",
        RowText::new(&release.tag).desc(desc, colour),
        false,
        |ui| {
            if button::button(ui, "Release page", Kind::Secondary).clicked() {
                ui.ctx()
                    .open_url(egui::OpenUrl::new_tab(release.page.clone()));
            }
            if can_update(view) && button::button(ui, "Update now", Kind::Primary).clicked() {
                to_host.send(HostRequest::UpdateNow);
            }
        },
    );
    if let UpdateStatus::Error(err) = &view.status {
        ui.add_space(8.0);
        text::secondary(ui, err, tokens.warning);
    }
    if !release.notes.trim().is_empty() {
        ui.add_space(12.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        update.notes.boxed(ui, &release.notes);
    }
}

fn authors_note(ui: &mut egui::Ui) {
    let tokens = Tokens::get(ui.ctx());
    let kinds = FieldKind::ALL
        .iter()
        .map(FieldKind::name)
        .collect::<Vec<_>>()
        .join(" \u{00B7} ");
    let body = || text::format(theme::regular(13.0), tokens.text_secondary, Some(19.0));
    let mut job = egui::text::LayoutJob::default();
    job.append("Declare fields of kind ", 0.0, body());
    job.append(
        &kinds,
        0.0,
        text::format(theme::mono(12.0), tokens.text_primary, Some(19.0)),
    );
    job.append(
        "; the settings page draws and saves them for you. See ",
        0.0,
        body(),
    );
    job.append(
        "src/plugins/<id>/README.md",
        0.0,
        text::format(theme::regular(13.0), tokens.accent, Some(19.0)),
    );
    job.append(".", 0.0, body());
    let width = ui.available_width().min(560.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        text::wrapped(ui, job);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::{HotkeyInfo, PluginInfo};

    fn plugin(name: &str, enabled: bool, keys: &[(&str, bool)]) -> PluginInfo {
        PluginInfo {
            id: name.to_lowercase(),
            name: name.to_string(),
            enabled,
            hotkeys: keys
                .iter()
                .map(|(binding, registered)| HotkeyInfo {
                    action: binding.to_string(),
                    label: format!("Action {binding}"),
                    binding: binding.to_string(),
                    default_binding: binding.to_string(),
                    registered: *registered,
                })
                .collect(),
            version: String::new(),
            author: String::new(),
            description: String::new(),
            readme: String::new(),
            config_path: Default::default(),
            fields: Vec::new(),
            status: None,
            page_action: None,
        }
    }

    #[test]
    fn the_palette_comes_first_and_only_live_keys_can_be_taken() {
        let snapshot = UiSnapshot {
            palette_hotkey: "Win+Alt+P".to_string(),
            palette_hotkey_registered: false,
            plugins: vec![
                plugin(
                    "Dimmer",
                    true,
                    &[("Win+Alt+F1", true), ("Win+Alt+F2", false)],
                ),
                plugin("Keeper", false, &[("Win+Alt+L", true)]),
            ],
            ..UiSnapshot::default()
        };
        let rows: Vec<(String, String, HotkeyState)> = hotkey_rows(&snapshot)
            .into_iter()
            .map(|row| (row.owner, row.binding, row.state))
            .collect();
        let expected = [
            ("WinCraft", "Win+Alt+P", HotkeyState::Taken),
            ("Dimmer", "Win+Alt+F1", HotkeyState::Registered),
            ("Dimmer", "Win+Alt+F2", HotkeyState::Taken),
            ("Keeper", "Win+Alt+L", HotkeyState::PluginOff),
        ]
        .map(|(owner, binding, state)| (owner.to_string(), binding.to_string(), state));
        assert_eq!(rows, expected);
    }
}

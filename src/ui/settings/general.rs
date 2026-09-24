use std::sync::Arc;

use crate::core::config::ThemeChoice;
use crate::core::hotkeys;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest, HostSetting, UiSnapshot};
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::capture::{self, Outcome};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::{choice, keycap, text, toggle};

const PALETTE_OWNER: &str = "host/palette";

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
                "General",
                None,
                "WinCraft follows the Windows theme unless you choose one here.",
                |_| {},
            );
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.y = 0.0;

            settings_row(
                ui,
                "autostart",
                RowText::new("Start with Windows")
                    .desc("Launch when you sign in.", tokens.text_secondary),
                false,
                |ui| {
                    let mut on = snapshot.start_with_windows;
                    if toggle::toggle(ui, &mut on).changed() {
                        to_host.send(HostRequest::SetHostSetting(HostSetting::StartWithWindows(
                            on,
                        )));
                    }
                },
            );

            settings_row(
                ui,
                "theme",
                RowText::new("Theme").desc(
                    "System follows the Windows app mode.",
                    tokens.text_secondary,
                ),
                false,
                |ui| {
                    let labels: Vec<&str> = ThemeChoice::ALL
                        .iter()
                        .map(|choice| choice.label())
                        .collect();
                    let current = ThemeChoice::ALL
                        .iter()
                        .position(|choice| *choice == snapshot.theme);
                    if let Some(index) = choice::segmented(ui, &labels, current) {
                        to_host.send(HostRequest::SetHostSetting(HostSetting::Theme(
                            ThemeChoice::ALL[index],
                        )));
                    }
                },
            );

            let (verdict, colour) = if snapshot.palette_hotkey_registered {
                ("registered", tokens.success)
            } else {
                ("taken by another app", tokens.warning)
            };
            settings_row(
                ui,
                "palette-hotkey",
                RowText::new("Palette hotkey").desc(verdict, colour),
                false,
                |ui| {
                    keycap::binding(ui, &keycap::spaced(&snapshot.palette_hotkey), false);
                    if button::button(ui, "Change", Kind::Secondary).clicked() {
                        state.begin_capture(PALETTE_OWNER.to_string());
                    }
                },
            );
            if let Some(session) = state
                .capture
                .as_mut()
                .filter(|session| session.owner == PALETTE_OWNER)
            {
                match capture::show(ui, session, |hotkey| {
                    page::verdict(hotkey, snapshot, &tokens)
                }) {
                    Some(Outcome::Apply(hotkey)) => {
                        to_host.send(HostRequest::SetHostSetting(HostSetting::PaletteHotkey(
                            hotkeys::format(hotkey),
                        )));
                        state.end_capture();
                    }
                    Some(Outcome::Cancel) => state.end_capture(),
                    None => {}
                }
            }

            file_row(
                ui,
                "config-file",
                "Config file",
                &snapshot.config_path,
                to_host,
            );
            file_row(ui, "log-file", "Log file", &snapshot.log_path, to_host);
        });
}

fn file_row(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    path: &std::path::Path,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    settings_row(ui, id, RowText::new(label), false, |ui| {
        text::single(
            ui,
            text::job(
                &page::display_path(path),
                theme::mono(13.0),
                tokens.text_secondary,
                None,
            ),
        );
        if button::button(ui, "Open", Kind::Secondary).clicked() {
            to_host.send(HostRequest::OpenPath(path.to_path_buf()));
        }
    });
}

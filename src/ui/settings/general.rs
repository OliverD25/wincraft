use std::sync::Arc;

use crate::core::config::{SearchConfig, ThemeChoice};
use crate::core::hotkeys;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest, HostSetting, UiSnapshot};
use crate::search::providers::shell::{self, Shell, SHELL_NAMES};
use crate::search::providers::web;
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::capture::{self, Outcome};
use crate::ui::widgets::input::{self, Face};
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

            page::section_header(ui, "Palette search");
            search_rows(ui, &snapshot.search, to_host);
        });
}

/// The palette's terminal shell, web search address and history switch.
/// Each change is sent whole to the host, which saves it and applies it at
/// once.
fn search_rows(ui: &mut egui::Ui, search: &SearchConfig, to_host: &Arc<HostChannel>) {
    let tokens = Tokens::get(ui.ctx());
    let send = |change: &dyn Fn(&mut SearchConfig)| {
        let mut next = search.clone();
        change(&mut next);
        to_host.send(HostRequest::SetHostSetting(HostSetting::Search(next)));
    };
    let installed = shell::pwsh_installed();
    let usable = |name: &str| name != "PowerShell 7" || installed;
    let labels: Vec<String> = SHELL_NAMES
        .iter()
        .map(|name| {
            if usable(name) {
                name.to_string()
            } else {
                format!("{name} (not installed)")
            }
        })
        .collect();
    let custom = search.terminal_shell == "Custom";
    let problem = if custom {
        None
    } else {
        Shell::from_settings(&search.terminal_shell, &search.terminal_custom, installed).1
    };
    let (desc, colour) = match &problem {
        Some(problem) => (problem.as_str(), tokens.warning),
        None => (
            "What commands typed after > run in. Ctrl+Enter opens the same shell in Windows Terminal.",
            tokens.text_secondary,
        ),
    };
    settings_row(
        ui,
        "terminal-shell",
        RowText::new("Terminal shell").desc(desc, colour),
        false,
        |ui| {
            let options: Vec<&str> = labels.iter().map(String::as_str).collect();
            let current = SHELL_NAMES
                .iter()
                .position(|name| *name == search.terminal_shell);
            if let Some(index) = choice::dropdown(ui, "terminal-shell", &options, current) {
                if usable(SHELL_NAMES[index]) {
                    send(&|next| next.terminal_shell = SHELL_NAMES[index].to_string());
                }
            }
        },
    );

    if custom {
        let (desc, colour) = match shell::custom_problem(&search.terminal_custom) {
            Some(problem) => (
                format!("{problem} Until then WSL bash is used."),
                tokens.warning,
            ),
            None => (
                "The command goes where {cmd} is, and the folder where {cwd} is.".to_string(),
                tokens.text_secondary,
            ),
        };
        settings_row(
            ui,
            "terminal-custom",
            RowText::new("Custom command").desc(&desc, colour),
            false,
            |ui| {
                let mut value = search.terminal_custom.clone();
                if input::text(ui, "terminal-custom", &mut value, Face::Mono, 360.0).changed() {
                    send(&|next| next.terminal_custom = value.clone());
                }
            },
        );
    }

    let (desc, colour) = if web::is_http(&search.web_url) {
        (
            "What ? searches. {query} is where the words go.",
            tokens.text_secondary,
        )
    } else {
        (
            "It must start with https:// or http://. Google is used until then.",
            tokens.warning,
        )
    };
    settings_row(
        ui,
        "web-url",
        RowText::new("Web search address").desc(desc, colour),
        false,
        |ui| {
            let mut value = search.web_url.clone();
            if input::text(ui, "web-url", &mut value, Face::Mono, 360.0).changed() {
                send(&|next| next.web_url = value.clone());
            }
        },
    );

    settings_row(
        ui,
        "terminal-history",
        RowText::new("Remember terminal history").desc(
            "Keeps the last 100 commands run from the palette in terminal_history.json, on this PC only.",
            tokens.text_secondary,
        ),
        false,
        |ui| {
            let mut on = search.terminal_history;
            if toggle::toggle(ui, &mut on).changed() {
                send(&|next| next.terminal_history = on);
            }
        },
    );
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

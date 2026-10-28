use std::sync::Arc;

use crate::core::config::SearchConfig;
use crate::core::theme::{self, Tokens};
use crate::core::tips::TipsConfig;
use crate::core::ui_bridge::{HostChannel, HostRequest, HostSetting, UiSnapshot};
use crate::search::ask_ai;
use crate::search::providers::shell::{self, Shell, SHELL_NAMES};
use crate::search::providers::web;
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::hotkey_dialog::{self, Target};
use crate::ui::widgets::input::{self, Face};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::{choice, text, toggle};

/// The palette's own hotkey on the Hotkeys page and in config.json.
const PALETTE_ID: &str = "wincraft.palette";

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
                "How WinCraft starts and searches.",
                |_| {},
            );
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.y = 0.0;

            settings_row(
                ui,
                "autostart",
                RowText::new("Start with Windows").desc(
                    snapshot
                        .autostart_note
                        .as_deref()
                        .unwrap_or("Launch when you sign in, with a logon task in Task Scheduler."),
                    tokens.text_secondary,
                ),
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

            let (verdict, colour) = if snapshot.palette_hotkey_registered {
                ("registered", tokens.success)
            } else {
                ("taken by another app", tokens.warning)
            };
            let mut change = false;
            settings_row(
                ui,
                "palette-hotkey",
                RowText::new("Palette hotkey").desc(verdict, colour),
                false,
                |ui| {
                    change = hotkey_dialog::trigger(ui, &snapshot.palette_hotkey).clicked();
                },
            );
            if change {
                if let Some(target) = Target::find(&snapshot.shortcuts, PALETTE_ID) {
                    state.open_hotkey(target);
                }
            }

            tips_rows(ui, &snapshot.tips, &mut state.tips_reset, to_host);

            file_row(
                ui,
                "config-file",
                "Config file",
                &snapshot.config_path,
                to_host,
            );
            file_row(ui, "log-file", "Log file", &snapshot.log_path, to_host);
            detailed_log_row(ui, to_host);

            page::section_header(ui, "Palette search");
            search_rows(ui, &snapshot.search, to_host);
            usage_rows(ui, &snapshot.search, &mut state.usage_cleared, to_host);
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
            "What Search the web opens. {query} is where the words go.",
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

    ask_ai_rows(ui, search, to_host);

    settings_row(
        ui,
        "everything",
        RowText::new("File search: Everything, then Windows index").desc(
            "Finds files and folders on every disk while you type, and after the . prefix. Uses Everything (voidtools) when it is running.",
            tokens.text_secondary,
        ),
        false,
        |ui| {
            let mut on = search.everything;
            if toggle::toggle(ui, &mut on).changed() {
                send(&|next| next.everything = on);
            }
        },
    );

    settings_row(
        ui,
        "file-search-fallback",
        RowText::new("Use the Windows index when Everything is not running").desc(
            "Slower than Everything, and it only finds what Windows Search has indexed. Off means no file search without Everything.",
            tokens.text_secondary,
        ),
        false,
        |ui| {
            let mut on = search.file_search_fallback;
            if toggle::toggle(ui, &mut on).changed() {
                send(&|next| next.file_search_fallback = on);
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

/// The AI website Ask AI opens, and the address of a custom one.
fn ask_ai_rows(ui: &mut egui::Ui, search: &SearchConfig, to_host: &Arc<HostChannel>) {
    let send = |change: &dyn Fn(&mut SearchConfig)| {
        let mut next = search.clone();
        change(&mut next);
        to_host.send(HostRequest::SetHostSetting(HostSetting::Search(next)));
    };
    let tokens = Tokens::get(ui.ctx());
    settings_row(
        ui,
        "ask-ai-site",
        RowText::new("Ask AI site").desc(
            "Ask AI and the ? prefix open this website in your browser with your question. WinCraft sends nothing itself.",
            tokens.text_secondary,
        ),
        false,
        |ui| {
            let current = ask_ai::SITE_NAMES
                .iter()
                .position(|name| *name == search.ask_ai_site);
            if let Some(index) =
                choice::dropdown(ui, "ask-ai-site", &ask_ai::SITE_NAMES, current)
            {
                send(&|next| next.ask_ai_site = ask_ai::SITE_NAMES[index].to_string());
            }
        },
    );
    if search.ask_ai_site != ask_ai::CUSTOM {
        return;
    }
    let (desc, colour) = match ask_ai::custom_problem(&search.ask_ai_custom) {
        Some(problem) => (
            format!("{problem} Claude is used until then."),
            tokens.warning,
        ),
        None => (
            "{query} is where the question goes.".to_string(),
            tokens.text_secondary,
        ),
    };
    settings_row(
        ui,
        "ask-ai-custom",
        RowText::new("Custom AI address").desc(&desc, colour),
        false,
        |ui| {
            let mut value = search.ask_ai_custom.clone();
            if input::text(ui, "ask-ai-custom", &mut value, Face::Mono, 360.0).changed() {
                send(&|next| next.ask_ai_custom = value.clone());
            }
        },
    );
}

/// How long "Cleared" shows after the button was clicked.
const CLEARED_NOTE: std::time::Duration = std::time::Duration::from_secs(3);

/// Ranking by use: the switch, and the button that forgets every pick.
fn usage_rows(
    ui: &mut egui::Ui,
    search: &SearchConfig,
    cleared: &mut Option<std::time::Instant>,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    settings_row(
        ui,
        "rank-by-use",
        RowText::new("Learn from my picks").desc(
            "Apps, commands, windows and folders you pick often move up, and the one you pick most for what you type is shown as the best match. On this PC only.",
            tokens.text_secondary,
        ),
        false,
        |ui| {
            let mut on = search.rank_by_use;
            if toggle::toggle(ui, &mut on).changed() {
                let mut next = search.clone();
                next.rank_by_use = on;
                to_host.send(HostRequest::SetHostSetting(HostSetting::Search(next)));
            }
        },
    );
    let showing = cleared.is_some_and(|at| at.elapsed() < CLEARED_NOTE);
    if !showing {
        *cleared = None;
    }
    let desc = if showing {
        "Cleared. Results are ranked by how well they match until you pick some again."
    } else {
        "Forgets which results you picked. They are kept in palette_usage.json."
    };
    settings_row(
        ui,
        "usage-history",
        RowText::new("Usage history").desc(desc, tokens.text_secondary),
        false,
        |ui| {
            if button::button(ui, "Clear usage history", Kind::Secondary).clicked() {
                to_host.send(HostRequest::ClearPaletteUsage);
                *cleared = Some(std::time::Instant::now());
                ui.ctx().request_repaint_after(CLEARED_NOTE);
            }
        },
    );
    if showing {
        ui.ctx().request_repaint_after(CLEARED_NOTE);
    }
}

/// How long "Done" shows after the button was clicked.
const RESET_NOTE: std::time::Duration = std::time::Duration::from_secs(3);

/// The switch for the small tips, and the button that starts them over.
fn tips_rows(
    ui: &mut egui::Ui,
    tips: &TipsConfig,
    reset: &mut Option<std::time::Instant>,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    settings_row(
        ui,
        "show-tips",
        RowText::new("Show tips").desc(
            "Short hints in the palette and on empty pages that explain a feature. Each one retires after a few shows.",
            tokens.text_secondary,
        ),
        false,
        |ui| {
            let mut on = tips.show;
            if toggle::toggle(ui, &mut on).changed() {
                let mut next = tips.clone();
                next.show = on;
                to_host.send(HostRequest::SetHostSetting(HostSetting::Tips(next)));
            }
        },
    );
    let showing = reset.is_some_and(|at| at.elapsed() < RESET_NOTE);
    if !showing {
        *reset = None;
    }
    let desc = if showing {
        "Done. Every tip will show again, if tips are on."
    } else {
        "Brings back tips you dismissed or have already seen."
    };
    settings_row(
        ui,
        "show-tips-again",
        RowText::new("Tips seen").desc(desc, tokens.text_secondary),
        false,
        |ui| {
            if button::button(ui, "Show all tips again", Kind::Secondary).clicked() {
                let mut next = tips.clone();
                next.reset();
                to_host.send(HostRequest::SetHostSetting(HostSetting::Tips(next)));
                *reset = Some(std::time::Instant::now());
                ui.ctx().request_repaint_after(RESET_NOTE);
            }
        },
    );
    if showing {
        ui.ctx().request_repaint_after(RESET_NOTE);
    }
}

/// The detailed log's switch. Its state is the host's switch itself, so it
/// is right however it was changed: here, from the palette or the tray.
fn detailed_log_row(ui: &mut egui::Ui, to_host: &Arc<HostChannel>) {
    let tokens = Tokens::get(ui.ctx());
    settings_row(
        ui,
        "detailed-log",
        RowText::new("Detailed log").desc(
            "Records everything WinCraft does, to find a bug: keys, window titles and palette text. The files stay on this PC. Turn it off when you are done.",
            tokens.text_secondary,
        ),
        false,
        |ui| {
            let mut on = crate::core::detailed::on();
            if toggle::toggle(ui, &mut on).changed() {
                to_host.send(HostRequest::SetHostSetting(HostSetting::DetailedLog(on)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeChoice;

    /// Draws the tips rows a few frames, with the note showing or not.
    fn draw(tips: &TipsConfig, reset: &mut Option<std::time::Instant>) {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(
            &ctx,
            crate::core::config::ThemeName::Classic,
            ThemeChoice::Dark,
        );
        let bridge = crate::core::ui_bridge::create();
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                tips_rows(ui, tips, reset, &bridge.to_host);
            });
            output.textures_delta.clear();
        }
        assert!(bridge.host_rx.try_recv().is_err(), "drawing sends nothing");
    }

    #[test]
    fn the_tips_rows_draw_with_tips_on_and_off_and_after_the_reset_button() {
        draw(&TipsConfig::default(), &mut None);
        let off = TipsConfig {
            show: false,
            ..TipsConfig::default()
        };
        draw(&off, &mut None);
        let mut reset = Some(std::time::Instant::now());
        draw(&TipsConfig::default(), &mut reset);
        assert!(reset.is_some(), "the note stays for its three seconds");
        let mut old = Some(std::time::Instant::now() - RESET_NOTE);
        draw(&TipsConfig::default(), &mut old);
        assert!(old.is_none(), "the note is gone after them");
    }
}

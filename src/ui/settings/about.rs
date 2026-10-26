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
use crate::ui::widgets::{text, toggle};

/// The release notes of a newer version, drawn like a plugin's README.
#[derive(Default)]
pub struct Update {
    notes: Readme,
    /// The notes of the installed version.
    installed: Readme,
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

            if let Some(notes) = &update.installed_notes {
                page::section_header(ui, &format!("What's new in {}", snapshot.version));
                ui.spacing_mut().item_spacing.y = 8.0;
                state.update.installed.boxed(ui, notes);
                ui.spacing_mut().item_spacing.y = 0.0;
            }

            ui.add_space(20.0);
            text::group_header(ui, "For plugin authors");
            ui.add_space(6.0);
            authors_note(ui);
        });
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
    if let Some((label, fraction)) = progress_step(&view.status) {
        ui.add_space(8.0);
        text::secondary(ui, &label, tokens.text_secondary);
        ui.add_space(6.0);
        progress_bar(ui, fraction);
    }
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

/// The line above the update's progress bar and how full the bar is: None
/// for no update under way, and a None fraction when the server gave no size.
fn progress_step(status: &UpdateStatus) -> Option<(String, Option<f32>)> {
    match status {
        // The checksum is checked once the last byte is in.
        UpdateStatus::Downloading(Some(100)) => {
            Some(("Checking the download\u{2026}".to_string(), Some(1.0)))
        }
        UpdateStatus::Downloading(Some(percent)) => Some((
            format!("Step 1 of 2 \u{00B7} Downloading\u{2026} {percent}%"),
            Some(f32::from(*percent) / 100.0),
        )),
        UpdateStatus::Downloading(None) => {
            Some(("Step 1 of 2 \u{00B7} Downloading\u{2026}".to_string(), None))
        }
        UpdateStatus::Installing => Some((
            "Step 2 of 2 \u{00B7} Installing \u{2014} WinCraft restarts in a moment".to_string(),
            Some(1.0),
        )),
        _ => None,
    }
}

/// A 4 px bar across the row: accent fill on a border-coloured track. With
/// no size known, a short segment slides along so it still looks alive.
fn progress_bar(ui: &mut egui::Ui, fraction: Option<f32>) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, tokens.border);
    let fill = match fraction {
        Some(fraction) => {
            let mut fill = rect;
            fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
            fill
        }
        None => {
            let time = ui.input(|input| input.time) as f32;
            let width = rect.width() * 0.25;
            let left = rect.left() + (time * 0.6).fract() * (rect.width() + width) - width;
            ui.ctx().request_repaint();
            egui::Rect::from_x_y_ranges(left..=left + width, rect.y_range()).intersect(rect)
        }
    };
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, 2.0, tokens.accent);
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
    #[test]
    fn the_page_no_longer_lists_hotkeys() {
        use crate::core::config::{ThemeChoice, ThemeName};
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot {
            palette_hotkey: "Win+Alt+P".to_string(),
            ..UiSnapshot::default()
        };
        let mut state = SettingsState::default();
        let mut words = String::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(800.0, 900.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                show(ui, &snapshot, &mut state, &bridge.to_host);
            });
            words.clear();
            for clipped in &output.shapes {
                if let egui::Shape::Text(shape) = &clipped.shape {
                    words.push_str(&shape.galley.text());
                    words.push('
');
                }
            }
        }
        assert!(words.contains("About"), "the page drew: {words}");
        assert!(!words.contains("Hotkeys"), "{words}");
        assert!(!words.contains("Open the palette"), "{words}");
    }

    #[test]
    fn the_progress_bar_follows_the_update_steps() {
        assert_eq!(progress_step(&UpdateStatus::Idle), None);
        assert_eq!(progress_step(&UpdateStatus::Checking), None);
        assert_eq!(progress_step(&UpdateStatus::Error("x".into())), None);
        let (label, fraction) = progress_step(&UpdateStatus::Downloading(Some(35))).unwrap();
        assert!(
            label.starts_with("Step 1 of 2") && label.ends_with("35%"),
            "{label}"
        );
        assert_eq!(fraction, Some(0.35));
        let (_, fraction) = progress_step(&UpdateStatus::Downloading(None)).unwrap();
        assert_eq!(fraction, None);
        let (label, fraction) = progress_step(&UpdateStatus::Downloading(Some(100))).unwrap();
        assert!(label.starts_with("Checking the download"), "{label}");
        assert_eq!(fraction, Some(1.0));
        let (label, _) = progress_step(&UpdateStatus::Installing).unwrap();
        assert!(label.starts_with("Step 2 of 2"), "{label}");
    }
}

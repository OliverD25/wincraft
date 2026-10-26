use std::sync::Arc;

use crate::core::config::ThemeChoice;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest, HostSetting, UiSnapshot};
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::{choice, text, theme_tile};

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page::page_header(
                ui,
                "Themes",
                None,
                "How WinCraft looks. Point at a theme to try it.",
                |_| {},
            );
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            theme_rows(ui, snapshot, state, to_host);
        });
}

/// The theme tiles and the mode. Pointing at a tile shows that theme in
/// every WinCraft window; a click keeps it.
fn theme_rows(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    settings_row(
        ui,
        "theme",
        RowText::new("Theme").desc(
            "Point at a theme to try it in every WinCraft window. Click it to keep it.",
            tokens.text_secondary,
        ),
        false,
        |_| {},
    );
    ui.add_space(12.0);
    if let Some(name) = theme_tile::picker(ui, snapshot.theme, &mut state.theme_preview) {
        to_host.send(HostRequest::SetHostSetting(HostSetting::Theme(name)));
    }
    if theme::windows_high_contrast() {
        ui.add_space(8.0);
        text::secondary(
            ui,
            "Windows high contrast is on, so WinCraft uses High contrast.",
            tokens.warning,
        );
    }
    ui.add_space(12.0);

    settings_row(
        ui,
        "mode",
        RowText::new("Mode").desc(
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
                .position(|choice| *choice == snapshot.mode);
            if let Some(index) = choice::segmented(ui, &labels, current) {
                to_host.send(HostRequest::SetHostSetting(HostSetting::Mode(
                    ThemeChoice::ALL[index],
                )));
            }
        },
    );
}

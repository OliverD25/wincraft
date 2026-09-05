use std::sync::Arc;

use crate::core::config::ThemeChoice;
use crate::core::hotkeys;
use crate::core::ui_bridge::{HostChannel, HostRequest, HostSetting, UiSnapshot};
use crate::ui::settings::SettingsState;
use crate::ui::widgets::hotkey_capture;

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    ui.heading("General");
    ui.add_space(12.0);

    let mut autostart = snapshot.start_with_windows;
    if ui.checkbox(&mut autostart, "Start with Windows").changed() {
        to_host.send(HostRequest::SetHostSetting(HostSetting::StartWithWindows(
            autostart,
        )));
    }
    ui.label(
        egui::RichText::new("Adds WinCraft to the Run key for your account only.")
            .weak()
            .small(),
    );
    ui.add_space(16.0);

    ui.horizontal(|ui| {
        ui.label("Theme");
        for choice in ThemeChoice::ALL {
            if ui
                .selectable_label(snapshot.theme == choice, choice.label())
                .clicked()
                && snapshot.theme != choice
            {
                to_host.send(HostRequest::SetHostSetting(HostSetting::Theme(choice)));
            }
        }
    });
    ui.add_space(16.0);

    ui.label("Palette hotkey");
    ui.horizontal(|ui| {
        let capturing = state.capture_owner.as_deref() == Some(PALETTE_OWNER);
        let label = if capturing {
            hotkey_capture::describe(hotkey_capture::live_modifiers())
        } else {
            snapshot.palette_hotkey.clone()
        };
        if ui.selectable_label(capturing, label).clicked() {
            state.begin_capture(PALETTE_OWNER.to_string());
        }
        if !snapshot.palette_hotkey_registered {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "not registered \u{2014} another app holds it",
            );
        }
    });
    if state.capture_owner.as_deref() == Some(PALETTE_OWNER) {
        ui.label(
            egui::RichText::new("Press the combination you want, or Esc to cancel.")
                .weak()
                .small(),
        );
        if let Some(hotkey) = hotkey_capture::take_result() {
            to_host.send(HostRequest::SetHostSetting(HostSetting::PaletteHotkey(
                hotkeys::format(hotkey),
            )));
            state.end_capture();
        }
        if hotkey_capture::take_cancelled() {
            state.end_capture();
        }
    }

    ui.add_space(20.0);
    ui.separator();
    ui.add_space(12.0);

    file_row(ui, "Config file", &snapshot.config_path, to_host);
    file_row(ui, "Log file", &snapshot.log_path, to_host);
}

const PALETTE_OWNER: &str = "host/palette";

fn file_row(ui: &mut egui::Ui, label: &str, path: &std::path::Path, to_host: &Arc<HostChannel>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(path.display().to_string())
                .monospace()
                .small(),
        );
        if ui.button("Open").clicked() {
            to_host.send(HostRequest::OpenPath(path.to_path_buf()));
        }
    });
}

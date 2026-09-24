use std::sync::Arc;

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{CommandId, HostChannel, HostRequest, PaletteEntry, UiSnapshot};
use crate::ui::fuzzy;

pub const WIDTH: f32 = 680.0;
pub const HEIGHT: f32 = 420.0;
const ROW_HEIGHT: f32 = 34.0;

#[derive(Default)]
pub struct Palette {
    pub query: String,
    selected: usize,
    focus_wanted: bool,
    focused_once: bool,
    matches: Vec<usize>,
}

impl Palette {
    pub fn opened(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.focus_wanted = true;
        self.focused_once = false;
        self.matches.clear();
    }

    fn rank(&mut self, snapshot: &UiSnapshot) {
        self.matches = if self.query.trim().is_empty() {
            (0..snapshot.commands.len()).collect()
        } else {
            let query = self.query.trim();
            let mut scored: Vec<(i32, usize)> = snapshot
                .commands
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    fuzzy::score_command(query, &entry.group, &entry.label)
                        .map(|points| (points, index))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            scored.into_iter().map(|(_, index)| index).collect()
        };
        if self.selected >= self.matches.len() {
            self.selected = self.matches.len().saturating_sub(1);
        }
    }

    /// Returns true when the palette should hide.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
    ) -> bool {
        self.rank(snapshot);

        let ctx = ui.ctx().clone();
        let mut hide = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !self.matches.is_empty() {
            self.selected = (self.selected + 1) % self.matches.len();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !self.matches.is_empty() {
            self.selected = (self.selected + self.matches.len() - 1) % self.matches.len();
        }
        let mut run = if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.chosen(snapshot)
        } else {
            None
        };

        let tokens = Tokens::get(&ctx);
        egui::Frame::NONE
            .fill(tokens.elevated_bg)
            .stroke(theme::stroke(&ctx, 1.0, tokens.border))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                let edit = egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search WinCraft commands\u{2026}")
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Heading);
                let response = ui.add(edit);
                if self.focus_wanted {
                    response.request_focus();
                    self.focus_wanted = false;
                }

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(2.0);

                if self.matches.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(24.0);
                        ui.weak("No command matches that.");
                    });
                    return;
                }

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let rows = self.matches.clone();
                        let grouped = self.query.trim().is_empty();
                        let mut last_group = String::new();
                        for (row, index) in rows.into_iter().enumerate() {
                            let entry = &snapshot.commands[index];
                            if grouped && entry.group != last_group {
                                last_group.clone_from(&entry.group);
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(&entry.group).weak().small());
                            }
                            if row_widget(ui, entry, row == self.selected) {
                                self.selected = row;
                                run = Some(entry.id);
                            }
                        }
                    });
            });

        // Focus arrives a frame or two after the window is shown, so hiding on
        // "not focused" before it has ever been focused would close the palette
        // in the same breath as opening it.
        match ctx.input(|i| i.viewport().focused) {
            Some(true) => self.focused_once = true,
            Some(false) if self.focused_once => hide = true,
            _ => {}
        }
        if let Some(id) = run {
            to_host.send(HostRequest::RunCommand(id));
            hide = true;
        }
        hide
    }

    fn chosen(&self, snapshot: &UiSnapshot) -> Option<CommandId> {
        let index = *self.matches.get(self.selected)?;
        snapshot.commands.get(index).map(|entry| entry.id)
    }
}

fn row_widget(ui: &mut egui::Ui, entry: &PaletteEntry, selected: bool) -> bool {
    let size = egui::vec2(ui.available_width(), ROW_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            ui.visuals().selection.bg_fill,
        );
        response.scroll_to_me(None);
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            ui.visuals().widgets.hovered.bg_fill,
        );
    }

    let text = rect.shrink2(egui::vec2(10.0, 0.0));
    ui.painter().text(
        text.left_center(),
        egui::Align2::LEFT_CENTER,
        &entry.label,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    if !entry.hint.is_empty() {
        ui.painter().text(
            text.right_center(),
            egui::Align2::RIGHT_CENTER,
            &entry.hint,
            egui::TextStyle::Small.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
    }
    response.clicked()
}

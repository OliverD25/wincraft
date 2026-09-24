use std::sync::{Arc, Mutex};

use egui::{pos2, vec2, Align2, Id, LayerId, Margin, Order, Rect, Response, Sense, Ui};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{ArrangeGroup, ArrangeSnapshot, HostChannel, HostRequest};
use crate::ui::settings;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::{choice, focus, row, text};

const TITLE: &str = "Arrange windows";
const ROW_HEIGHT: f32 = 48.0;
const BUTTON_BAR: f32 = 56.0;

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-arrange")
}

struct Shared {
    snapshot: Option<ArrangeSnapshot>,
    group: usize,
    selected: Option<isize>,
    to_host: Arc<HostChannel>,
    closed: bool,
    window: isize,
}

/// The mouse way to set a taskbar group's thumbnail order: one row per
/// window in the group's order, dragged into place.
pub struct Arrange {
    shared: Arc<Mutex<Shared>>,
    palette_hwnd: isize,
}

impl Arrange {
    pub fn new(to_host: Arc<HostChannel>, palette_hwnd: isize) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                snapshot: None,
                group: 0,
                selected: None,
                to_host,
                closed: false,
                window: 0,
            })),
            palette_hwnd,
        }
    }

    pub fn open(&self, snapshot: ArrangeSnapshot) {
        self.update(snapshot);
        if let Ok(mut shared) = self.shared.lock() {
            shared.closed = false;
            shared.window = 0;
        }
    }

    /// Takes a fresh list and keeps the same program's group on screen.
    pub fn update(&self, snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let current = shared
            .snapshot
            .as_ref()
            .and_then(|old| old.groups.get(shared.group))
            .map(|group| group.exe.clone());
        shared.group = current
            .and_then(|exe| snapshot.groups.iter().position(|group| group.exe == exe))
            .unwrap_or(0);
        shared.snapshot = Some(snapshot);
    }

    pub fn was_closed(&self) -> bool {
        self.shared
            .lock()
            .map(|shared| shared.closed)
            .unwrap_or(false)
    }

    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.closed = false;
            shared.selected = None;
            shared.window = 0;
        }
    }

    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let palette_hwnd = self.palette_hwnd;
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([520.0, 600.0])
            .with_min_inner_size([420.0, 360.0]);
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }

        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let Ok(mut shared) = shared.lock() else {
                return;
            };
            let tokens = Tokens::get(ui.ctx());
            if shared.window == 0 {
                shared.window = settings::find_window(TITLE, palette_hwnd);
            }
            if shared.window != 0 {
                settings::sync_title_bar(shared.window, tokens.dark);
            }

            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(tokens.window_bg)
                        .inner_margin(Margin::same(24)),
                )
                .show(ui, |ui| page(ui, &mut shared));

            if ui.ctx().input(|i| i.viewport().close_requested()) {
                shared.closed = true;
            }
        });
    }
}

fn page(ui: &mut Ui, shared: &mut Shared) {
    let tokens = Tokens::get(ui.ctx());
    text::title(ui, TITLE, None);
    ui.add_space(6.0);
    text::page_description(
        ui,
        "Drag a window to its place in the taskbar group. The order is saved and comes back after a reboot.",
    );
    ui.add_space(16.0);

    let Some(snapshot) = shared.snapshot.clone() else {
        return;
    };
    if snapshot.groups.is_empty() {
        empty_state(
            ui,
            "No windows to arrange",
            "None of the watched programs has a window open.",
        );
        return;
    }
    let group_index = shared.group.min(snapshot.groups.len() - 1);
    if snapshot.groups.len() > 1 {
        let labels: Vec<&str> = snapshot
            .groups
            .iter()
            .map(|group| group.label.as_str())
            .collect();
        if let Some(chosen) = choice::dropdown(ui, "arrange-group", &labels, Some(group_index)) {
            shared.group = chosen;
            shared.selected = None;
        }
        ui.add_space(12.0);
    } else {
        text::group_header(ui, &snapshot.groups[group_index].label);
        ui.add_space(4.0);
    }
    let group = &snapshot.groups[group_index];
    let order: Vec<isize> = group.windows.iter().map(|window| window.hwnd).collect();

    let list_height = (ui.available_height() - BUTTON_BAR).max(ROW_HEIGHT);
    let mut wanted: Option<Vec<isize>> = None;
    egui::ScrollArea::vertical()
        .max_height(list_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (index, window) in group.windows.iter().enumerate() {
                let selected = shared.selected == Some(window.hwnd);
                let response = list_row(ui, &window.label, &window.detail, selected);
                if response.clicked() {
                    shared.selected = Some(window.hwnd);
                }
                response.dnd_set_drag_payload(index);
                if response.dragged() {
                    drag_ghost(ui, &window.label);
                }
                if let Some(from) = response.dnd_hover_payload::<usize>() {
                    let slot = drop_slot(ui, &response, index);
                    if *from != index {
                        let y = if slot == index {
                            response.rect.top()
                        } else {
                            response.rect.bottom()
                        };
                        ui.painter().hline(
                            response.rect.x_range(),
                            y,
                            theme::stroke(ui.ctx(), 2.0, tokens.accent),
                        );
                    }
                }
                if let Some(from) = response.dnd_release_payload::<usize>() {
                    let slot = drop_slot(ui, &response, index);
                    let moved = move_to_slot(&order, *from, slot);
                    if moved != order {
                        shared.selected = Some(order[*from]);
                        wanted = Some(moved);
                    }
                }
            }
        });
    if !group.note.is_empty() {
        ui.add_space(8.0);
        text::secondary(ui, &group.note, tokens.text_secondary);
    }

    ui.add_space(12.0);
    let position = shared
        .selected
        .and_then(|hwnd| order.iter().position(|h| *h == hwnd));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let up = position.is_some_and(|i| i > 0);
        let down = position.is_some_and(|i| i + 1 < order.len());
        if ui
            .add_enabled_ui(up, |ui| button::button(ui, "Move up", Kind::Secondary))
            .inner
            .clicked()
        {
            if let Some(i) = position {
                wanted = Some(move_to_slot(&order, i, i - 1));
            }
        }
        if ui
            .add_enabled_ui(down, |ui| button::button(ui, "Move down", Kind::Secondary))
            .inner
            .clicked()
        {
            if let Some(i) = position {
                wanted = Some(move_to_slot(&order, i, i + 2));
            }
        }
        if let Some(restore) = snapshot.restore {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button::button(ui, "Restore layout", Kind::Secondary).clicked() {
                    shared.to_host.send(HostRequest::RunCommand(restore));
                }
            });
        }
    });

    if let Some(order) = wanted {
        apply_locally(shared, group_index, &order);
        shared.to_host.send(HostRequest::ReorderGroup {
            plugin: snapshot.plugin.clone(),
            exe: group.exe.clone(),
            order,
        });
    }
}

/// Shows the new order straight away; the host's own list follows a moment
/// later and replaces it.
fn apply_locally(shared: &mut Shared, group_index: usize, order: &[isize]) {
    let Some(group) = shared
        .snapshot
        .as_mut()
        .and_then(|snapshot| snapshot.groups.get_mut(group_index))
    else {
        return;
    };
    reorder_group(group, order);
}

fn reorder_group(group: &mut ArrangeGroup, order: &[isize]) {
    group.windows.sort_by_key(|window| {
        order
            .iter()
            .position(|h| *h == window.hwnd)
            .unwrap_or(usize::MAX)
    });
}

/// The handout's list row: 48 high, a 1 px rule below, the hover tint, and
/// the selection tint with the accent rule when selected.
fn list_row(ui: &mut Ui, label: &str, detail: &str, selected: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let body = rect.shrink2(vec2(0.0, 2.0));
    if selected {
        row::paint_selection(ui, body);
    } else if response.hovered() || response.dragged() {
        ui.painter().rect_filled(body, 4, tokens.hover_bg);
    }
    let painter = ui.painter();
    let opacity = if response.dragged() { 0.5 } else { 1.0 };
    let text_x = rect.left() + 12.0;
    let label_galley = painter.layout_job(text::job(
        label,
        theme::lora(14.0),
        theme::with_alpha(tokens.text_primary, opacity),
        None,
    ));
    let detail_galley = painter.layout_job(text::job(
        detail,
        theme::lora(12.0),
        theme::with_alpha(tokens.text_secondary, opacity),
        None,
    ));
    let stack = label_galley.size().y
        + if detail.is_empty() {
            0.0
        } else {
            detail_galley.size().y + 2.0
        };
    let top = rect.center().y - stack / 2.0;
    painter.galley(pos2(text_x, top), label_galley.clone(), tokens.text_primary);
    if !detail.is_empty() {
        painter.galley(
            pos2(text_x, top + label_galley.size().y + 2.0),
            detail_galley,
            tokens.text_secondary,
        );
    }
    let rule = theme::snap(1.0, ui.ctx().pixels_per_point());
    painter.rect_filled(
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - rule), rect.right_bottom()),
        0,
        tokens.border,
    );
    response.on_hover_cursor(egui::CursorIcon::Grab)
}

/// The row being dragged follows the pointer as a small label.
fn drag_ghost(ui: &Ui, label: &str) {
    let tokens = Tokens::get(ui.ctx());
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return;
    };
    let painter = ui
        .ctx()
        .layer_painter(LayerId::new(Order::Tooltip, Id::new("arrange-ghost")));
    let galley = painter.layout_no_wrap(label.to_string(), theme::lora(14.0), tokens.text_primary);
    let rect = Rect::from_min_size(pointer + vec2(12.0, 8.0), galley.size() + vec2(20.0, 12.0));
    painter.rect_filled(rect, 4, tokens.elevated_bg);
    painter.rect_stroke(
        rect,
        4,
        theme::stroke(ui.ctx(), 1.0, tokens.border),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.left_center() + vec2(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        theme::lora(14.0),
        tokens.text_primary,
    );
}

/// Where a drop on row `index` lands: before it when the pointer is in its
/// upper half, after it otherwise.
fn drop_slot(ui: &Ui, response: &Response, index: usize) -> usize {
    let y = ui
        .ctx()
        .pointer_interact_pos()
        .map(|pointer| pointer.y)
        .unwrap_or(response.rect.center().y);
    if y < response.rect.center().y {
        index
    } else {
        index + 1
    }
}

/// Moves the item at `from` so it lands in front of what is now at `slot`
/// (`slot == len` means the end).
fn move_to_slot(order: &[isize], from: usize, slot: usize) -> Vec<isize> {
    let mut moved = order.to_vec();
    if from >= moved.len() {
        return moved;
    }
    let item = moved.remove(from);
    let at = if slot > from { slot - 1 } else { slot }.min(moved.len());
    moved.insert(at, item);
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::ArrangeWindow;

    #[test]
    fn moving_to_a_slot_counts_slots_in_the_old_order() {
        let order = [10, 20, 30, 40];
        assert_eq!(move_to_slot(&order, 0, 2), [20, 10, 30, 40]);
        assert_eq!(move_to_slot(&order, 3, 0), [40, 10, 20, 30]);
        assert_eq!(move_to_slot(&order, 1, 4), [10, 30, 40, 20]);
        assert_eq!(move_to_slot(&order, 2, 2), order);
        assert_eq!(move_to_slot(&order, 2, 3), order);
        assert_eq!(move_to_slot(&order, 9, 0), order);
    }

    #[test]
    fn a_group_is_redrawn_in_the_new_order_at_once() {
        let window = |hwnd| ArrangeWindow {
            hwnd,
            label: hwnd.to_string(),
            detail: String::new(),
        };
        let mut group = ArrangeGroup {
            exe: "chrome.exe".to_string(),
            label: "Chrome".to_string(),
            windows: vec![window(1), window(2), window(3)],
            note: String::new(),
        };
        reorder_group(&mut group, &[3, 1, 2]);
        let order: Vec<isize> = group.windows.iter().map(|w| w.hwnd).collect();
        assert_eq!(order, [3, 1, 2]);
    }
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::text::TextWrapping;
use egui::{pos2, vec2, Rect, Response, Sense, StrokeKind, Ui, UiBuilder, Vec2};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE,
};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeSnapshot, HostChannel, HostRequest,
};
use crate::ui::dwm_thumbs::{self, Placement, Thumbnail};
use crate::ui::settings;
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::{choice, focus, text};

/// Unique, so the strip's window can be found among the UI thread's windows.
const TITLE: &str = "WinCraft Arrange";

const PADDING: f32 = 12.0;
/// 16:9, small enough that a group of 15+ windows fits on one screen.
const THUMB: Vec2 = Vec2::new(144.0, 81.0);
const CARD_INSET: f32 = 4.0;
const NAME_HEIGHT: f32 = 16.0;
const CARD: Vec2 = Vec2::new(
    THUMB.x + 2.0 * CARD_INSET,
    CARD_INSET + THUMB.y + CARD_INSET + NAME_HEIGHT + CARD_INSET,
);
const CARD_GAP: f32 = 8.0;
const HEADER: f32 = 15.0;
const HEADER_GAP: f32 = 6.0;
const ROW_GAP: f32 = 12.0;
const EMPTY_PANEL: Vec2 = Vec2::new(520.0, 150.0);
/// The note under the rows when more than one desktop is shown.
const FOOTER: f32 = 28.0;
/// The group switcher above the rows, when there is more than one group.
const SWITCHER: f32 = 44.0;
/// Rows that fit exactly can round to a pixel too tall and grow a scroll bar.
const SLACK: f32 = 4.0;
/// Space left between the strip and the taskbar.
const TASKBAR_GAP: f32 = 12.0;

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-arrange")
}

/// One desktop's windows, in thumbnail order.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub title: String,
    pub desktop: String,
    /// Indexes into the group's windows.
    pub members: Vec<usize>,
}

/// The windows of one program, one row per desktop: the current desktop
/// first, titled "This desktop" and shown even when empty, then the others
/// in Task View order. A window with no known desktop is on this one.
pub fn rows(group: &ArrangeGroup, desktops: &[ArrangeDesktop]) -> Vec<Row> {
    let current = desktops
        .iter()
        .find(|desktop| desktop.current)
        .map(|desktop| desktop.id.clone())
        .unwrap_or_default();
    let known = |id: &str| desktops.iter().any(|desktop| desktop.id == id);
    let mut rows = vec![Row {
        title: "This desktop".to_string(),
        desktop: current.clone(),
        members: group
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.desktop == current || !known(&window.desktop))
            .map(|(index, _)| index)
            .collect(),
    }];
    for desktop in desktops.iter().filter(|desktop| !desktop.current) {
        let members: Vec<usize> = group
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.desktop == desktop.id)
            .map(|(index, _)| index)
            .collect();
        if !members.is_empty() {
            rows.push(Row {
                title: desktop.name.clone(),
                desktop: desktop.id.clone(),
                members,
            });
        }
    }
    rows
}

/// How many cards fit on one line within `width`.
pub fn cards_per_line(width: f32) -> usize {
    (((width - 2.0 * PADDING + CARD_GAP) / (CARD.x + CARD_GAP)).floor() as usize).max(1)
}

/// The panel size for the rows: as wide as the widest row needs, at most
/// `limit.x`, with longer rows wrapping onto more lines; as tall as that
/// makes it, at most `limit.y`, beyond which the rows scroll.
pub fn panel_size(rows: &[Row], limit: Vec2) -> Vec2 {
    let widest = rows
        .iter()
        .map(|row| row.members.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let per_line = widest.min(cards_per_line(limit.x));
    let width = 2.0 * PADDING + per_line as f32 * CARD.x + (per_line - 1) as f32 * CARD_GAP + SLACK;
    let lines: f32 = rows
        .iter()
        .map(|row| {
            let lines = row.members.len().max(1).div_ceil(per_line) as f32;
            HEADER + HEADER_GAP + lines * CARD.y + (lines - 1.0) * CARD_GAP
        })
        .sum();
    let count = rows.len().max(1) as f32;
    let footer = if rows.len() > 1 { FOOTER } else { 0.0 };
    let height = 2.0 * PADDING + lines + (count - 1.0) * ROW_GAP + footer + SLACK;
    vec2(width.min(limit.x), height.min(limit.y))
}

/// The pointer's monitor: its work area (the screen minus the taskbar) in
/// physical pixels and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Monitor {
    work: [i32; 4],
    scale: f32,
}

impl Monitor {
    fn under_pointer() -> Option<Self> {
        let mut point = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut point) };
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return None;
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let work = info.rcWork;
        Some(Self {
            work: [work.left, work.top, work.right, work.bottom],
            scale: dpi_x as f32 / 96.0,
        })
    }

    /// What the strip may use, in logical points: 90 % of the work area's
    /// width and 60 % of its height; taller than that, it scrolls.
    fn limit(&self) -> Vec2 {
        let width = (self.work[2] - self.work[0]) as f32 / self.scale;
        let height = (self.work[3] - self.work[1]) as f32 / self.scale;
        vec2(width * 0.9, height * 0.6)
    }

    /// The window rectangle for a panel: centred, its bottom edge just above
    /// the taskbar.
    fn window_rect(&self, panel: Vec2) -> [i32; 4] {
        let width = (panel.x * self.scale).round() as i32;
        let height = (panel.y * self.scale).round() as i32;
        let left = self.work[0] + (self.work[2] - self.work[0] - width) / 2;
        let bottom = self.work[3] - (TASKBAR_GAP * self.scale).round() as i32;
        let top = (bottom - height).max(self.work[1]);
        [left, top, width, height]
    }
}

struct Drag {
    hwnd: isize,
    /// Where the card was grabbed, from its top-left corner.
    grab: Vec2,
}

struct Shared {
    snapshot: Option<ArrangeSnapshot>,
    /// The taskbar group on screen.
    key: Option<String>,
    /// The card the arrow keys act on.
    cursor: Option<isize>,
    drag: Option<Drag>,
    /// Last frame's layout, for finding where a dragged card would land.
    row_rects: Vec<Rect>,
    card_rects: Vec<Vec<Rect>>,
    to_host: Arc<HostChannel>,
    closed: bool,
    window: isize,
    monitor: Option<Monitor>,
    focused_once: bool,
    thumbs: HashMap<isize, Option<Thumbnail>>,
}

impl Shared {
    fn group(&self) -> Option<&ArrangeGroup> {
        let snapshot = self.snapshot.as_ref()?;
        let key = self.key.as_ref()?;
        snapshot.groups.iter().find(|group| &group.key == key)
    }

    fn panel(&self) -> Vec2 {
        let limit = self
            .monitor
            .map(|monitor| monitor.limit())
            .unwrap_or(vec2(1600.0, 900.0));
        match (self.group(), &self.snapshot) {
            (Some(group), Some(snapshot)) => {
                let mut size = panel_size(&rows(group, &snapshot.desktops), limit);
                if snapshot.groups.len() > 1 {
                    size.y = (size.y + SWITCHER).min(limit.y);
                    size.x = size.x.max(2.0 * PADDING + 240.0).min(limit.x);
                }
                size
            }
            _ => EMPTY_PANEL.min(limit),
        }
    }
}

/// The mouse way to see and order a program's windows: live pictures of all
/// of them, one row per desktop, in taskbar order.
pub struct Arrange {
    shared: Arc<Mutex<Shared>>,
    palette_hwnd: isize,
}

impl Arrange {
    pub fn new(to_host: Arc<HostChannel>, palette_hwnd: isize) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                snapshot: None,
                key: None,
                cursor: None,
                drag: None,
                row_rects: Vec::new(),
                card_rects: Vec::new(),
                to_host,
                closed: false,
                window: 0,
                monitor: None,
                focused_once: false,
                thumbs: HashMap::new(),
            })),
            palette_hwnd,
        }
    }

    pub fn open(&self, snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        shared.key = snapshot
            .groups
            .get(snapshot.focus)
            .map(|group| group.key.clone());
        shared.snapshot = Some(snapshot);
        shared.closed = false;
        shared.window = 0;
        shared.focused_once = false;
        shared.cursor = None;
        shared.drag = None;
        shared.monitor = Monitor::under_pointer();
    }

    /// Takes fresh data and keeps the same taskbar group on screen.
    pub fn update(&self, snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let still_there = shared
            .key
            .as_ref()
            .is_some_and(|key| snapshot.groups.iter().any(|group| &group.key == key));
        if !still_there {
            shared.key = snapshot
                .groups
                .get(snapshot.focus)
                .map(|group| group.key.clone());
        }
        shared.snapshot = Some(snapshot);
    }

    pub fn was_closed(&self) -> bool {
        self.shared
            .lock()
            .map(|shared| shared.closed)
            .unwrap_or(false)
    }

    /// Unregisters every picture; runs on the UI thread, which owns them.
    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.closed = false;
            shared.window = 0;
            shared.drag = None;
            shared.thumbs.clear();
        }
    }

    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let palette_hwnd = self.palette_hwnd;
        let panel = self
            .shared
            .lock()
            .map(|shared| shared.panel())
            .unwrap_or(EMPTY_PANEL);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(true)
            .with_inner_size(panel);
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }

        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let Ok(mut shared) = shared.lock() else {
                return;
            };
            if shared.window == 0 {
                shared.window = settings::find_window(TITLE, palette_hwnd);
                if shared.window != 0 {
                    let border = theme::colorref(Tokens::get(ui.ctx()).border);
                    dwm_thumbs::round_corners(shared.window as HWND, border);
                    shared.to_host.send(HostRequest::FocusWindow(shared.window));
                }
            }
            let panel = shared.panel();
            if let (Some(monitor), true) = (shared.monitor, shared.window != 0) {
                keep_placed(shared.window as HWND, monitor.window_rect(panel));
            }

            let (escape, focused) = ui
                .ctx()
                .input(|i| (i.key_pressed(egui::Key::Escape), i.viewport().focused));
            if focused == Some(true) {
                shared.focused_once = true;
            }
            if escape || (shared.focused_once && focused == Some(false)) {
                shared.closed = true;
            }
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                shared.closed = true;
            }

            strip(ui, &mut shared);

            // The root viewport is the hidden palette and only repaints when
            // asked; it is what stops showing this viewport once closed.
            if shared.closed {
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

/// Moves the window where it belongs whenever it is not there: winit resizes
/// it itself when it lands on a monitor with other scaling.
fn keep_placed(hwnd: HWND, wanted: [i32; 4]) {
    let mut now = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(hwnd, &mut now) };
    let current = [
        now.left,
        now.top,
        now.right - now.left,
        now.bottom - now.top,
    ];
    if current != wanted {
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                wanted[0],
                wanted[1],
                wanted[2],
                wanted[3],
                SWP_NOACTIVATE,
            )
        };
    }
}

/// A place in a row while laying out: a window, or the gap where the window
/// being dragged would land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Window(usize),
    Gap,
}

/// The rows as they look during a drag: the dragged window taken out, and a
/// gap at `target`, given as (row, index among the remaining windows).
pub fn preview(
    rows: &[Row],
    dragged: Option<usize>,
    target: Option<(usize, usize)>,
) -> Vec<Vec<Slot>> {
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut slots: Vec<Slot> = row
                .members
                .iter()
                .copied()
                .filter(|member| Some(*member) != dragged)
                .map(Slot::Window)
                .collect();
            if let Some((target_row, index)) = target {
                if target_row == row_index {
                    slots.insert(index.min(slots.len()), Slot::Gap);
                }
            }
            slots
        })
        .collect()
}

/// Every window, row by row, after `dragged` is dropped at `target`.
pub fn dropped_order(rows: &[Row], dragged: usize, target: (usize, usize)) -> Vec<usize> {
    preview(rows, Some(dragged), Some(target))
        .into_iter()
        .flatten()
        .map(|slot| match slot {
            Slot::Window(member) => member,
            Slot::Gap => dragged,
        })
        .collect()
}

/// Where the pointer would drop a window, from last frame's layout: the row
/// under it (or the nearest) and how many of that row's cards come before
/// it, on the lines above or to its left on its own line.
fn drop_target(
    pointer: egui::Pos2,
    row_rects: &[Rect],
    card_rects: &[Vec<Rect>],
) -> Option<(usize, usize)> {
    let row = row_rects
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |r: &Rect| {
                if r.y_range().contains(pointer.y) {
                    0.0
                } else {
                    (r.center().y - pointer.y).abs()
                }
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(index, _)| index)?;
    let index = card_rects
        .get(row)
        .map(|cards| {
            cards
                .iter()
                .filter(|card| {
                    card.max.y <= pointer.y
                        || (card.y_range().contains(pointer.y) && card.center().x < pointer.x)
                })
                .count()
        })
        .unwrap_or(0);
    Some((row, index))
}

/// A picture to put on screen once the whole frame is laid out, so that one
/// under the dragged card or under a menu can be hidden instead.
struct Pending {
    hwnd: isize,
    dest: Rect,
    clip: Rect,
    opacity: u8,
}

fn strip(ui: &mut Ui, shared: &mut Shared) {
    let tokens = Tokens::get(ui.ctx());
    let panel = ui.max_rect();
    ui.painter().rect_filled(panel, 0, tokens.elevated_bg);

    let content = panel.shrink(PADDING);
    let Some((group, desktops)) = shared
        .group()
        .cloned()
        .zip(shared.snapshot.as_ref().map(|s| s.desktops.clone()))
    else {
        let watched = shared
            .snapshot
            .as_ref()
            .map(|s| s.watched.join(", "))
            .unwrap_or_default();
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            empty_state(
                ui,
                "No windows to arrange",
                &format!("No windows of {watched} are open."),
            );
        });
        shared.thumbs.clear();
        return;
    };
    let plugin = shared
        .snapshot
        .as_ref()
        .map(|s| s.plugin.clone())
        .unwrap_or_default();

    let mut content = content;
    if let Some(snapshot) = shared.snapshot.clone().filter(|s| s.groups.len() > 1) {
        let labels: Vec<&str> = snapshot.groups.iter().map(|g| g.label.as_str()).collect();
        let current = snapshot.groups.iter().position(|g| g.key == group.key);
        let area = Rect::from_min_size(content.min, vec2(content.width(), SWITCHER));
        let chosen = ui
            .scope_builder(UiBuilder::new().max_rect(area), |ui| {
                choice::dropdown(ui, "arrange-group", &labels, current)
            })
            .inner;
        if let Some(index) = chosen.filter(|index| Some(*index) != current) {
            shared.key = Some(snapshot.groups[index].key.clone());
            shared.cursor = None;
            shared.drag = None;
            ui.ctx().request_repaint();
        }
        content.min.y += SWITCHER;
    }

    let rows = rows(&group, &desktops);
    shared
        .thumbs
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let menu_open = egui::Popup::is_any_open(&ctx);
    let pointer = ctx.pointer_latest_pos();
    let mut actions: Vec<ArrangeAction> = Vec::new();
    let mut activate: Option<isize> = None;

    let flat: Vec<usize> = rows.iter().flat_map(|row| row.members.clone()).collect();
    if shared
        .cursor
        .is_none_or(|hwnd| !flat.iter().any(|m| group.windows[*m].hwnd == hwnd))
    {
        shared.cursor = flat.first().map(|m| group.windows[*m].hwnd);
    }

    // The drag: follow the pointer until the button comes up, then drop.
    let dragged = shared.drag.as_ref().and_then(|drag| {
        group
            .windows
            .iter()
            .position(|window| window.hwnd == drag.hwnd)
    });
    if dragged.is_none() {
        shared.drag = None;
    }
    let target = match (dragged, pointer) {
        (Some(_), Some(pointer)) => drop_target(pointer, &shared.row_rects, &shared.card_rects),
        _ => None,
    };
    if let Some(member) = dragged {
        if !ctx.input(|i| i.pointer.primary_down()) {
            if let Some(target) = target {
                actions.extend(drop_actions(&group, &rows, member, target));
            }
            shared.drag = None;
        }
    }
    let dragging = shared.drag.is_some();
    let layout = preview(
        &rows,
        dragged.filter(|_| dragging),
        target.filter(|_| dragging),
    );

    let mut pending: Vec<Pending> = Vec::new();
    let mut row_rects: Vec<Rect> = Vec::new();
    let mut card_rects: Vec<Vec<Rect>> = Vec::new();
    let footer = if rows.len() > 1 { FOOTER } else { 0.0 };
    let rows_area = Rect::from_min_max(content.min, pos2(content.max.x, content.max.y - footer));

    ui.scope_builder(UiBuilder::new().max_rect(rows_area), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("arrange-rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
                for (row_index, row) in rows.iter().enumerate() {
                    if row_index > 0 {
                        ui.add_space(ROW_GAP);
                    }
                    let row_top = ui.cursor().min.y;
                    text::single(ui, text::section_job(&row.title, tokens.text_disabled));
                    ui.add_space(HEADER_GAP);
                    let mut cards_here: Vec<Rect> = Vec::new();
                    // Rows wrap rather than scroll sideways, so the one scroll
                    // area left is the vertical one and takes the wheel.
                    ui.scope(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(CARD_GAP, CARD_GAP);
                            if layout[row_index].is_empty() {
                                placeholder(ui, "No windows on this desktop");
                                return;
                            }
                            for slot in &layout[row_index] {
                                let Slot::Window(member) = *slot else {
                                    gap(ui);
                                    continue;
                                };
                                let window = &group.windows[member];
                                let desktop_name = desktop_name(&desktops, &window.desktop);
                                let has_picture = !menu_open
                                    && thumbnail(shared, window.hwnd)
                                        .is_some_and(|thumb| thumb.source_size().is_some());
                                let is_cursor = shared.cursor == Some(window.hwnd);
                                let (response, picture) =
                                    card(ui, &window.label, desktop_name, has_picture, is_cursor);
                                cards_here.push(response.rect);
                                if has_picture {
                                    pending.push(Pending {
                                        hwnd: window.hwnd,
                                        dest: picture,
                                        clip: ui.clip_rect(),
                                        opacity: 255,
                                    });
                                }
                                if response.clicked() {
                                    activate = Some(window.hwnd);
                                }
                                if response.drag_started() {
                                    if let Some(at) = response.interact_pointer_pos() {
                                        shared.drag = Some(Drag {
                                            hwnd: window.hwnd,
                                            grab: at - response.rect.min,
                                        });
                                        shared.cursor = Some(window.hwnd);
                                    }
                                }
                                response.context_menu(|ui| {
                                    if let Some(action) =
                                        card_menu(ui, window.hwnd, &window.desktop, &desktops)
                                    {
                                        actions.push(action);
                                    }
                                });
                            }
                        });
                    });
                    card_rects.push(cards_here);
                    row_rects.push(Rect::from_min_max(
                        pos2(rows_area.min.x, row_top),
                        pos2(rows_area.max.x, ui.cursor().min.y),
                    ));
                }
            });
    });
    shared.row_rects = row_rects;
    shared.card_rects = card_rects;

    // The dragged card rides above everything, following the pointer.
    let mut floating: Option<Rect> = None;
    if let (Some(drag), Some(member), Some(pointer)) = (shared.drag.as_ref(), dragged, pointer) {
        let rect = Rect::from_min_size(pointer - drag.grab, CARD);
        let window = &group.windows[member];
        let layer = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("arrange-drag"),
        ));
        layer.rect_filled(rect, 6, tokens.hover_bg);
        let area = Rect::from_min_size(rect.min + Vec2::splat(CARD_INSET), THUMB);
        layer.rect(
            area,
            4,
            tokens.panel_bg,
            theme::stroke(&ctx, 1.0, tokens.accent),
            StrokeKind::Inside,
        );
        if !menu_open {
            pending.push(Pending {
                hwnd: window.hwnd,
                dest: area.shrink(1.0),
                clip: Rect::EVERYTHING,
                opacity: 204,
            });
        }
        floating = Some(rect);
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    place_pictures(
        shared,
        &pending,
        floating,
        dragged.map(|m| group.windows[m].hwnd),
        ppp,
    );

    if footer > 0.0 {
        let job = text::job(
            "Order on other desktops applies at the next restore.",
            theme::lora(12.0),
            tokens.text_secondary,
            None,
        );
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            pos2(content.min.x, content.max.y - galley.size().y),
            galley,
            tokens.text_secondary,
        );
    }

    if !menu_open && !dragging {
        keyboard(
            &ctx,
            shared,
            &group,
            &rows,
            &flat,
            &mut actions,
            &mut activate,
        );
    }

    for action in &actions {
        apply_locally(shared, action);
    }
    for action in actions {
        shared.to_host.send(HostRequest::Arrange {
            plugin: plugin.clone(),
            action,
        });
    }
    if let Some(hwnd) = activate {
        shared.to_host.send(HostRequest::Arrange {
            plugin,
            action: ArrangeAction::Activate(hwnd),
        });
        shared.closed = true;
    }
}

fn desktop_name<'a>(desktops: &'a [ArrangeDesktop], id: &str) -> &'a str {
    desktops
        .iter()
        .find(|desktop| desktop.id == id)
        .map(|desktop| desktop.name.as_str())
        .unwrap_or("")
}

/// The window's picture, registered on first use. Only possible once the
/// strip's own window exists, since it is the destination.
fn thumbnail(shared: &mut Shared, hwnd: isize) -> Option<&mut Thumbnail> {
    let destination = shared.window as HWND;
    shared
        .thumbs
        .entry(hwnd)
        .or_insert_with(|| {
            (!destination.is_null())
                .then(|| Thumbnail::register(destination, hwnd as HWND))
                .flatten()
        })
        .as_mut()
}

/// Puts every picture where its card is. DWM draws pictures on top of
/// everything in the window, so one under the dragged card is hidden, and
/// pictures of windows without a card this frame are hidden too.
fn place_pictures(
    shared: &mut Shared,
    pending: &[Pending],
    floating: Option<Rect>,
    dragged: Option<isize>,
    pixels_per_point: f32,
) {
    let placed: Vec<isize> = pending.iter().map(|p| p.hwnd).collect();
    for (hwnd, thumb) in shared.thumbs.iter_mut() {
        if let Some(thumb) = thumb {
            if !placed.contains(hwnd) {
                thumb.place(None);
            }
        }
    }
    for item in pending {
        let covered =
            floating.is_some_and(|rect| Some(item.hwnd) != dragged && rect.intersects(item.dest));
        let Some(thumb) = thumbnail(shared, item.hwnd) else {
            continue;
        };
        let Some(size) = thumb.source_size() else {
            thumb.place(None);
            continue;
        };
        if covered {
            thumb.place(None);
            continue;
        }
        let frame = dwm_thumbs::visible_frame(item.hwnd as HWND)
            .unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), size));
        let dest = Rect::from_center_size(
            item.dest.center(),
            dwm_thumbs::fit(frame.size(), item.dest.size()),
        );
        let placement =
            dwm_thumbs::crop(dest, item.clip, frame).map(|(shown, cropped)| Placement {
                dest: dwm_thumbs::physical(shown, pixels_per_point),
                source: dwm_thumbs::physical(cropped, 1.0),
                opacity: item.opacity,
            });
        thumb.place(placement);
    }
}

/// What a drop means: a move to the target row's desktop when that differs,
/// then the whole group's new order.
fn drop_actions(
    group: &ArrangeGroup,
    rows: &[Row],
    dragged: usize,
    target: (usize, usize),
) -> Vec<ArrangeAction> {
    let mut actions = Vec::new();
    let from = rows.iter().position(|row| row.members.contains(&dragged));
    let to = &rows[target.0];
    if from != Some(target.0) && !to.desktop.is_empty() {
        actions.push(ArrangeAction::MoveToDesktop {
            hwnd: group.windows[dragged].hwnd,
            desktop: to.desktop.clone(),
        });
    }
    let order: Vec<isize> = dropped_order(rows, dragged, target)
        .into_iter()
        .map(|member| group.windows[member].hwnd)
        .collect();
    let now: Vec<isize> = rows
        .iter()
        .flat_map(|row| row.members.iter().map(|m| group.windows[*m].hwnd))
        .collect();
    if order != now || !actions.is_empty() {
        actions.push(ArrangeAction::Reorder {
            group: group.key.clone(),
            order,
        });
    }
    actions
}

/// ←/→ move the cursor, Ctrl+←/→ move its window within its row, Enter
/// switches to it. Esc is handled with the other ways to close.
fn keyboard(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    rows: &[Row],
    flat: &[usize],
    actions: &mut Vec<ArrangeAction>,
    activate: &mut Option<isize>,
) {
    let Some(position) = shared
        .cursor
        .and_then(|hwnd| flat.iter().position(|m| group.windows[*m].hwnd == hwnd))
    else {
        return;
    };
    let member = flat[position];
    let (ctrl_left, ctrl_right, left, right, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    if enter {
        *activate = Some(group.windows[member].hwnd);
        return;
    }
    if left && position > 0 {
        shared.cursor = Some(group.windows[flat[position - 1]].hwnd);
    }
    if right && position + 1 < flat.len() {
        shared.cursor = Some(group.windows[flat[position + 1]].hwnd);
    }
    let Some(row) = rows.iter().position(|row| row.members.contains(&member)) else {
        return;
    };
    let at = rows[row]
        .members
        .iter()
        .position(|m| *m == member)
        .unwrap_or(0);
    if ctrl_left && at > 0 {
        actions.extend(drop_actions(group, rows, member, (row, at - 1)));
    }
    if ctrl_right && at + 1 < rows[row].members.len() {
        actions.extend(drop_actions(group, rows, member, (row, at + 1)));
    }
}

/// Shows a change at once; the plugin's own data follows a moment later and
/// replaces it.
fn apply_locally(shared: &mut Shared, action: &ArrangeAction) {
    let Some(key) = shared.key.clone() else {
        return;
    };
    let Some(group) = shared
        .snapshot
        .as_mut()
        .and_then(|snapshot| snapshot.groups.iter_mut().find(|group| group.key == key))
    else {
        return;
    };
    match action {
        ArrangeAction::MoveToDesktop { hwnd, desktop } => {
            if let Some(window) = group.windows.iter_mut().find(|w| w.hwnd == *hwnd) {
                window.desktop = desktop.clone();
            }
        }
        ArrangeAction::Reorder { order, .. } => {
            group.windows.sort_by_key(|window| {
                order
                    .iter()
                    .position(|h| *h == window.hwnd)
                    .unwrap_or(usize::MAX)
            });
        }
        ArrangeAction::Close(hwnd) => group.windows.retain(|window| window.hwnd != *hwnd),
        ArrangeAction::Activate(_) => {}
    }
}

/// The right-click menu. Closing a window is only offered here, never on a
/// key, because it cannot be undone.
fn card_menu(
    ui: &mut Ui,
    hwnd: isize,
    current: &str,
    desktops: &[ArrangeDesktop],
) -> Option<ArrangeAction> {
    let mut chosen = None;
    if ui.button("Close window").clicked() {
        chosen = Some(ArrangeAction::Close(hwnd));
        ui.close();
    }
    ui.menu_button("Move to desktop", |ui| {
        for desktop in desktops.iter().filter(|desktop| desktop.id != current) {
            if ui.button(&desktop.name).clicked() {
                chosen = Some(ArrangeAction::MoveToDesktop {
                    hwnd,
                    desktop: desktop.id.clone(),
                });
                ui.close();
            }
        }
    });
    chosen
}

/// One window: its picture area (the live picture is put on top of it
/// later) and its name underneath. Without a picture the area shows the name
/// and desktop instead. Returns the card's response and the picture area.
fn card(
    ui: &mut Ui,
    label: &str,
    desktop: &str,
    has_picture: bool,
    is_cursor: bool,
) -> (Response, Rect) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(CARD, Sense::click_and_drag());
    let lit = response.hovered() || is_cursor;
    let painter = ui.painter();
    if lit {
        painter.rect_filled(rect, 6, tokens.hover_bg);
    }
    let area = Rect::from_min_size(rect.min + Vec2::splat(CARD_INSET), THUMB);
    let border = if lit { tokens.accent } else { tokens.border };
    painter.rect(
        area,
        4,
        tokens.panel_bg,
        theme::stroke(ui.ctx(), 1.0, border),
        StrokeKind::Inside,
    );
    let picture = area.shrink(1.0);
    if !has_picture {
        name_card(ui, picture, label, desktop);
    }

    let mut job = text::job(label, theme::lora(12.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: THUMB.x,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(area.left(), area.bottom() + CARD_INSET),
        galley,
        tokens.text_primary,
    );
    (response.on_hover_cursor(egui::CursorIcon::Grab), picture)
}

/// The gap a dragged card would land in.
fn gap(ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(CARD, Sense::hover());
    let area = Rect::from_min_size(rect.min + Vec2::splat(CARD_INSET), THUMB);
    ui.painter().rect(
        area,
        4,
        theme::with_alpha(tokens.accent, 0.12),
        theme::stroke(ui.ctx(), 1.0, tokens.accent),
        StrokeKind::Inside,
    );
}

/// An empty row, still a place to drop a window on.
fn placeholder(ui: &mut Ui, text_line: &str) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(CARD, Sense::hover());
    let area = Rect::from_min_size(rect.min + Vec2::splat(CARD_INSET), THUMB);
    ui.painter().rect(
        area,
        4,
        egui::Color32::TRANSPARENT,
        theme::stroke(ui.ctx(), 1.0, tokens.border),
        StrokeKind::Inside,
    );
    let galley = ui.painter().layout_job(text::job(
        text_line,
        theme::lora(12.0),
        tokens.text_secondary,
        None,
    ));
    ui.painter().galley(
        area.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
}

/// Stands in for a picture DWM cannot give, and for all pictures while a menu
/// is open, since DWM would draw them over it.
fn name_card(ui: &Ui, area: Rect, label: &str, desktop: &str) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let mut job = text::job(label, theme::cormorant(15.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: area.width() - 16.0,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    job.halign = egui::Align::Center;
    let title = painter.layout_job(job);
    let detail = painter.layout_job(text::job(
        desktop,
        theme::lora(12.0),
        tokens.text_secondary,
        None,
    ));
    let height = title.size().y
        + if desktop.is_empty() {
            0.0
        } else {
            4.0 + detail.size().y
        };
    let top = area.center().y - height / 2.0;
    painter.galley(
        pos2(area.center().x, top),
        title.clone(),
        tokens.text_primary,
    );
    if !desktop.is_empty() {
        painter.galley(
            pos2(
                area.center().x - detail.size().x / 2.0,
                top + title.size().y + 4.0,
            ),
            detail,
            tokens.text_secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge::ArrangeWindow;

    fn desktop(id: &str, name: &str, current: bool) -> ArrangeDesktop {
        ArrangeDesktop {
            id: id.to_string(),
            name: name.to_string(),
            current,
        }
    }

    fn group(desktops: &[&str]) -> ArrangeGroup {
        ArrangeGroup {
            key: "chrome.exe".to_string(),
            label: "Chrome".to_string(),
            windows: desktops
                .iter()
                .enumerate()
                .map(|(i, d)| ArrangeWindow {
                    hwnd: i as isize,
                    label: format!("w{i}"),
                    desktop: d.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_current_desktop_comes_first_then_task_view_order() {
        let desktops = [
            desktop("home", "Home", false),
            desktop("work", "Work", true),
            desktop("read", "Reading", false),
        ];
        let rows = rows(&group(&["read", "work", "home", "work", ""]), &desktops);
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["This desktop", "Home", "Reading"]);
        assert_eq!(rows[0].members, [1, 3, 4]);
        assert_eq!(rows[1].members, [2]);
        assert_eq!(rows[2].members, [0]);
    }

    #[test]
    fn this_desktop_is_listed_even_when_it_has_no_windows() {
        let desktops = [
            desktop("home", "Home", false),
            desktop("work", "Work", true),
        ];
        let rows = rows(&group(&["home"]), &desktops);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].members.is_empty());
        assert_eq!(rows[1].members, [0]);
    }

    #[test]
    fn the_panel_grows_with_the_widest_row_up_to_the_limit() {
        let row = |n: usize| Row {
            title: String::new(),
            desktop: String::new(),
            members: (0..n).collect(),
        };
        let size = panel_size(&[row(3), row(1)], vec2(5000.0, 5000.0));
        assert_eq!(
            size.x,
            2.0 * PADDING + 3.0 * CARD.x + 2.0 * CARD_GAP + SLACK
        );
        assert_eq!(
            size.y,
            2.0 * PADDING + 2.0 * (HEADER + HEADER_GAP + CARD.y) + ROW_GAP + FOOTER + SLACK
        );
    }

    #[test]
    fn a_long_row_wraps_onto_more_lines_instead_of_scrolling() {
        let row = |n: usize| Row {
            title: String::new(),
            desktop: String::new(),
            members: (0..n).collect(),
        };
        let limit = vec2(2.0 * PADDING + 5.0 * CARD.x + 4.0 * CARD_GAP + 10.0, 5000.0);
        assert_eq!(cards_per_line(limit.x), 5);
        let size = panel_size(&[row(12)], limit);
        assert_eq!(
            size.x,
            2.0 * PADDING + 5.0 * CARD.x + 4.0 * CARD_GAP + SLACK
        );
        // 12 cards at 5 a line make 3 lines.
        assert_eq!(
            size.y,
            2.0 * PADDING + HEADER + HEADER_GAP + 3.0 * CARD.y + 2.0 * CARD_GAP + SLACK
        );
        let short = panel_size(&[row(12)], vec2(limit.x, 200.0));
        assert_eq!(short.y, 200.0);
    }

    fn two_rows() -> Vec<Row> {
        vec![
            Row {
                title: "This desktop".to_string(),
                desktop: "work".to_string(),
                members: vec![0, 1, 2],
            },
            Row {
                title: "Reading".to_string(),
                desktop: "read".to_string(),
                members: vec![3],
            },
        ]
    }

    #[test]
    fn a_drag_shows_a_gap_where_the_window_will_land() {
        let rows = two_rows();
        let layout = preview(&rows, Some(0), Some((0, 2)));
        assert_eq!(layout[0], [Slot::Window(1), Slot::Window(2), Slot::Gap]);
        assert_eq!(layout[1], [Slot::Window(3)]);
        let untouched = preview(&rows, None, None);
        assert_eq!(
            untouched[0],
            [Slot::Window(0), Slot::Window(1), Slot::Window(2)]
        );
    }

    #[test]
    fn a_drop_gives_the_whole_new_order_row_by_row() {
        let rows = two_rows();
        assert_eq!(dropped_order(&rows, 0, (0, 2)), [1, 2, 0, 3]);
        assert_eq!(dropped_order(&rows, 2, (0, 0)), [2, 0, 1, 3]);
        assert_eq!(dropped_order(&rows, 1, (1, 0)), [0, 2, 1, 3]);
        assert_eq!(dropped_order(&rows, 3, (0, 1)), [0, 3, 1, 2]);
    }

    #[test]
    fn a_drop_on_another_row_moves_the_window_to_that_desktop_first() {
        let rows = two_rows();
        let group = group(&["work", "work", "work", "read"]);
        let actions = drop_actions(&group, &rows, 1, (1, 1));
        assert_eq!(
            actions[0],
            ArrangeAction::MoveToDesktop {
                hwnd: 1,
                desktop: "read".to_string()
            }
        );
        assert_eq!(
            actions[1],
            ArrangeAction::Reorder {
                group: "chrome.exe".to_string(),
                order: vec![0, 2, 3, 1]
            }
        );
        assert!(drop_actions(&group, &rows, 0, (0, 0)).is_empty());
    }

    #[test]
    fn the_drop_target_comes_from_last_frames_layout() {
        let row_rects = [
            Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 200.0)),
            Rect::from_min_max(pos2(0.0, 216.0), pos2(800.0, 416.0)),
        ];
        let card = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), CARD);
        let card_rects = vec![
            vec![card(0.0, 20.0), card(228.0, 20.0)],
            vec![card(0.0, 236.0)],
        ];
        assert_eq!(
            drop_target(pos2(300.0, 100.0), &row_rects, &card_rects),
            Some((0, 1))
        );
        assert_eq!(
            drop_target(pos2(500.0, 100.0), &row_rects, &card_rects),
            Some((0, 2))
        );
        assert_eq!(
            drop_target(pos2(10.0, 300.0), &row_rects, &card_rects),
            Some((1, 0))
        );
        assert_eq!(
            drop_target(pos2(10.0, 900.0), &row_rects, &card_rects),
            Some((1, 1))
        );

        // A wrapped row: two cards on the first line, one on the second.
        let wrapped = vec![vec![card(0.0, 20.0), card(228.0, 20.0), card(0.0, 200.0)]];
        let tall = [Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 400.0))];
        assert_eq!(
            drop_target(pos2(300.0, 250.0), &tall, &wrapped),
            Some((0, 3))
        );
        assert_eq!(
            drop_target(pos2(10.0, 250.0), &tall, &wrapped),
            Some((0, 2))
        );
        assert_eq!(drop_target(pos2(10.0, 60.0), &tall, &wrapped), Some((0, 0)));
    }
}
